import { createElement, act, StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from '../App'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
const granted = { microphone: 'granted', speech: 'granted', location: 'granted', voiceAvailable: true }
function mockNative(handler: (command: string, args?: any) => any) {
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const result = await handler(command, args)
    return result ?? (command === 'startup_permissions' || command === 'permission_status' ? granted : undefined)
  })
}
const empty = { messages: [], question: '', chart: null, place: null, sections: [], revisions: [], audit: [], revision: 0, status: '', busy: false }
const handlers = new Map<string, (event: any) => void>()
const dialogMethods = ['showModal', 'close'].map(key => [key, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, key)] as const)
beforeEach(() => {
  handlers.clear()
  vi.mocked(listen).mockImplementation(async (name, handler) => { handlers.set(name, handler); return () => { if (handlers.get(name) === handler) handlers.delete(name) } })
  mockNative(async command => command.startsWith('conversation_') ? empty : command === 'voice_finish' ? { id: 7, text: 'Where is my ring?' } : undefined)
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value: function(this: HTMLDialogElement) { this.setAttribute('open', ''); this.querySelector<HTMLButtonElement>('[autofocus]')?.focus() } },
    close: { configurable: true, value: function(this: HTMLDialogElement) { if (this.open) { this.removeAttribute('open'); this.dispatchEvent(new Event('close')) } } },
  })
})
afterEach(() => {
  for (const [key, descriptor] of dialogMethods) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, key, descriptor)
    else Reflect.deleteProperty(HTMLDialogElement.prototype, key)
  }
  vi.clearAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers()
})
async function render(strict = false) {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const element = document.createElement('div'); document.body.append(element)
  const root = createRoot(element)
  await act(async () => root.render(strict ? createElement(StrictMode, null, createElement(App)) : createElement(App)))
  return { element, dispose: async () => { await act(async () => root.unmount()); element.remove() } }
}
async function click(element: HTMLElement, label: string) {
  await act(async () => element.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!.click())
}
async function record(element: HTMLElement) { await click(element, 'Speak your question'); await click(element, 'Finish speaking') }
function commands() { return vi.mocked(invoke).mock.calls.map(([command]) => command) }
function generation() {
  return (vi.mocked(invoke).mock.calls.filter(([command, args]) => command === 'voice_listen' && (args as any).enabled).at(-1)![1] as any).generation as number
}
async function update(state: string, id?: number, lease = generation()) {
  await act(async () => handlers.get('horary-voice')?.({ payload: { generation: lease, state, id } }))
}

it('opens as a voice-only document without text entry or visible operating instructions', async () => {
  const { element, dispose } = await render()
  try {
    expect(element.querySelector('main')!.querySelectorAll('button,input,textarea,select,[contenteditable],header,details')).toHaveLength(0)
    expect(element.querySelector('button[aria-label="Earlier readings"]')).not.toBeNull()
    expect(element.querySelector('button[aria-label="Speak your question"]')?.textContent).toBe('?')
    expect(element.querySelector('main')!.textContent).toBe('What would you like to know?')
    expect(element.textContent).not.toMatch(/Write|Hold Space|Option–Space|Return to continue|Begin a fresh reading|Our earlier readings/)
    await click(element, 'Speak your question')
    expect(element.querySelector('[role="alert"]')?.className).toBe('visually-hidden')
    expect(element.querySelector('.listening-orb')?.getAttribute('title')).toContain('desktop app')
    expect(invoke).not.toHaveBeenCalled()
  } finally { await dispose() }
})
it('speaks the opening once in StrictMode and arms follow-up listening only after speech finishes', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let finish: () => void = () => {}
  mockNative(command => command === 'conversation_open' ? Promise.resolve(empty) : command === 'voice_speak' ? new Promise<void>(yes => { finish = yes }) : Promise.resolve(undefined))
  const { element, dispose } = await render(true)
  try {
    expect(commands().filter(c => c === 'conversation_open')).toHaveLength(1)
    expect(commands().filter(c => c === 'voice_speak')).toHaveLength(1)
    expect(vi.mocked(invoke).mock.calls.some(([c, a]) => c === 'voice_listen' && (a as any).enabled)).toBe(false)
    await act(async () => finish())
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_listen', { generation: generation(), enabled: true, followUp: true }])
    expect(element.querySelector('textarea')).toBeNull()
  } finally { await dispose() }
})
it('consumes an addressed wake receipt once, speaks the reply, and listens for the follow-up', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const answer = 'Does Bob own the books?'
  mockNative(async command => command === 'conversation_open' ? empty : command === 'conversation_voice' ? { ...empty, messages: [{ role: 'user', text: 'Will Bob sell his books?' }, { role: 'assistant', text: answer }] } : undefined)
  const { element, dispose } = await render()
  try {
    const lease = generation()
    await update('waiting'); await update('listening')
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('listening')
    await update('heard', 41, lease); await update('heard', 41, lease)
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === 'conversation_voice')).toEqual([['conversation_voice', { id: 41 }]])
    expect(commands()).not.toContain('conversation_send')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_speak', { text: answer }])
    expect(element.textContent).toContain(answer)
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_listen', { generation: generation(), enabled: true, followUp: true }])
  } finally { await dispose() }
})
it('ignores wake receipts after blur or from an obsolete listening lease', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    const old = generation()
    await act(async () => window.dispatchEvent(new Event('blur')))
    await update('heard', 50, old)
    expect(commands()).not.toContain('conversation_voice')
    await act(async () => window.dispatchEvent(new Event('focus')))
    expect(generation()).toBeGreaterThan(old)
    await update('heard', 51, old)
    expect(commands()).not.toContain('conversation_voice')
    await update('waiting')
    expect(element.querySelector('.listening-orb')?.getAttribute('title')).toContain('Oracle')
    expect(element.querySelector('main')!.textContent).not.toContain('Oracle')
  } finally { await dispose() }
})
it('pauses wake listening on Escape and does not immediately restart it', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    const lease = generation()
    await update('waiting')
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true })))
    await update('heard', 60, lease)
    expect(commands()).not.toContain('conversation_voice')
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('idle')
    const last = vi.mocked(invoke).mock.calls.filter(([c]) => c === 'voice_listen').at(-1)![1] as any
    expect(last.enabled).toBe(false)
  } finally { await dispose() }
})
it('can finish an addressed turn from the orb without starting another microphone', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    await update('listening')
    await click(element, 'Finish speaking')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_listen_finish', { generation: generation() }])
    expect(commands()).not.toContain('voice_start')
    expect(commands()).not.toContain('voice_finish')
  } finally { await dispose() }
})
it('retains the click-to-listen fallback when on-device wake listening is unavailable', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    await update('unavailable')
    await record(element)
    expect(commands()).toContain('voice_start')
    expect(commands()).toContain('voice_finish')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_voice', { id: 7 }])
    expect(commands()).not.toContain('conversation_send')
  } finally { await dispose() }
})
it('starts listening on a click and finishes on the next click, never on a Space key release', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    await click(element, 'Speak your question')
    expect(element.querySelector('.listening-orb')?.getAttribute('aria-pressed')).toBe('true')
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', code: 'Space', bubbles: true, cancelable: true })))
    expect(commands()).not.toContain('voice_finish')
    await click(element, 'Finish speaking')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_voice', { id: 7 }])
  } finally { await dispose() }
})
it('sends direct audio by its native receipt without a required transcription', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'voice_finish' ? { id: 9, text: null } : command.startsWith('conversation_') ? empty : undefined)
  const { element, dispose } = await render()
  try { await record(element); expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_voice', { id: 9 }]); expect(element.querySelector('textarea')).toBeNull() }
  finally { await dispose() }
})
it('speaks the final interpretation when the reading ends in document passages', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const answer = 'This suggests interest, but does not yet settle marriage within the year.'
  mockNative(async command => command === 'conversation_open' ? empty : command === 'voice_finish' ? { id: 10, text: null } : command === 'conversation_voice' ? { ...empty, messages: [{ role: 'user', text: 'Will I marry?' }], sections: [{ title: 'An answer taking shape', body: answer, step: 'judgment', revision: 1, evidence: [] }] } : undefined)
  const { element, dispose } = await render()
  try { await record(element); expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_speak', { text: answer }]); expect(element.textContent).toContain(answer) }
  finally { await dispose() }
})
it('never captures Space or Option–Space globally', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { dispose } = await render()
  try {
    for (const altKey of [false, true]) {
      const space = new KeyboardEvent('keydown', { key: ' ', code: 'Space', altKey, bubbles: true, cancelable: true })
      await act(async () => document.body.dispatchEvent(space))
      expect(space.defaultPrevented).toBe(false)
    }
    expect(commands()).not.toContain('voice_start')
  } finally { await dispose() }
})
it('cancels a pending microphone start without submitting audio or restarting capture', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let ready: () => void = () => {}
  mockNative(command => command === 'conversation_open' ? Promise.resolve(empty) : command === 'voice_start' ? new Promise<void>(yes => { ready = yes }) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await click(element, 'Speak your question'); await click(element, 'Cancel listening'); await act(async () => ready())
    expect(commands().filter(c => c === 'voice_start')).toHaveLength(1)
    expect(commands()).toContain('voice_cancel')
    expect(commands()).not.toContain('voice_finish')
    expect(commands()).not.toContain('conversation_voice')
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('idle')
  } finally { await dispose() }
})
it('speaks microphone failure and confines its UI explanation to the tooltip', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'conversation_open' ? Promise.resolve(empty) : command === 'voice_start' ? Promise.reject(new Error('Internal microphone details')) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await click(element, 'Speak your question')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_speak', { text: 'I can’t hear you yet.' }])
    expect(element.querySelector('[role="alert"]')?.className).toBe('visually-hidden')
    expect(element.querySelector('.listening-orb')?.getAttribute('title')).toBe('I can’t hear you yet.')
    expect(element.textContent).not.toContain('Internal microphone details')
  } finally { await dispose() }
})
it('cancels capture on blur rather than submitting an unfinished recording', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    await click(element, 'Speak your question'); await act(async () => window.dispatchEvent(new Event('blur')))
    expect(commands()).toContain('voice_cancel'); expect(commands()).not.toContain('conversation_voice')
    expect(element.querySelector('.listening-orb')?.getAttribute('aria-pressed')).toBe('false')
  } finally { await dispose() }
})
it('supplies native device coordinates and clock context before the first voice turn, once per leaf', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'get_current_location' ? { latitude: 38.657, longitude: -77.249, accuracyMeters: 800 } : command === 'voice_finish' ? { id: 7, text: 'Question' } : command.startsWith('conversation_') ? empty : undefined)
  const { element, dispose } = await render()
  try {
    expect(commands()).toContain('get_current_location')
    await record(element)
    const context = vi.mocked(invoke).mock.calls.find(([c]) => c === 'conversation_device_context')?.[1] as any
    expect(context.context).toMatchObject({ latitude: 38.657, longitude: -77.249, accuracyMeters: 800, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone })
    expect(commands().indexOf('conversation_device_context')).toBeLessThan(commands().indexOf('conversation_voice'))
    await record(element); expect(commands().filter(c => c === 'get_current_location')).toHaveLength(1)
  } finally { await dispose() }
})
it('continues the voice turn when device location permission is unavailable', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'get_current_location' ? Promise.reject(new Error('denied')) : command === 'voice_finish' ? Promise.resolve({ id: 7, text: 'Question' }) : command.startsWith('conversation_') ? Promise.resolve(empty) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await record(element)
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_device_context', { context: { timezone: Intl.DateTimeFormat().resolvedOptions().timeZone, locale: navigator.language, latitude: null, longitude: null, accuracyMeters: null } }])
    expect(commands()).toContain('conversation_voice'); expect(element.textContent).not.toContain('denied')
  } finally { await dispose() }
})
it('pauses work without accepting a late reply', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let reply: (value: unknown) => void = () => {}
  mockNative(command => command === 'conversation_open' || command === 'conversation_snapshot' ? Promise.resolve(empty) : command === 'voice_finish' ? Promise.resolve({ id: 7, text: 'Question' }) : command === 'conversation_voice' ? new Promise(yes => { reply = yes }) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await record(element); await click(element, 'Pause reading')
    expect(commands()).toContain('conversation_cancel')
    await act(async () => reply({ ...empty, messages: [{ role: 'assistant', text: 'Late reply' }] }))
    expect(element.querySelector('main')!.textContent).not.toContain('Late reply')
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('idle')
  } finally { await dispose() }
})
it('ignores stale snapshots and exposes accepted speech as read-only passages', async () => {
  vi.useFakeTimers(); vi.stubGlobal('__TAURI_INTERNALS__', {})
  let next = { ...empty, snapshotId: 10 }
  mockNative(command => command === 'conversation_snapshot' || command === 'conversation_open' ? Promise.resolve(next) : command === 'voice_finish' ? Promise.resolve({ id: 7, text: 'Question' }) : command === 'conversation_voice' ? new Promise(() => {}) : Promise.resolve(undefined))
  const { element, dispose } = await render()
  try {
    await record(element)
    next = { ...empty, snapshotId: 3, messages: [{ role: 'user', text: 'Stale question' }] } as typeof next
    await act(async () => vi.advanceTimersByTimeAsync(700)); expect(element.textContent).not.toContain('Stale question')
    next = { ...empty, snapshotId: 11, messages: [{ role: 'user', text: 'Question' }] } as typeof next
    await act(async () => vi.advanceTimersByTimeAsync(700)); expect(element.querySelector('.passage.user')?.textContent).toBe('Question')
    expect(element.querySelectorAll('textarea,[contenteditable],[role="textbox"]')).toHaveLength(0)
  } finally { await dispose() }
})
it('keeps chart and testimony inline, with earlier words read-only', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'conversation_open' ? { ...empty, messages: [{ role: 'user', text: 'My ring' }], question: 'My ring', revision: 1, chartAfterMessage: 1, chart: { timestampMs: 0, houses: Array.from({ length: 12 }, (_, i) => ({ longitude: i * 30 })), bodies: [] }, place: { label: 'London, GB', timezone: 'Europe/London' }, sections: [{ title: 'The question', body: 'An inanimate possession.', evidence: ['e0'], revision: 1, after_message: 1 }] } : undefined)
  const { element, dispose } = await render()
  try {
    expect(element.querySelector('figure')).not.toBeNull(); expect(element.querySelector('aside')).toBeNull()
    expect(element.textContent).toContain('An inanimate possession.'); expect(element.querySelector('[contenteditable]')).toBeNull()
    expect(element.querySelector('.passage.user')?.textContent).toBe('My ring')
  } finally { await dispose() }
})
it('keeps earlier readings in a closed history pane and reopens one without speaking an old greeting', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const fresh = { ...empty, readingId: 'new', snapshotId: 2, savedReadings: [{ id: 'saved-old', title: 'Old question', savedAtMs: 0 }] }
  mockNative(async command => command === 'conversation_open' ? fresh : command === 'conversation_reopen' ? { ...empty, readingId: 'old', snapshotId: 3, messages: [{ role: 'user', text: 'Old question' }] } : undefined)
  const { element, dispose } = await render()
  try {
    const pane = element.querySelector<HTMLDialogElement>('dialog')!
    expect(pane.open).toBe(false); expect(element.querySelector('main')!.textContent).not.toContain('Old question')
    await click(element, 'Earlier readings'); expect(pane.open).toBe(true); expect(document.body.style.overflow).toBe('hidden')
    await act(async () => element.querySelector<HTMLButtonElement>('.history-leaf')!.click())
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_reopen', { id: 'saved-old' }]); expect(pane.open).toBe(false)
    expect(element.querySelector('main')!.textContent).toContain('Old question'); expect(commands().filter(c => c === 'voice_speak')).toHaveLength(1)
  } finally { await dispose() }
})
it('starts a new leaf from the history icon and speaks its question', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'conversation_open' ? { ...empty, readingId: 'first', snapshotId: 3, messages: [{ role: 'user', text: 'Where is my ring?' }] } : command === 'conversation_fresh' ? { ...empty, readingId: 'second', snapshotId: 4 } : undefined)
  const { element, dispose } = await render()
  try {
    await click(element, 'Earlier readings'); await act(async () => element.querySelector<HTMLButtonElement>('.new-reading')!.click())
    expect(element.querySelector('main')!.textContent).not.toContain('Where is my ring?')
    expect(commands()).toContain('conversation_fresh'); expect(vi.mocked(invoke).mock.calls).toContainEqual(['voice_speak', { text: 'What would you like to know?' }])
  } finally { await dispose() }
})
it('uses icons and one-word labels in empty history, with descriptions only in tooltips', async () => {
  const { element, dispose } = await render()
  try {
    await click(element, 'Earlier readings')
    const pane = element.querySelector<HTMLDialogElement>('dialog')!
    expect(pane.textContent?.trim()).toBe('History'); expect(pane.querySelector('p')).toBeNull()
    expect(pane.querySelector('.new-reading')?.textContent).toBe(''); expect(pane.querySelector('.new-reading')?.getAttribute('title')).toBe('Start a new reading')
  } finally { await dispose() }
})
it('closing history restores focus and Escape there does not cancel the reading', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  const { element, dispose } = await render()
  try {
    await click(element, 'Earlier readings')
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
    expect(commands()).not.toContain('conversation_cancel')
    await act(async () => element.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true })))
    expect(element.querySelector('dialog')!.open).toBe(false); expect(document.body.style.overflow).toBe('')
    expect(document.activeElement).toBe(element.querySelector('button[aria-label="Earlier readings"]'))
  } finally { await dispose() }
})
it('lets history be viewed while working but blocks replacing the current reading', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'conversation_open' || command === 'conversation_snapshot' ? { ...empty, busy: true, savedReadings: [{ id: 'old', title: 'Saved question', savedAtMs: 0 }] } : undefined)
  const { element, dispose } = await render()
  try {
    await click(element, 'Earlier readings'); expect(element.querySelector('dialog')!.open).toBe(true)
    expect(element.querySelector<HTMLButtonElement>('.history-leaf')!.disabled).toBe(true); expect(element.querySelector<HTMLButtonElement>('.new-reading')!.disabled).toBe(true)
    await act(async () => element.querySelector<HTMLButtonElement>('.new-reading')!.click()); expect(commands()).not.toContain('conversation_fresh')
  } finally { await dispose() }
})
it('keeps detailed processing receipts out of the document and behind two closed disclosures', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'conversation_open' ? { ...empty, audit: [{ error: 'Specific failure' }], method: { records: [{ stage: 'intake', guideSha256: 'hash', validationError: 'Duplicate worksheet field', generation: { elapsedMs: 123 }, input: { question: 'A question' }, raw: 'original', worksheet: { question: 'A question' } }] } } : undefined)
  const { element, dispose } = await render()
  try {
    const outer = element.querySelector<HTMLDetailsElement>('details.reading-journal')!
    expect(outer.open).toBe(false); expect(outer.closest('dialog')?.open).toBe(false); expect(outer.querySelector('details')?.open).toBe(false)
    expect(element.querySelector('main')!.textContent).not.toMatch(/Notes|Processing|Receipts/)
    expect(outer.textContent).toContain('Output'); expect(outer.textContent).toContain('Specific failure'); expect(outer.textContent).toContain('Native validation: Duplicate worksheet field')
  } finally { await dispose() }
})
it('keeps method receipts inspectable even when no tool log exists', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(async command => command === 'conversation_open' ? { ...empty, method: { records: [{ stage: 'intake', guideSha256: 'hash', generation: { elapsedMs: 12 }, input: {}, raw: 'original output', worksheet: {} }] } } : undefined)
  const { element, dispose } = await render()
  try {
    const receipt = element.querySelector('details.reading-journal')!
    expect(receipt.textContent).toContain('original output')
    expect(receipt.closest('dialog')?.open).toBe(false)
    expect(element.querySelector('main')!.textContent).not.toContain('original output')
  } finally { await dispose() }
})

it('requests consent at startup once in StrictMode and keeps the document silent until it resolves', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let consent: (access: typeof granted) => void = () => {}
  mockNative(command => command === 'startup_permissions' ? new Promise(yes => { consent = yes }) : command === 'conversation_open' ? empty : undefined)
  const { element, dispose } = await render(true)
  try {
    expect(commands().filter(c => c === 'startup_permissions')).toHaveLength(1)
    expect(commands()).not.toContain('voice_speak')
    expect(vi.mocked(invoke).mock.calls.some(([c,a]) => c === 'voice_listen' && (a as any).enabled)).toBe(false)
    await act(async () => consent(granted))
    expect(commands().indexOf('startup_permissions')).toBeLessThan(commands().indexOf('get_current_location'))
    expect(commands()).toContain('voice_speak')
    expect(element.querySelector('textarea')).toBeNull()
  } finally { await dispose() }
})
it('keeps an explicit pause while startup consent is still pending', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let consent: (access: typeof granted) => void = () => {}
  mockNative(command => command === 'startup_permissions' ? new Promise(yes => { consent = yes }) : command === 'conversation_open' ? empty : undefined)
  const { element, dispose } = await render()
  try {
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true })))
    await act(async () => consent(granted))
    expect(commands()).not.toContain('voice_speak')
    expect(vi.mocked(invoke).mock.calls.some(([c,a]) => c === 'voice_listen' && (a as any).enabled)).toBe(false)
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('idle')
    await click(element, 'Speak your question')
    expect(commands()).toContain('voice_start')
  } finally { await dispose() }
})
it('does not show endless microphone startup when the native window is in the background', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  let active = false
  mockNative((command, args) => command === 'conversation_open' ? empty : command === 'voice_listen' && args?.enabled ? active : undefined)
  const { element, dispose } = await render()
  try {
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('idle')
    expect(vi.mocked(invoke).mock.calls.filter(([c,a]) => c === 'voice_listen' && (a as any).enabled)).toHaveLength(1)
    active = true
    await act(async () => window.dispatchEvent(new Event('focus')))
    await update('waiting')
    expect(vi.mocked(invoke).mock.calls.filter(([c,a]) => c === 'voice_listen' && (a as any).enabled)).toHaveLength(2)
    expect(element.querySelector('.listening-orb')?.getAttribute('data-state')).toBe('waiting')
  } finally { await dispose() }
})
it('offers unlabelled text entry after microphone denial and sends the words to the same reading', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'startup_permissions' ? { ...granted, microphone: 'denied', voiceAvailable: false } : command === 'conversation_open' ? { ...empty, readingId: 'leaf' } : command === 'conversation_send' ? { ...empty, readingId: 'leaf', messages: [{ role: 'assistant', text: 'Who is Bob to you?' }] } : undefined)
  const { element, dispose } = await render()
  try {
    const entry = element.querySelector<HTMLTextAreaElement>('textarea')!
    expect(entry).not.toBeNull(); expect(entry.placeholder).toBe('')
    expect(element.querySelector('form')?.textContent).toBe('')
    expect(vi.mocked(invoke).mock.calls.some(([c,a]) => c === 'voice_listen' && (a as any).enabled)).toBe(false)
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(entry, 'How many fish will Bob sell?')
      entry.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await click(element, 'Send')
    expect(vi.mocked(invoke).mock.calls).toContainEqual(['conversation_send', { text: 'How many fish will Bob sell?', readingId: 'leaf' }])
    expect(commands()).not.toContain('voice_start'); expect(entry.value).toBe('')
    expect(element.textContent).toContain('Who is Bob to you?')
    await click(element, 'Microphone access')
    expect(commands()).toContain('open_microphone_permissions')
  } finally { await dispose() }
})
it('refreshes changed microphone authorization on return and retries consent on the next launch', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'startup_permissions' ? { ...granted, microphone: 'denied', voiceAvailable: false } : command === 'conversation_open' ? empty : undefined)
  const first = await render()
  expect(first.element.querySelector('textarea')).not.toBeNull()
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(first.element.querySelector('textarea')).toBeNull()
  expect(commands()).toContain('permission_status')
  await first.dispose()
  const second = await render()
  try {
    expect(commands().filter(c => c === 'startup_permissions')).toHaveLength(2)
    expect(second.element.querySelector('textarea')).not.toBeNull()
  } finally { await second.dispose() }
})
it('acquires the device location on the next turn after location access is granted', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'startup_permissions' ? { ...granted, location: 'denied' } : command === 'conversation_open' || command === 'conversation_voice' ? { ...empty, readingId: 'leaf' } : command === 'voice_finish' ? { id: 7, text: 'Where is my ring?' } : command === 'get_current_location' ? { latitude: 38.657, longitude: -77.249, accuracyMeters: 100 } : undefined)
  const { element, dispose } = await render()
  try {
    expect(commands()).not.toContain('get_current_location')
    await act(async () => window.dispatchEvent(new Event('focus')))
    await record(element)
    expect(commands()).toContain('get_current_location')
    const context = vi.mocked(invoke).mock.calls.filter(([command]) => command === 'conversation_device_context').at(-1)![1] as any
    expect(context.readingId).toBe('leaf')
    expect(context.context.latitude).toBe(38.657)
    expect(context.context.longitude).toBe(-77.249)
    expect(commands().indexOf('get_current_location')).toBeLessThan(commands().indexOf('conversation_voice'))
  } finally { await dispose() }
})
it('does not replace voice with text when only location or speech permission is denied', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'startup_permissions' ? { ...granted, speech: 'denied', location: 'denied' } : command === 'voice_finish' ? { id: 7, text: 'Question' } : command.startsWith('conversation_') ? empty : undefined)
  const { element, dispose } = await render()
  try {
    expect(element.querySelector('textarea')).toBeNull()
    expect(commands()).not.toContain('get_current_location')
    await update('unavailable'); await record(element)
    expect(commands()).toContain('conversation_voice')
  } finally { await dispose() }
})
it('preserves typed words when sending fails', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {})
  mockNative(command => command === 'startup_permissions' ? { ...granted, voiceAvailable: false } : command === 'conversation_open' ? empty : command === 'conversation_send' ? Promise.reject(new Error('interrupted')) : undefined)
  const { element, dispose } = await render()
  try {
    const entry = element.querySelector<HTMLTextAreaElement>('textarea')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(entry, 'Where is my ring?')
      entry.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await click(element, 'Send')
    expect(entry.value).toBe('Where is my ring?')
    expect(element.querySelector('[role="alert"]')?.className).toBe('visually-hidden')
  } finally { await dispose() }
})
it('gives new and close history actions the same quiet visual treatment', async () => {
  const { element, dispose } = await render()
  try {
    await click(element, 'Earlier readings')
    expect(element.querySelectorAll('.history-action')).toHaveLength(2)
    expect(element.querySelector('.new-reading')?.classList.contains('history-action')).toBe(true)
    expect(element.querySelector('.history-close')?.classList.contains('history-action')).toBe(true)
  } finally { await dispose() }
})
