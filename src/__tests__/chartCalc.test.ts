import { describe, it, expect } from 'vitest'
import {
  formatDeg,
  formatDegArcMin,
  dmsToDecimal,
  calculateChart,
} from '../chartCalc'

// --- formatDeg ---

describe('formatDeg', () => {
  it('formats whole degrees', () => {
    expect(formatDeg(30)).toBe('30°00′')
  })
  it('formats degrees with arc minutes', () => {
    expect(formatDeg(30.5)).toBe('30°30′')
  })
  it('formats zero', () => {
    expect(formatDeg(0)).toBe('0°00′')
  })
  it('pads single-digit minutes', () => {
    // 0.125 * 60 = 7.5 → floor = 7 (exact binary, no floating point drift)
    expect(formatDeg(0.125)).toBe('0°07′')
  })
  it('handles 359 degrees', () => {
    expect(formatDeg(359)).toBe('359°00′')
  })
})

// --- formatDegArcMin ---

describe('formatDegArcMin', () => {
  it('formats zero', () => {
    expect(formatDegArcMin(0)).toBe('0° 00′')
  })
  it('formats half degree', () => {
    expect(formatDegArcMin(0.5)).toBe('0° 30′')
  })
  it('formats whole degrees', () => {
    expect(formatDegArcMin(5)).toBe('5° 00′')
  })
  it('rounds arc minutes', () => {
    expect(formatDegArcMin(5.5)).toBe('5° 30′')
  })
  it('carries over when minutes round to 60', () => {
    expect(formatDegArcMin(5.9999)).toBe('6° 00′')
  })
  it('pads single-digit minutes', () => {
    expect(formatDegArcMin(1.1)).toBe('1° 06′')
  })
})

// --- dmsToDecimal ---

describe('dmsToDecimal', () => {
  it('converts north latitude', () => {
    expect(dmsToDecimal(40, 30, 0, 'N')).toBeCloseTo(40.5)
  })
  it('converts south latitude as negative', () => {
    expect(dmsToDecimal(40, 30, 0, 'S')).toBeCloseTo(-40.5)
  })
  it('converts east longitude', () => {
    expect(dmsToDecimal(74, 0, 0, 'E')).toBeCloseTo(74)
  })
  it('converts west longitude as negative', () => {
    expect(dmsToDecimal(74, 0, 0, 'W')).toBeCloseTo(-74)
  })
  it('handles zero', () => {
    expect(dmsToDecimal(0, 0, 0, 'N')).toBe(0)
  })
  it('converts seconds correctly', () => {
    expect(dmsToDecimal(0, 0, 3600, 'N')).toBeCloseTo(1)
  })
  it('converts full DMS', () => {
    expect(dmsToDecimal(51, 30, 0, 'N')).toBeCloseTo(51.5)
  })
})

// --- calculateChart ---

describe('calculateChart', () => {
  describe('input validation', () => {
    it('rejects invalid date', () => {
      const result = calculateChart(new Date('not-a-date'), 40, -74)
      expect(result.error).toBe('Invalid date/time.')
      expect(result.summary).toBeUndefined()
    })

    it('rejects latitude > 90', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), 91, 0)
      expect(result.error).toBe('Latitude must be between -90 and 90.')
    })

    it('rejects latitude < -90', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), -91, 0)
      expect(result.error).toBe('Latitude must be between -90 and 90.')
    })

    it('rejects longitude > 180', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), 0, 181)
      expect(result.error).toBe('Longitude must be between -180 and 180.')
    })

    it('rejects longitude < -180', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), 0, -181)
      expect(result.error).toBe('Longitude must be between -180 and 180.')
    })

    it('accepts boundary latitude 90', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), 90, 0)
      expect(result.error).toBeUndefined()
    })

    it('accepts boundary longitude 180', () => {
      const result = calculateChart(new Date('2024-01-01T12:00:00'), 0, 180)
      expect(result.error).toBeUndefined()
    })
  })

  describe('output structure', () => {
    const dt = new Date('2024-06-15T14:30:00')
    const lat = 40.7128
    const lon = -74.006

    it('returns a summary for valid input', () => {
      const result = calculateChart(dt, lat, lon)
      expect(result.error).toBeUndefined()
      expect(result.summary).toBeDefined()
    })

    it('returns exactly 12 houses', () => {
      const { summary } = calculateChart(dt, lat, lon)
      expect(summary!.houses).toHaveLength(12)
    })

    it('returns exactly 10 planets', () => {
      const { summary } = calculateChart(dt, lat, lon)
      expect(summary!.planets).toHaveLength(10)
    })

    it('returns all expected planet names', () => {
      const { summary } = calculateChart(dt, lat, lon)
      const names = summary!.planets.map(p => p.name)
      expect(names).toContain('Sun')
      expect(names).toContain('Moon')
      expect(names).toContain('Mercury')
      expect(names).toContain('Venus')
      expect(names).toContain('Mars')
      expect(names).toContain('Jupiter')
      expect(names).toContain('Saturn')
      expect(names).toContain('Uranus')
      expect(names).toContain('Neptune')
      expect(names).toContain('Pluto')
    })

    it('all planet ecliptic positions are 0–360', () => {
      const { summary } = calculateChart(dt, lat, lon)
      for (const p of summary!.planets) {
        expect(p.eclipticDegrees).toBeGreaterThanOrEqual(0)
        expect(p.eclipticDegrees).toBeLessThan(360)
      }
    })

    it('all house cusps are 0–360', () => {
      const { summary } = calculateChart(dt, lat, lon)
      for (const h of summary!.houses) {
        expect(h.eclipticDegrees).toBeGreaterThanOrEqual(0)
        expect(h.eclipticDegrees).toBeLessThan(360)
      }
    })

    it('house numbers run 1–12', () => {
      const { summary } = calculateChart(dt, lat, lon)
      const nums = summary!.houses.map(h => h.house)
      expect(nums).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
    })

    it('ascendant, descendant, midheaven, IC are non-empty strings', () => {
      const { summary } = calculateChart(dt, lat, lon)
      expect(summary!.ascendant).toBeTruthy()
      expect(summary!.descendant).toBeTruthy()
      expect(summary!.midheaven).toBeTruthy()
      expect(summary!.ic).toBeTruthy()
    })

    it('aspectsList is an array', () => {
      const { summary } = calculateChart(dt, lat, lon)
      expect(Array.isArray(summary!.aspectsList)).toBe(true)
    })

    it('each aspect has from, to, type, orb fields', () => {
      const { summary } = calculateChart(dt, lat, lon)
      for (const a of summary!.aspectsList) {
        expect(typeof a.from).toBe('string')
        expect(typeof a.to).toBe('string')
        expect(typeof a.type).toBe('string')
        expect(typeof a.orb).toBe('string')
      }
    })

    it('astroChartData has planets, cusps, aspects', () => {
      const { summary } = calculateChart(dt, lat, lon)
      const d = summary!.astroChartData
      expect(typeof d.planets).toBe('object')
      expect(Array.isArray(d.cusps)).toBe(true)
      expect(Array.isArray(d.aspects)).toBe(true)
    })

    it('time object has timezone, local, utc fields', () => {
      const { summary } = calculateChart(dt, lat, lon)
      expect(typeof summary!.time.timezone).toBe('string')
      expect(typeof summary!.time.local).toBe('string')
      expect(typeof summary!.time.utc).toBe('string')
    })
  })

  describe('known astronomical values', () => {
    // 2024-06-21 is the summer solstice: Sun should be in Cancer (sign 4, ~0°)
    it('Sun is in Gemini or Cancer near summer solstice 2024', () => {
      const { summary } = calculateChart(new Date('2024-06-21T12:00:00'), 51.5, -0.1)
      const sun = summary!.planets.find(p => p.name === 'Sun')!
      expect(['Gemini', 'Cancer']).toContain(sun.sign)
    })

    // Moon moves ~13°/day so its sign is less predictable, but position must be valid
    it('Moon position is a valid ecliptic degree', () => {
      const { summary } = calculateChart(new Date('2024-06-21T12:00:00'), 51.5, -0.1)
      const moon = summary!.planets.find(p => p.name === 'Moon')!
      expect(moon.eclipticDegrees).toBeGreaterThanOrEqual(0)
      expect(moon.eclipticDegrees).toBeLessThan(360)
    })

    // Aspect types must be from the known set
    it('all aspect types are recognised', () => {
      const { summary } = calculateChart(new Date('2024-06-21T12:00:00'), 40.7, -74.0)
      const validTypes = new Set(['conjunction', 'sextile', 'square', 'trine', 'opposition'])
      for (const a of summary!.aspectsList) {
        expect(validTypes.has(a.type)).toBe(true)
      }
    })

    // applying must be boolean or null
    it('applying field is boolean or null', () => {
      const { summary } = calculateChart(new Date('2024-06-21T12:00:00'), 40.7, -74.0)
      for (const a of summary!.aspectsList) {
        expect(a.applying === true || a.applying === false || a.applying === null).toBe(true)
      }
    })
  })
})
