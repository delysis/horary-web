import { describe, expect, it } from 'vitest'
import { searchLocalCities } from '../localCities'

describe('local city and postal search', () => {
  it('resolves exact US city/state/ZIP queries offline', () => {
    const [result] = searchLocalCities('Alexandria, VA 22301', 3)

    expect(result.label).toBe('Alexandria, VA 22301')
    expect(result.latitude).toBe(38.82)
    expect(result.longitude).toBe(-77.0589)
    expect(result.timezone).toBe('America/New_York')
  })

  it('resolves US city/state queries without ZIP codes', () => {
    const [result] = searchLocalCities('Alexandria VA', 3)

    expect(result.label).toBe('Alexandria, VA')
    expect(result.timezone).toBe('America/New_York')
  })

  it('keeps existing international city search working', () => {
    const [result] = searchLocalCities('London', 3)

    expect(result.label).toBe('London, GB')
  })
})
