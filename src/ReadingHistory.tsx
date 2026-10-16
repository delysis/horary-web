import { useEffect, useRef } from 'react'
import { ReadingPassage } from './ReadingDocument'
import type { Revision } from './ReadingDocument'

export type SavedReading = { id: string; title: string; savedAtMs: number }
export type MethodRecord = { stage: string; guideSha256: string; validationError?: string | null; generation: Record<string, unknown>; input: unknown; raw: string; worksheet: unknown }

export function ReadingHistory({ open, close, saved = [], revisions = [], records = [], audit = [], unavailable, openReading }: {
  open: boolean; close: () => void; saved?: SavedReading[]; revisions?: Revision[];
  records?: MethodRecord[]; audit?: unknown[]; unavailable: boolean; openReading: (id?: string) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    if (!open || !dialog.current) return
    const pane = dialog.current
    const overflow = document.body.style.overflow
    pane.showModal()
    document.body.style.overflow = 'hidden'
    return () => { pane.close(); document.body.style.overflow = overflow }
  }, [open])
  return <dialog ref={dialog} id="reading-history" className="history-pane" aria-labelledby="history-title" onClose={close} onCancel={e => { e.preventDefault(); close() }} onClick={e => {
    if (e.target !== e.currentTarget) return
    const bounds = e.currentTarget.getBoundingClientRect()
    if (e.clientX < bounds.left || e.clientX > bounds.right || e.clientY < bounds.top || e.clientY > bounds.bottom) close()
  }}>
    <div className="history-heading"><h2 id="history-title">History</h2><button type="button" className="quiet-icon history-action new-reading" aria-label="New reading" title={unavailable ? 'Pause the current reading before starting another.' : 'Start a new reading'} disabled={unavailable} onClick={() => openReading()}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg></button><button type="button" className="quiet-icon history-action history-close" aria-label="Close history" title="Close history" onClick={close} autoFocus><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 7 10 10M17 7 7 17" /></svg></button></div>
    <div className="history-scroll">
      {!!saved.length && <ol className="history-leaves">{saved.slice().reverse().map(reading => <li key={reading.id}><button type="button" className="history-leaf" disabled={unavailable} onClick={() => openReading(reading.id)}><span>{reading.title}</span><time dateTime={new Date(reading.savedAtMs).toISOString()}>{new Date(reading.savedAtMs).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}</time></button></li>)}</ol>}
      {!!revisions.length && <details className="history-versions"><summary title="Earlier versions of this reading">Versions</summary>{revisions.map((r, i) => <article key={i}><p className="source-note">Reading {r.number}</p><h3>{r.question}</h3>{r.sections.map((s, n) => <ReadingPassage key={n} section={s} />)}</article>)}</details>}
      {!!(audit.length || records.length) && <details className="reading-journal"><summary title="Detailed local processing records">Receipts</summary><details><summary>Processing</summary>{records.map((r, i) => <details key={i}><summary>{r.stage} · {String(r.generation.elapsedMs)} ms</summary><p>Guide SHA256: {r.guideSha256}</p><p>Native validation: {r.validationError || 'accepted'}</p><pre>{JSON.stringify(r.generation, null, 2)}</pre><details><summary>Input</summary><pre>{JSON.stringify(r.input, null, 2)}</pre></details><details><summary>Output</summary><pre>{r.raw}</pre></details><details><summary>Proposal</summary><pre>{JSON.stringify(r.worksheet, null, 2)}</pre></details></details>)}{!!audit.length && <details><summary>Log</summary><pre>{JSON.stringify(audit, null, 2)}</pre></details>}</details></details>}
    </div>
  </dialog>
}
