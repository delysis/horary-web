import { calculateChart as calculateHoraryChart } from './astro/chart.js'
import { ASPECT_DEFINITIONS } from './astro/horarySettings.js'
import { signInfo } from './astro/utils.js'

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

function roundToArcMinute(deg: number): number {
  return Math.round(deg * 60) / 60
}

export function dtLocalNowValue(timezone = Intl.DateTimeFormat().resolvedOptions().timeZone) {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
  }).formatToParts(new Date())
  const part = (type: string) => parts.find(p => p.type === type)!.value
  return `${part('year')}-${part('month')}-${part('day')}T${part('hour')}:${part('minute')}`
}

// Keep Eileen's five major aspects and five-degree orbs in the shared calculation.
const aspectConfig = Object.fromEntries(Object.entries(ASPECT_DEFINITIONS).map(([key, aspect]) => [
  key, { enabled: aspect.major, orb: 5 },
]))

export function calculateChart(dt: Date, lat: number, lon: number, timezone = 'UTC') {
  if (Number.isNaN(dt.getTime())) return { error: 'Invalid date/time.' }
  if (!Number.isFinite(lat) || lat < -90 || lat > 90) return { error: 'Latitude must be between -90 and 90.' }
  if (!Number.isFinite(lon) || lon < -180 || lon > 180) return { error: 'Longitude must be between -180 and 180.' }
  if (Math.abs(lat) === 90) return { error: 'Regiomontanus houses are undefined at the geographic poles. Choose a latitude below 90°.' }
  try {
    const calculated = calculateHoraryChart({ date: dt, lat, lng: lon, houseSystem: 'regiomontanus', planetSet: 'modern', aspectConfig })
    const horary = { ...calculated, positions: calculated.positions as Record<string, { longitude: number; retrograde: boolean }> }
    const houses: ChartSummary['houses'] = horary.houses.cusps.map((longitude: number, index: number) => ({
      house: index + 1, eclipticDegrees: longitude, sign: signInfo(longitude).name,
      formatted: formatDeg(longitude % 30),
    }))
    const planets = Object.entries(horary.positions)
      .filter(([name]) => name !== 'NorthNode' && name !== 'SouthNode')
      .map(([name, position]) => ({
        key: name.toLowerCase(), name, eclipticDegrees: position.longitude,
        sign: signInfo(position.longitude).name, formatted: formatDeg(position.longitude % 30),
      }))
    const aspects = horary.aspects.filter(a => a.major && planets.some(p => p.name === a.planet1) && planets.some(p => p.name === a.planet2))
    // The fact adapter receives these exact aspects and positions, without recalculation.
    horary.aspects = aspects
    delete horary.positions.NorthNode
    delete horary.positions.SouthNode
    const angle = (longitude: number) => `${signInfo(longitude).name} ${formatDeg(longitude % 30)}`
    const summary: ChartSummary = {
      ascendant: angle(horary.houses.asc), descendant: angle(horary.houses.cusps[6]),
      midheaven: angle(horary.houses.mc), ic: angle(horary.houses.cusps[3]),
      houses, planets,
      time: { timezone, local: dt.toLocaleString('en-US', { timeZone: timezone }), utc: dt.toISOString() },
      astroChartData: {
        planets: Object.fromEntries(planets.map(p => [p.name, [roundToArcMinute(p.eclipticDegrees), horary.positions[p.name].retrograde ? -1 : 1]])),
        cusps: houses.map(h => roundToArcMinute(h.eclipticDegrees)),
        aspects: aspects.map(a => ({ point: { name: a.planet1 }, toPoint: { name: a.planet2 }, aspect: { name: a.aspectName.toLowerCase() } })),
      },
      aspectsList: aspects.map(a => ({ from: a.planet1, to: a.planet2, type: a.aspectName.toLowerCase(), orb: formatDegArcMin(a.orb), applying: a.exact ? null : a.applying })),
    }
    return { summary, horary }
  } catch (error) {
    return { error: error instanceof Error ? error.message : 'Failed to calculate chart.' }
  }
}
