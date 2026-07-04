export type AppInfo = {
  productName: string
  version: string
  runtime: string
}

export type ModelInfo = {
  id: string
  displayName: string
  filename: string
  sizeBytes: number
  sha256: string
}

export type ModelStatus = {
  running: boolean
  modelId?: string | null
  port?: number | null
  backend?: string | null
  logPath?: string | null
}

export type NativeLlamaHealth = {
  compiled: boolean
  running: boolean
  modelId?: string | null
  backend: string
  ctxSize?: number | null
  parallel?: number | null
  speculativeDecodingSupported: boolean
  speculativeDecodingActive: boolean
  draftModelId?: string | null
  hotCacheEntries: number
  hotCacheHits: number
  coldCacheHits: number
  coldCacheWrites: number
  speculativeDraftTokens: number
  speculativeAcceptedTokens: number
}

export type StartLlamaRequest = {
  modelId: string
  ctxSize?: number
  nGpuLayers?: string
  parallel?: number
  continuousBatching?: boolean
  cacheRamMb?: number
  cacheIdleSlots?: boolean
  coldKvCache?: boolean
  draftModelId?: string
  specDraftNMax?: number
}

export type HoraryInterpretation = {
  summary: string
  directAnswer?: string | null
  confidence: 'low' | 'medium' | 'high'
  judgementTrace: Array<{
    stepId: string
    finding: string
    chartEvidence: string
    confidence: 'low' | 'medium' | 'high'
  }>
  keyFactors: Array<{
    factor: string
    chartEvidence: string
    interpretation: string
  }>
  cautions: string[]
  followUpQuestions: string[]
}

export type InterpretationStreamStarted = {
  generationId: string
}

export type InterpretationRequest = {
  question: string
  chart: unknown
  settings?: Record<string, unknown>
}

export type LocationCandidate = {
  id: string
  label: string
  name: string
  latitude: number
  longitude: number
  timezone: string
  country: string
  provider: string
}

export type ReverseLocationLabel = {
  label: string
  name: string
  country: string
  latitude: number
  longitude: number
  timezone: string
  provider: string
  distanceKm: number
}

export type CurrentLocation = {
  latitude: number
  longitude: number
  accuracyMeters?: number | null
  provider: string
  authorizationStatus: number
}

export type CurrentLocationDetectionStarted = {
  detectionId: string
  timeoutMs: number
}

export type CurrentLocationDetectedEvent = {
  detectionId: string
  location: CurrentLocation
}

export type CurrentLocationMessageEvent = {
  detectionId: string
  message: string
}

type StreamHandlers = {
  token?: (payload: { generationId: string; text: string }) => void
  complete?: (payload: { generationId: string; interpretation: HoraryInterpretation }) => void
  error?: (payload: { generationId: string; message: string }) => void
  cancelled?: (payload: { generationId: string; message: string }) => void
}

type LocationDetectionHandlers = {
  detected?: (payload: CurrentLocationDetectedEvent) => void
  error?: (payload: CurrentLocationMessageEvent) => void
  cancelled?: (payload: CurrentLocationMessageEvent) => void
}

export function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

async function invokeTauri<T>(command: string, payload?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error(`Tauri command unavailable in web runtime: ${command}`)
  }
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, payload)
}

export async function initializeTauriBridge() {
  if (!isTauriRuntime()) return null
  const appInfo = await invokeTauri<AppInfo>('get_app_info')
  document.documentElement.dataset.runtime = appInfo.runtime
  return appInfo
}

export async function geocodeLocation(query: string) {
  return invokeTauri<LocationCandidate[]>('geocode_location', { req: { query } })
}

export async function reverseGeocodeLocation(latitude: number, longitude: number) {
  return invokeTauri<ReverseLocationLabel | null>('reverse_geocode_location', {
    req: { latitude, longitude },
  })
}

export async function getCurrentLocation(timeoutMs = 6000) {
  return invokeTauri<CurrentLocation>('get_current_location', {
    req: { timeoutMs },
  })
}

export async function startCurrentLocationDetection(timeoutMs = 6000) {
  return invokeTauri<CurrentLocationDetectionStarted>('start_current_location_detection', {
    req: { timeoutMs },
  })
}

export async function cancelCurrentLocationDetection(detectionId: string) {
  return invokeTauri<boolean>('cancel_current_location_detection', { detectionId })
}

export async function listenCurrentLocationDetectionEvents(handlers: LocationDetectionHandlers) {
  if (!isTauriRuntime()) {
    throw new Error('Tauri event listeners unavailable in web runtime')
  }

  const { listen } = await import('@tauri-apps/api/event')
  const unlisteners = await Promise.all([
    listen('current-location-detected', event => handlers.detected?.(event.payload as CurrentLocationDetectedEvent)),
    listen('current-location-error', event => handlers.error?.(event.payload as CurrentLocationMessageEvent)),
    listen('current-location-cancelled', event => handlers.cancelled?.(event.payload as CurrentLocationMessageEvent)),
  ])

  return () => {
    for (const unlisten of unlisteners) unlisten()
  }
}

export async function listModels() {
  return invokeTauri<ModelInfo[]>('list_models')
}

export async function importModel(path: string) {
  return invokeTauri<ModelInfo>('import_model', { req: { path } })
}

export async function getModelStatus() {
  return invokeTauri<ModelStatus>('get_model_status')
}

export async function getNativeLlamaHealth() {
  return invokeTauri<NativeLlamaHealth>('get_native_llama_health')
}

export async function startLlama(req: StartLlamaRequest) {
  return invokeTauri<ModelStatus>('start_llama', { req })
}

export async function stopLlama() {
  return invokeTauri<boolean>('stop_llama')
}

export async function startInterpretationStream(req: InterpretationRequest) {
  return invokeTauri<InterpretationStreamStarted>('start_interpretation_stream', { req })
}

export async function cancelInterpretationStream(generationId: string) {
  return invokeTauri<boolean>('cancel_interpretation_stream', { generationId })
}

export async function listenAiInterpretationEvents(handlers: StreamHandlers) {
  if (!isTauriRuntime()) {
    throw new Error('Tauri event listeners unavailable in web runtime')
  }

  const { listen } = await import('@tauri-apps/api/event')
  const unlisteners = await Promise.all([
    listen('ai-interpretation-token', event => handlers.token?.(event.payload as { generationId: string; text: string })),
    listen('ai-interpretation-complete', event => handlers.complete?.(event.payload as { generationId: string; interpretation: HoraryInterpretation })),
    listen('ai-interpretation-error', event => handlers.error?.(event.payload as { generationId: string; message: string })),
    listen('ai-interpretation-cancelled', event => handlers.cancelled?.(event.payload as { generationId: string; message: string })),
  ])

  return () => {
    for (const unlisten of unlisteners) unlisten()
  }
}
