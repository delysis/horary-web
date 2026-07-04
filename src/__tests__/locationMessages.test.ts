import { describe, expect, it } from 'vitest'
import {
  browserPermissionResetHint,
  locationFailureMessage,
  locationUnsupportedMessage,
} from '../locationMessages'

describe('location UI messages', () => {
  it('reports macOS Location Services for native permission denial', () => {
    const message = locationFailureMessage(
      new Error('Location access is blocked for Horary in macOS Location Services.'),
      true,
    )

    expect(message).toContain('did not receive a location')
    expect(message).toContain('Search for a city')
    expect(message).not.toContain('browser')
  })

  it('reports browser settings for web permission denial', () => {
    const message = locationFailureMessage({ code: 1, message: 'Permission denied' }, false)

    expect(message).toContain('browser settings')
    expect(message).toContain('enter coordinates manually')
  })

  it('keeps timeout failures actionable', () => {
    expect(locationFailureMessage({ code: 3, message: 'Timeout expired' }, true)).toBe(
      'Location detection timed out. Search for a city or enter coordinates manually.',
    )
  })

  it('only shows browser reset hints outside the native runtime', () => {
    const desktopSafari = 'Mozilla/5.0 Safari/605.1.15'

    expect(browserPermissionResetHint(desktopSafari, true)).toBe('')
    expect(browserPermissionResetHint(desktopSafari, false)).toContain('lock icon')
  })

  it('has explicit unsupported messages for native and web runtimes', () => {
    expect(locationUnsupportedMessage(true)).toContain('Native location detection')
    expect(locationUnsupportedMessage(false)).toContain('browser')
  })
})
