export const FALLBACK_APP_VERSION = '0.0.0';

export function resolveAppVersion(version = readInjectedAppVersion()) {
    const normalizedVersion = typeof version === 'string' ? version.trim() : '';
    return normalizedVersion || FALLBACK_APP_VERSION;
}

function readInjectedAppVersion() {
    return typeof __WHORARY_APP_VERSION__ === 'string' ? __WHORARY_APP_VERSION__ : '';
}

export const APP_VERSION = resolveAppVersion();
