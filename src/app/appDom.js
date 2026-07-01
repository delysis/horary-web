import { APP_VERSION, resolveAppVersion } from './version.js';

export const LEGACY_ZODIAC_STORAGE_KEY = 'whorary_zodiac';

export function showHoraryError(message, { documentRef = globalThis.document } = {}) {
    const el = documentRef?.getElementById?.('horaryErrors');
    if (!el) return;
    el.hidden = message === '';
    el.textContent = message;
}

export function downloadJson(
    filename,
    payload,
    {
        documentRef = globalThis.document,
        urlApi = globalThis.URL,
        BlobCtor = globalThis.Blob,
    } = {},
) {
    const blob = new BlobCtor([`${JSON.stringify(payload, null, 2)}\n`], { type: 'application/json' });
    const url = urlApi.createObjectURL(blob);
    const link = documentRef.createElement('a');
    link.href = url;
    link.download = filename;
    link.rel = 'noopener';
    documentRef.body.append(link);
    link.click();
    link.remove();
    urlApi.revokeObjectURL(url);
}

export function removeLegacyZodiacSetting({ storage = globalThis.localStorage } = {}) {
    storage?.removeItem?.(LEGACY_ZODIAC_STORAGE_KEY);
}

export function setDefaultDateInput({
    documentRef = globalThis.document,
    now = new Date(),
} = {}) {
    const dateInput = documentRef?.getElementById?.('inputDate');
    if (!dateInput) return false;
    dateInput.value = now.toISOString().split('T')[0];
    return true;
}

export function renderAppVersion({
    documentRef = globalThis.document,
    version = APP_VERSION,
} = {}) {
    const appVersion = documentRef?.getElementById?.('appVersion');
    if (!appVersion) return false;
    appVersion.textContent = resolveAppVersion(version);
    return true;
}

export function renderInitialChartPlaceholder({ documentRef = globalThis.document } = {}) {
    const container = documentRef?.getElementById?.('chartWheel');
    if (!container) return false;
    container.innerHTML = `<div class="empty-state">
      <div class="icon">&#10022;</div>
      <p>Enter chart data and tap Calculate to cast a chart</p>
    </div>`;
    return true;
}
