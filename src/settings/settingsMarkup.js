import { escapeHtml } from '../ui/html.js';

export function renderHouseSystemOptions(systems, selectedHouseSystem) {
    return systems.map(system => {
        const selected = system.key === selectedHouseSystem ? 'selected' : '';
        return `<option value="${escapeHtml(system.key)}" ${selected}>${escapeHtml(system.name)}</option>`;
    }).join('');
}

export function renderHouseSystemPills(systems, selectedHouseSystem) {
    return systems.map(system => {
        const active = system.key === selectedHouseSystem ? 'active' : '';
        return `<button class="radio-pill ${active}" data-value="${escapeHtml(system.key)}">${escapeHtml(system.name)}</button>`;
    }).join('');
}

export function renderAspectSettingsMarkup(aspectSettings) {
    return Object.entries(aspectSettings || {}).map(([key, aspect]) => `
      <div class="aspect-setting-row">
        <label class="aspect-setting-toggle">
          <input type="checkbox" data-aspect-enabled="${escapeHtml(key)}" ${aspect.enabled ? 'checked' : ''} />
          <span class="aspect-symbol" style="color:${escapeHtml(aspect.color)}">${escapeHtml(aspect.symbol)}</span>
          <span>${escapeHtml(aspect.name)}</span>
        </label>
        <label class="aspect-orb-field">
          <span>Orb</span>
          <input
            type="number"
            class="form-input aspect-orb-input"
            min="0"
            max="30"
            step="0.25"
            value="${escapeHtml(aspect.orb)}"
            data-aspect-orb="${escapeHtml(key)}"
            aria-label="${escapeHtml(aspect.name)} orb in degrees"
          />
        </label>
      </div>
    `).join('');
}
