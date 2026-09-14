import { createElement, act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import App from '../App'
import { invoke } from '@tauri-apps/api/core'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const empty = { messages: [], question: '', chart: null, place: null, sections: [], revisions: [], audit: [], revision: 0, status: '', busy: false }
afterEach(() => { vi.clearAllMocks(); vi.unstubAllGlobals() })
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
    expect(element.textContent).toContain('A moment. Let me sit with this.')
    await act(async () => reject('native model internal failure'))
    expect(element.querySelector('textarea')!.value).toBe('Where is my ring?')
    expect(element.textContent).not.toContain('native model internal failure')
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
