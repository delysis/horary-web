import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { calculateChart, dtLocalNowValue, formatDeg } from './chartCalc'
import { ChartWheel } from './ChartWheel'
import { AspectGrid } from './AspectGrid'
import { AiPanel } from './AiPanel'
import { ReviewNotes } from './ReviewNotes'
import { MethodReview } from './MethodReview'
import type { HoraryInterpretation } from './tauriBridge'
import { resolveChartTime, validateDms, nudge_chart_time, applyBookMethod } from './ai/aiCoreWasm'
import tzlookup from 'tz-lookup'
import { buildHoraryChartFacts } from './ai/chartFacts.js'
import { getAllPositions } from './astro/planets.js'
import { dateToJD } from './astro/time.js'
import { reverseLocalCity, searchLocalCities, type LocalCity } from './localCities'
import { LOCATION_DETECT_TIMEOUT_MS, browserPermissionResetHint, locationFailureMessage, locationUnsupportedMessage } from './locationMessages'
import {
  cancelCurrentLocationDetection,
  geocodeLocation as geocodeLocationNative,
  isTauriRuntime,
  listenCurrentLocationDetectionEvents,
  reverseGeocodeLocation as reverseGeocodeLocationNative,
  startCurrentLocationDetection,
} from './tauriBridge.ts'
import './App.css'

function decimalToDMS(decimal: number): { deg: string; min: string; sec: string } {
  const abs = Math.abs(decimal)
  let deg = Math.floor(abs)
  const minFloat = (abs - deg) * 60
  let min = Math.floor(minFloat)
  let sec = Math.round((minFloat - min) * 60)
  if (sec >= 60) { sec = 0; min += 1 }
  if (min >= 60) { min = 0; deg += 1 }
  return { deg: String(deg), min: String(min).padStart(2, '0'), sec: String(sec).padStart(2, '0') }
}

function fmtDMS(deg: string, min: string, sec: string, sign: string) {
  return `${deg}° ${min}′ ${sec}″ ${sign}`
}

function App() {
  const now = dtLocalNowValue()
  const [dateLocal, setDateLocal] = useState(now.slice(0, 10))
  const [timeHour, setTimeHour] = useState(now.slice(11, 13))
  const [timeMinute, setTimeMinute] = useState(now.slice(14, 16))
  const [amPm, setAmPm] = useState<'AM' | 'PM'>(Number(now.slice(11, 13)) < 12 ? 'AM' : 'PM')

  const [isEditing, setIsEditing] = useState(false)
  const [castSnapshot, setCastSnapshot] = useState({ date: now.slice(0, 10), hour: now.slice(11, 13), minute: now.slice(14, 16), amPm: Number(now.slice(11, 13)) < 12 ? 'AM' as const : 'PM' as const })
  const [geolocating, setGeolocating] = useState(false)
  const [locationDetected, setLocationDetected] = useState(false)
  const [locationSet, setLocationSet] = useState(false)
  const [geoError, setGeoError] = useState('')
  const detectedLocation = useRef<{ latDeg: string; latMin: string; latSec: string; latSign: 'N' | 'S'; lonDeg: string; lonMin: string; lonSec: string; lonSign: 'E' | 'W'; timezone: string } | null>(null)

  const [locationName, setLocationName] = useState('')
  const [detectedCityName, setDetectedCityName] = useState('')
  const [locationCandidates, setLocationCandidates] = useState<LocalCity[]>([])
  const [locationSearching, setLocationSearching] = useState(false)
  const [locationError, setLocationError] = useState('')
  const [timeOccurrence, setTimeOccurrence] = useState('')
  const [enableJudgement, setEnableJudgement] = useState(false)
  const [locationTimezone, setLocationTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone)

  const [latDeg, setLatDeg] = useState('')
  const [latMin, setLatMin] = useState('')
  const [latSec, setLatSec] = useState('')
  const [latSign, setLatSign] = useState<'N' | 'S'>('N')
  const [lonDeg, setLonDeg] = useState('')
  const [lonMin, setLonMin] = useState('')
  const [lonSec, setLonSec] = useState('')
  const [lonSign, setLonSign] = useState<'E' | 'W'>('W')
  const [reviewInterpretation, setReviewInterpretation] = useState<HoraryInterpretation | null>(null)
  const [question, setQuestion] = useState('')
  const [submitError, setSubmitError] = useState('')
  const [judgementActive, setJudgementActive] = useState(false)
  const [judgementRequestId, setJudgementRequestId] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [showAngles, setShowAngles] = useState(() => localStorage.getItem('showAngles') !== 'false')
  const [showHouses, setShowHouses] = useState(() => localStorage.getItem('showHouses') === 'true')
  const [showPlanets, setShowPlanets] = useState(() => localStorage.getItem('showPlanets') !== 'false')
  const [showAspects, setShowAspects] = useState(() => localStorage.getItem('showAspects') === 'true')
  const [showAspectGrid, setShowAspectGrid] = useState(() => localStorage.getItem('showAspectGrid') === 'true')
  const [use24Hour, setUse24Hour] = useState(() => localStorage.getItem('use24Hour') === 'true')
  const [nudgeUnit, setNudgeUnit] = useState<'minute'|'hour'|'day'|'week'|'month'|'year'>('day')
  const [darkMode, setDarkMode] = useState(() => {
    const stored = localStorage.getItem('darkMode')
    return stored !== null ? stored === 'true' : window.matchMedia('(prefers-color-scheme: dark)').matches
  })
  useEffect(() => {
    document.body.style.backgroundColor = darkMode ? '#171e22' : '#f6f3ed'
    document.body.style.color = darkMode ? '#e9e6df' : '#283c3d'
  }, [darkMode])
  const [locationInputMode, setLocationInputMode] = useState<'search' | 'coordinates'>(() => localStorage.getItem('locationInputMode') === 'coordinates' ? 'coordinates' : 'search')
  const settingsRef = useRef<HTMLDivElement>(null)
  const questionViewRef = useRef<HTMLTextAreaElement>(null)
  const coordFallback = useRef<Record<string, string>>({})
  const locationRequestId = useRef(0)
  const activeNativeLocationDetection = useRef<{
    detectionId: string
    requestId: number
    timeoutId: number
    unlisten: () => void
  } | null>(null)

  useLayoutEffect(() => {
    const ref = questionViewRef.current
    if (ref) { ref.style.height = 'auto'; ref.style.height = ref.scrollHeight + 'px' }
  }, [question, isEditing])

  useEffect(() => {
    if (!showSettings) return
    function handleClickOutside(e: MouseEvent) {
      if ((e.target as Element).closest('[aria-controls="chart-inspector"]')) return
      if (settingsRef.current && !settingsRef.current.contains(e.target as Node)) {
        setShowSettings(false)
      }
    }
    function handleEscape(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setShowSettings(false)
        document.querySelector<HTMLButtonElement>('[aria-controls="chart-inspector"]')?.focus()
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [showSettings])

  useEffect(() => {
    return () => {
      const active = activeNativeLocationDetection.current
      if (!active) return
      window.clearTimeout(active.timeoutId)
      active.unlisten()
      void cancelCurrentLocationDetection(active.detectionId).catch(() => {})
      activeNativeLocationDetection.current = null
    }
  }, [])

  function clearActiveNativeLocationDetection(detectionId?: string) {
    const active = activeNativeLocationDetection.current
    if (!active || (detectionId && active.detectionId !== detectionId)) return null
    window.clearTimeout(active.timeoutId)
    active.unlisten()
    activeNativeLocationDetection.current = null
    return active
  }

  function cancelActiveNativeLocationDetection() {
    const active = clearActiveNativeLocationDetection()
    if (active) {
      void cancelCurrentLocationDetection(active.detectionId).catch(() => {})
    }
    return active
  }

  async function applyDetectedCoordinates(lat: number, lon: number, requestId?: number) {
    const ld = decimalToDMS(lat)
    const lo = decimalToDMS(lon)
    setLatDeg(ld.deg); setLatMin(ld.min); setLatSec(ld.sec)
    setLatSign(lat >= 0 ? 'N' : 'S')
    setLonDeg(lo.deg); setLonMin(lo.min); setLonSec(lo.sec)
    setLonSign(lon >= 0 ? 'E' : 'W')
    let tz = tzlookup(lat, lon)
    let cityName = ''
    if (isTauriRuntime()) {
      try {
        const label = await reverseGeocodeLocationNative(lat, lon)
        if (label?.timezone) tz = label.timezone
        if (label?.label) cityName = label.label
      } catch { /* keep offline coordinate timezone */ }
    } else {
      const label = reverseLocalCity(lat, lon)
      if (label?.timezone) tz = label.timezone
      if (label?.label) cityName = label.label
    }
    if (requestId != null && requestId !== locationRequestId.current) return
    setLocationTimezone(tz)
    if (!isEditing) applyNow(tz)
    detectedLocation.current = {
      latDeg: ld.deg, latMin: ld.min, latSec: ld.sec, latSign: lat >= 0 ? 'N' : 'S',
      lonDeg: lo.deg, lonMin: lo.min, lonSec: lo.sec, lonSign: lon >= 0 ? 'E' : 'W',
      timezone: tz,
    }
    setDetectedCityName(cityName)
    setLocationDetected(true)
    setLocationSet(true); setSubmitError('')
    setGeolocating(false)
  }

  async function detectLocation() {
    const requestId = ++locationRequestId.current
    const nativeRuntime = isTauriRuntime()
    cancelActiveNativeLocationDetection()
    setGeolocating(true)
    setGeoError('')

    if (nativeRuntime) {
      let unlisten: (() => void) | null = null
      try {
        unlisten = await listenCurrentLocationDetectionEvents({
          detected: (payload) => {
            const active = activeNativeLocationDetection.current
            if (!active || active.requestId !== requestId || active.detectionId !== payload.detectionId) return
            clearActiveNativeLocationDetection(payload.detectionId)
            void applyDetectedCoordinates(payload.location.latitude, payload.location.longitude, requestId).catch((error) => {
              if (requestId !== locationRequestId.current) return
              setGeolocating(false)
              setLocationDetected(false)
              setGeoError(locationFailureMessage(error, true))
            })
          },
          error: (payload) => {
            const active = activeNativeLocationDetection.current
            if (!active || active.requestId !== requestId || active.detectionId !== payload.detectionId) return
            clearActiveNativeLocationDetection(payload.detectionId)
            setGeolocating(false)
            setLocationDetected(false)
            setGeoError(locationFailureMessage(new Error(payload.message), true))
          },
          cancelled: (payload) => {
            const active = activeNativeLocationDetection.current
            if (!active || active.requestId !== requestId || active.detectionId !== payload.detectionId) return
            clearActiveNativeLocationDetection(payload.detectionId)
            setGeolocating(false)
            setLocationDetected(false)
            setGeoError('Location detection cancelled. Search for a city or enter coordinates manually.')
          },
        })
        if (requestId !== locationRequestId.current) {
          unlisten()
          return
        }
        const started = await startCurrentLocationDetection(LOCATION_DETECT_TIMEOUT_MS)
        if (requestId !== locationRequestId.current) {
          unlisten()
          void cancelCurrentLocationDetection(started.detectionId).catch(() => {})
          return
        }
        const timeoutId = window.setTimeout(() => {
          const active = activeNativeLocationDetection.current
          if (!active || active.requestId !== requestId || active.detectionId !== started.detectionId) return
          clearActiveNativeLocationDetection(started.detectionId)
          locationRequestId.current += 1
          setGeolocating(false)
          setLocationDetected(false)
          setGeoError(locationFailureMessage({ code: 3, message: 'Location detection timed out.' }, true))
          void cancelCurrentLocationDetection(started.detectionId).catch(() => {})
        }, started.timeoutMs + 1000)
        activeNativeLocationDetection.current = {
          detectionId: started.detectionId,
          requestId,
          timeoutId,
          unlisten,
        }
      } catch (error) {
        if (unlisten) unlisten()
        if (requestId !== locationRequestId.current) return
        setGeolocating(false)
        setLocationDetected(false)
        setGeoError(locationFailureMessage(error, true))
      }
      return
    }

    if (navigator.geolocation) {
      const timeoutId = window.setTimeout(() => {
        if (requestId !== locationRequestId.current) return
        locationRequestId.current += 1
        setGeolocating(false)
        setLocationDetected(false)
        setGeoError(locationFailureMessage({ code: 3, message: 'Location detection timed out.' }, nativeRuntime))
      }, LOCATION_DETECT_TIMEOUT_MS + 1000)

      navigator.geolocation.getCurrentPosition(
        async (pos) => {
          window.clearTimeout(timeoutId)
          if (requestId !== locationRequestId.current) return
          try {
            await applyDetectedCoordinates(pos.coords.latitude, pos.coords.longitude, requestId)
          } catch (error) {
            if (requestId !== locationRequestId.current) return
            setGeolocating(false)
            setLocationDetected(false)
            setGeoError(locationFailureMessage(error, nativeRuntime))
          }
        },
        (err) => {
          window.clearTimeout(timeoutId)
          if (requestId === locationRequestId.current) {
            setGeolocating(false)
            setLocationDetected(false)
            setGeoError(locationFailureMessage(err, nativeRuntime))
          }
        },
        { enableHighAccuracy: false, timeout: LOCATION_DETECT_TIMEOUT_MS, maximumAge: 300000 }
      )
      return
    }

    setGeolocating(false)
    setLocationDetected(false)
    setGeoError(locationUnsupportedMessage(false))
  }

  function cancelDetectLocation() {
    locationRequestId.current += 1
    cancelActiveNativeLocationDetection()
    setGeolocating(false)
    setLocationDetected(false)
    setGeoError('Location detection cancelled. Search for a city or enter coordinates manually.')
  }

  function saveCastSnapshot() {
    if (parsed.error) { setSubmitError(parsed.error); return false }
    if (parsed.dt > new Date()) {
      setSubmitError('The date and time cannot be in the future.')
      return false
    }
    setSubmitError('')
    setCastSnapshot({ date: dateLocal, hour: timeHour, minute: timeMinute, amPm })
    return true
  }

  function generateJudgement() {
    if (!locationSet) { setSubmitError('Set the location before casting a chart.'); return }
    if (!saveCastSnapshot()) return
    if (chart.error) { setSubmitError(chart.error); return }
    if (!enableJudgement) return
    if (!question.trim()) {
      setSubmitError('Write the horary question before generating a judgement.')
      return
    }
    setJudgementRequestId(requestId => requestId + 1)
  }

  function resetToCastTime() {
    setDateLocal(castSnapshot.date)
    setTimeHour(castSnapshot.hour)
    setTimeMinute(castSnapshot.minute)
    setAmPm(castSnapshot.amPm)
  }

  function applyNow(timezone: string) {
    const n = dtLocalNowValue(timezone)
    const snapshot = { date: n.slice(0, 10), hour: n.slice(11, 13), minute: n.slice(14, 16), amPm: Number(n.slice(11, 13)) < 12 ? 'AM' as const : 'PM' as const }
    setDateLocal(snapshot.date); setTimeHour(snapshot.hour); setTimeMinute(snapshot.minute); setAmPm(snapshot.amPm)
    setCastSnapshot(snapshot)
    setTimeOccurrence('')
  }

  function resetToNow() {
    const d = detectedLocation.current
    const timezone = d?.timezone || locationTimezone
    applyNow(timezone)
    if (d) {
      setLatDeg(d.latDeg); setLatMin(d.latMin); setLatSec(d.latSec); setLatSign(d.latSign)
      setLonDeg(d.lonDeg); setLonMin(d.lonMin); setLonSec(d.lonSec); setLonSign(d.lonSign)
      setLocationSet(true); setSubmitError('')
      setLocationDetected(true)
      setDetectedCityName(reverseLocalCity(validateDms(d.latDeg, d.latMin, d.latSec, d.latSign), validateDms(d.lonDeg, d.lonMin, d.lonSec, d.lonSign))?.label || '')
    }
    setLocationTimezone(timezone)
    setLocationName('')
    setSubmitError('')
    setIsEditing(false)
  }

const display12Hour = (() => {
    const h = Number(timeHour) || 0
    if (h === 0 || h === 12) return '12'
    return String(h > 12 ? h - 12 : h)
  })()

  function handleHour12Change(val: string) {
    let h = Number(val)
    if (isNaN(h)) return
    const oldH = Number(display12Hour)
    // Wrap around
    if (h > 12) h = 1
    if (h < 1) h = 12
    // Flip AM/PM when crossing between 11 and 12
    let newAmPm = amPm
    if (h === 12 && oldH === 11) newAmPm = amPm === 'AM' ? 'PM' : 'AM'
    if (h === 11 && oldH === 12) newAmPm = amPm === 'AM' ? 'PM' : 'AM'
    setAmPm(newAmPm)
    setTimeHour(newAmPm === 'AM' ? String(h === 12 ? 0 : h) : String(h === 12 ? 12 : h + 12))
  }

  function handleDatePartChange(part: 'year' | 'month' | 'day', value: string) {
    const [y, m, d] = dateLocal.split('-').map(Number)
    let ny = y, nm = m, nd = d
    if (part === 'year') ny = parseInt(value)
    if (part === 'month') nm = parseInt(value)
    if (part === 'day') nd = parseInt(value)
    const maxDay = new Date(ny, nm, 0).getDate()
    if (nd > maxDay) nd = maxDay
    setDateLocal(`${String(ny).padStart(4, '0')}-${String(nm).padStart(2, '0')}-${String(nd).padStart(2, '0')}`)
    setSubmitError('')
  }

  function handleAmPmChange(val: 'AM' | 'PM') {
    setAmPm(val)
    const h = Number(timeHour) || 0
    if (val === 'AM' && h >= 12) setTimeHour(String(h - 12))
    if (val === 'PM' && h < 12) setTimeHour(String(h + 12))
  }

  function nudgeTime(direction: number) {
    try {
      const next = nudge_chart_time(`${dateLocal}T${timeHour.padStart(2, '0')}:${timeMinute.padStart(2, '0')}`, nudgeUnit, direction)
      setDateLocal(next.slice(0, 10)); setTimeHour(next.slice(11, 13)); setTimeMinute(next.slice(14, 16))
      setAmPm(Number(next.slice(11, 13)) < 12 ? 'AM' : 'PM'); setTimeOccurrence(''); setSubmitError('')
    } catch (error) { setSubmitError(String(error)) }
  }

  function useManualCoordinates() {
    if (latDeg.trim() === '' || lonDeg.trim() === '') {
      setLocationError('Enter latitude and longitude first.')
      return
    }

    let lat: number, lon: number
    try {
      lat = validateDms(latDeg, latMin, latSec, latSign)
      lon = validateDms(lonDeg, lonMin, lonSec, lonSign)
    } catch (error) { setLocationError(String(error)); return }

    locationRequestId.current += 1
    cancelActiveNativeLocationDetection()
    setGeolocating(false)
    setGeoError('')
    setLocationError('')
    setLocationTimezone(tzlookup(lat, lon))
    if (!isEditing) applyNow(tzlookup(lat, lon))
    setDetectedCityName(`${fmtDMS(latDeg || '0', latMin || '00', latSec || '00', latSign)}, ${fmtDMS(lonDeg || '0', lonMin || '00', lonSec || '00', lonSign)}`)
    setLocationDetected(true)
    setLocationSet(true); setSubmitError('')
  }

  function chooseLocation(result: LocalCity) {
    locationRequestId.current++
    cancelActiveNativeLocationDetection()
    setGeolocating(false)
    const lat = result.latitude, lon = result.longitude
    const ld = decimalToDMS(lat), lo = decimalToDMS(lon)
    setLatDeg(ld.deg); setLatMin(ld.min); setLatSec(ld.sec); setLatSign(lat >= 0 ? 'N' : 'S')
    setLonDeg(lo.deg); setLonMin(lo.min); setLonSec(lo.sec); setLonSign(lon >= 0 ? 'E' : 'W')
    const timezone = result.timezone || tzlookup(lat, lon)
    setLocationTimezone(timezone)
    if (!isEditing) applyNow(timezone)
    setLocationSet(true); setSubmitError(''); setLocationDetected(true); setDetectedCityName(result.label)
    setLocationCandidates([]); setLocationError(''); setGeoError('')
  }

  async function searchLocation() {
    if (!locationName.trim() || locationSearching) return
    const requestId = ++locationRequestId.current
    cancelActiveNativeLocationDetection()
    setGeolocating(false); setLocationSearching(true); setLocationError(''); setLocationCandidates([])
    try {
      const data = isTauriRuntime() ? await geocodeLocationNative(locationName) : searchLocalCities(locationName, 8)
      if (requestId !== locationRequestId.current) return
      if (!data.length) { setLocationError('No match in the offline city list. Enter coordinates manually.'); return }
      setLocationCandidates(data)
    } catch { setLocationError('The offline location index could not be read. Enter coordinates manually.') }
    finally { setLocationSearching(false) }
  }

  const parsed = useMemo(() => {
    let dt = new Date(NaN)
    try {
      if (!timeHour.trim() || !timeMinute.trim()) throw new Error('Enter both an hour and a minute.')
      dt = resolveChartTime(`${dateLocal}T${timeHour.padStart(2, '0')}:${timeMinute.padStart(2, '0')}`, locationTimezone, timeOccurrence)
      const lat = validateDms(latDeg, latMin, latSec, latSign)
      const lon = validateDms(lonDeg, lonMin, lonSec, lonSign)
      return { dt, lat, lon, error: '' }
    } catch (error) { return { dt, lat: NaN, lon: NaN, error: String(error) } }
  }, [dateLocal, timeHour, timeMinute, locationTimezone, timeOccurrence, latDeg, latMin, latSec, latSign, lonDeg, lonMin, lonSec, lonSign])

  const chart = useMemo(() => calculateChart(parsed.dt, parsed.lat, parsed.lon, locationTimezone), [parsed, locationTimezone])
  const aiChartFacts = useMemo(() => {
    if (!chart.horary) return null
    const facts = buildHoraryChartFacts(chart.horary, {
      locationLabel: `${parsed.lat}, ${parsed.lon}`,
      timezone: locationTimezone,
      localTime: `${dateLocal} ${timeHour.padStart(2, '0')}:${timeMinute.padStart(2, '0')} ${locationTimezone}`,
    })
    const names = ['Sun','Moon','Mercury','Venus','Mars','Jupiter','Saturn']
    const jd = dateToJD(parsed.dt)
    const motionSamples = Array.from({ length: 169 }, (_, hours) => ({ hours,
      positions: Object.fromEntries(Object.entries(getAllPositions(jd + hours / 24, names)).map(([name, position]) => [name, position.longitude])),
    }))
    return applyBookMethod({ ...facts, motionSamples })
  }, [chart.horary, dateLocal, locationTimezone, parsed, timeHour, timeMinute])

  useEffect(() => { setReviewInterpretation(null) }, [question, aiChartFacts])

  const locationFields = (
    <>
      {locationInputMode === 'search' ? (
        <div style={{ marginBottom: 12 }}>
          <label>
            Location
            <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
              <input
                type="text"
                value={locationName}
                onChange={(e) => setLocationName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && searchLocation()}
                placeholder="Search for a city or place"
                style={{ flex: 1, padding: '0.5em 0.8em' }}
              />
              <button onClick={searchLocation} disabled={locationSearching}>
                {locationSearching ? 'Searching…' : 'Search'}
              </button>
            </div>
            {locationCandidates.length ? <ul aria-label="Choose a location">{locationCandidates.map((candidate, i) => <li key={i}><button type="button" onClick={() => chooseLocation(candidate)}>{candidate.label}</button></li>)}</ul> : null}
            {locationError && <div style={{ color: '#c55', marginTop: 4 }}>{locationError}</div>}
            <button
              type="button"
              onClick={() => { localStorage.setItem('locationInputMode', 'coordinates'); setLocationInputMode('coordinates'); setLocationError('') }}
              style={{ marginTop: 8 }}
            >
              Enter coordinates manually
            </button>
          </label>
        </div>
      ) : (
        <div style={{ marginBottom: 12 }}>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <label>
              Latitude
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap', marginTop: 4 }}>
                <input type="number" min={0} max={90} value={latDeg} onFocus={() => { coordFallback.current.latDeg = latDeg }} onBlur={(e) => { if (e.target.value === '') setLatDeg(coordFallback.current.latDeg ?? '0') }} onChange={(e) => setLatDeg(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Degrees" />
                <span>°</span>
                <input type="number" min={0} max={59} value={latMin} onFocus={() => { coordFallback.current.latMin = latMin }} onBlur={(e) => { if (e.target.value === '') setLatMin(coordFallback.current.latMin ?? '0') }} onChange={(e) => setLatMin(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Minutes" />
                <span>′</span>
                <input type="number" min={0} max={59} value={latSec} onFocus={() => { coordFallback.current.latSec = latSec }} onBlur={(e) => { if (e.target.value === '') setLatSec(coordFallback.current.latSec ?? '0') }} onChange={(e) => setLatSec(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Seconds" />
                <span>″</span>
                <select value={latSign} onChange={(e) => setLatSign(e.target.value as 'N' | 'S')} style={{ padding: '0.5em 0.8em' }}>
                  <option value="N">N</option>
                  <option value="S">S</option>
                </select>
              </div>
            </label>
            <label>
              Longitude
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap', marginTop: 4 }}>
                <input type="number" min={0} max={180} value={lonDeg} onFocus={() => { coordFallback.current.lonDeg = lonDeg }} onBlur={(e) => { if (e.target.value === '') setLonDeg(coordFallback.current.lonDeg ?? '0') }} onChange={(e) => setLonDeg(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Degrees" />
                <span>°</span>
                <input type="number" min={0} max={59} value={lonMin} onFocus={() => { coordFallback.current.lonMin = lonMin }} onBlur={(e) => { if (e.target.value === '') setLonMin(coordFallback.current.lonMin ?? '0') }} onChange={(e) => setLonMin(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Minutes" />
                <span>′</span>
                <input type="number" min={0} max={59} value={lonSec} onFocus={() => { coordFallback.current.lonSec = lonSec }} onBlur={(e) => { if (e.target.value === '') setLonSec(coordFallback.current.lonSec ?? '0') }} onChange={(e) => setLonSec(e.target.value)} style={{ width: '6ch', padding: '0.5em 0.4em' }} title="Seconds" />
                <span>″</span>
                <select value={lonSign} onChange={(e) => setLonSign(e.target.value as 'E' | 'W')} style={{ padding: '0.5em 0.8em' }}>
                  <option value="E">E</option>
                  <option value="W">W</option>
                </select>
              </div>
            </label>
          </div>
          <button onClick={useManualCoordinates} style={{ marginTop: 12 }}>
            Use coordinates
          </button>
          <button type="button" onClick={() => { localStorage.setItem('locationInputMode', 'search'); setLocationInputMode('search'); setLocationError('') }} style={{ marginLeft: 8 }}>
            Search for a city
          </button>
          {locationError && <div style={{ color: '#c55', marginTop: 4 }}>{locationError}</div>}
        </div>
      )}
      <label style={{ display: 'block', marginTop: 12 }}>
        Timezone
        <input aria-label="Timezone" value={locationTimezone} onChange={e => { setLocationTimezone(e.target.value); setTimeOccurrence('') }} style={{ width: '100%' }} />
      </label>
      {parsed.error.includes('occurs twice') || timeOccurrence ? <label>
        Repeated local time
        <select aria-label="Repeated local time" value={timeOccurrence} onChange={e => setTimeOccurrence(e.target.value)}>
          <option value="">Choose occurrence</option><option value="earlier">First occurrence</option><option value="later">Second occurrence</option>
        </select>
      </label> : null}
    </>
  )

  return (
    <div className={`horary-app ${darkMode ? 'night-mode' : ''} ${showSettings ? 'inspector-open' : ''}`}>
      <header className="app-toolbar">
        <a className="wordmark" href="#question">Horary<span aria-hidden="true">✦</span></a>
        <span className="toolbar-caption">A question. A moment. A chart.</span>
        <button aria-expanded={showSettings} aria-controls="chart-inspector" className="quiet-button" onClick={() => setShowSettings(s => !s)}>Chart settings</button>
      </header>
      <main className="reading-workspace">
        <section className="question-composer" id="question">
          <label htmlFor="horary-question" className="eyebrow">What is your question?</label>
          <textarea id="horary-question" ref={questionViewRef} value={question} onChange={e => setQuestion(e.target.value)}
            placeholder="Where is the lost ring?" rows={2} onKeyDown={e => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); generateJudgement() } }} />
          <div className="cast-bar">
            <button className="cast-context quiet-button" onClick={() => { setIsEditing(true); setShowSettings(true) }}>
              {locationSet ? detectedCityName || 'Selected location' : 'Choose the astrologer’s location'}<span> · {dateLocal} · {timeHour.padStart(2, '0')}:{timeMinute.padStart(2, '0')} · {locationTimezone}</span>
            </button>
            <button className="primary-button" onClick={generateJudgement} disabled={judgementActive}>{judgementActive ? 'Reading…' : enableJudgement ? 'Read chart' : 'Cast chart'}</button>
          </div>
          {submitError ? <p role="alert" className="ai-error">{submitError}</p> : null}
          {!locationSet ? <div className="first-location">
            <p>Use the place where the astrologer understands the question.</p>
            {locationFields}
            <button disabled={geolocating} onClick={detectLocation}>{geolocating ? 'Finding your location…' : 'Use my location'}</button>
            {geolocating ? <button onClick={cancelDetectLocation}>Cancel</button> : null}
            {geoError ? <p role="alert" className="ai-error">{geoError}{browserPermissionResetHint(navigator.userAgent, isTauriRuntime())}</p> : null}
          </div> : null}
        </section>
        {locationSet && chart.summary ? <div className="chart-reading-layout">
          <section className="chart-stage" aria-label="Horary chart">
            <div className="section-heading"><span className="eyebrow">The chart</span><span className="subtle">Regiomontanus · Tropical</span></div>
            <ChartWheel data={chart.summary.astroChartData} darkMode={darkMode} />
            <div className="chart-caption"><span>ASC {chart.summary.ascendant}</span><span>MC {chart.summary.midheaven}</span></div>
            <details className="time-explorer"><summary>Explore this moment</summary>
              <p>Move the chart to inspect a nearby time. The reading clears when its evidence changes.</p>
              <div className="time-explorer-controls">
                <button aria-label="Earlier" onClick={() => nudgeTime(-1)}>←</button>
                <select value={nudgeUnit} onChange={e => setNudgeUnit(e.target.value as typeof nudgeUnit)} aria-label="Time increment">
                  {['minute','hour','day','week','month','year'].map(unit => <option key={unit} value={unit}>{unit}</option>)}
                </select>
                <button aria-label="Later" onClick={() => nudgeTime(1)}>→</button><button onClick={resetToCastTime}>Original moment</button>
              </div>
            </details>
          </section>
          <section className="reading-stage" aria-label="Reading">
            {enableJudgement ? <AiPanel requestId={judgementRequestId} question={question} chartFacts={aiChartFacts} darkMode={darkMode}
              onActivityChange={setJudgementActive} onInterpretationChange={setReviewInterpretation} />
              : <div className="reading-invitation"><span className="eyebrow">The reading</span><h2>Start with the question.</h2>
                <p>Read the chart yourself, or invite a local model to offer an interpretation using Frawley’s method.</p>
                <button onClick={() => setEnableJudgement(true)}>Set up a local reading</button>
                <p className="subtle">Your question stays on this device.</p>
              </div>}
          </section>
        </div> : <div className="empty-reading" aria-hidden="true"><span>☉</span><p>Every chart begins with a particular moment.</p></div>}
        {locationSet && (parsed.error || chart.error) ? <p role="alert" className="ai-error">{parsed.error || chart.error}</p> : null}
        {locationSet && chart.summary ? <div className="study-tools">
          <details><summary>Chart tables</summary>
            {showAngles ? <p>ASC {chart.summary.ascendant} · DSC {chart.summary.descendant} · MC {chart.summary.midheaven} · IC {chart.summary.ic}</p> : null}
            {showHouses ? <table className="evidence-table"><thead><tr><th>House</th><th>Cusp</th></tr></thead><tbody>{chart.summary.houses.map(h => <tr key={h.house}><td>{h.house}</td><td>{h.sign} {h.formatted || formatDeg(h.eclipticDegrees)}</td></tr>)}</tbody></table> : null}
            {showPlanets ? <table className="evidence-table"><thead><tr><th>Planet</th><th>Position</th></tr></thead><tbody>{chart.summary.planets.map(p => <tr key={p.key}><td>{p.name}</td><td>{p.sign} {p.formatted}</td></tr>)}</tbody></table> : null}
            {showAspects ? <table className="evidence-table"><thead><tr><th>Aspect</th><th>Orb</th><th>Motion</th></tr></thead><tbody>{chart.summary.aspectsList.map((a,i) => <tr key={i}><td>{a.from} {a.type} {a.to}</td><td>{a.orb}</td><td>{a.applying === null ? 'Exact' : a.applying ? 'Applying' : 'Separating'}</td></tr>)}</tbody></table> : null}
            {showAspectGrid ? <AspectGrid planets={chart.summary.planets.map(p => p.name)} aspects={chart.summary.aspectsList} /> : null}
          </details>
          <details><summary>The method &amp; evidence</summary>
            <MethodReview question={question} chart={aiChartFacts} interpretation={enableJudgement ? reviewInterpretation : null} />
          </details>
          <details><summary>My review notes</summary><ReviewNotes question={question} chart={aiChartFacts} interpretation={enableJudgement ? reviewInterpretation : null} /></details>
        </div> : null}
        <footer className="workspace-footer"><span>Following John Frawley · The Horary Textbook</span><span>Working edition for Eileen’s review</span></footer>
      </main>
      {showSettings ? <aside className="chart-inspector" id="chart-inspector" aria-label="Chart settings" ref={settingsRef}>
        <div className="inspector-heading"><h2>Chart settings</h2><button aria-label="Close settings" onClick={() => setShowSettings(false)}>×</button></div>
        <p className="subtle">Cast for the moment and place where the astrologer understands the question. Frawley, pp. 137–141.</p>
        <label className="toggle-row"><input type="checkbox" checked={isEditing} onChange={e => { setIsEditing(e.target.checked); if (!e.target.checked) resetToNow() }} />Set a particular moment</label>
        {isEditing ? <>          <div style={{ display: 'flex', flexDirection: 'column', gap: 12, marginBottom: 12 }}>
            <label>
              Date
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', marginTop: 4, flexWrap: 'wrap' }}>
                {(() => {
                  const [dy, dm, dd] = dateLocal.split('-').map(Number)
                  const curYear = Number(dtLocalNowValue(locationTimezone).slice(0, 4))
                  const months = ['January','February','March','April','May','June','July','August','September','October','November','December']
                  const daysInMonth = new Date(dy, dm, 0).getDate()
                  const maxMonth = 12
                  const maxDay = daysInMonth
                  const selStyle = { padding: '0.5em 1.2em', fontSize: 'inherit', fontFamily: 'inherit' }
                  return (<>
                    <select value={dm} onChange={(e) => handleDatePartChange('month', e.target.value)} style={selStyle}>
                      {months.slice(0, maxMonth).map((name, i) => <option key={i+1} value={i+1}>{name}</option>)}
                    </select>
                    <select value={dd} onChange={(e) => handleDatePartChange('day', e.target.value)} style={selStyle}>
                      {Array.from({ length: maxDay }, (_, i) => i + 1).map(d => <option key={d} value={d}>{d}</option>)}
                    </select>
                    <select value={dy} onChange={(e) => handleDatePartChange('year', e.target.value)} style={selStyle}>
                      {Array.from({ length: curYear - 1800 + 1 }, (_, i) => curYear - i).map(y => <option key={y} value={y}>{y}</option>)}
                    </select>
                  </>)
                })()}
              </div>
            </label>
            <label>
              Time
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', marginTop: 4, flexWrap: 'wrap' }}>
                {(() => {
                  const numStyle = { width: '3.5ch', padding: '0.5em 0.4em', fontSize: 'inherit', fontFamily: 'inherit', textAlign: 'center' as const }
                  const selStyle = { padding: '0.5em 0.8em', fontSize: 'inherit', fontFamily: 'inherit' }
                  return use24Hour ? (
                    <>
                      <input type="number" min={0} max={23} value={timeHour} onChange={(e) => setTimeHour(e.target.value)} style={numStyle} title="Hour" />
                      <span>:</span>
                      <input type="number" min={0} max={59} value={timeMinute} onChange={(e) => setTimeMinute(e.target.value)} style={numStyle} title="Minute" />
                    </>
                  ) : (
                    <>
                      <input type="number" value={display12Hour} onChange={(e) => handleHour12Change(e.target.value)} style={numStyle} title="Hour" />
                      <span>:</span>
                      <input type="number" min={0} max={59} value={timeMinute} onChange={(e) => setTimeMinute(e.target.value)} style={numStyle} title="Minute" />
                      <select value={amPm} onChange={(e) => handleAmPmChange(e.target.value as 'AM' | 'PM')} style={selStyle}>
                        <option value="AM">AM</option>
                        <option value="PM">PM</option>
                      </select>
                    </>
                  )
                })()}
              </div>
            </label>
          </div>
</> : <p>{dateLocal} · {timeHour}:{timeMinute}</p>}
        <button onClick={resetToNow}>Use current time</button>
        {locationSet ? locationFields : null}
        {locationDetected ? <p className="subtle">{detectedCityName}</p> : null}
        <details><summary>Display</summary>
          <label className="toggle-row"><input type="checkbox" checked={darkMode} onChange={e => { setDarkMode(e.target.checked); localStorage.setItem('darkMode',String(e.target.checked)) }} />Night palette</label>
          <label className="toggle-row"><input type="checkbox" checked={use24Hour} onChange={e => { setUse24Hour(e.target.checked); localStorage.setItem('use24Hour',String(e.target.checked)) }} />24-hour time</label>
          {[['showAngles','Angles',showAngles,setShowAngles],['showHouses','Houses',showHouses,setShowHouses],['showPlanets','Planets',showPlanets,setShowPlanets],['showAspects','Aspects',showAspects,setShowAspects],['showAspectGrid','Aspect grid',showAspectGrid,setShowAspectGrid]].map(([key,label,value,setter]) => <label className="toggle-row" key={key as string}><input type="checkbox" checked={value as boolean} onChange={e => { localStorage.setItem(key as string,String(e.target.checked)); (setter as (v:boolean)=>void)(e.target.checked) }} />{label as string}</label>)}
        </details>
        <details><summary>Local interpretation</summary><label className="toggle-row"><input type="checkbox" checked={enableJudgement} onChange={e => setEnableJudgement(e.target.checked)} />Enable model readings</label><p className="subtle">Optional. The chart and its calculated evidence work without a model.</p></details>
      </aside> : null}
    </div>
  )
}

export default App
