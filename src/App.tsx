import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { calculateChart, dmsToDecimal, dtLocalNowValue, formatDeg } from './chartCalc'
import { ChartWheel } from './ChartWheel'
import { AspectGrid } from './AspectGrid'
import { AiPanel } from './AiPanel'
import { buildAiChartFacts } from './aiChartFacts'
import { buildHoraryChartFacts } from './ai/chartFacts.js'
import { calculateChart as calculateAdvancedHoraryChart } from './astro/chart.js'
import { reverseLocalCity, searchLocalCities } from './localCities'
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
  const [locationSearching, setLocationSearching] = useState(false)
  const [locationError, setLocationError] = useState('')
  const [locationTimezone, setLocationTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone)

  const [latDeg, setLatDeg] = useState('')
  const [latMin, setLatMin] = useState('')
  const [latSec, setLatSec] = useState('')
  const [latSign, setLatSign] = useState<'N' | 'S'>('N')
  const [lonDeg, setLonDeg] = useState('')
  const [lonMin, setLonMin] = useState('')
  const [lonSec, setLonSec] = useState('')
  const [lonSign, setLonSign] = useState<'E' | 'W'>('W')
  const [question, setQuestion] = useState('')
  const [submitError, setSubmitError] = useState('')
  const [judgementActive, setJudgementActive] = useState(false)
  const [judgementRequestId, setJudgementRequestId] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [showAngles, setShowAngles] = useState(() => localStorage.getItem('showAngles') === 'true')
  const [showHouses, setShowHouses] = useState(() => localStorage.getItem('showHouses') === 'true')
  const [showPlanets, setShowPlanets] = useState(() => localStorage.getItem('showPlanets') === 'true')
  const [showAspects, setShowAspects] = useState(() => localStorage.getItem('showAspects') === 'true')
  const [showAspectGrid, setShowAspectGrid] = useState(() => localStorage.getItem('showAspectGrid') === 'true')
  const [use24Hour, setUse24Hour] = useState(() => localStorage.getItem('use24Hour') === 'true')
  const [nudgeUnit, setNudgeUnit] = useState<'minute'|'hour'|'day'|'week'|'month'|'year'>('day')
  const [darkMode, setDarkMode] = useState(() => {
    const stored = localStorage.getItem('darkMode')
    return stored !== null ? stored === 'true' : window.matchMedia('(prefers-color-scheme: dark)').matches
  })
  useEffect(() => {
    document.body.style.backgroundColor = darkMode ? '#242424' : '#ffffff'
    document.body.style.color = darkMode ? 'rgba(255,255,255,0.87)' : '#213547'
  }, [darkMode])
  const [locationInputMode, setLocationInputMode] = useState<'search' | 'coordinates'>(() => localStorage.getItem('locationInputMode') === 'coordinates' ? 'coordinates' : 'search')
  const settingsRef = useRef<HTMLDivElement>(null)
  const questionEditRef = useRef<HTMLTextAreaElement>(null)
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
    const ref = isEditing ? questionEditRef.current : questionViewRef.current
    if (ref) { ref.style.height = 'auto'; ref.style.height = ref.scrollHeight + 'px' }
  }, [question, isEditing])

  useEffect(() => {
    if (!showSettings) return
    function handleClickOutside(e: MouseEvent) {
      if (settingsRef.current && !settingsRef.current.contains(e.target as Node)) {
        setShowSettings(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
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
    let tz = Intl.DateTimeFormat().resolvedOptions().timeZone
    let cityName = ''
    if (isTauriRuntime()) {
      try {
        const label = await reverseGeocodeLocationNative(lat, lon)
        if (label?.timezone) tz = label.timezone
        if (label?.label) cityName = label.label
      } catch { /* keep system timezone */ }
    } else {
      const label = reverseLocalCity(lat, lon)
      if (label?.timezone) tz = label.timezone
      if (label?.label) cityName = label.label
    }
    if (requestId != null && requestId !== locationRequestId.current) return
    setLocationTimezone(tz)
    detectedLocation.current = {
      latDeg: ld.deg, latMin: ld.min, latSec: ld.sec, latSign: lat >= 0 ? 'N' : 'S',
      lonDeg: lo.deg, lonMin: lo.min, lonSec: lo.sec, lonSign: lon >= 0 ? 'E' : 'W',
      timezone: tz,
    }
    setDetectedCityName(cityName)
    setLocationDetected(true)
    setLocationSet(true)
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
    const h = String(Number(timeHour) || 0).padStart(2, '0')
    const m = String(Number(timeMinute) || 0).padStart(2, '0')
    if (new Date(`${dateLocal}T${h}:${m}`) > new Date()) {
      setSubmitError('The date and time cannot be in the future.')
      return false
    }
    setSubmitError('')
    setCastSnapshot({ date: dateLocal, hour: timeHour, minute: timeMinute, amPm })
    return true
  }

  function generateJudgement() {
    if (!saveCastSnapshot()) return
    if (!question.trim()) {
      setSubmitError('Write the horary question before generating a judgement.')
      return
    }
    if (!isEditing && !locationSet) {
      setSubmitError('Set the location before generating a judgement.')
      return
    }
    setSubmitError('')
    setJudgementRequestId(requestId => requestId + 1)
  }

  function resetToCastTime() {
    setDateLocal(castSnapshot.date)
    setTimeHour(castSnapshot.hour)
    setTimeMinute(castSnapshot.minute)
    setAmPm(castSnapshot.amPm)
  }

  function resetToNow() {
    const n = dtLocalNowValue()
    setDateLocal(n.slice(0, 10))
    setTimeHour(n.slice(11, 13))
    setTimeMinute(n.slice(14, 16))
    setAmPm(Number(n.slice(11, 13)) < 12 ? 'AM' : 'PM')
    if (detectedLocation.current) {
      const d = detectedLocation.current
      setLatDeg(d.latDeg); setLatMin(d.latMin); setLatSec(d.latSec); setLatSign(d.latSign)
      setLonDeg(d.lonDeg); setLonMin(d.lonMin); setLonSec(d.lonSec); setLonSign(d.lonSign)
      setLocationTimezone(d.timezone)
    }
    setLocationName('')
    if (!detectedLocation.current) setLocationTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone)
    saveCastSnapshot()
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

  function nudgeTime(direction: 1 | -1) {
    const h = String(Number(timeHour) || 0).padStart(2, '0')
    const m = String(Number(timeMinute) || 0).padStart(2, '0')
    const dt = new Date(`${dateLocal}T${h}:${m}`)
    if (nudgeUnit === 'minute') dt.setMinutes(dt.getMinutes() + direction)
    else if (nudgeUnit === 'hour') dt.setHours(dt.getHours() + direction)
    else if (nudgeUnit === 'day') dt.setDate(dt.getDate() + direction)
    else if (nudgeUnit === 'week') dt.setDate(dt.getDate() + direction * 7)
    else if (nudgeUnit === 'month') dt.setMonth(dt.getMonth() + direction)
    else if (nudgeUnit === 'year') dt.setFullYear(dt.getFullYear() + direction)
    const y = dt.getFullYear()
    const mo = String(dt.getMonth() + 1).padStart(2, '0')
    const d = String(dt.getDate()).padStart(2, '0')
    setDateLocal(`${y}-${mo}-${d}`)
    setTimeHour(String(dt.getHours()))
    setTimeMinute(String(dt.getMinutes()).padStart(2, '0'))
    setAmPm(dt.getHours() < 12 ? 'AM' : 'PM')
  }

  function useManualCoordinates() {
    if (latDeg.trim() === '' || lonDeg.trim() === '') {
      setLocationError('Enter latitude and longitude first.')
      return
    }

    const lat = dmsToDecimal(Number(latDeg) || 0, Number(latMin) || 0, Number(latSec) || 0, latSign)
    const lon = dmsToDecimal(Number(lonDeg) || 0, Number(lonMin) || 0, Number(lonSec) || 0, lonSign)
    if (!Number.isFinite(lat) || lat < -90 || lat > 90) {
      setLocationError('Latitude must be between 0° and 90°.')
      return
    }
    if (!Number.isFinite(lon) || lon < -180 || lon > 180) {
      setLocationError('Longitude must be between 0° and 180°.')
      return
    }

    locationRequestId.current += 1
    cancelActiveNativeLocationDetection()
    setGeolocating(false)
    setGeoError('')
    setLocationError('')
    setLocationTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone)
    setDetectedCityName(`${fmtDMS(latDeg || '0', latMin || '00', latSec || '00', latSign)}, ${fmtDMS(lonDeg || '0', lonMin || '00', lonSec || '00', lonSign)}`)
    setLocationDetected(true)
    setLocationSet(true)
  }

  async function searchLocation() {
    if (!locationName.trim() || locationSearching) return
    locationRequestId.current += 1
    cancelActiveNativeLocationDetection()
    setGeolocating(false)
    setLocationSearching(true)
    setLocationError('')
    setGeoError('')
    try {
      if (isTauriRuntime()) {
        const data = await geocodeLocationNative(locationName)
        if (!data.length) { setLocationError('Location not found.'); return }
        const result = data[0]
        const lat = result.latitude
        const lon = result.longitude
        const ld = decimalToDMS(lat)
        const lo = decimalToDMS(lon)
        setLatDeg(ld.deg); setLatMin(ld.min); setLatSec(ld.sec)
        setLatSign(lat >= 0 ? 'N' : 'S')
        setLonDeg(lo.deg); setLonMin(lo.min); setLonSec(lo.sec)
        setLonSign(lon >= 0 ? 'E' : 'W')
        setLocationSet(true)
        setLocationDetected(true)
        setDetectedCityName(result.label)
        setLocationTimezone(result.timezone || locationTimezone)
        return
      }
      const data = searchLocalCities(locationName, 1)
      if (!data.length) { setLocationError('Location not found.'); return }
      const lat = data[0].latitude
      const lon = data[0].longitude
      if (!Number.isFinite(lat) || !Number.isFinite(lon)) { setLocationError('Invalid coordinates returned.'); return }
      const ld = decimalToDMS(lat)
      const lo = decimalToDMS(lon)
      setLatDeg(ld.deg); setLatMin(ld.min); setLatSec(ld.sec)
      setLatSign(lat >= 0 ? 'N' : 'S')
      setLonDeg(lo.deg); setLonMin(lo.min); setLonSec(lo.sec)
      setLonSign(lon >= 0 ? 'E' : 'W')
      setLocationSet(true)
      setLocationDetected(true)
      setDetectedCityName(data[0].label)
      setLocationTimezone(data[0].timezone || locationTimezone)
    } catch {
      setLocationError('Search failed. The offline location index could not be read.')
    } finally {
      setLocationSearching(false)
    }
  }

  const parsed = useMemo(() => {
    const h = String(Number(timeHour) || 0).padStart(2, '0')
    const m = String(Number(timeMinute) || 0).padStart(2, '0')
    const dt = new Date(`${dateLocal}T${h}:${m}`)
    const latDec = dmsToDecimal(Number(latDeg) || 0, Number(latMin) || 0, Number(latSec) || 0, latSign)
    const lonDec = dmsToDecimal(Number(lonDeg) || 0, Number(lonMin) || 0, Number(lonSec) || 0, lonSign)
    return { dt, lat: latDec, lon: lonDec }
  }, [dateLocal, timeHour, timeMinute, latDeg, latMin, latSec, latSign, lonDeg, lonMin, lonSec, lonSign])

  const chart = useMemo(() => calculateChart(parsed.dt, parsed.lat, parsed.lon), [parsed])
  const aiChartFacts = useMemo(() => {
    if (!chart.summary) return null

    const locationLabel = detectedCityName || locationName || `${fmtDMS(latDeg, latMin, latSec, latSign)}, ${fmtDMS(lonDeg, lonMin, lonSec, lonSign)}`
    const localHour = String(Number(timeHour) || 0).padStart(2, '0')
    const localMinute = String(Number(timeMinute) || 0).padStart(2, '0')
    try {
      const horaryChart = (calculateAdvancedHoraryChart as unknown as (input: {
        date: Date
        lat: number
        lng: number
        localDate: string
        localTime: string
        houseSystem: string
        planetSet: string
      }) => unknown)({
        date: parsed.dt,
        lat: parsed.lat,
        lng: parsed.lon,
        localDate: dateLocal,
        localTime: `${localHour}:${localMinute}`,
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
      })
      return (buildHoraryChartFacts as (chart: unknown, options: Record<string, unknown>) => unknown)(horaryChart, {
        locationLabel,
        timezone: locationTimezone,
        localTime: `${dateLocal} ${localHour}:${localMinute} ${locationTimezone}`,
      })
    } catch {
      return buildAiChartFacts(chart.summary, {
        dt: parsed.dt,
        lat: parsed.lat,
        lon: parsed.lon,
        timezone: locationTimezone,
        locationLabel,
      })
    }
  }, [chart.summary, dateLocal, detectedCityName, latDeg, latMin, latSec, latSign, locationName, locationTimezone, lonDeg, lonMin, lonSec, lonSign, parsed.dt, parsed.lat, parsed.lon, timeHour, timeMinute])

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
          {locationError && <div style={{ color: '#c55', marginTop: 4 }}>{locationError}</div>}
        </div>
      )}
    </>
  )

  return (
    <div className={darkMode ? 'night-mode' : ''} style={{ maxWidth: 568, margin: '0 auto', padding: 24, textAlign: 'left', minHeight: '100vh', background: darkMode ? '#242424' : '#ffffff', color: darkMode ? 'rgba(255,255,255,0.87)' : '#213547' }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', position: 'relative', zIndex: 200 }}>
        <h1 style={{ marginBottom: 4 }}>Horary Calculator</h1>
        <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <button className="icon-btn" onClick={() => { const d = !darkMode; setDarkMode(d); localStorage.setItem('darkMode', String(d)) }} style={{ fontSize: '1.2em', background: 'none', border: 'none', cursor: 'pointer', opacity: 0.8, color: 'inherit' }} title={darkMode ? 'Switch to day mode' : 'Switch to night mode'}>
            {darkMode ? '☀︎' : '☽︎'}
          </button>
        <div style={{ position: 'relative' }} ref={settingsRef}>
          <button className="icon-btn" onClick={() => setShowSettings(s => !s)} style={{ fontSize: '1.2em', background: 'none', border: 'none', cursor: 'pointer', opacity: 0.7, color: 'inherit' }} title="Settings">⚙︎</button>
          {showSettings && (
            <div style={{ position: 'absolute', right: 0, top: '100%', background: darkMode ? '#1a1a1a' : '#ffffff', color: darkMode ? 'rgba(255,255,255,0.87)' : '#213547', border: `1px solid ${darkMode ? '#444' : '#ccc'}`, borderRadius: 8, padding: 12, minWidth: 220, zIndex: 9999 }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap', cursor: 'pointer' }}>
                <input type="checkbox" checked={isEditing} onChange={(e) => { if (!e.target.checked) saveCastSnapshot(); setIsEditing(e.target.checked); if (!e.target.checked) { resetToNow(); setSubmitError('') } }} />
                Look up past question
              </label>
              <hr style={{ border: 'none', borderTop: darkMode ? '1px solid #444' : '1px solid #ddd', margin: '8px 0' }} />
              {[['showAngles', 'Show angles', showAngles, setShowAngles], ['showHouses', 'Show houses', showHouses, setShowHouses], ['showPlanets', 'Show planets', showPlanets, setShowPlanets], ['showAspects', 'Show aspects list', showAspects, setShowAspects], ['showAspectGrid', 'Show aspects chart', showAspectGrid, setShowAspectGrid]].map(([key, label, value, setter]) => (
                <label key={key as string} style={{ display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap', marginBottom: 4 }}>
                  <input type="checkbox" checked={value as boolean} onChange={(e) => { localStorage.setItem(key as string, String(e.target.checked)); (setter as (v: boolean) => void)(e.target.checked) }} />
                  {label as string}
                </label>
              ))}
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap', marginTop: 4 }}>
                <input type="checkbox"
                  checked={showAngles && showHouses && showPlanets && showAspects && showAspectGrid}
                  onChange={(e) => { const v = e.target.checked; localStorage.setItem('showAngles', String(v)); localStorage.setItem('showHouses', String(v)); localStorage.setItem('showPlanets', String(v)); localStorage.setItem('showAspects', String(v)); localStorage.setItem('showAspectGrid', String(v)); setShowAngles(v); setShowHouses(v); setShowPlanets(v); setShowAspects(v); setShowAspectGrid(v) }}
                />
                Show all
              </label>
              <hr style={{ border: 'none', borderTop: darkMode ? '1px solid #444' : '1px solid #ddd', margin: '8px 0' }} />
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap' }}>
                <input type="checkbox" checked={use24Hour} onChange={(e) => { localStorage.setItem('use24Hour', String(e.target.checked)); setUse24Hour(e.target.checked) }} />
                Use 24-hour time
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap', marginTop: 4 }}>
                <input type="checkbox" checked={locationInputMode === 'coordinates'} onChange={(e) => { const m = e.target.checked ? 'coordinates' : 'search'; localStorage.setItem('locationInputMode', m); setLocationInputMode(m); setLocationError('') }} />
                Enter coordinates manually
              </label>
            </div>
          )}
        </div>
        </div>
      </div>

      {!isEditing && (
        <div style={{ marginTop: 32, marginBottom: 16, opacity: 0.85 }}>
          <div><b>Date:</b> {parsed.dt.toLocaleDateString([], { year: 'numeric', month: 'long', day: 'numeric' })}</div>
          <div><b>Time:</b> {parsed.dt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: !use24Hour })}</div>
          {geolocating && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 4, marginBottom: 12, flexWrap: 'wrap' }}>
                <span style={{ opacity: 0.6 }}>Detecting location…</span>
                <button onClick={cancelDetectLocation} style={{ padding: '0.25em 0.7em' }}>
                  Cancel
                </button>
              </div>
              {locationFields}
            </>
          )}
          {!geolocating && locationDetected && (
            <div><b>Location:</b> {detectedCityName || `${fmtDMS(latDeg, latMin, latSec, latSign)}, ${fmtDMS(lonDeg, lonMin, lonSec, lonSign)}`}</div>
          )}
          {!geolocating && !locationDetected && (
            <>
              <button onClick={detectLocation} style={{ marginBottom: geoError ? 6 : 12 }}>Detect my location</button>
              {geoError && (
                <div style={{ color: '#c55', marginBottom: 10, fontSize: '0.9em', lineHeight: 1.35, overflowWrap: 'anywhere' }}>
                  {geoError}{browserPermissionResetHint(navigator.userAgent, isTauriRuntime())}
                </div>
              )}
              {locationFields}
            </>
          )}
        </div>
      )}

      {isEditing && (
        <div style={{ marginTop: 32, marginBottom: 16 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12, marginBottom: 12 }}>
            <label>
              Date
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', marginTop: 4, flexWrap: 'wrap' }}>
                {(() => {
                  const [dy, dm, dd] = dateLocal.split('-').map(Number)
                  const now = new Date()
                  const curYear = now.getFullYear(), curMonth = now.getMonth() + 1, curDay = now.getDate()
                  const months = ['January','February','March','April','May','June','July','August','September','October','November','December']
                  const daysInMonth = new Date(dy, dm, 0).getDate()
                  const maxMonth = dy === curYear ? curMonth : 12
                  const maxDay = (dy === curYear && dm === curMonth) ? curDay : daysInMonth
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
                  const now = new Date()
                  const [dy, dm, dd] = dateLocal.split('-').map(Number)
                  const isToday = dy === now.getFullYear() && dm === now.getMonth() + 1 && dd === now.getDate()
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
                        {now.getHours() >= 12 || !isToday ? <option value="PM">PM</option> : null}
                      </select>
                    </>
                  )
                })()}
              </div>
            </label>
          </div>
          {locationFields}
          <button style={{ marginTop: 12 }} onClick={resetToNow}>
            Use current time &amp; place
          </button>
          <div style={{ marginTop: 16 }}>
            Question
            <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', marginTop: 4 }}>
              <textarea
                ref={questionEditRef}
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                placeholder="What is your question?"
                rows={1}
                style={{ flex: 1, resize: 'none', overflow: 'hidden', fontFamily: 'inherit', fontSize: 'inherit', padding: '0.5em 0.8em' }}
              />
              <button onClick={generateJudgement} disabled={judgementActive}>
                {judgementActive ? 'Working...' : 'Generate Judgement'}
              </button>
            </div>
            {submitError && <div style={{ color: '#c55', marginTop: 6, fontSize: '0.9em' }}>{submitError}</div>}
          </div>
        </div>
      )}

      {!isEditing && (
        <div style={{ marginBottom: 16 }}>
          Question
          <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', marginTop: 4 }}>
            <textarea
              ref={questionViewRef}
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="What is your question?"
              rows={1}
              style={{ flex: 1, resize: 'none', overflow: 'hidden', fontFamily: 'inherit', fontSize: 'inherit', padding: '0.5em 0.8em' }}
            />
            <button onClick={generateJudgement} disabled={judgementActive}>
              {judgementActive ? 'Working...' : 'Generate Judgement'}
            </button>
          </div>
          {submitError && <div style={{ color: '#c55', marginTop: 6, fontSize: '0.9em' }}>{submitError}</div>}
        </div>
      )}

      {(isEditing || locationSet) && chart.summary ? (
        <div style={{ marginTop: 12, padding: 12, border: 'none', borderRadius: 8, overflowX: 'auto' }}>
          <h2 style={{ marginTop: 0 }}>Chart wheel</h2>
          <ChartWheel data={chart.summary.astroChartData} darkMode={darkMode} />
        </div>
      ) : null}

      {(isEditing || locationSet) && chart.summary ? (
        <AiPanel
          requestId={judgementRequestId}
          question={question}
          chartFacts={aiChartFacts}
          darkMode={darkMode}
          onActivityChange={setJudgementActive}
        />
      ) : null}

      <div style={{ marginTop: 16 }}>
        {(isEditing || locationSet) && chart.error ? (
          <div style={{ padding: 12, border: '1px solid #c33', borderRadius: 8 }}>
            <b>Error:</b> {chart.error}
          </div>
        ) : (isEditing || locationSet) && chart.summary ? (
          <>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'center', marginBottom: 16, flexWrap: 'wrap' }}>
            <button onClick={() => nudgeTime(-1)}>◀</button>
            <select value={nudgeUnit} onChange={(e) => setNudgeUnit(e.target.value as typeof nudgeUnit)} title="Nudge increment" style={{ padding: '0.5em 1.2em', fontSize: 'inherit', fontFamily: 'inherit' }}>
              <option value="minute">Minute</option>
              <option value="hour">Hour</option>
              <option value="day">Day</option>
              <option value="week">Week</option>
              <option value="month">Month</option>
              <option value="year">Year</option>
            </select>
            <button onClick={() => nudgeTime(1)}>▶</button>
            <button onClick={resetToCastTime}>Reset</button>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: (showAngles || showHouses) && (showPlanets || showAspects) ? '1fr 1fr' : '1fr', gap: 0 }}>
            {(showAngles || showHouses) && (
              <div style={{ padding: '12px 6px 12px 12px', border: 'none', borderRadius: 8, minWidth: 0 }}>
                {showAngles && (
                  <>
                    <h2 style={{ marginTop: 0 }}>Angles</h2>
                    <ul style={{ marginTop: 8 }}>
                      <li><b>ASC:</b> {chart.summary.ascendant}</li>
                      <li><b>DSC:</b> {chart.summary.descendant}</li>
                      <li><b>MC:</b> {chart.summary.midheaven}</li>
                      <li><b>IC:</b> {chart.summary.ic}</li>
                    </ul>
                  </>
                )}
                {showHouses && (
                  <>
                    <h2 style={{ marginTop: showAngles ? 16 : 0 }}>Houses</h2>
                    <ul style={{ marginTop: 8 }}>
                      {chart.summary.houses.map((h) => (
                        <li key={h.house}>
                          <b>House {h.house}:</b> {h.sign} {h.formatted || formatDeg(h.eclipticDegrees)}
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>
            )}

            {(showPlanets || showAspects) && (
              <div style={{ padding: '12px 12px 12px 6px', border: 'none', borderRadius: 8, minWidth: 0, overflow: 'hidden' }}>
                {showPlanets && (
                  <>
                    <h2 style={{ marginTop: 0 }}>Planets</h2>
                    <ul style={{ marginTop: 8 }}>
                      {chart.summary.planets.map((p) => (
                        <li key={p.key}>
                          <b>{p.name}:</b> {p.sign} {p.formatted || formatDeg(p.eclipticDegrees)}
                        </li>
                      ))}
                    </ul>
                  </>
                )}
                {showAspects && (
                  <>
                    <h2 style={{ marginTop: showPlanets ? 16 : 0 }}>Aspects</h2>
                    <ul style={{ marginTop: 8 }}>
                      {chart.summary.aspectsList.length === 0 && <li>No aspects found with default orbs.</li>}
                      {chart.summary.aspectsList.map((a, idx) => (
                        <li key={`${a.from}-${a.to}-${idx}`}>
                          <b>{a.from}</b> {a.type.charAt(0).toUpperCase() + a.type.slice(1)} <b>{a.to}</b> {a.orb}{a.applying != null ? ` — ${a.applying ? 'applying' : 'separating'}` : ''}
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>
            )}
          </div>
          {showAspectGrid && (
            <div style={{ marginTop: 16, padding: 12, border: 'none', borderRadius: 8, overflow: 'hidden' }}>
              <h2 style={{ marginTop: 0 }}>Aspects chart</h2>
              <AspectGrid
                planets={chart.summary.planets.map(p => p.name)}
                aspects={chart.summary.aspectsList}
              />
            </div>
          )}
          </>
        ) : null}
      </div>
    </div>
  )
}

export default App
