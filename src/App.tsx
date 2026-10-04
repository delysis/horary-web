import { Fragment, useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ReadingChart, ReadingPassage } from './ReadingDocument'
import type { Chart, Fact, Progress, Revision, Section } from './ReadingDocument'
import './App.css'

type Message = { role: string; text: string }
type Session = { messages: Message[]; question: string; chart: Chart | null; chartAfterMessage?: number; snapshotId?: number; place: { label: string; timezone: string } | null; sections: Section[]; revisions?: Revision[]; facts?: Fact[]; progress?: Progress[]; revision: number; status: string; busy: boolean }
const EMPTY: Session = { messages: [], question: '', chart: null, place: null, sections: [], revision: 0, status: '', busy: false }
const native = () => '__TAURI_INTERNALS__' in window

export default function App() {
  const [session, setSession] = useState<Session>(EMPTY)
  const [draft, setDraft] = useState('')
  const [pending, setPending] = useState(false)
  const [recording, setRecording] = useState(false)
  const [notice, setNotice] = useState('')
  const [dark, setDark] = useState(() => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false)
  const input = useRef<HTMLTextAreaElement>(null)
  const page = useRef<HTMLElement>(null)
  const tail = useRef<HTMLDivElement>(null)
  const turn = useRef(0)
  const hold = useRef(false)
  const capture = useRef(false)
  const starting = useRef(false)
  const abandonCapture = useRef(false)
  const busyRef = useRef(false)
  const mounted = useRef(true)
  const submitted = useRef('')
  const acceptedSnapshot = useRef(0)
  const contextReady = useRef<Promise<void> | null>(null)
  const busy = pending || session.busy
  const accept = useCallback((s: Session) => {
    if (!mounted.current) return
    const sequence = s.snapshotId || 0
    if (sequence < acceptedSnapshot.current) return
    acceptedSnapshot.current = sequence
    setSession(s)
    if (submitted.current && s.messages.some(m => m.role === 'user' && m.text === submitted.current)) {
      const kept = submitted.current; submitted.current = ''
      setDraft(current => current === kept ? '' : current)
    }
  }, [])
  useEffect(() => {
    mounted.current = true
    page.current?.focus()
    const theme = window.matchMedia?.('(prefers-color-scheme: dark)')
    const change = (e: MediaQueryListEvent) => setDark(e.matches)
    theme?.addEventListener?.('change', change)
    if (native()) void invoke<Session>('conversation_snapshot').then(accept).catch(() => setNotice('I couldn’t open our earlier conversation. It has been left untouched.'))
    return () => { mounted.current = false; theme?.removeEventListener?.('change', change) }
  }, [accept])
  useEffect(() => {
    if (!busy || !native()) return
    const timer = window.setInterval(() => { void invoke<Session>('conversation_snapshot').then(accept).catch(() => {}) }, 700)
    return () => window.clearInterval(timer)
  }, [busy, accept])
  useEffect(() => {
    const nearEnd = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 240
    if (nearEnd) tail.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' })
  }, [session.messages.length, session.sections.length])
  async function quiet() { if (native()) await invoke('voice_stop_speaking').catch(() => {}) }
  async function prepareContext() {
    if (!contextReady.current) contextReady.current = (async () => {
      const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
      let location: { latitude: number; longitude: number; accuracyMeters?: number } | undefined
      if (!session.place && !session.chart) {
        location = await invoke<typeof location>('get_current_location', { req: { timeoutMs: 6000 } }).catch(() => undefined)
      }
      await invoke('conversation_device_context', { context: { timezone, locale: navigator.language, latitude: location?.latitude ?? null, longitude: location?.longitude ?? null, accuracyMeters: location?.accuracyMeters ?? null } }).catch(() => {})
    })()
    await contextReady.current
  }
  async function deliver(text: string, aloud: boolean, id: number) {
    await prepareContext()
    if (id !== turn.current) return
    submitted.current = text
    const next = await invoke<Session>('conversation_send', { text })
    if (!mounted.current || id !== turn.current) return
    accept(next); setDraft(current => current === text ? '' : current)
    const reply = next.messages.at(-1)
    if (aloud && reply?.role === 'assistant') await invoke('voice_speak', { text: reply.text }).catch(() => {})
  }
  async function send(text = draft) {
    if (!text.trim() || busyRef.current || session.busy) return
    if (!native()) { setNotice('This page is a sketch. Our conversation comes alive in the desktop app.'); return }
    const id = ++turn.current
    busyRef.current = true; setPending(true); setNotice('')
    try { await quiet(); await deliver(text, false, id) }
    catch { setNotice('Something interrupted us. Your words are still here.'); setDraft(current => current || text) }
    finally { busyRef.current = false; if (mounted.current) setPending(false) }
  }
  const pause = useCallback(async () => {
    abandonCapture.current = true
    ++turn.current; hold.current = false
    await quiet()
    if (capture.current) { capture.current = false; setRecording(false); await invoke('voice_cancel').catch(() => {}) }
    if (busyRef.current || session.busy) { await invoke('conversation_cancel').catch(() => {}); setNotice('We can pause here.') }
  }, [session.busy])
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); void pause() } }
    const blur = () => { if (capture.current || starting.current) void pause() }
    window.addEventListener('keydown', key); window.addEventListener('blur', blur)
    return () => { window.removeEventListener('keydown', key); window.removeEventListener('blur', blur) }
  }, [pause])
  async function finishSpeaking() {
    hold.current = false
    if (!capture.current) return
    capture.current = false; setRecording(false)
    const id = ++turn.current
    busyRef.current = true; setPending(true); setNotice('')
    try {
      const text = await invoke<string>('voice_finish')
      if (id !== turn.current) return
      setDraft(current => current || text)
      await deliver(text, true, id)
    } catch { if (id === turn.current) setNotice('I didn’t quite catch that. Try once more, or write it here.') }
    finally { busyRef.current = false; if (mounted.current) setPending(false) }
  }
  async function beginSpeaking() {
    if (busyRef.current || session.busy || starting.current || capture.current) return
    if (!native()) { setNotice('This page is a sketch. Our conversation comes alive in the desktop app.'); return }
    hold.current = true; starting.current = true; abandonCapture.current = false; setNotice('')
    try {
      await quiet(); await invoke('voice_start')
      if (abandonCapture.current || !mounted.current) { await invoke('voice_cancel').catch(() => {}); return }
      capture.current = true; setRecording(true)
      if (!hold.current) await finishSpeaking()
    } catch { setNotice('I can’t hear you yet. You can write here, too.') }
    finally { starting.current = false }
  }
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.repeat || e.code !== 'Space' || e.metaKey || e.ctrlKey) return
      const editing = e.target instanceof Element && !!e.target.closest('textarea,input,select,[contenteditable="true"]')
      if (editing && !e.altKey) return
      e.preventDefault(); void beginSpeaking()
    }
    const up = (e: KeyboardEvent) => {
      if (!e.defaultPrevented && e.code === 'Space' && hold.current) { e.preventDefault(); void finishSpeaking() }
    }
    window.addEventListener('keydown', down); window.addEventListener('keyup', up)
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up) }
  })
  useEffect(() => {
    if (!recording) return
    const timer = window.setTimeout(() => { void finishSpeaking() }, 120000)
    return () => window.clearTimeout(timer)
    // Native capture bounds its storage independently of the page.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording])
  useEffect(() => () => { if (capture.current && native()) void invoke('voice_cancel').catch(() => {}) }, [])
  function material(after: number) {
    return <>{session.chart && (session.chartAfterMessage || 1) === after && <ReadingChart key={`chart-${session.revision}`} chart={session.chart} place={session.place} dark={dark} facts={session.facts} />}{session.sections.filter(s => (s.after_message || 1) === after).map(section => <ReadingPassage key={`${section.revision}-${section.step || section.title}`} section={section} />)}</>
  }
  return <main ref={page} className="unfolding-page" data-listening={recording} data-thinking={busy} tabIndex={0} aria-label="Your unfolding reading">
    <article className="living-document">
      <p className="opening-question">What would you like to know?</p>
      <div className="document-conversation" role="log" aria-label="Conversation" aria-live="polite" aria-relevant="additions text">
        {session.messages.map((message, i) => <Fragment key={i}><div className={`passage ${message.role}`}>
          {message.role === 'user' ? <p contentEditable={!busy} suppressContentEditableWarning role="textbox" aria-label="Your earlier words, editable" onBlur={e => {
            const correction = e.currentTarget.textContent?.trim()
            e.currentTarget.textContent = message.text
            if (correction && correction !== message.text) { setDraft(`A correction to “${message.text}”: ${correction}`); input.current?.focus() }
          }}>{message.text}</p> : message.text.split('\n\n').map((p, n) => <p key={n}>{p}</p>)}
        </div>{material(i + 1)}</Fragment>)}
      </div>
      <div className="document-present">
        {busy && <p className="passing-thought" role="status"><span className="ink-wisp" aria-hidden="true" />A little quiet, while the thread finds its way.</p>}
        {notice && <p className="gentle-notice" role="status">{notice}</p>}
        {!busy && <p className={`voice-invitation ${recording ? 'listening' : ''}`} tabIndex={0} aria-label="Hold to speak; release to finish" onPointerDown={e => { e.currentTarget.setPointerCapture(e.pointerId); void beginSpeaking() }} onPointerUp={() => void finishSpeaking()} onPointerCancel={() => void pause()} onKeyDown={e => { if ((e.code === 'Space' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); void beginSpeaking() } }} onKeyUp={e => { if (e.code === 'Space' || e.key === 'Enter') { e.preventDefault(); void finishSpeaking() } }}>{recording ? 'I’m listening…' : session.messages.length ? 'There’s more room here. Hold Space to speak.' : 'Hold Space to speak. Or write below.'}</p>}
        <textarea ref={input} className="document-answer" aria-label="Your next words" placeholder={busy ? 'A thought to return to…' : '…'} value={draft} rows={2} maxLength={8000} disabled={recording} onChange={e => { setDraft(e.target.value); e.currentTarget.style.height = 'auto'; e.currentTarget.style.height = `${e.currentTarget.scrollHeight}px` }} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send() } }} />
        <p className="page-instruction">{busy ? 'Esc to pause.' : draft.trim() ? 'Return to continue. Option–Space to speak while writing.' : session.messages.length ? 'Your earlier words can be changed. Nothing is lost.' : 'Return to continue. Or hold the words above to speak.'}</p>
      </div>
      {!!session.revisions?.length && <details className="earlier-leaves"><summary>Earlier leaves</summary>{session.revisions.map((r, i) => <article key={i}><p className="source-note">Reading {r.number}</p><h2>{r.question}</h2>{r.sections.map((s, n) => <ReadingPassage key={n} section={s} />)}<p className="source-note">To return, ask “Bring back reading {r.number}.”</p></article>)}</details>}
      {!!session.progress?.length && <details className="reading-journal"><summary>Notes from this reading</summary><ol>{session.progress.slice(-24).map((p, i) => <li key={i}><time>{new Date(p.atMs).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}</time><span>{p.detail}</span></li>)}</ol><p className="source-note">Your observations can be spoken or written here. They remain with the conversation.</p></details>}
      <div ref={tail} />
    </article>
  </main>
}
