import { createElement, act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import App from '../App'
import { invoke } from '@tauri-apps/api/core'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const empty = { messages: [], question: '', chart: null, place: null, sections: [], revisions: [], audit: [], revision: 0, status: '', busy: false }
afterEach(() => { vi.clearAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers() })
async function render() {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const element = document.createElement('div'); document.body.append(element)
  const root = createRoot(element)
  await act(async () => root.render(createElement(App)))
  return { element, dispose: async () => { await act(async () => root.unmount()); element.remove() } }
}
async function type(element: HTMLElement, text: string) {
  await act(async () => {
    const input = element.querySelector('textarea')!
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, text)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
async function submit(element: HTMLElement) {
  await act(async () => element.querySelector('textarea')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
}
it('opens as one unbranded document without buttons, settings or chart forms', async () => {
  const { element, dispose } = await render()
  try {
    expect(element.querySelectorAll('button,input,select,header')).toHaveLength(0)
    expect(element.querySelectorAll('textarea')).toHaveLength(1)
    expect(element.textContent).not.toMatch(/Settings|Cast chart|Hugging|Gemma|GB|model|horary/i)
    expect(element.textContent).toContain('What would you like to know?')
    await type(element, 'Where is my ring?'); await submit(element)
    expect(element.textContent).toContain('This page is a sketch')
    expect(invoke).not.toHaveBeenCalled()
    expect(element.querySelector('textarea')!.value).toBe('Where is my ring?')
  } finally { await dispose() }
})
it('keeps unsent words after delivery failure and prevents duplicate submissions', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let reject: (reason: string) => void = () => {}
  vi.mocked(invoke).mockImplementation(command => {
    if (command === 'conversation_snapshot') return Promise.resolve(empty)
    if (command === 'conversation_send') return new Promise((_, no) => { reject = no })
    return Promise.resolve(undefined)
  })
  const { element, dispose } = await render()
  try {
    await type(element, 'Where is my ring?'); await submit(element); await submit(element)
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === 'conversation_send')).toHaveLength(1)
    expect(element.textContent).toContain('A little quiet, while the thread finds its way.')
    await act(async () => reject('native model internal failure'))
    expect(element.querySelector('textarea')!.value).toBe('Where is my ring?')
    expect(element.textContent).not.toContain('native model internal failure')
  } finally { await dispose() }
})

it('starts voice from Space anywhere on the page without clicking the invitation', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockImplementation(command => command === 'conversation_snapshot' || command === 'conversation_voice' ? Promise.resolve(empty) : command === 'voice_finish' ? Promise.resolve({ id: 7, text: 'Where is my ring?' }) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', code: 'Space', bubbles: true, cancelable: true })))
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'voice_start')).toBe(true)
    expect(element.textContent).toContain('I’m listening…')
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', code: 'Space', bubbles: true, cancelable: true })))
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'voice_finish')).toBe(true)
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_voice', { id: 7 }])
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'conversation_send')).toBe(false)
  } finally { await dispose() }
})

it('sends direct audio by its native receipt without a required transcription', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockImplementation(command => command === 'conversation_snapshot' || command === 'conversation_voice' ? Promise.resolve(empty) : command === 'voice_finish' ? Promise.resolve({ id: 9, text: null }) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await type(element, 'A thought for later')
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', code: 'Space', bubbles: true, cancelable: true })))
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', code: 'Space', bubbles: true, cancelable: true })))
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_voice', { id: 9 }])
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'conversation_send')).toBe(false)
    expect(element.querySelector('textarea')!.value).toBe('A thought for later')
  } finally { await dispose() }
})

it('preserves ordinary spaces in writing and supports Option–Space there', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockImplementation(command => command === 'conversation_snapshot' ? Promise.resolve(empty) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    const input = element.querySelector('textarea')!
    const space = new KeyboardEvent('keydown', { key: ' ', code: 'Space', bubbles: true, cancelable: true })
    await act(async () => input.dispatchEvent(space))
    expect(space.defaultPrevented).toBe(false)
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'voice_start')).toBe(false)
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', code: 'Space', altKey: true, bubbles: true, cancelable: true })))
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'voice_start')).toBe(true)
    await act(async () => window.dispatchEvent(new Event('blur')))
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'voice_cancel')).toBe(true)
  } finally { await dispose() }
})

it('supplies native device coordinates and clock context before the first question, once', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockImplementation(command => command === 'get_current_location' ? Promise.resolve({ latitude: 38.657, longitude: -77.249, accuracyMeters: 800 }) : command === 'conversation_snapshot' || command === 'conversation_send' ? Promise.resolve(empty) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'get_current_location')).toBe(false)
    await type(element, 'Will I get married?'); await submit(element)
    const context = vi.mocked(invoke).mock.calls.find(([c]) => c === 'conversation_device_context')?.[1] as { context: Record<string, unknown> }
    expect(context.context).toMatchObject({ latitude: 38.657, longitude: -77.249, accuracyMeters: 800, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone })
    const commands = vi.mocked(invoke).mock.calls.map(([c]) => c)
    expect(commands.indexOf('conversation_device_context')).toBeLessThan(commands.indexOf('conversation_send'))
    await type(element, 'A future partner'); await submit(element)
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === 'get_current_location')).toHaveLength(1)
  } finally { await dispose() }
})

it('continues the question when device location permission is unavailable', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockImplementation(command => command === 'get_current_location' ? Promise.reject(new Error('denied')) : command === 'conversation_snapshot' || command === 'conversation_send' ? Promise.resolve(empty) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await type(element, 'Will I get married?'); await submit(element)
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_device_context', { context: { timezone: Intl.DateTimeFormat().resolvedOptions().timeZone, locale: navigator.language, latitude: null, longitude: null, accuracyMeters: null } }])
    expect(vi.mocked(invoke).mock.calls.some(([c]) => c === 'conversation_send')).toBe(true)
    expect(element.textContent).not.toContain('denied')
  } finally { await dispose() }
})

it('ignores stale snapshots before acknowledging or erasing submitted words', async () => {
  vi.useFakeTimers()
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let next = { ...empty, snapshotId: 10 }
  vi.mocked(invoke).mockImplementation(command => command === 'conversation_snapshot' ? Promise.resolve(next) : command === 'conversation_send' ? new Promise(() => {}) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await type(element, 'Question'); await submit(element)
    next = { ...empty, snapshotId: 3, messages: [{ role: 'user', text: 'Question' }] } as typeof next
    await act(async () => vi.advanceTimersByTimeAsync(700))
    expect(element.querySelector('textarea')!.value).toBe('Question')
    next = { ...next, snapshotId: 11 }
    await act(async () => vi.advanceTimersByTimeAsync(700))
    expect(element.querySelector('textarea')!.value).toBe('')
  } finally { await dispose() }
})
it('places chart and testimony inline and turns an edited passage into an explicit correction', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  vi.mocked(invoke).mockResolvedValue({ ...empty, messages: [{ role: 'user', text: 'My ring' }], question: 'My ring', revision: 1, chartAfterMessage: 1,
    chart: { timestampMs: 0, houses: Array.from({ length: 12 }, (_, i) => ({ longitude: i * 30 })), bodies: [] },
    place: { label: 'London, GB', timezone: 'Europe/London' },
    sections: [{ title: 'The question', body: 'An inanimate possession.', evidence: ['e0'], revision: 1, after_message: 1 }],
  })
  const { element, dispose } = await render()
  try {
    expect(element.querySelector('figure')).not.toBeNull()
    expect(element.querySelector('aside')).toBeNull()
    expect(element.textContent).toContain('An inanimate possession.')
    const passage = element.querySelector<HTMLElement>('[contenteditable=true]')!
    await act(async () => { passage.textContent = 'My sister’s ring'; passage.dispatchEvent(new FocusEvent('focusout', { bubbles: true })) })
    expect(element.querySelector('textarea')!.value).toBe('A correction to “My ring”: My sister’s ring')
    expect(passage.textContent).toBe('My ring')
  } finally { await dispose() }
})
it('does not erase a new thought typed while the previous reply is pending', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let resolve: (value: unknown) => void = () => {}
  vi.mocked(invoke).mockImplementation(command => command === 'conversation_snapshot' ? Promise.resolve(empty) : command === 'conversation_send' ? new Promise(yes => { resolve = yes }) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await type(element, 'First question'); await submit(element)
    await type(element, 'Another detail I remembered')
    await act(async () => resolve({ ...empty, messages: [{ role: 'assistant', text: 'Tell me more.' }] }))
    expect(element.querySelector('textarea')!.value).toBe('Another detail I remembered')
  } finally { await dispose() }
})
