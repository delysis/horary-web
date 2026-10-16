import cities from './data/cities.json'
import usLocations from './data/us_locations.json'

type CityRecord = {
  name: string
  country: string
  lat: number
  lng: number
  tz: string
}

type UsZipRecord = {
  zip: string
  city: string
  state: string
  lat: number
  lng: number
  tz: string
}

type UsPlaceRecord = {
  city: string
  state: string
  lat: number
  lng: number
  tz: string
  zipCount: number
}

type UsLocations = {
  zips: UsZipRecord[]
  places: UsPlaceRecord[]
}

export type LocalCity = {
  label: string
  name: string
  country: string
  latitude: number
  longitude: number
  timezone: string
  distanceKm?: number
}

const CITY_RECORDS = cities as CityRecord[]
const US_LOCATIONS = usLocations as UsLocations
const EARTH_RADIUS_KM = 6371.0088

export function searchLocalCities(query: string, limit = 8): LocalCity[] {
  const normalized = normalizeLocationText(query)
  if (normalized.length < 2) return []

  const scored: Array<{ score: number; city: LocalCity }> = [
    ...searchUsLocations(normalized),
    ...CITY_RECORDS
      .filter(city => city.name.toLowerCase().includes(normalized))
      .map(city => ({ score: 90 + cityRank(city.name, normalized), city: cityToLocalCity(city) })),
  ]

  const seen = new Set<string>()
  return scored
    .sort((a, b) => a.score - b.score || a.city.label.localeCompare(b.city.label))
    .filter(({ city }) => {
      const key = `${city.label}|${city.latitude}|${city.longitude}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    .slice(0, Math.max(1, limit))
    .map(({ city }) => city)
}

export function reverseLocalCity(latitude: number, longitude: number, maxDistanceKm = 50): LocalCity | null {
  if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) return null

  let best: { city: CityRecord; distanceKm: number } | null = null
  for (const city of CITY_RECORDS) {
    const distanceKm = distance(latitude, longitude, city.lat, city.lng)
    if (!best || distanceKm < best.distanceKm) best = { city, distanceKm }
  }

  if (!best || best.distanceKm > maxDistanceKm) return null
  return { ...cityToLocalCity(best.city), distanceKm: Math.round(best.distanceKm * 1000) / 1000 }
}

function cityRank(name: string, query: string) {
  const normalized = name.toLowerCase()
  if (normalized.startsWith(query)) return 0
  if (normalized.includes(` ${query}`)) return 1
  return 2
}

function searchUsLocations(normalizedQuery: string): Array<{ score: number; city: LocalCity }> {
  const parsed = parseUsQuery(normalizedQuery)
  if (!parsed.state && !parsed.zip) return []
  const scored: Array<{ score: number; city: LocalCity }> = []

  if (parsed.zip) {
    for (const record of US_LOCATIONS.zips) {
      if (record.zip === parsed.zip) scored.push({ score: 0, city: usZipToLocalCity(record) })
      else if (parsed.zip.length >= 3 && record.zip.startsWith(parsed.zip)) {
        scored.push({ score: 12 + record.zip.length - parsed.zip.length, city: usZipToLocalCity(record) })
      }
    }
  }

  if (parsed.cityQuery) {
    for (const place of US_LOCATIONS.places) {
      if (parsed.state && place.state !== parsed.state) continue
      const city = normalizeLocationText(place.city)
      const rank = city === parsed.cityQuery ? 20
        : city.startsWith(parsed.cityQuery) ? 30
          : city.includes(parsed.cityQuery) ? 45
            : null
      if (rank == null) continue
      scored.push({
        score: rank + (parsed.state ? 0 : 20) + Math.min(place.zipCount, 30),
        city: usPlaceToLocalCity(place),
      })
    }

    for (const record of US_LOCATIONS.zips) {
      if (parsed.state && record.state !== parsed.state) continue
      const city = normalizeLocationText(record.city)
      const rank = city === parsed.cityQuery ? 70 : city.startsWith(parsed.cityQuery) ? 80 : null
      if (rank == null) continue
      scored.push({ score: rank, city: usZipToLocalCity(record) })
    }
  }

  return scored
}

function parseUsQuery(normalizedQuery: string) {
  const zip = normalizedQuery.split(' ').find(part => /^\d{5}$/.test(part)) || ''
  let withoutZip = zip ? normalizeLocationText(normalizedQuery.replace(zip, ' ')) : normalizedQuery
  let state = ''

  for (const [abbr, name] of US_STATES) {
    const stateName = normalizeLocationText(name)
    const padded = ` ${withoutZip} `
    if (padded.includes(` ${abbr.toLowerCase()} `) || padded.includes(` ${stateName} `)) {
      state = abbr
      withoutZip = removeWord(withoutZip, abbr.toLowerCase())
      withoutZip = normalizeLocationText(withoutZip.replace(stateName, ' '))
      break
    }
  }

  return { cityQuery: withoutZip, state, zip }
}

function normalizeLocationText(value: string) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim()
    .replace(/\s+/g, ' ')
}

function removeWord(value: string, word: string) {
  return value.split(/\s+/).filter(part => part !== word).join(' ')
}

function cityToLocalCity(city: CityRecord): LocalCity {
  return {
    label: `${city.name}, ${city.country}`,
    name: city.name,
    country: city.country,
    latitude: city.lat,
    longitude: city.lng,
    timezone: city.tz,
  }
}

function usZipToLocalCity(record: UsZipRecord): LocalCity {
  return {
    label: `${record.city}, ${record.state} ${record.zip}`,
    name: record.city,
    country: 'US',
    latitude: record.lat,
    longitude: record.lng,
    timezone: record.tz,
  }
}

function usPlaceToLocalCity(place: UsPlaceRecord): LocalCity {
  return {
    label: `${place.city}, ${place.state}`,
    name: place.city,
    country: 'US',
    latitude: place.lat,
    longitude: place.lng,
    timezone: place.tz,
  }
}

function distance(lat1: number, lon1: number, lat2: number, lon2: number) {
  const dLat = toRadians(lat2 - lat1)
  const dLon = toRadians(lon2 - lon1)
  const a = Math.sin(dLat / 2) ** 2
    + Math.cos(toRadians(lat1)) * Math.cos(toRadians(lat2)) * Math.sin(dLon / 2) ** 2
  return EARTH_RADIUS_KM * 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a))
}

function toRadians(degrees: number) {
  return degrees * Math.PI / 180
}

const US_STATES: Array<[string, string]> = [
  ['AL', 'Alabama'], ['AK', 'Alaska'], ['AZ', 'Arizona'], ['AR', 'Arkansas'],
  ['CA', 'California'], ['CO', 'Colorado'], ['CT', 'Connecticut'], ['DE', 'Delaware'],
  ['DC', 'District of Columbia'], ['FL', 'Florida'], ['GA', 'Georgia'], ['HI', 'Hawaii'],
  ['ID', 'Idaho'], ['IL', 'Illinois'], ['IN', 'Indiana'], ['IA', 'Iowa'],
  ['KS', 'Kansas'], ['KY', 'Kentucky'], ['LA', 'Louisiana'], ['ME', 'Maine'],
  ['MD', 'Maryland'], ['MA', 'Massachusetts'], ['MI', 'Michigan'], ['MN', 'Minnesota'],
  ['MS', 'Mississippi'], ['MO', 'Missouri'], ['MT', 'Montana'], ['NE', 'Nebraska'],
  ['NV', 'Nevada'], ['NH', 'New Hampshire'], ['NJ', 'New Jersey'], ['NM', 'New Mexico'],
  ['NY', 'New York'], ['NC', 'North Carolina'], ['ND', 'North Dakota'], ['OH', 'Ohio'],
  ['OK', 'Oklahoma'], ['OR', 'Oregon'], ['PA', 'Pennsylvania'], ['RI', 'Rhode Island'],
  ['SC', 'South Carolina'], ['SD', 'South Dakota'], ['TN', 'Tennessee'], ['TX', 'Texas'],
  ['UT', 'Utah'], ['VT', 'Vermont'], ['VA', 'Virginia'], ['WA', 'Washington'],
  ['WV', 'West Virginia'], ['WI', 'Wisconsin'], ['WY', 'Wyoming'],
]
