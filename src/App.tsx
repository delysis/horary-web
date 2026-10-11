import { Fragment, useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ReadingChart, ReadingPassage } from './ReadingDocument'
import type { Chart, Fact, Progress, Revision, Section } from './ReadingDocument'
import { ReadingHistory } from './ReadingHistory'
import type { MethodRecord, SavedReading } from './ReadingHistory'
import './App.css'

type Message = { role: string; text: string }
type Session = { readingId?: string; savedReadings?: SavedReading[]; method?: { records: MethodRecord[] }; audit?: unknown[]; messages: Message[]; question: string; chart: Chart | null; chartAfterMessage?: number; snapshotId?: number; place: { label: string; timezone: string } | null; sections: Section[]; revisions?: Revision[]; facts?: Fact[]; progress?: Progress[]; revision: number; status: string; busy: boolean }
type Heard = { generation: number; state: 'waiting' | 'listening' | 'heard' | 'unavailable'; id?: number }
type Permissions = { microphone: string; speech: string; location: string; voiceAvailable: boolean }
const EMPTY: Session = { messages: [], question: '', chart: null, place: null, sections: [], revision: 0, status: '', busy: false }
const OPENING = 'What would you like to know?'
const native = () => '__TAURI_INTERNALS__' in window

export default function App() {
  const [session, setSession] = useState<Session>(EMPTY)
  const [ready, setReady] = useState(false)
  const [pending, setPending] = useState(false)
  const [recording, setRecording] = useState(false)
  const [openingMic, setOpeningMic] = useState(false)
  const [manual, setManual] = useState(false)
  const [speaking, setSpeaking] = useState(false)
  const [foreground, setForeground] = useState(true)
  const [wakeState, setWakeState] = useState<'off' | 'opening' | 'waiting' | 'listening'>('off')
  const [wakeUnavailable, setWakeUnavailable] = useState(false)
  const [voicePaused, setVoicePaused] = useState(false)
  const [wakeRegistered, setWakeRegistered] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [notice, setNotice] = useState('')
  const [permissions, setPermissions] = useState<Permissions | null>(null)
  const [textFallback, setTextFallback] = useState(false)
  const [draft, setDraft] = useState('')
  const [dark, setDark] = useState(() => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false)
  const page = useRef<HTMLElement>(null)
  const tail = useRef<HTMLDivElement>(null)
  const historyTrigger = useRef<HTMLButtonElement>(null)
  const turn = useRef(0)
  const wakeGeneration = useRef(0)
  const followUp = useRef(false)
  const canHear = useRef(true)
  const heard = useRef<(update: Heard) => void>(() => {})
  const capture = useRef(false)
  const starting = useRef(false)
  const abandonCapture = useRef(false)
  const busyRef = useRef(false)
  const mounted = useRef(true)
  const acceptedSnapshot = useRef(0)
  const contextReady = useRef<Promise<void> | null>(null)
  const newLeaf = useRef<() => void>(() => {})
  const opening = useRef<Promise<Session> | null>(null)
  const authorizing = useRef<Promise<Permissions> | null>(null)
  const granted = useRef<Permissions | null>(null)
  const permissionEpoch = useRef(0)
  const speechTurn = useRef(0)
  const busy = pending || session.busy
  const accept = useCallback((s: Session) => {
    if (!mounted.current) return
    const sequence = s.snapshotId || 0
    if (sequence < acceptedSnapshot.current) return
    acceptedSnapshot.current = sequence
    setSession(s)
  }, [])
  async function quiet() { if (native()) await invoke('voice_stop_speaking').catch(() => {}) }
  async function say(text: string) {
    if (!native() || !mounted.current) return
    const spokenTurn = ++speechTurn.current
    setSpeaking(true)
    try { await invoke('voice_speak', { text }) }
    catch { /* The written reading remains available if an installed voice is absent. */ }
    finally { if (mounted.current && spokenTurn === speechTurn.current) setSpeaking(false) }
  }
  const prepareContext = useCallback(async (current: Session) => {
    if (!contextReady.current) contextReady.current = (async () => {
      const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
      let location: { latitude: number; longitude: number; accuracyMeters?: number } | undefined
      if (!current.place && !current.chart && granted.current?.location === 'granted') location = await invoke<typeof location>('get_current_location', { req: { timeoutMs: 6000 } }).catch(() => undefined)
      await invoke('conversation_device_context', { ...(current.readingId ? { readingId: current.readingId } : {}), context: { timezone, locale: navigator.language, latitude: location?.latitude ?? null, longitude: location?.longitude ?? null, accuracyMeters: location?.accuracyMeters ?? null } })
    })()
    const prepared = contextReady.current
    try { await prepared }
    catch (error) { if (contextReady.current === prepared) contextReady.current = null; throw error }
  }, [])
  useEffect(() => {
    mounted.current = true
    let live = true
    page.current?.focus()
    const theme = window.matchMedia?.('(prefers-color-scheme: dark)')
    const change = (e: MediaQueryListEvent) => setDark(e.matches)
    theme?.addEventListener?.('change', change)
    if (native()) void (async () => {
      const attempt = turn.current
      try {
        opening.current ??= invoke<Session>('conversation_open')
        authorizing.current ??= invoke<Permissions>('startup_permissions')
        const [next, access] = await Promise.all([opening.current, authorizing.current])
        if (!live) return
        granted.current = access; setPermissions(access); setTextFallback(!access.voiceAvailable)
        accept(next)
        await prepareContext(next).catch(() => { if (live) setNotice('I may need to ask where we are.') })
        if (!live) return
        if (attempt === turn.current && !next.messages.length && !next.sections.length) { await say(OPENING); followUp.current = true }
        if (live) setReady(true)
      } catch {
        if (live) {
          // Permission acquisition is independent of opening the reading. A
          // failed device service must leave a usable conversational fallback.
          setTextFallback(true)
          const next = await opening.current?.catch(() => null)
          if (next && live) { accept(next); setReady(true) }
          setNotice('Voice is unavailable. You can still write to me here.')
        }
      }
    })()
    return () => { live = false; mounted.current = false; theme?.removeEventListener?.('change', change) }
  }, [accept, prepareContext])
  useEffect(() => {
    if (!busy || !native()) return
    const attempt = turn.current
    const timer = window.setInterval(() => { void invoke<Session>('conversation_snapshot').then(s => { if (attempt === turn.current) accept(s) }).catch(() => {}) }, 700)
    return () => window.clearInterval(timer)
  }, [busy, accept])
  useEffect(() => {
    const nearEnd = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 240
    if (nearEnd) tail.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' })
  }, [session.messages.length, session.sections.length])
  async function stopWake() {
    const generation = ++wakeGeneration.current
    if (native()) await invoke('voice_listen', { generation, enabled: false, followUp: false }).catch(() => {})
  }
  useEffect(() => {
    if (!native()) return
    let disposed = false
    let stop: (() => void) | undefined
    void listen<Heard>('horary-voice', event => heard.current(event.payload)).then(unlisten => {
      if (disposed) unlisten()
      else { stop = unlisten; setWakeRegistered(true) }
    }).catch(() => setWakeUnavailable(true))
    return () => { disposed = true; stop?.() }
  }, [])
  useEffect(() => {
    if (!native() || !wakeRegistered) return
    if (!ready || textFallback || busy || speaking || manual || historyOpen || !foreground || voicePaused || wakeUnavailable) { setWakeState('off'); return }
    const generation = ++wakeGeneration.current
    setWakeState('opening')
    void invoke<boolean>('voice_listen', { generation, enabled: true, followUp: followUp.current }).then(active => {
      if (active === false && mounted.current && generation === wakeGeneration.current) { canHear.current = false; setForeground(false); setWakeState('off') }
    }).catch(() => {
      if (mounted.current && generation === wakeGeneration.current) { setWakeUnavailable(true); setWakeState('off') }
    })
    followUp.current = false
    return () => { void stopWake() }
  }, [ready, textFallback, busy, speaking, manual, historyOpen, foreground, voicePaused, wakeUnavailable, wakeRegistered])
  useEffect(() => {
    if (!native() || !ready) return
    let live = true
    const refresh = () => {
      const epoch = ++permissionEpoch.current
      void invoke<Permissions>('permission_status').then(access => {
        if (!live || !mounted.current || epoch !== permissionEpoch.current) return
        if (access.location === 'granted' && granted.current?.location !== 'granted') contextReady.current = null
        granted.current = access; setPermissions(access); setTextFallback(!access.voiceAvailable)
        if (access.voiceAvailable) { setVoicePaused(false); setWakeUnavailable(false); setNotice('') }
      }).catch(() => {})
    }
    window.addEventListener('focus', refresh)
    return () => { live = false; window.removeEventListener('focus', refresh) }
  }, [ready])
  const scope = () => session.readingId ? { readingId: session.readingId } : {}
  async function openLeaf(id?: string) {
    if (!native() || !ready || busyRef.current || session.busy || capture.current || starting.current || wakeState === 'listening') return
    const attempt = ++turn.current
    busyRef.current = true; setPending(true); setNotice('')
    try {
      await stopWake(); await quiet()
      const next = await invoke<Session>(id ? 'conversation_reopen' : 'conversation_fresh', id ? { id } : {})
      if (!mounted.current || attempt !== turn.current) return
      contextReady.current = null; accept(next)
      setHistoryOpen(false); setVoicePaused(false)
      page.current?.focus(); window.scrollTo?.({ top: 0, behavior: 'smooth' })
      if (!id) { await say(OPENING); followUp.current = true }
    } catch { if (attempt === turn.current) setNotice('I couldn’t open that leaf. This reading is still here.') }
    finally { if (attempt === turn.current) { busyRef.current = false; if (mounted.current) setPending(false) } }
  }
  useEffect(() => { newLeaf.current = () => { void openLeaf() } })
  useEffect(() => {
    if (!native()) return
    let disposed = false
    let stop: (() => void) | undefined
    void listen('horary-new-reading', () => newLeaf.current()).then(unlisten => { if (disposed) unlisten(); else stop = unlisten }).catch(() => {})
    return () => { disposed = true; stop?.() }
  }, [])
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n') { e.preventDefault(); void openLeaf() } }
    window.addEventListener('keydown', key); return () => window.removeEventListener('keydown', key)
  })
  const pause = useCallback(async () => {
    abandonCapture.current = true
    ++turn.current
    setVoicePaused(true); setWakeState('off')
    await stopWake(); await quiet()
    if (capture.current) { capture.current = false; setRecording(false); setManual(false); await invoke('voice_cancel').catch(() => {}) }
    if (busyRef.current || session.busy) { await invoke('conversation_cancel').catch(() => {}); busyRef.current = false; setPending(false) }
    setNotice('')
  }, [session.busy])
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape' && !historyOpen) { e.preventDefault(); void pause() } }
    const leave = () => {
      canHear.current = false; setForeground(false)
      abandonCapture.current = true
      if (capture.current) { capture.current = false; setRecording(false); setManual(false); void invoke('voice_cancel').catch(() => {}) }
    }
    const focus = () => { if (!document.hidden) { canHear.current = true; setForeground(true) } }
    const visibility = () => { if (document.hidden) leave(); else focus() }
    window.addEventListener('keydown', key); window.addEventListener('blur', leave); window.addEventListener('focus', focus); document.addEventListener('visibilitychange', visibility)
    return () => { window.removeEventListener('keydown', key); window.removeEventListener('blur', leave); window.removeEventListener('focus', focus); document.removeEventListener('visibilitychange', visibility) }
  }, [pause, historyOpen])
  async function deliverVoice(id: number, attempt: number) {
    await prepareContext(session)
    if (attempt !== turn.current) return
    const next = await invoke<Session>('conversation_voice', { id, ...scope() })
    if (!mounted.current || attempt !== turn.current) return
    accept(next); setWakeUnavailable(false)
    const reply = next.messages.at(-1)
    const spoken = reply?.role === 'assistant' ? reply.text : next.sections.slice().reverse().find(s => s.step === 'judgment')?.body
    if (spoken) { await say(spoken); if (attempt === turn.current) followUp.current = true }
  }
  async function submitText() {
    const text = draft.trim()
    if (!textFallback || !ready || !native() || !text || busyRef.current || session.busy) return
    const attempt = ++turn.current
    busyRef.current = true; setPending(true); setNotice('')
    try {
      await prepareContext(session)
      if (attempt !== turn.current) return
      const next = await invoke<Session>('conversation_send', { text, ...scope() })
      if (!mounted.current || attempt !== turn.current) return
      accept(next); setDraft('')
      const reply = next.messages.at(-1)
      if (reply?.role === 'assistant') await say(reply.text)
    } catch { if (attempt === turn.current) setNotice('Your words are still here. Shall we try again?') }
    finally { if (attempt === turn.current) { busyRef.current = false; if (mounted.current) setPending(false) } }
  }
  async function consumeHeard(id: number) {
    if (busyRef.current || session.busy || !canHear.current || historyOpen || capture.current || starting.current) return
    const attempt = ++turn.current
    busyRef.current = true; setPending(true); setNotice(''); setWakeState('off')
    try { await deliverVoice(id, attempt) }
    catch { if (attempt === turn.current) { setVoicePaused(true); setNotice('Something interrupted the reading. Your earlier words are still here.'); await say('Something interrupted the reading. Shall we try again?') } }
    finally { if (attempt === turn.current) { busyRef.current = false; if (mounted.current) setPending(false) } }
  }
  useEffect(() => {
    heard.current = update => {
      if (!mounted.current || update.generation !== wakeGeneration.current || !canHear.current || busyRef.current || historyOpen) return
      if (update.state === 'unavailable') { setWakeUnavailable(true); setWakeState('off') }
      else if (update.state === 'heard' && update.id !== undefined) void consumeHeard(update.id)
      else if (update.state === 'waiting' || update.state === 'listening') setWakeState(update.state)
    }
  })
  async function finishSpeaking() {
    if (!capture.current) return
    capture.current = false; setRecording(false)
    const attempt = ++turn.current
    busyRef.current = true; setPending(true); setNotice('')
    try {
      const voice = await invoke<{ id: number; text: string | null }>('voice_finish')
      if (attempt !== turn.current) return
      await deliverVoice(voice.id, attempt)
    } catch { if (attempt === turn.current) { setVoicePaused(true); setNotice('I didn’t quite catch that.'); await say('I didn’t quite catch that.') } }
    finally { if (attempt === turn.current) { busyRef.current = false; if (mounted.current) { setPending(false); setManual(false) } } }
  }
  async function beginSpeaking() {
    if (busyRef.current || session.busy || starting.current || capture.current) return
    if (!native()) { setNotice('Voice is available in the desktop app.'); return }
    if (!ready) return
    starting.current = true; abandonCapture.current = false; setManual(true); setOpeningMic(true); setNotice('')
    try {
      await stopWake(); await quiet()
      if (abandonCapture.current || !mounted.current) return
      await invoke('voice_start')
      if (abandonCapture.current || !mounted.current) { await invoke('voice_cancel').catch(() => {}); return }
      capture.current = true; setRecording(true); setVoicePaused(false)
    } catch { if (mounted.current && !abandonCapture.current) { setTextFallback(true); setNotice('I can’t hear you yet.'); await say('I can’t hear you yet.') } }
    finally { starting.current = false; if (mounted.current) { setOpeningMic(false); if (!capture.current) setManual(false) } }
  }
  function toggleSpeaking() {
    if (textFallback && !busy && !speaking) void recoverMicrophone()
    else if (starting.current || busyRef.current || session.busy || speaking) void pause()
    else if (capture.current) void finishSpeaking()
    else if (wakeState === 'listening') void invoke('voice_listen_finish', { generation: wakeGeneration.current }).catch(() => {})
    else void beginSpeaking()
  }
  async function recoverMicrophone() {
    if (!native()) return
    try {
      if (permissions?.microphone === 'denied' || permissions?.microphone === 'restricted') {
        await invoke('open_microphone_permissions')
      } else {
        const access = await invoke<Permissions>('permission_status')
        if (!mounted.current) return
        granted.current = access; setPermissions(access)
        if (access.voiceAvailable) { setTextFallback(false); setVoicePaused(false); setWakeUnavailable(false); setNotice('') }
        else await invoke('open_microphone_permissions')
      }
    } catch { setNotice('Microphone access is unavailable. Your words can still reach me here.') }
  }
  const closeHistory = useCallback(() => { setHistoryOpen(false); historyTrigger.current?.focus() }, [])
  async function showHistory() {
    if (capture.current || starting.current || wakeState === 'listening') await pause()
    else await stopWake()
    setHistoryOpen(true)
  }
  useEffect(() => {
    if (!recording) return
    const timer = window.setTimeout(() => { void finishSpeaking() }, 120000)
    return () => window.clearTimeout(timer)
    // Native capture bounds its storage independently of the page.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording])
  useEffect(() => () => { abandonCapture.current = true; ++turn.current; void stopWake(); void quiet(); if (capture.current && native()) void invoke('voice_cancel').catch(() => {}) }, [])
  function material(after: number) {
    return <>{session.chart && (session.chartAfterMessage || 1) === after && <ReadingChart key={`chart-${session.revision}`} chart={session.chart} place={session.place} dark={dark} facts={session.facts} />}{session.sections.filter(s => (s.after_message || 1) === after).map(section => <ReadingPassage key={`${section.revision}-${section.method_stage || section.title}`} section={section} />)}</>
  }
  const listening = recording || wakeState === 'listening'
  const voiceState = listening ? 'listening' : openingMic || wakeState === 'opening' || native() && !ready ? 'opening' : speaking ? 'speaking' : busy ? 'thinking' : wakeState === 'waiting' ? 'waiting' : 'idle'
  const voiceLabel = listening ? 'Finish speaking' : openingMic ? 'Cancel listening' : busy ? 'Pause reading' : speaking ? 'Pause speech' : textFallback ? 'Microphone access' : 'Speak your question'
  const voiceTip = notice || (voiceState === 'waiting' ? 'Say “Oracle”, or touch to speak.' : voiceLabel)
  return <>
    <button ref={historyTrigger} type="button" className="quiet-icon history-trigger" aria-label="Earlier readings" aria-haspopup="dialog" aria-expanded={historyOpen} aria-controls="reading-history" title="Earlier readings" onClick={() => void showHistory()}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4h10a2 2 0 0 1 2 2v11M5 7h10a2 2 0 0 1 2 2v10H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z" /><path d="M7 12h6M7 15h4" /></svg></button>
    <main ref={page} className="unfolding-page" data-listening={listening} data-thinking={busy} tabIndex={-1} aria-label="Your unfolding reading">
      <article className="living-document">
        <p className="opening-question">{OPENING}</p>
        <div className="document-conversation" role="log" aria-label="Conversation" aria-live="polite" aria-relevant="additions text">
          {session.messages.map((message, i) => <Fragment key={i}><div className={`passage ${message.role}`}>{message.text.split('\n\n').map((p, n) => <p key={n}>{p}</p>)}</div>{material(i + 1)}</Fragment>)}
        </div>
        {busy && <p className="visually-hidden" role="status">{session.status || 'Considering your question.'}</p>}
        {notice && <p id="reading-notice" className="visually-hidden" role="alert">{notice}</p>}
        {textFallback && <form className="fallback-words" onSubmit={e => { e.preventDefault(); void submitText() }}>
          <textarea aria-label="Your words" title="Write your question; Return sends it, Shift–Return adds a line." value={draft} maxLength={8000} disabled={!ready || busy} rows={2} onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void submitText() } }} />
          <button type="submit" className="quiet-icon" aria-label="Send" title="Send your words" disabled={!ready || busy || !draft.trim()}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 7-7 7 7M12 5v14" /></svg></button>
        </form>}
        <div ref={tail} />
      </article>
    </main>
    <div className="listening-dock"><button type="button" className="listening-orb" data-state={voiceState} data-notice={!!notice} aria-label={voiceLabel} aria-describedby={notice ? 'reading-notice' : undefined} aria-pressed={listening} title={voiceTip} onClick={toggleSpeaking}><span aria-hidden="true">{notice ? '!' : '?'}</span></button><span className="visually-hidden" role="status">{listening ? 'Listening.' : openingMic ? 'Opening the microphone.' : ''}</span></div>
    <ReadingHistory open={historyOpen} close={closeHistory} saved={session.savedReadings} revisions={session.revisions} records={session.method?.records} audit={session.audit} unavailable={busy || listening || openingMic || native() && !ready} openReading={id => void openLeaf(id)} />
  </>
}
