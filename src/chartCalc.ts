import { Horoscope, Origin } from 'circular-natal-horoscope-js/dist/index.js'
import { AspectCalculator } from '@astrodraw/astrochart'

export type ChartSummary = {
  ascendant: string
  descendant: string
  midheaven: string
  ic: string
  time: { timezone: string; local: string; utc: string }
  houses: Array<{ house: number; eclipticDegrees: number; sign: string; formatted: string }>
  planets: Array<{ key: string; name: string; eclipticDegrees: number; sign: string; formatted: string }>
  astroChartData: { planets: Record<string, number[]>; cusps: number[]; aspects: any[] }
  aspectsList: Array<{ from: string; to: string; type: string; orb: string; applying: boolean | null }>
}

export function pad2(n: number) {
  return String(n).padStart(2, '0')
}

export function formatDeg(deg: number) {
  const d = Math.floor(deg)
  const mFloat = (deg - d) * 60
  const m = Math.floor(mFloat)
  return `${d}°${pad2(m)}′`
}

/** Format decimal degrees as degrees and arc minutes (e.g. for aspect orbs). */
export function formatDegArcMin(deg: number): string {
  const d = Math.floor(deg)
  const mFloat = (deg - d) * 60
  const m = Math.round(mFloat)
  if (m >= 60) return `${d + 1}° 00′`
  return `${d}° ${pad2(m)}′`
}

/** Parse DMS (degrees, minutes, seconds) to decimal degrees. Sign: N/E = positive, S/W = negative. */
export function dmsToDecimal(
  deg: number,
  min: number,
  sec: number,
  sign: 'N' | 'S' | 'E' | 'W',
): number {
  const abs = deg + min / 60 + sec / 3600
  if (sign === 'S' || sign === 'W') return -abs
  return abs
}

function calcOrb(pos1: number, pos2: number, aspectDeg: number): number {
  const diff = ((pos1 - pos2) % 360 + 360) % 360
  const orb = (target: number) => Math.min(Math.abs(diff - target), 360 - Math.abs(diff - target))
  return Math.min(orb(aspectDeg), orb(360 - aspectDeg))
}

function isApplying(pos1: number, speed1: number, pos2: number, speed2: number, aspectDeg: number): boolean {
  const dailySpeed1 = speed1 * 86400
  const dailySpeed2 = speed2 * 86400
  const currentOrb = calcOrb(pos1, pos2, aspectDeg)
  const futureOrb = calcOrb(pos1 + dailySpeed1, pos2 + dailySpeed2, aspectDeg)
  return futureOrb < currentOrb
}

function roundToArcMinute(deg: number): number {
  return Math.round(deg * 60) / 60
}

export function dtLocalNowValue() {
  const now = new Date()
  return `${now.getFullYear()}-${pad2(now.getMonth() + 1)}-${pad2(now.getDate())}T${pad2(
    now.getHours(),
  )}:${pad2(now.getMinutes())}`
}

export function calculateChart(dt: Date, lat: number, lon: number): { summary?: ChartSummary; error?: string } {
  if (Number.isNaN(dt.getTime())) return { error: 'Invalid date/time.' }
  if (!Number.isFinite(lat) || lat < -90 || lat > 90) return { error: 'Latitude must be between -90 and 90.' }
  if (!Number.isFinite(lon) || lon < -180 || lon > 180) return { error: 'Longitude must be between -180 and 180.' }

  try {
    const origin = new Origin({
      year: dt.getFullYear(),
      month: dt.getMonth(),
      date: dt.getDate(),
      hour: dt.getHours(),
      minute: dt.getMinutes(),
      latitude: lat,
      longitude: lon,
    })

    const horoscope = new Horoscope({
      origin,
      zodiac: 'tropical',
      houseSystem: 'regiomontanus',
      aspectPoints: [],
      aspectWithPoints: [],
      aspectTypes: [],
      language: 'en',
    })

    const asc = horoscope.Ascendant
    const mc = horoscope.Midheaven
    const houses = horoscope.Houses.map((h: any, idx: number) => {
      const cusp = h.ChartPosition?.StartPosition?.Ecliptic
      return {
        house: idx + 1,
        eclipticDegrees: cusp?.DecimalDegrees,
        sign: h.Sign?.label ?? h.Sign?.key ?? '',
        formatted: cusp?.ArcDegreesFormatted30 ?? '',
      }
    })

    const MEAN_DAILY_MOTION: Record<string, number> = {
      Sun: 0.9856, Moon: 13.176, Mercury: 1.3833, Venus: 1.2, Mars: 0.524,
      Jupiter: 0.0831, Saturn: 0.0335, Uranus: 0.0119, Neptune: 0.0061, Pluto: 0.004,
    }

    const bodyKeys: Array<[string, string]> = [
      ['sun', 'Sun'],
      ['moon', 'Moon'],
      ['mercury', 'Mercury'],
      ['venus', 'Venus'],
      ['mars', 'Mars'],
      ['jupiter', 'Jupiter'],
      ['saturn', 'Saturn'],
      ['uranus', 'Uranus'],
      ['neptune', 'Neptune'],
      ['pluto', 'Pluto'],
    ]

    const planetSpeeds: Record<string, number> = {}
    const planetRetrograde: Record<string, boolean> = {}
    const planets = bodyKeys
      .map(([key, name]) => {
        const b: any = (horoscope.CelestialBodies as any)[key]
        if (!b) return undefined
        const ecliptic = b.ChartPosition?.Ecliptic?.DecimalDegrees
        const dailyMotion = MEAN_DAILY_MOTION[name] ?? 0
        planetSpeeds[name] = (b.isRetrograde ? -dailyMotion : dailyMotion) / 86400
        planetRetrograde[name] = b.isRetrograde ?? false
        return {
          key,
          name,
          eclipticDegrees: ecliptic,
          sign: b.Sign?.label ?? b.Sign?.key ?? '',
          formatted: b.ChartPosition?.Ecliptic?.ArcDegreesFormatted30 ?? '',
        }
      })
      .filter(Boolean) as ChartSummary['planets']

    const astroPlanets: Record<string, number[]> = {}
    for (const p of planets) {
      astroPlanets[p.name] = [p.eclipticDegrees, planetRetrograde[p.name] ? -1 : 1]
    }
    const cusps = houses.map((h: { eclipticDegrees: number }) => h.eclipticDegrees).filter((x: number) => Number.isFinite(x)) as number[]

    const aspectPoints: Record<string, number[]> = { ...astroPlanets }
    const aspectCalc = new AspectCalculator(aspectPoints, {
      ASPECTS: {
        conjunction: { degree: 0, orbit: 10, color: 'transparent' },
        sextile: { degree: 60, orbit: 10, color: '#5a5' },
        square: { degree: 90, orbit: 10, color: '#FF4500' },
        trine: { degree: 120, orbit: 10, color: '#27AE60' },
        opposition: { degree: 180, orbit: 10, color: '#FF0000' },
      },
    } as any)
    const astroAspects: any[] = aspectCalc.radix(aspectPoints)
    const seenAspects = new Set<string>()
    const aspectsList = astroAspects.flatMap((a) => {
      const from = a.point?.name ?? ''
      const to = a.toPoint?.name ?? ''
      const type = a.aspect?.name ?? ''
      const key = [from, to].sort().join('|') + '|' + type
      if (seenAspects.has(key)) return []
      seenAspects.add(key)
      const precNum = typeof a.precision === 'number' ? a.precision : parseFloat(String(a.precision ?? 0))
      const orbStr = Number.isFinite(precNum) ? formatDegArcMin(precNum) : String(a.precision ?? '')
      const fromPos = astroPlanets[from]?.[0]
      const toPos = astroPlanets[to]?.[0]
      const fromSpeed = planetSpeeds[from]
      const toSpeed = planetSpeeds[to]
      const aspectDeg = a.aspect?.degree ?? 0
      const applying = (fromPos != null && toPos != null && fromSpeed != null && toSpeed != null)
        ? isApplying(fromPos, fromSpeed, toPos, toSpeed, aspectDeg)
        : null
      return [{ from, to, type, orb: orbStr, applying }]
    })

    const summary: ChartSummary = {
      ascendant: `${asc.Sign?.label ?? asc.Sign?.key ?? ''} ${
        asc.ChartPosition?.Ecliptic?.ArcDegreesFormatted30 ?? formatDeg(asc.ChartPosition?.Ecliptic?.DecimalDegrees ?? 0)
      }`,
      descendant: houses[6] ? `${houses[6].sign} ${houses[6].formatted || formatDeg(houses[6].eclipticDegrees)}` : '',
      midheaven: `${mc.Sign?.label ?? mc.Sign?.key ?? ''} ${
        mc.ChartPosition?.Ecliptic?.ArcDegreesFormatted30 ?? formatDeg(mc.ChartPosition?.Ecliptic?.DecimalDegrees ?? 0)
      }`,
      ic: houses[3] ? `${houses[3].sign} ${houses[3].formatted || formatDeg(houses[3].eclipticDegrees)}` : '',
      houses,
      planets,
      time: {
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        local: String((origin as any).localTimeFormatted ?? ''),
        utc: String((origin as any).utcTimeFormatted ?? ''),
      },
      astroChartData: {
        planets: Object.fromEntries(
          Object.entries(astroPlanets).map(([k, v]) => [k, [roundToArcMinute(v[0]), v[1]]])
        ),
        cusps: cusps.map(roundToArcMinute),
        aspects: astroAspects,
      },
      aspectsList,
    }

    return { summary }
  } catch (e) {
    return { error: e instanceof Error ? e.message : 'Failed to calculate chart.' }
  }
}
