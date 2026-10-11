import {
  cancelInterpretationStream, getModelStatus, importModel, isTauriRuntime,
  listenAiInterpretationEvents, listModels, startInterpretationStream, startLlama,
  RECOMMENDED_MODEL_ID,
  type HoraryInterpretation, type InterpretationRequest,
} from '../tauriBridge.ts'
import { buildInterpretationPrompt, ensureAiCoreLoaded, validateInterpretationContent } from './aiCoreWasm.ts'
import type { Engine, Conversation } from '@litert-lm/core'

export type StreamCallbacks = { token?: (text: string) => void }
export type JudgementEngineStatus = { prepared: boolean; running: boolean; runtime: 'tauri' | 'web' }
export interface JudgementEngine {
  useModel(model: File | string): Promise<void>
  useInstalledModel(id: string): void
  prepare(): Promise<void>
  generate(request: InterpretationRequest, callbacks?: StreamCallbacks): Promise<HoraryInterpretation>
  cancel(): void
  dispose(): void
  status(): JudgementEngineStatus
}

export function createJudgementEngine(): JudgementEngine {
  return isTauriRuntime() ? new TauriJudgementEngine() : new BrowserJudgementEngine()
}

class TauriJudgementEngine implements JudgementEngine {
  private prepared = false
  private running = false
  private cancelled = false
  private disposed = false
  private generationId: string | null = null
  private cancelPending: (() => void) | null = null
  private modelId: string | null = null

  useInstalledModel(id: string) {
    if (this.running) throw new Error('Cancel the current judgement before changing models.')
    this.modelId = id
    this.prepared = false
  }

  async useModel(model: File | string) {
    if (this.running) throw new Error('Cancel the current judgement before changing models.')
    if (typeof model !== 'string' || !model.trim()) throw new Error('Enter the full path to a local GGUF model.')
    this.modelId = (await importModel(model.trim())).id
    this.prepared = false
  }

  async prepare() {
    if (this.disposed) throw new Error('Judgement panel was closed.')
    // The backend may have stopped since the previous reading.
    const status = await getModelStatus()
    const models = this.modelId ? [] : await listModels()
    const modelId = this.modelId || models.find(m => m.id === RECOMMENDED_MODEL_ID)?.id || models[0]?.id
    if (!status.running || (modelId && status.modelId !== modelId)) {
      if (!modelId) throw new Error('Select “Set up local model” to download the reading model first.')
      await startLlama({ modelId, ctxSize: 16384, nGpuLayers: 'auto' })
    }
    this.prepared = true
  }

  async generate(request: InterpretationRequest, callbacks: StreamCallbacks = {}) {
    if (this.running) throw new Error('A judgement is already running.')
    this.running = true
    this.cancelled = false
    try {
      await this.prepare()
      this.checkCancelled()
      return await new Promise<HoraryInterpretation>((resolve, reject) => {
        let settled = false
        let unlisten: (() => void) | undefined
        const early: Array<() => void> = []
        const finish = (result: () => void) => {
          if (settled) return
          settled = true
          unlisten?.()
          this.cancelPending = null
          result()
        }
        const fail = (error: unknown) => finish(() => reject(error instanceof Error ? error : new Error(String(error))))
        const deliver = (id: string, result: () => void) => {
          if (settled) return
          if (!this.generationId) { early.push(() => deliver(id, result)); return }
          if (id === this.generationId && !this.cancelled) result()
        }
        const cancelStarted = () => {
          if (!this.generationId || settled) return
          void cancelInterpretationStream(this.generationId).then(
            () => fail(new Error('Judgement cancelled.')), fail,
          )
        }
        this.cancelPending = cancelStarted
        void listenAiInterpretationEvents({
          token: p => deliver(p.generationId, () => callbacks.token?.(p.text)),
          complete: p => deliver(p.generationId, () => finish(() => resolve(p.interpretation))),
          error: p => deliver(p.generationId, () => fail(new Error(p.message))),
          cancelled: p => deliver(p.generationId, () => fail(new Error(p.message || 'Judgement cancelled.'))),
        }).then(async listener => {
          unlisten = listener
          this.checkCancelled()
          const started = await startInterpretationStream(request)
          this.generationId = started.generationId
          if (this.cancelled || this.disposed) { cancelStarted(); return }
          for (const event of early) event()
          early.length = 0
        }).catch(fail)
      })
    } finally {
      this.running = false
      this.generationId = null
      this.cancelPending = null
    }
  }

  private checkCancelled() {
    if (this.cancelled || this.disposed) throw new Error('Judgement cancelled.')
  }
  cancel() { this.cancelled = true; this.cancelPending?.() }
  dispose() { this.disposed = true; this.cancel() }
  status(): JudgementEngineStatus { return { prepared: this.prepared, running: this.running, runtime: 'tauri' } }
}

class BrowserJudgementEngine implements JudgementEngine {
  private running = false
  private cancelled = false
  private disposed = false
  private model: File | null = null
  private engine: Engine | null = null
  private preparing: Promise<void> | null = null
  private conversation: Conversation | null = null

  useInstalledModel() { throw new Error('Installed GGUF models are available in the desktop app.') }

  async useModel(model: File | string) {
    if (this.running || this.preparing) throw new Error('Wait for the current judgement to finish before changing models.')
    if (typeof model === 'string' || !model.name.endsWith('.litertlm')) throw new Error('Choose a local .litertlm model file.')
    await this.engine?.delete()
    this.engine = null
    this.model = model
  }

  async prepare() {
    if (this.disposed) throw new Error('Judgement panel was closed.')
    if (this.engine) return
    if (this.preparing) return this.preparing
    if (!(navigator as Navigator & { gpu?: unknown }).gpu) throw new Error('Local AI requires a browser with WebGPU support. The chart calculator still works.')
    if (!this.model) throw new Error('Choose a local .litertlm model in Judgement setup first.')
    this.preparing = (async () => {
      await ensureAiCoreLoaded()
      const { Engine, getOrLoadGlobalLiteRtLm } = await import('@litert-lm/core')
      // Runtime assets ship with the app; the question and model file stay on-device.
      await getOrLoadGlobalLiteRtLm(`${import.meta.env.BASE_URL}litert/`)
      const engine = await Engine.create({ model: this.model!, mainExecutorSettings: { maxNumTokens: 16384 } })
      if (this.disposed) { await engine.delete(); throw new Error('Judgement cancelled.') }
      this.engine = engine
    })().finally(() => { this.preparing = null })
    return this.preparing
  }

  async generate(request: InterpretationRequest, callbacks: StreamCallbacks = {}) {
    if (this.running) throw new Error('A judgement is already running.')
    this.running = true
    this.cancelled = false
    try {
      await this.prepare()
      this.checkCancelled()
      const prompt = await buildInterpretationPrompt(request)
      this.checkCancelled()
      this.conversation = await this.engine!.createConversation({
        preface: { messages: [{ role: 'system', content: `${prompt.messages.find(m => m.role === 'system')?.content || ''}\n\nResponse format:\n${JSON.stringify(prompt.responseFormat)}` }] },
      })
      this.checkCancelled()
      const reader = this.conversation.sendMessageStreaming(prompt.messages.filter(m => m.role === 'user').map(m => ({ role: 'user', content: m.content }))).getReader()
      let output = ''
      try {
        while (true) {
          const { done, value } = await reader.read()
          this.checkCancelled()
          if (done) break
          const text = typeof value.content === 'string' ? value.content : value.content?.filter(p => p.type === 'text').map(p => p.text).join('') || ''
          output += text
          if (output.length > 64_000) { this.conversation.cancel(); throw new Error('The model did not finish a bounded judgement. Try a different model.') }
          callbacks.token?.(text)
        }
      } finally { reader.releaseLock() }
      const result = await validateInterpretationContent(output, prompt.risk)
      this.checkCancelled()
      return result
    } finally {
      try { await this.conversation?.delete() }
      finally { this.conversation = null; this.running = false }
    }
  }

  private checkCancelled() {
    if (this.cancelled || this.disposed) throw new Error('Judgement cancelled.')
  }
  cancel() { this.cancelled = true; this.conversation?.cancel() }
  dispose() {
    this.disposed = true
    this.cancel()
    // Engine deletion serializes with active inference inside LiteRT.
    const engine = this.engine
    this.engine = null
    void engine?.delete().catch(() => {})
  }
  status(): JudgementEngineStatus { return { prepared: this.engine !== null, running: this.running, runtime: 'web' } }
}
