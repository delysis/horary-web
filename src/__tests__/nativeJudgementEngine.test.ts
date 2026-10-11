import { beforeEach, describe, expect, it, vi } from 'vitest'

const bridge = vi.hoisted(() => ({
  listenAiInterpretationEvents: vi.fn(), startInterpretationStream: vi.fn(),
  getModelStatus: vi.fn(), listModels: vi.fn(), startLlama: vi.fn(),
  cancelInterpretationStream: vi.fn(), importModel: vi.fn(),
}))
vi.mock('../tauriBridge.ts', () => ({ ...bridge, RECOMMENDED_MODEL_ID: "gemma-4-12b-qat", isTauriRuntime: () => true }))
vi.mock('../ai/aiCoreWasm.ts', () => ({ buildInterpretationPrompt: vi.fn(), validateInterpretationContent: vi.fn() }))

describe('native judgement lifecycle', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    bridge.getModelStatus.mockResolvedValue({ running: true })
    bridge.listModels.mockResolvedValue([])
    bridge.cancelInterpretationStream.mockResolvedValue(true)
  })

  it('receives completion even before the start command returns its generation id', async () => {
    const unlisten = vi.fn()
    let handlers: { complete: (payload: unknown) => void }
    bridge.listenAiInterpretationEvents.mockImplementation(async value => { handlers = value; return unlisten })
    const result = { summary: 'Completed before the IPC reply' }
    bridge.startInterpretationStream.mockImplementation(async () => {
      handlers.complete({ generationId: 'early', interpretation: result })
      return { generationId: 'early' }
    })
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    const engine = createJudgementEngine()
    const pending = engine.generate({ question: 'Will it proceed?', chart: {} })
    expect(await Promise.race([pending, new Promise(resolve => setTimeout(() => resolve('lost completion'), 100))])).toEqual(result)
    expect(unlisten).toHaveBeenCalledOnce()
    expect(engine.status().running).toBe(false)
  })

  it('prefers the installed recommended pair over an older running model', async () => {
    bridge.getModelStatus.mockResolvedValue({ running: true, modelId: 'old-model' })
    bridge.listModels.mockResolvedValue([{ id: 'old-model' }, { id: 'gemma-4-12b-qat' }])
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    await createJudgementEngine().prepare()
    expect(bridge.startLlama).toHaveBeenCalledWith(expect.objectContaining({ modelId: 'gemma-4-12b-qat' }))
  })

  it('selects the recommended model after setup even when a custom file was selected earlier', async () => {
    bridge.importModel.mockResolvedValue({ id: 'custom' })
    bridge.getModelStatus.mockResolvedValue({ running: true, modelId: 'custom' })
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    const engine = createJudgementEngine()
    await engine.useModel('/custom.gguf')
    engine.useInstalledModel('gemma-4-12b-qat')
    await engine.prepare()
    expect(bridge.startLlama).toHaveBeenCalledWith(expect.objectContaining({ modelId: 'gemma-4-12b-qat' }))
  })
})
