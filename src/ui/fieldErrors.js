import { escapeHtml } from './html.js';

export const CHART_FIELD_IDS = Object.freeze({
    date: 'inputDate',
    time: 'inputTime',
    offset: 'inputOffset',
    lat: 'inputLat',
    lng: 'inputLng',
});

export function setFieldError(input, message, { documentRef = globalThis.document } = {}) {
    if (!input) return;

    const errorId = `${input.id}Error`;
    let error = documentRef?.getElementById?.(errorId);
    if (!error) {
        error = documentRef?.createElement?.('div');
        if (!error) return;
        error.id = errorId;
        error.className = 'field-error';
        input.insertAdjacentElement?.('afterend', error);
    }

    input.setAttribute?.('aria-invalid', 'true');
    input.setAttribute?.('aria-describedby', errorId);
    error.textContent = message;
    error.hidden = false;
}

export function clearFieldError(input, { documentRef = globalThis.document } = {}) {
    if (!input) return;
    const errorId = `${input.id}Error`;
    const error = documentRef?.getElementById?.(errorId);
    input.removeAttribute?.('aria-invalid');
    input.removeAttribute?.('aria-describedby');
    if (error) {
        error.textContent = '';
        error.hidden = true;
    }
}

export function showChartErrors(errors, {
    documentRef = globalThis.document,
    fieldIds = CHART_FIELD_IDS,
    summaryId = 'calcErrors',
} = {}) {
    const summary = documentRef?.getElementById?.(summaryId);
    clearChartFieldErrors({ documentRef, fieldIds });

    for (const error of errors || []) {
        setFieldError(documentRef?.getElementById?.(fieldIds[error.field]), error.message, { documentRef });
    }

    if (summary) {
        summary.hidden = false;
        summary.innerHTML = (errors || [])
            .map(error => `<div>${escapeHtml(error.message)}</div>`)
            .join('');
    }
}

export function clearChartErrors({
    documentRef = globalThis.document,
    fieldIds = CHART_FIELD_IDS,
    summaryId = 'calcErrors',
} = {}) {
    const summary = documentRef?.getElementById?.(summaryId);
    clearChartFieldErrors({ documentRef, fieldIds });
    if (summary) {
        summary.hidden = true;
        summary.innerHTML = '';
    }
}

function clearChartFieldErrors({ documentRef, fieldIds }) {
    for (const inputId of Object.values(fieldIds)) {
        clearFieldError(documentRef?.getElementById?.(inputId), { documentRef });
    }
}
