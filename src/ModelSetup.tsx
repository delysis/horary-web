import { useEffect, useState } from 'react'
import { getModelSetupStatus, installRecommendedModel, pauseModelSetup, type ModelSetupStatus } from './tauriBridge.ts'

export function ModelSetup({ disabled, onReady, onActivityChange }: { disabled: boolean; onReady: () => void; onActivityChange?: (active: boolean) => void }) {
  const [status, setStatus] = useState<ModelSetupStatus | null>(null)
  const [starting, setStarting] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    let mounted = true
    let pending = false
    const refresh = async () => {
      if (pending) return
      pending = true
      try { const next = await getModelSetupStatus(); if (mounted) setStatus(next) }
      catch (e) { if (mounted) setError(message(e)) }
      finally { pending = false }
    }
    void refresh()
    const timer = setInterval(() => void refresh(), 1000)
    return () => { mounted = false; clearInterval(timer) }
  }, [])
  async function install() {
    setStarting(true); setError('')
    try {
      await installRecommendedModel()
      setStatus(await getModelSetupStatus())
      onReady()
    } catch (e) { setError(message(e)) }
    finally { setStarting(false) }
  }
  const active = starting || status?.active
  useEffect(() => { onActivityChange?.(Boolean(active)); return () => onActivityChange?.(false) }, [active, onActivityChange])
  return <div className="model-setup" aria-label="Local model setup">
    <h3>Gemma 4 12B QAT</h3>
    {!status?.ready ? <>
    <p>A one-time setup for private readings on this computer. Horary downloads the model and its matching image and audio support from Hugging Face. No account or additional software is needed.</p>
    <p>Up to 7.15 GB of storage; allow 24 GB memory, with 32 GB recommended. The app checks memory before loading. Existing cached files are reused. Downloads can be paused and resumed.</p>
    </> : null}
    {status?.ready ? <p role="status">Local model installed.</p> : null}
    {active ? <>
      <p role="status">{status?.modelName || 'Preparing model setup'} · {phaseName(status?.phase)}</p>
      {status && status.totalBytes > 0 ? <><progress aria-label="Model setup progress" max={status.totalBytes} value={status.completedBytes} /><span> {gb(status.completedBytes)} / {gb(status.totalBytes)} GB</span></> : null}
      <button onClick={() => void pauseModelSetup().catch(e => setError(message(e)))}>Pause setup</button>
    </> : <button disabled={disabled || !status} onClick={() => void install()}>{status?.ready ? 'Check model files' : status?.phase === 'paused' || status?.phase === 'error' ? 'Resume model setup' : 'Set up local model'}</button>}
    {error || status?.message ? <p role="alert">{error || status?.message}</p> : null}
    <details><summary>Model source and storage</summary>
      <p>Google’s Gemma 4 12B IT QAT and its matching multimodal projector (google/gemma-4-12B-it-qat-q4_0-gguf), Apache 2.0. Horary verifies both files before use.</p>
      <p>Shared Hugging Face cache: <code>{status?.cachePath || 'Locating cache…'}</code>. Horary stores no second copy. After setup, readings work offline.</p>
    </details>
  </div>
}
function gb(bytes: number) { return (bytes / 1e9).toFixed(2) }
function phaseName(phase?: string) { return ({ downloading: 'Downloading', verifying: 'Checking file integrity', waiting: 'Waiting for shared cache', checking: 'Checking existing files' } as Record<string, string>)[phase || ''] || 'Preparing' }
function message(e: unknown) { return e && typeof e === 'object' && 'message' in e ? String(e.message) : String(e) }
