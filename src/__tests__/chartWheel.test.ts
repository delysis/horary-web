import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { expect, it } from 'vitest'
import { ChartWheel } from '../ChartWheel'

it('scales the complete chart coordinate system at narrow widths', () => {
  const markup = renderToStaticMarkup(createElement(ChartWheel, {
    data: { cusps: Array.from({ length: 12 }, (_, i) => i * 30), planets: { Sun: [15] }, aspects: [] },
  }))
  const container = document.createElement('div')
  container.innerHTML = markup
  const svg = container.querySelector('svg')!
  expect(svg.getAttribute('viewBox')).toBe('0 0 520 520')
  expect(svg.style.maxWidth).toBe('100%')
  expect(svg.getAttribute('aria-label')).toContain('ask about any position')
})
