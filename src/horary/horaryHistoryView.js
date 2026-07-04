import { escapeHtml } from '../ui/html.js';

export function renderHoraryHistoryMarkup(
    history,
    {
        tauriRuntime = false,
        canRenderSavedChart = () => false,
        entryDate = entry => new Date(entry.time),
    } = {},
) {
    return (Array.isArray(history) ? history : []).map(entry => {
        const d = entryDate(entry);
        const canLoad = tauriRuntime || canRenderSavedChart(entry.chart);
        const escapedId = escapeHtml(entry.id);
        return `<li class="history-item" data-id="${escapedId}">
      <div class="history-main">
        <span class="question">"${escapeHtml(entry.question)}"</span>
        <span class="time">${escapeHtml(d.toLocaleDateString())} ${escapeHtml(d.toLocaleTimeString())}</span>
      </div>
      <div class="history-actions" aria-label="Saved chart actions">
        <button class="btn btn-secondary btn-sm" type="button" data-history-action="load" data-id="${escapedId}" ${canLoad ? '' : 'disabled'}>&#8617; Load</button>
        <button class="btn btn-secondary btn-sm" type="button" data-history-action="export" data-id="${escapedId}">&#8681; Export</button>
        <button class="btn btn-secondary btn-sm" type="button" data-history-action="delete" data-id="${escapedId}">&#10005; Delete</button>
      </div>
    </li>`;
    }).join('');
}

export function safeHistoryExportFilename(value) {
    return `horary-${safeFilePart(value)}.json`;
}

export function safeFilePart(value) {
    const normalized = String(value)
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '');
    return (normalized || 'chart').slice(0, 80);
}
