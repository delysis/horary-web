import { createElement, act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import App from '../App'

vi.mock('../ai/aiCoreWasm', async () => {
  const { createRequire } = await import('node:module')
  const core = createRequire(import.meta.url)('../generated/horary_ai_core_node/horary_ai_core.js')
  return {
    ...core,
    resolveChartTime: (local: string, timezone: string, occurrence: string) => new Date(core.resolve_chart_time(local, timezone, occurrence)),
    validateDms: (d: string, m: string, s: string, sign: string) => core.validate_dms(d.trim() ? Number(d) : NaN, Number(m), Number(s), sign),
    applyBookMethod: (chart: unknown) => JSON.parse(core.apply_book_method_json(JSON.stringify(chart))),
  }
})

it('settings toggle closes reliably and Escape returns focus to its opener', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false }))
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  try {
    await act(async () => root.render(createElement(App)))
    const manual = Array.from(container.querySelectorAll('button')).find(b => b.textContent === 'Enter coordinates manually')!
    await act(async () => manual.click())
    const search = Array.from(container.querySelectorAll('button')).find(b => b.textContent === 'Search for a city')!
    expect(search).toBeDefined()
    await act(async () => search.click())
    expect(container.querySelector('input[placeholder="Search for a city or place"]')).not.toBeNull()
    const opener = container.querySelector<HTMLButtonElement>('[aria-controls="chart-inspector"]')!
    await act(async () => opener.click())
    expect(container.querySelector('#chart-inspector')).not.toBeNull()
    await act(async () => opener.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })))
    await act(async () => opener.click())
    expect(container.querySelector('#chart-inspector')).toBeNull()
    await act(async () => opener.click())
    await act(async () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(container.querySelector('#chart-inspector')).toBeNull()
    expect(document.activeElement).toBe(opener)
    const cast = Array.from(container.querySelectorAll('button')).find(b => b.textContent === 'Cast chart')!
    await act(async () => cast.click())
    expect(container.textContent).toContain('Set the location before casting')
    const input = container.querySelector<HTMLInputElement>('input[placeholder="Search for a city or place"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'London')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => Array.from(container.querySelectorAll('button')).find(b => b.textContent === 'Search')!.click())
    await act(async () => Array.from(container.querySelectorAll('button')).find(b => b.textContent === 'London, GB')!.click())
    expect(container.textContent).not.toContain('Set the location before casting')
    expect(container.querySelector('[aria-label="Horary chart"]')).not.toBeNull()
  } finally {
    await act(async () => root.unmount())
    container.remove()
    vi.unstubAllGlobals()
  }
})
