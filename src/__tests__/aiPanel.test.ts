import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { AiPanel } from '../AiPanel'

const engine = vi.hoisted(() => ({ generate: vi.fn(), cancel: vi.fn(), dispose: vi.fn(), status: () => ({ running: false, runtime: 'web' }), useModel: vi.fn() }))
vi.mock('../ai/judgementEngine.ts', () => ({ createJudgementEngine: () => engine }))

it('cancels on a changed chart and never shows its late result under the new chart', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  let complete!: (result: unknown) => void
  engine.generate.mockImplementation(() => new Promise(resolve => { complete = resolve }))
  const container = document.createElement('div')
  const root = createRoot(container)
  const props = { requestId: 0, question: 'Where is the ring?', chartFacts: { id: 1 }, darkMode: false }
  try {
    await act(async () => root.render(createElement(AiPanel, props)))
    await act(async () => root.render(createElement(AiPanel, { ...props, requestId: 1 })))
    expect(engine.generate).toHaveBeenCalledOnce()
    await act(async () => root.render(createElement(AiPanel, { ...props, requestId: 1, chartFacts: { id: 2 } })))
    expect(engine.cancel).toHaveBeenCalled()
    await act(async () => complete({ summary: 'Obsolete reading', judgementTrace: [], keyFactors: [], cautions: [], followUpQuestions: [] }))
    expect(container.textContent).not.toContain('Obsolete reading')
    expect(container.textContent).not.toContain('Preparing a local reading')
  } finally { await act(async () => root.unmount()) }
  expect(engine.dispose).toHaveBeenCalledOnce()
})
