import { Fragment, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ChartWheel } from './ChartWheel'
import './App.css'

type Message = { role: string; text: string }
type Section = { title: string; body: string; evidence: string[]; revision: number; after_message?: number }
type Chart = { timestampMs: number; houses: { longitude: number }[]; bodies: { name: string; longitude: number; retrograde: boolean }[]; aspects?: { planet1: string; planet2: string; aspect: string }[] }
type Session = { messages: Message[]; question: string; chart: Chart | null; chartAfterMessage?: number; snapshotId?: number; place: { label: string; timezone: string } | null; sections: Section[]; revision: number; status: string; busy: boolean }
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
  const busy = pending || session.busy
  useEffect(() => {
    mounted.current = true
    page.current?.focus()
    const theme = window.matchMedia?.('(prefers-color-scheme: dark)')
    const change = (e: MediaQueryListEvent) => setDark(e.matches)
    theme?.addEventListener?.('change', change)
    if (native()) void invoke<Session>('conversation_snapshot').then(s => { if (mounted.current) setSession(s) }).catch(() => setNotice('I couldn’t open our earlier conversation. It has been left untouched.'))
    return () => { mounted.current = false; theme?.removeEventListener?.('change', change) }
  }, [])
  useEffect(() => {
    if (!busy || !native()) return
    const timer = window.setInterval(() => {
      void invoke<Session>('conversation_snapshot').then(s => { if (mounted.current) setSession(current => (s.snapshotId || 0) < (current.snapshotId || 0) ? current : s) }).catch(() => {})
    }, 900)
    return () => window.clearInterval(timer)
  }, [busy])
  useEffect(() => {
    // New material can unfold without dragging someone away from an earlier passage.
    const nearEnd = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 200
    if (nearEnd) tail.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' })
  }, [session.messages.length, session.sections.length])
  async function quiet() { if (native()) await invoke('voice_stop_speaking').catch(() => {}) }
  async function deliver(text: string, aloud: boolean, id: number) {
    const next = await invoke<Session>('conversation_send', { text })
    if (!mounted.current || id !== turn.current) return
    setSession(next); setDraft(current => current === text ? '' : current)
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
    finally { busyRef.current = false; setPending(false) }
  }
  async function pause() {
    abandonCapture.current = true
    ++turn.current; hold.current = false
    await quiet()
    if (capture.current) { capture.current = false; setRecording(false); await invoke('voice_cancel').catch(() => {}) }
    if (busyRef.current || session.busy) { await invoke('conversation_cancel').catch(() => {}); setNotice('We can pause here.') }
  }
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
    finally { busyRef.current = false; setPending(false) }
  }
  async function beginSpeaking() {
    if (busyRef.current || session.busy || starting.current || capture.current) return
    if (!native()) { setNotice('This page is a sketch. Our conversation comes alive in the desktop app.'); return }
    hold.current = true; starting.current = true; abandonCapture.current = false; setNotice('')
    try {
      await quiet(); await invoke('voice_start')
      if (abandonCapture.current) { await invoke('voice_cancel').catch(() => {}); return }
      capture.current = true; setRecording(true)
      if (!hold.current) await finishSpeaking()
    } catch { setNotice('I can’t hear you yet. You can write here, too.') }
    finally { starting.current = false }
  }
  useEffect(() => {
    if (!recording) return
    const timer = window.setTimeout(() => { void finishSpeaking() }, 120000)
    return () => window.clearTimeout(timer)
    // The native recorder also bounds capture independently of the page.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording])
  function chart() {
    if (!session.chart) return null
    const data = { planets: Object.fromEntries(session.chart.bodies.map(b => [b.name, [b.longitude, b.retrograde ? -1 : 1]])), cusps: session.chart.houses.map(h => h.longitude), aspects: (session.chart.aspects || []).map(a => ({ point: { name: a.planet1 }, toPoint: { name: a.planet2 }, aspect: { name: a.aspect.toLowerCase() } })) }
    return <figure className="document-chart" key={`chart-${session.revision}`}><ChartWheel data={data} darkMode={dark} /><figcaption>{session.place?.label} · {new Date(session.chart.timestampMs).toLocaleString(undefined, { timeZone: session.place?.timezone, dateStyle: 'medium', timeStyle: 'short' })}</figcaption></figure>
  }
  function material(after: number) {
    return <>{session.chart && (session.chartAfterMessage || 1) === after && chart()}{session.sections.filter(s => (s.after_message || 1) === after).map(section => <section className="document-section" key={`${section.revision}-${section.title}-${section.body}`}><h2>{section.title}</h2>{section.body.split('\n\n').map((p, n) => <p key={n}>{p}</p>)}</section>)}</>
  }
  return <main ref={page} className="unfolding-page" tabIndex={0} aria-label="Your unfolding reading" onKeyDown={e => {
    if (e.key === 'Escape') { e.preventDefault(); void pause() }
    if (e.code === 'Space' && e.target === e.currentTarget && !e.repeat) { e.preventDefault(); void beginSpeaking() }
  }} onKeyUp={e => { if (e.code === 'Space' && e.target === e.currentTarget) { e.preventDefault(); void finishSpeaking() } }}>
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
        {busy && <p className="passing-thought" role="status">A moment. Let me sit with this.</p>}
        {notice && <p className="gentle-notice" role="status">{notice}</p>}
        {!busy && <p className={`voice-invitation ${recording ? 'listening' : ''}`} tabIndex={0} aria-label="Hold to speak; release to finish" onPointerDown={e => { e.currentTarget.setPointerCapture(e.pointerId); void beginSpeaking() }} onPointerUp={() => void finishSpeaking()} onPointerCancel={() => void pause()} onKeyDown={e => { if ((e.code === 'Space' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); void beginSpeaking() } }} onKeyUp={e => { if (e.code === 'Space' || e.key === 'Enter') { e.preventDefault(); void finishSpeaking() } }}>{recording ? 'I’m listening…' : session.messages.length ? 'There’s more room here.' : 'Hold here to speak. Or write below.'}</p>}
        <textarea ref={input} className="document-answer" aria-label="Your next words" placeholder={busy ? 'A thought to return to…' : '…'} value={draft} rows={2} maxLength={8000} disabled={recording} onChange={e => { setDraft(e.target.value); e.currentTarget.style.height = 'auto'; e.currentTarget.style.height = `${e.currentTarget.scrollHeight}px` }} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send() } }} />
        <p className="page-instruction">{busy ? 'Esc to pause.' : draft.trim() ? 'Return to continue.' : session.messages.length ? 'You can return to any of your words and change them.' : 'Return to continue. Space to speak.'}</p>
      </div>
      <div ref={tail} />
    </article>
  </main>
}
