import type { ChartSummary } from './chartCalc'

const SIGN_RULERS: Record<string, string> = {
  aries: 'Mars',
  taurus: 'Venus',
  gemini: 'Mercury',
  cancer: 'Moon',
  leo: 'Sun',
  virgo: 'Mercury',
  libra: 'Venus',
  scorpio: 'Mars',
  sagittarius: 'Jupiter',
  capricorn: 'Saturn',
  aquarius: 'Saturn',
  pisces: 'Jupiter',
}

const MAJOR_ASPECTS = new Set(['conjunction', 'sextile', 'square', 'trine', 'opposition'])

type BuildFactsInput = {
  dt: Date
  lat: number
  lon: number
  timezone: string
  locationLabel: string
}

export function buildAiChartFacts(summary: ChartSummary, input: BuildFactsInput) {
  const cusps = summary.houses.map(house => house.eclipticDegrees)

  return {
    castLocalTime: input.dt.toLocaleString([], {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }),
    castUtcTime: input.dt.toISOString(),
    timezone: input.timezone,
    location: {
      label: input.locationLabel,
      latitude: round(input.lat, 6),
      longitude: round(input.lon, 6),
    },
    ascendant: angleFact(summary.houses[0]),
    midheaven: angleFact(summary.houses[9]) ?? textAngleFact(summary.midheaven),
    houses: summary.houses.map(house => ({
      number: house.house,
      sign: house.sign,
      degree: round(degreeWithinSign(house.eclipticDegrees), 4),
      absoluteLongitude: round(norm360(house.eclipticDegrees), 4),
      ruler: signRuler(house.sign),
    })),
    bodies: summary.planets.map(planet => ({
      name: planet.name,
      sign: planet.sign,
      degree: round(degreeWithinSign(planet.eclipticDegrees), 4),
      absoluteLongitude: round(norm360(planet.eclipticDegrees), 4),
      house: getHouseNumber(planet.eclipticDegrees, cusps),
      retrograde: summary.astroChartData.planets[planet.name]?.[1] === -1,
      speed: null,
      dignity: null,
      accidentalDignity: null,
    })),
    aspects: summary.aspectsList
      .filter(aspect => MAJOR_ASPECTS.has(aspect.type.toLowerCase()))
      .map(aspect => ({
        planet1: aspect.from,
        aspectName: titleCase(aspect.type),
        planet2: aspect.to,
        orb: aspect.orb,
        applying: aspect.applying === true,
        separating: aspect.applying === false,
        exact: false,
        major: true,
      })),
    derived: {
      zodiac: 'tropical',
      houseSystem: 'regiomontanus',
      chartSect: null,
      ascendantRuler: signRuler(summary.houses[0]?.sign),
      partOfFortune: null,
      accidentalDignities: {},
      receptions: [],
      voidOfCourseMoon: null,
      antisciaContacts: [],
      solarConditions: [],
      planetaryHour: null,
      timingPatterns: [],
    },
  }
}

function angleFact(house: ChartSummary['houses'][number] | undefined) {
  if (!house) return null
  return {
    sign: house.sign,
    degree: round(degreeWithinSign(house.eclipticDegrees), 4),
    absoluteLongitude: round(norm360(house.eclipticDegrees), 4),
  }
}

function textAngleFact(text: string) {
  return text ? { label: text } : null
}

function signRuler(sign: string | undefined) {
  return SIGN_RULERS[String(sign || '').toLowerCase()] || 'unknown'
}

function getHouseNumber(longitude: number, cusps: number[]) {
  const lon = norm360(longitude)
  for (let index = 0; index < 12; index += 1) {
    const start = norm360(cusps[index])
    const end = norm360(cusps[(index + 1) % 12])
    const distanceFromStart = norm360(lon - start)
    const houseSpan = norm360(end - start)
    if (houseSpan > 0.001 && distanceFromStart < houseSpan) return index + 1
  }
  return 1
}

function degreeWithinSign(longitude: number) {
  return norm360(longitude) % 30
}

function norm360(value: number) {
  return ((value % 360) + 360) % 360
}

function round(value: number, decimals: number) {
  const factor = 10 ** decimals
  return Math.round(value * factor) / factor
}

function titleCase(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1).toLowerCase()
}
