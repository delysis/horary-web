import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import tzLookup from 'tz-lookup'

const inputPath = process.argv[2]
const outputPath = process.argv[3] || path.join('src', 'data', 'us_locations.json')

if (!inputPath) {
  console.error('Usage: node scripts/generate-us-geodata.mjs /path/to/US.txt [src/data/us_locations.json]')
  process.exit(1)
}

const text = fs.readFileSync(inputPath, 'utf8')
const zips = []
const placeGroups = new Map()

for (const line of text.split(/\r?\n/)) {
  if (!line.trim()) continue
  const columns = line.split('\t')
  const [
    country,
    zip,
    city,
    stateName,
    state,
    county,
    ,
    ,
    ,
    latRaw,
    lngRaw,
  ] = columns

  if (country !== 'US' || !zip || !city || !state || !latRaw || !lngRaw) continue
  const lat = Number(latRaw)
  const lng = Number(lngRaw)
  if (!Number.isFinite(lat) || !Number.isFinite(lng)) continue

  let tz = ''
  try {
    tz = tzLookup(lat, lng)
  } catch {
    tz = ''
  }

  const record = {
    zip,
    city,
    state,
    stateName,
    county,
    lat: roundCoord(lat),
    lng: roundCoord(lng),
    tz,
  }
  zips.push(record)

  const groupKey = `${city.toLowerCase()}|${state}`
  const group = placeGroups.get(groupKey) || {
    city,
    state,
    stateName,
    latSum: 0,
    lngSum: 0,
    count: 0,
    timezoneCounts: new Map(),
  }
  group.latSum += lat
  group.lngSum += lng
  group.count += 1
  if (tz) group.timezoneCounts.set(tz, (group.timezoneCounts.get(tz) || 0) + 1)
  placeGroups.set(groupKey, group)
}

zips.sort((a, b) => a.zip.localeCompare(b.zip))

const places = [...placeGroups.values()]
  .map(group => ({
    city: group.city,
    state: group.state,
    stateName: group.stateName,
    lat: roundCoord(group.latSum / group.count),
    lng: roundCoord(group.lngSum / group.count),
    tz: mostCommonTimezone(group.timezoneCounts),
    zipCount: group.count,
  }))
  .sort((a, b) => a.state.localeCompare(b.state) || a.city.localeCompare(b.city))

const payload = {
  source: {
    name: 'GeoNames US postal codes',
    url: 'https://download.geonames.org/export/zip/US.zip',
    license: 'Creative Commons Attribution 4.0',
    generatedAt: new Date().toISOString(),
  },
  zips,
  places,
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true })
fs.writeFileSync(outputPath, `${JSON.stringify(payload)}\n`)
console.log(`Wrote ${zips.length} ZIP records and ${places.length} place records to ${outputPath}`)

function roundCoord(value) {
  return Math.round(value * 1000000) / 1000000
}

function mostCommonTimezone(counts) {
  let best = ''
  let bestCount = -1
  for (const [timezone, count] of counts) {
    if (count > bestCount || (count === bestCount && timezone < best)) {
      best = timezone
      bestCount = count
    }
  }
  return best
}
