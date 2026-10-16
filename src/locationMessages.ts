type GeolocationLikeError = {
  code?: number
  message?: string
}

export const LOCATION_DETECT_TIMEOUT_MS = 6_000

export function locationFailureMessage(error: unknown, nativeRuntime: boolean) {
  const message = extractErrorMessage(error)
  const denied = isPermissionDenied(error, message)
  const timedOut = isTimeout(error, message)

  if (nativeRuntime) {
    if (denied) {
      return 'Horary did not receive a location from macOS. Search for a city or enter coordinates manually.'
    }
    if (timedOut) {
      return 'Location detection timed out. Search for a city or enter coordinates manually.'
    }
    if (message) {
      return `${message} Search for a city or enter coordinates manually.`
    }
    return 'Could not detect location. Search for a city or enter coordinates manually.'
  }

  if (denied) {
    return 'Location access denied. Allow it in your browser settings, search for a city, or enter coordinates manually.'
  }
  if (timedOut) {
    return 'Location detection timed out. Search for a city or enter coordinates manually.'
  }
  return 'Could not detect location. Search for a city or enter coordinates manually.'
}

export function locationUnsupportedMessage(nativeRuntime: boolean) {
  return nativeRuntime
    ? 'Native location detection is unavailable. Search for a city or enter coordinates manually.'
    : 'Geolocation is not supported by this browser. Search for a city or enter coordinates manually.'
}

export function browserPermissionResetHint(userAgent: string, nativeRuntime: boolean) {
  if (nativeRuntime) return ''
  return /Chrome|Firefox|Safari|Edge/.test(userAgent) && !/Mobi|Android/i.test(userAgent)
    ? ' Click the lock icon in your browser address bar to reset it.'
    : ''
}

function extractErrorMessage(error: unknown) {
  if (error instanceof Error) return error.message.trim()
  if (typeof error === 'string') return error.trim()
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message
    return typeof message === 'string' ? message.trim() : ''
  }
  return ''
}

function isPermissionDenied(error: unknown, message: string) {
  const code = (error as GeolocationLikeError | null)?.code
  return code === 1 || /denied|blocked|not authorized|not allowed/i.test(message)
}

function isTimeout(error: unknown, message: string) {
  const code = (error as GeolocationLikeError | null)?.code
  return code === 3 || /timed out|timeout/i.test(message)
}
