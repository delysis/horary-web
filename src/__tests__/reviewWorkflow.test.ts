import { createElement, act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ReviewNotes, REVIEW_NOTES_KEY } from '../ReviewNotes'

vi.mock('../ai/aiCoreWasm', async () => {
  const { createRequire } = await import('node:module')
  return createRequire(import.meta.url)('../generated/horary_ai_core_node/horary_ai_core.js')
})

let root: Root
let container: HTMLDivElement
const context = { question: 'Where is the lost ring?', chart: { castUtcTime: '2024-06-21T12:00:00Z', houses: [1, 2] }, interpretation: null }

async function click(label: string) {
  const button = Array.from(container.querySelectorAll('button')).find(b => b.textContent === label)!
  expect(button, label).toBeDefined()
  await act(async () => button.click())
}
async function writeNote(value: string) {
  const textarea = container.querySelector('textarea')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, value)
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const values = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value) },
    clear: () => values.clear(),
  })
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})
afterEach(async () => { if (root) await act(async () => root.unmount()); container?.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals() })

describe('review notes', () => {
  it('preserves the chart captured when writing begins and survives remount', async () => {
    await act(async () => root.render(createElement(ReviewNotes, context)))
    await click('Add review note')
    await writeNote('Please explain the house assignment.')
    await act(async () => root.render(createElement(ReviewNotes, { ...context, question: 'A different question', chart: { houses: [7] } })))
    await click('Save note')
    const saved = JSON.parse(localStorage.getItem(REVIEW_NOTES_KEY)!)
    expect(saved[0].context.question).toBe(context.question)
    expect(saved[0].context.chart).toEqual(context.chart)
    expect(saved[0].note).toBe('Please explain the house assignment.')
    await act(async () => root.render(null))
    await act(async () => root.render(createElement(ReviewNotes, context)))
    expect(container.textContent).toContain('Review notes (1)')
    expect(container.textContent).toContain('Please explain the house assignment.')
  })

  it('attaches the selected method instruction and keeps an existing draft intact', async () => {
    const reviewTarget = { requestId: 1, step: { id: 'house_assignment', title: 'House Assignment', instruction: 'Select a house for each actor.' } }
    await act(async () => root.render(createElement(ReviewNotes, { ...context, reviewTarget })))
    expect(container.textContent).toContain('Method step: House Assignment')
    await writeNote('Keep this draft.')
    await act(async () => root.render(createElement(ReviewNotes, { ...context, reviewTarget: { requestId: 2, step: { ...reviewTarget.step, title: 'Another step' } } })))
    expect(container.querySelector('textarea')!.value).toBe('Keep this draft.')
    await click('Save note')
    expect(JSON.parse(localStorage.getItem(REVIEW_NOTES_KEY)!)[0].context.methodStep).toEqual(reviewTarget.step)
  })

  it('reports storage failure without pretending that a note was saved', async () => {
    await act(async () => root.render(createElement(ReviewNotes, context)))
    await click('Add review note'); await writeNote('Do not lose me.')
    vi.spyOn(localStorage, 'setItem').mockImplementation(() => { throw new DOMException('Quota', 'QuotaExceededError') })
    await click('Save note')
    expect(container.querySelector('[role="alert"]')!.textContent).toContain('Could not save')
    expect(container.querySelector('textarea')!.value).toBe('Do not lose me.')
    expect(localStorage.getItem(REVIEW_NOTES_KEY)).toBeNull()
  })

  it('does not overwrite corrupt saved notes', async () => {
    localStorage.setItem(REVIEW_NOTES_KEY, '[{"unrecognized":"keep"}]')
    await act(async () => root.render(createElement(ReviewNotes, context)))
    expect(container.querySelector('[role="alert"]')!.textContent).toContain('preserved')
    await click('Add review note'); await writeNote('A new note'); await click('Save note')
    expect(localStorage.getItem(REVIEW_NOTES_KEY)).toBe('[{"unrecognized":"keep"}]')
  })
})
