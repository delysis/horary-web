import { useEffect, useRef, useState } from 'react'
import type { MethodStep } from './MethodReview'
import { read_review_notes_json, append_review_note_json } from './ai/aiCoreWasm'
import { saveReviewExport, type HoraryInterpretation } from './tauriBridge.ts'

export const REVIEW_NOTES_KEY = 'horary.reviewNotes.v1'
export type ReviewNote = {
  id: string
  createdAt: string
  category: string
  note: string
  context: { question: string; chart: unknown; interpretation: HoraryInterpretation | null; build: string; methodStep?: MethodStep }
}

type ReviewNotesProps = { question: string; chart: unknown; interpretation: HoraryInterpretation | null; reviewTarget?: { step: MethodStep; requestId: number } | null }

export function ReviewNotes({ question, chart, interpretation, reviewTarget }: ReviewNotesProps) {
  const details = useRef<HTMLDetailsElement>(null)
  const lastTarget = useRef<number | null>(null)
  const [initial] = useState(() => {
    try { return { notes: JSON.parse(read_review_notes_json(localStorage.getItem(REVIEW_NOTES_KEY) || '[]')) as ReviewNote[], error: '' } }
    catch { return { notes: [] as ReviewNote[], error: 'Saved review notes could not be read. The original data has been preserved.' } }
  })
  const [notes, setNotes] = useState(initial.notes)
  const [draft, setDraft] = useState<ReviewNote['context'] | null>(null)
  const [category, setCategory] = useState('Astrology or interpretation')
  const [note, setNote] = useState('')
  const [message, setMessage] = useState('')
  const [error, setError] = useState(initial.error)

  useEffect(() => {
    if (!reviewTarget || lastTarget.current === reviewTarget.requestId) return
    lastTarget.current = reviewTarget.requestId
    if (details.current) details.current.open = true
    if (draft) { setMessage('Save or discard your draft, then select the method step again.'); return }
    setDraft(structuredClone({ question, chart, interpretation, build: __HORARY_BUILD__, methodStep: reviewTarget.step }))
    setCategory('Astrology or interpretation'); setMessage(''); setError('')
    details.current?.scrollIntoView?.({ behavior: 'smooth', block: 'nearest' })
  }, [reviewTarget, draft, question, chart, interpretation])

  function save() {
    if (!draft || !note.trim()) return
    const entry = { id: crypto.randomUUID(), createdAt: new Date().toISOString(), category, note: note.trim(), context: draft }
    try {
      // Append to the current stored records, including notes saved in another tab.
      const serialized = append_review_note_json(localStorage.getItem(REVIEW_NOTES_KEY) || '[]', JSON.stringify(entry))
      localStorage.setItem(REVIEW_NOTES_KEY, serialized)
      setNotes(JSON.parse(serialized))
      setError(''); setDraft(null); setNote(''); setMessage('Review note saved on this device.')
    } catch { setError('Could not save review notes in this browser. Copy your note before leaving the page. Existing notes have been preserved.') }
  }

  async function exportNotes() {
    let latest: ReviewNote[]
    try { latest = JSON.parse(read_review_notes_json(localStorage.getItem(REVIEW_NOTES_KEY) || '[]')) }
    catch { setError('Could not read saved notes for export. Existing data has been preserved.'); return }
    try {
      const saved = await saveReviewExport(`horary-review-notes-${new Date().toISOString().slice(0, 10)}.json`, { format: 'horary-review-notes', version: 1, notes: latest })
      setMessage(saved ? 'Review notes exported.' : 'Export cancelled.')
    } catch (error) { setError(`Could not export review notes: ${String(error)}`) }

  }

  return <section className="review-notes" aria-label="Review notes">
    <details ref={details}>
      <summary>Review notes{notes.length ? ` (${notes.length})` : ''}</summary>
      <p>Record an astrology correction, a confusing interaction, or a feature idea. Each note keeps a copy of the question, chart, and completed reading visible when you start the note.</p>
      <button disabled={draft !== null} onClick={() => {
        setDraft(structuredClone({ question, chart, interpretation, build: __HORARY_BUILD__ }))
        setMessage(''); setError('')
      }}>Add review note</button>
      {draft ? <div className="review-draft">
        <p>Reviewing: {draft.question || 'Chart or general app feedback'}</p>
        {draft.methodStep ? <p>Method step: <strong>{draft.methodStep.title}</strong></p> : null}
        <label>Topic<select aria-label="Review topic" value={category} onChange={e => setCategory(e.target.value)}>
          {['Astrology or interpretation', 'Chart calculation', 'App experience', 'Feature idea'].map(topic => <option key={topic}>{topic}</option>)}
        </select></label>
        <label>Your note<textarea aria-label="Your review note" rows={5} value={note} onChange={e => setNote(e.target.value)} placeholder="What should change, and what would you expect instead?" /></label>
        <button disabled={!note.trim()} onClick={save}>Save note</button>
        <button onClick={() => { setDraft(null); setNote('') }}>Discard draft</button>
      </div> : null}
      {message ? <p role="status">{message}</p> : null}
      {error ? <p role="alert">{error}</p> : null}
      {notes.length ? <>
        <p>Export includes the saved questions, coordinates, and readings. Nothing is sent automatically.</p>
        <button onClick={exportNotes}>Export review notes</button>
        <ol>{notes.map(item => <li key={item.id}>
          <details><summary>{item.category} · {new Date(item.createdAt).toLocaleDateString()}</summary>
            <p>{item.context.question}</p><p style={{ whiteSpace: 'pre-wrap' }}>{item.note}</p>
          </details>
        </li>)}</ol>
      </> : null}
    </details>
  </section>
}
