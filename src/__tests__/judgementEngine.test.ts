import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { HoraryInterpretation } from '../tauriBridge'

const prompt = {
  promptVersion: 'horary-interpretation-v4',
  schemaVersion: '2026-07-03',
  traditionProfile: 'traditional-horary-textbook-v1',
  risk: 'ordinary',
  responseFormat: { type: 'json_schema', json_schema: { name: 'horary_interpretation' } },
  messages: [
    { role: 'system', content: 'Use supplied deterministic chart facts.' },
    { role: 'user', content: '{"question":"Where should we sit?"}' },
  ],
}

const interpretation: HoraryInterpretation = {
  summary: 'The chart gives a usable symbolic answer.',
  directAnswer: 'Sit where the route is easiest and least obstructed.',
  confidence: 'medium',
  judgementTrace: [{
    stepId: 'synthesis_pass',
    finding: 'The supplied testimonies are constructive.',
    chartEvidence: 'Moon trine Venus, 2.1 degree applying orb',
    confidence: 'medium',
  }],
  keyFactors: [{
    factor: 'Moon applying to Venus',
    chartEvidence: 'Moon trine Venus, 2.1 degree applying orb',
    interpretation: 'The matter has cooperation.',
  }],
  cautions: [],
  followUpQuestions: [],
}

const buildInterpretationPrompt = vi.fn()
const validateInterpretationContent = vi.fn()
const createConversation = vi.fn()
const cancel = vi.fn()
const engineCreate = vi.fn()

vi.mock('../ai/aiCoreWasm.ts', () => ({
  buildInterpretationPrompt,
  ensureAiCoreLoaded: vi.fn().mockResolvedValue(undefined),
  validateInterpretationContent,
}))

vi.mock('@litert-lm/core', () => ({
  getOrLoadGlobalLiteRtLm: vi.fn().mockResolvedValue(undefined),
  Engine: {
    create: engineCreate,
  },
}))

describe('web judgement engine', () => {
  beforeEach(() => {
    vi.resetModules()
    buildInterpretationPrompt.mockReset()
    validateInterpretationContent.mockReset()
    createConversation.mockReset()
    cancel.mockReset()
    engineCreate.mockReset()
    buildInterpretationPrompt.mockResolvedValue(prompt)
    validateInterpretationContent.mockResolvedValue(interpretation)
    Object.defineProperty(navigator, 'gpu', {
      configurable: true,
      value: {},
    })
  })

  it('uses LiteRT streaming and Rust WASM prompt/validation on the web path', async () => {
    const chunks: string[] = []
    createConversation.mockResolvedValue({
      cancel,
      delete: vi.fn(),
      sendMessageStreaming: () => new ReadableStream({
        start(controller) {
          controller.enqueue({ content: [{ type: 'text', text: '{"summary":' }] })
          controller.enqueue({ content: [{ type: 'text', text: '"ok"}' }] })
          controller.close()
        },
      }),
    })
    engineCreate.mockResolvedValue({ createConversation })
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    const engine = createJudgementEngine()
    await engine.useModel(new File(['model'], 'test.litertlm'))

    const result = await engine.generate({
      question: 'Where should we sit?',
      chart: { houses: [], bodies: [], aspects: [] },
      settings: { tradition: 'traditional' },
    }, {
      token: token => chunks.push(token),
    })

    expect(engineCreate).toHaveBeenCalledWith({
      model: expect.any(File),
      mainExecutorSettings: { maxNumTokens: 16384 },
    })
    expect(createConversation).toHaveBeenCalledWith({
      preface: {
        messages: [{
          role: 'system',
          content: expect.stringContaining('Response format:'),
        }],
      },
    })
    expect(validateInterpretationContent).toHaveBeenCalledWith('{"summary":"ok"}', 'ordinary')
    expect(chunks).toEqual(['{"summary":', '"ok"}'])
    expect(result).toEqual(interpretation)
  })

  it('fails clearly when WebGPU is unavailable', async () => {
    Object.defineProperty(navigator, 'gpu', {
      configurable: true,
      value: undefined,
    })
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    const engine = createJudgementEngine()
    await engine.useModel(new File(['model'], 'test.litertlm'))

    await expect(engine.prepare()).rejects.toThrow(/WebGPU/)
    expect(engineCreate).not.toHaveBeenCalled()
  })

  it('does not generate when cancelled while the model is loading', async () => {
    let loaded!: (value: unknown) => void
    engineCreate.mockImplementation(() => new Promise(resolve => { loaded = resolve }))
    const { createJudgementEngine } = await import('../ai/judgementEngine')
    const engine = createJudgementEngine()
    await engine.useModel(new File(['model'], 'test.litertlm'))
    const pending = engine.generate({ question: 'Will it proceed?', chart: {} })
    const assertion = expect(pending).rejects.toThrow(/cancelled/i)
    await vi.waitFor(() => expect(engineCreate).toHaveBeenCalled())
    engine.cancel()
    loaded({ createConversation })
    await assertion
    expect(createConversation).not.toHaveBeenCalled()
    expect(engine.status().running).toBe(false)
  })
})
