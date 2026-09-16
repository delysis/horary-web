import { useCallback, useEffect, useRef, useState } from 'react'
import { createJudgementEngine, type JudgementEngine } from './ai/judgementEngine.ts'
import { chooseNativeModelFile, isTauriRuntime, RECOMMENDED_MODEL_ID, type HoraryInterpretation } from './tauriBridge.ts'
import { ModelSetup } from './ModelSetup'

type AiPanelProps = {
  requestId: number
  question: string
  chartFacts: unknown | null
  darkMode: boolean
  onActivityChange?: (active: boolean) => void
  onInterpretationChange?: (interpretation: HoraryInterpretation | null) => void
}

export function AiPanel({ requestId, question, chartFacts, darkMode, onActivityChange, onInterpretationChange }: AiPanelProps) {
  const engine = useRef<JudgementEngine | null>(null)
  const [busy, setBusy] = useState(false)
  const [setupBusy, setSetupBusy] = useState(false)
  const [interpretation, setInterpretation] = useState<HoraryInterpretation | null>(null)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const [modelPath, setModelPath] = useState('')
  const [modelName, setModelName] = useState('')
  const handledRequestId = useRef(0)
  const attempt = useRef(0)

  useEffect(() => {
    const current = createJudgementEngine()
    engine.current = current
    return () => { current.dispose(); engine.current = null }
  }, [])

  useEffect(() => {
    attempt.current++
    engine.current?.cancel()
    setInterpretation(null)
    setError('')
    setMessage('')
  }, [question, chartFacts])

  useEffect(() => { onInterpretationChange?.(interpretation) }, [interpretation, onInterpretationChange])
  useEffect(() => {
    onActivityChange?.(busy || setupBusy)
    return () => onActivityChange?.(false)
  }, [busy, setupBusy, onActivityChange])

  const handleGenerate = useCallback(async () => {
    const current = engine.current
    if (!current || current.status().running || setupBusy) return
    if (!chartFacts || !question.trim()) { setError('Cast a chart and write the question first.'); return }
    const id = ++attempt.current
    setBusy(true)
    setInterpretation(null)
    setError('')
    setMessage('Preparing a local reading…')
    try {
      const result = await current.generate({ question, chart: chartFacts, settings: {
        tradition: 'traditional', houseSystem: 'regiomontanus', zodiac: 'tropical', tone: 'plain',
      } }, { token: () => { if (id === attempt.current) setMessage('Reading the chart…') } })
      if (id === attempt.current && engine.current === current) setInterpretation(result)
    } catch (err) {
      if (id === attempt.current && engine.current === current) setError(errorMessage(err))
    } finally {
      if (engine.current === current) { setBusy(false); setMessage('') }
    }
  }, [chartFacts, question, setupBusy])

  useEffect(() => {
    if (!requestId || requestId === handledRequestId.current) return
    handledRequestId.current = requestId
    void handleGenerate()
  }, [handleGenerate, requestId])

  async function selectModel(model: File | string) {
    const current = engine.current
    if (!current) return
    setBusy(true); setError(''); setMessage('Opening local model…')
    try {
      await current.useModel(model)
      if (engine.current === current) setModelName(typeof model === 'string' ? model.split(/[\\/]/).pop() || model : model.name)
    } catch (err) { if (engine.current === current) setError(errorMessage(err)) }
    finally { if (engine.current === current) { setBusy(false); setMessage('') } }
  }

  return (
    <section className={`ai-panel ${darkMode ? 'night' : ''}`} aria-label="Horary judgement">
      <h2>Judgement</h2>
      <p className="ai-help">An experimental local reading for review. Eileen’s judgement and corrections guide its development.</p>
      {isTauriRuntime() ? <ModelSetup disabled={busy} onActivityChange={setSetupBusy} onReady={() => {
        engine.current?.useInstalledModel(RECOMMENDED_MODEL_ID)
        setModelName('Gemma 4 12B QAT')
      }} /> : null}
      <details>
        <summary>Advanced model setup{modelName ? ` · ${modelName}` : ''}</summary>
        {isTauriRuntime() ? <div>
          <p>Use an instruction-tuned GGUF model. An already installed model is used automatically.</p>
          <button disabled={busy} onClick={() => { void chooseNativeModelFile().then(path => { if (path) { setModelPath(path); return selectModel(path) } }).catch(err => setError(errorMessage(err))) }}>Choose model file…</button>
          <label>Local GGUF file path<input aria-label="Local GGUF file path" value={modelPath} onChange={e => setModelPath(e.target.value)} disabled={busy} /></label>
          <button disabled={busy || !modelPath.trim()} onClick={() => void selectModel(modelPath)}>Import model</button>
        </div> : <div>
          <p>Choose a compatible Gemma instruction model in .litertlm format. Your question and file stay on this device. WebGPU is required; large models may exceed your device’s memory.</p>
          <label>Local model file<input aria-label="Local model file" type="file" accept=".litertlm" disabled={busy} onChange={e => { const file = e.target.files?.[0]; if (file) void selectModel(file) }} /></label>
        </div>}
      </details>
      <div className="ai-action-row">
        <button disabled={busy || setupBusy || !question.trim()} onClick={() => void handleGenerate()}>Read chart</button>
        {busy ? <button onClick={() => { setMessage('Cancelling…'); engine.current?.cancel() }}>Cancel</button> : null}
      </div>
      {message ? <p role="status">{message}</p> : null}
      {error ? <p role="alert" className="ai-error">{error}</p> : null}
      {interpretation ? <InterpretationView interpretation={interpretation} /> : null}
    </section>
  )
}

function InterpretationView({ interpretation }: { interpretation: HoraryInterpretation }) {
  return <div className="ai-output">
    <p>{interpretation.directAnswer || interpretation.summary}</p>
    {interpretation.directAnswer && interpretation.summary !== interpretation.directAnswer ? <p>{interpretation.summary}</p> : null}
    <p className="ai-help">Model confidence: {interpretation.confidence}. This is not a calibrated probability.</p>
    <InlineList title="Cautions" items={interpretation.cautions} />
    <InlineList title="Questions to clarify" items={interpretation.followUpQuestions} />
    <details>
      <summary>Chart evidence and interpretation</summary>
      {interpretation.keyFactors.map((factor, index) => <div className="ai-factor" key={index}>
        <strong>{factor.factor}</strong><div className="ai-evidence">{factor.chartEvidence}</div><p>{factor.interpretation}</p>
      </div>)}
      <details><summary>Review the experimental method</summary>
        {interpretation.judgementTrace.map((step, index) => <div className="ai-factor" key={index}>
          <strong>{step.stepId.replaceAll('_', ' ')}</strong><div className="ai-evidence">{step.chartEvidence}</div><p>{step.finding}</p>
        </div>)}
      </details>
    </details>
  </div>
}

function InlineList({ title, items }: { title: string; items: string[] }) {
  if (!items.length) return null
  return <><h3>{title}</h3><ul>{items.map((item, index) => <li key={index}>{item}</li>)}</ul></>
}
function errorMessage(error: unknown) {
  if (error && typeof error === 'object' && 'message' in error) return String(error.message)
  return String(error)
}
