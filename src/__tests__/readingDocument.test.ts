import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it } from 'vitest'
import { ReadingChart, ReadingPassage } from '../ReadingDocument'

it('puts the proposed answer in the document while keeping raw chart facts in its margin', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const element = document.createElement('div'); document.body.append(element)
  const root = createRoot(element)
  try {
    await act(async () => root.render(createElement(ReadingPassage, { section: {
      title: 'An answer taking shape', body: 'There is interest, but the chart does not yet establish commitment within the next year.', revision: 1, evidence: ['e1'], step: 'judgment', worksheet: { checks: { scope_of_answer: { state: 'unestablished', finding: 'The search covers only seven days.' } } },
      facts: [{ id: 'e1', kind: 'reception', label: 'Mars → Venus', detail: 'Raw calculated reception', planets: ['Mars', 'Venus'] }],
    } })))
    const section = element.querySelector('section')!
    expect(Array.from(section.children).filter(e => e.tagName !== 'DETAILS').map(e => e.textContent).join(' ')).toContain('commitment within the next year')
    expect(section.querySelector('.calculated-testimony')).toBeNull()
    expect(section.querySelector('details')?.textContent).toContain('Raw calculated reception')
    expect(section.querySelector('details')?.textContent).toContain('The search covers only seven days.')
  } finally { await act(async () => root.unmount()); element.remove() }
})

it('lets a planet reveal its calculated facts without a model call', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const element = document.createElement('div'); document.body.append(element)
  const root = createRoot(element)
  try {
    await act(async () => root.render(createElement(ReadingChart, {
      chart: { timestampMs: 1789387200000, houses: Array.from({ length: 12 }, (_, i) => ({ longitude: i * 30 })), bodies: [{ name: 'Jupiter', longitude: 136.116, retrograde: false }] },
      place: { label: 'London', timezone: 'Europe/London' }, dark: false,
      facts: [{ id: 'e0', kind: 'position', label: 'Jupiter', detail: 'Jupiter at 16.116° Leo, house 9.', planets: ['Jupiter'] }],
    })))
    const planet = element.querySelector<SVGGElement>('[aria-label^="Follow Jupiter"]')!
    expect(planet).not.toBeNull()
    await act(async () => planet.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    expect(element.querySelector('aside')?.textContent).toContain('Jupiter at 16.116° Leo, house 9.')
  } finally { await act(async () => root.unmount()); element.remove() }
})

it('keeps calculation, source rule and inference distinct in a passage margin', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const element = document.createElement('div'); document.body.append(element)
  const root = createRoot(element)
  try {
    await act(async () => root.render(createElement(ReadingPassage, { section: {
      title: 'The people', body: 'The first house represents you.', revision: 1, evidence: ['e7'], step: 'significators',
      facts: [{ id: 'e7', kind: 'house', label: 'House 1', detail: 'The first cusp is in Leo; Sun is its ruler.', planets: ['Sun'] }],
      rules: [{ id: 'significators', title: 'Who stands for whom', explanation: 'The cusp’s traditional ruler signifies the house.', pages: '15–38' }],
      because: 'The first house belongs to the person asking.', roles: [{ label: 'You', house: 1, planet: 'Sun', reason: 'You are asking about yourself.' }],
    } })))
    expect(element.querySelector('details')?.open).toBe(false)
    expect(element.textContent).toContain('In the chart')
    expect(element.textContent).toContain('printed pp. 15–38 · editorial paraphrase')
    expect(element.textContent).toContain('they do not prove it correct')
  } finally { await act(async () => root.unmount()); element.remove() }
})
