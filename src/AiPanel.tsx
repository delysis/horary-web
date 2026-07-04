import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  cancelInterpretationStream,
  getModelStatus,
  isTauriRuntime,
  listenAiInterpretationEvents,
  listModels,
  startInterpretationStream,
  startLlama,
  type HoraryInterpretation,
  type ModelInfo,
  type ModelStatus,
} from './tauriBridge.ts'

type AiPanelProps = {
  requestId: number
  question: string
  chartFacts: unknown | null
  darkMode: boolean
  onActivityChange?: (active: boolean) => void
}

export function AiPanel({ requestId, question, chartFacts, darkMode, onActivityChange }: AiPanelProps) {
  const runtime = useMemo(() => isTauriRuntime(), [])
  const [models, setModels] = useState<ModelInfo[]>([])
  const [status, setStatus] = useState<ModelStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [streaming, setStreaming] = useState(false)
  const [generationId, setGenerationId] = useState<string | null>(null)
  const [interpretation, setInterpretation] = useState<HoraryInterpretation | null>(null)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const generationIdRef = useRef<string | null>(null)
  const handledRequestId = useRef(0)

  const refreshEngineState = useCallback(async () => {
    const [nextModels, nextStatus] = await Promise.all([listModels(), getModelStatus()])
    setModels(nextModels)
    setStatus(nextStatus)
    return { models: nextModels, status: nextStatus }
  }, [])

  useEffect(() => {
    if (!runtime) return
    let mounted = true
    let unlisten: (() => void) | null = null

    async function init() {
      try {
        unlisten = await listenAiInterpretationEvents({
          complete(payload) {
            if (payload.generationId !== generationIdRef.current) return
            setInterpretation(payload.interpretation)
            setStreaming(false)
            setBusy(false)
            setGenerationId(null)
            generationIdRef.current = null
            setMessage('')
          },
          error(payload) {
            if (payload.generationId !== generationIdRef.current) return
            setError(readingErrorMessage(payload.message))
            setMessage('')
            setStreaming(false)
            setBusy(false)
            setGenerationId(null)
            generationIdRef.current = null
          },
          cancelled(payload) {
            if (payload.generationId !== generationIdRef.current) return
            setMessage(payload.message || 'Judgement cancelled.')
            setStreaming(false)
            setBusy(false)
            setGenerationId(null)
            generationIdRef.current = null
          },
        })
        if (mounted) await refreshEngineState()
      } catch (eventError) {
        console.error('Judgement engine discovery failed', eventError)
      }
    }

    init()
    return () => {
      mounted = false
      unlisten?.()
    }
  }, [refreshEngineState, runtime])

  const ensureJudgementEngine = useCallback(async () => {
    let nextStatus = status
    let nextModels = models
    if (!nextStatus || nextModels.length === 0) {
      const refreshed = await refreshEngineState()
      nextStatus = refreshed.status
      nextModels = refreshed.models
    }
    if (nextStatus.running) return

    const model = nextModels[0]
    if (!model) {
      throw new Error('Judgement is not installed in this copy of Horary.')
    }

    setMessage('Preparing judgement...')
    const started = await startLlama({
      modelId: model.id,
      ctxSize: 16384,
      nGpuLayers: 'auto',
      parallel: 2,
      continuousBatching: true,
      cacheRamMb: 4096,
      cacheIdleSlots: true,
      coldKvCache: true,
      specDraftNMax: 3,
    })
    setStatus(started)
  }, [models, refreshEngineState, status])

  const handleGenerate = useCallback(async () => {
    if (busy || streaming) return
    if (!chartFacts) {
      setError('Cast a chart before generating a judgement.')
      return
    }
    if (!question.trim()) {
      setError('Write the horary question before generating a judgement.')
      return
    }

    setBusy(true)
    setStreaming(false)
    setInterpretation(null)
    setError('')
    setMessage('Preparing judgement...')
    try {
      await ensureJudgementEngine()
      setMessage('Reading the chart...')
      const started = await startInterpretationStream({
        question,
        chart: chartFacts,
        settings: {
          tradition: 'traditional',
          houseSystem: 'regiomontanus',
          zodiac: 'tropical',
          tone: 'plain',
        },
      })
      generationIdRef.current = started.generationId
      setGenerationId(started.generationId)
      setStreaming(true)
    } catch (generateError) {
      generationIdRef.current = null
      setGenerationId(null)
      setStreaming(false)
      setBusy(false)
      setError(readingErrorMessage(generateError))
      setMessage('')
    }
  }, [busy, chartFacts, ensureJudgementEngine, question, streaming])

  useEffect(() => {
    if (!requestId || requestId === handledRequestId.current) return
    handledRequestId.current = requestId
    void handleGenerate()
  }, [handleGenerate, requestId])

  useEffect(() => {
    onActivityChange?.(busy || streaming)
  }, [busy, onActivityChange, streaming])

  async function handleCancel() {
    if (!generationId) return
    setMessage('Cancelling judgement...')
    try {
      await cancelInterpretationStream(generationId)
    } catch (cancelError) {
      setError(readingErrorMessage(cancelError))
    }
  }

  if (!runtime || (!message && !error && !streaming && !interpretation)) return null

  return (
    <section className={`ai-panel ${darkMode ? 'night' : ''}`} aria-label="Horary judgement">
      <div className="ai-panel-header">
        <h2>Judgement</h2>
      </div>

      {message ? (
        <div className="ai-status-line" aria-live="polite">
          {busy || streaming ? <span className="ai-busy-dot" aria-hidden="true" /> : null}
          {message}
        </div>
      ) : null}

      {streaming ? (
        <div className="ai-action-row">
          <button onClick={handleCancel} disabled={!generationId}>
            Cancel
          </button>
        </div>
      ) : null}

      {error ? <div className="ai-error">{error}</div> : null}
      {interpretation ? <InterpretationView interpretation={interpretation} /> : null}
    </section>
  )
}

function InterpretationView({ interpretation }: { interpretation: HoraryInterpretation }) {
  return (
    <div className="ai-output">
      <h3>Summary</h3>
      <p>{interpretation.summary}</p>
      {interpretation.directAnswer ? (
        <>
          <h3>Answer</h3>
          <p>{interpretation.directAnswer}</p>
        </>
      ) : null}
      <h3>Reasoning</h3>
      {interpretation.judgementTrace.map(step => (
        <div className="ai-factor" key={`${step.stepId}-${step.chartEvidence}`}>
          <strong>{step.stepId}</strong>
          <div className="ai-evidence">{step.chartEvidence}</div>
          <p>{step.finding}</p>
        </div>
      ))}
      <h3>Key Factors</h3>
      {interpretation.keyFactors.map(factor => (
        <div className="ai-factor" key={`${factor.factor}-${factor.chartEvidence}`}>
          <strong>{factor.factor}</strong>
          <div className="ai-evidence">{factor.chartEvidence}</div>
          <p>{factor.interpretation}</p>
        </div>
      ))}
      <InlineList title="Cautions" items={interpretation.cautions} />
      <InlineList title="Follow Up Questions" items={interpretation.followUpQuestions} />
    </div>
  )
}

function InlineList({ title, items }: { title: string; items: string[] }) {
  if (items.length === 0) return null
  return (
    <>
      <h3>{title}</h3>
      <ul>{items.map(item => <li key={item}>{item}</li>)}</ul>
    </>
  )
}

function readingErrorMessage(error: unknown) {
  const raw = error instanceof Error ? error.message : String(error)
  if (/prompt plus max tokens|exceeds ctxSize|context/i.test(raw)) {
    return 'The chart evidence is too large to read cleanly. Please try again.'
  }
  if (/invalid interpretation JSON|interpretation.*JSON|judgementTrace|keyFactors|chartEvidence/i.test(raw)) {
    return 'The judgement could not be read cleanly. Please try again.'
  }
  if (/model|gguf|llama|native|backend|runtime|inference/i.test(raw)) {
    return 'The judgement engine is not ready. Please try again in a moment.'
  }
  return raw
}
