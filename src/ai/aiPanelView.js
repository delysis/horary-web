import { escapeHtml } from '../ui/html.js';

export function formatModelSize(bytes) {
    if (!Number.isFinite(bytes) || bytes <= 0) return 'unknown size';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = bytes;
    let index = 0;
    while (value >= 1024 && index < units.length - 1) {
        value /= 1024;
        index++;
    }
    return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

export function selectedImportedModelId(models = [], {
    preferredModelId = '',
    currentValue = '',
    runningModelId = '',
} = {}) {
    return preferredModelId || currentValue || runningModelId || models[0]?.id || '';
}

export function renderImportedModelOptions(models = [], selectedModelId = '') {
    if (!Array.isArray(models) || models.length === 0) {
        return '<option value="">No imported models</option>';
    }

    return models.map(model => {
        const selected = model.id === selectedModelId ? ' selected' : '';
        return `<option value="${escapeHtml(model.id)}"${selected}>${escapeHtml(model.displayName)} (${formatModelSize(model.sizeBytes)})</option>`;
    }).join('');
}

export function downloadableManifestModels(manifest) {
    return (manifest?.models || []).filter(model =>
        model.enabled !== false && model.source?.url && hasDownloadableModelProfile(model)
    );
}

export function hasDownloadableModelProfile(model) {
    return Boolean(
        model?.sha256
        && Number.isFinite(model.sizeBytes)
        && model.sizeBytes > 0
        && model.license
        && Number.isFinite(model.minRamGb)
        && model.minRamGb > 0
        && Number.isFinite(model.recommendedRamGb)
        && model.recommendedRamGb >= model.minRamGb
        && Number.isFinite(model.defaultContextTokens)
        && model.defaultContextTokens > 0
        && validDefaultParams(model.defaultParams)
    );
}

function validDefaultParams(params) {
    return Boolean(
        params
        && typeof params === 'object'
        && !Array.isArray(params)
        && Number.isFinite(params.temperature)
        && params.temperature >= 0
        && params.temperature <= 2
        && Number.isFinite(params.top_p)
        && params.top_p > 0
        && params.top_p <= 1
        && Number.isInteger(params.max_tokens)
        && params.max_tokens > 0
        && params.max_tokens <= 16384
    );
}

export function renderDownloadModelOptions(models = [], selectedModelId = '') {
    if (!Array.isArray(models) || models.length === 0) {
        return '<option value="">No verified catalog models</option>';
    }

    return models.map(model => {
        const selected = model.id === selectedModelId ? ' selected' : '';
        return `<option value="${escapeHtml(model.id)}"${selected}>${escapeHtml(model.displayName)} (${formatModelSize(model.sizeBytes)})</option>`;
    }).join('');
}

export function computeAiControlState({
    tauriRuntime = false,
    busy = false,
    streaming = false,
    generationId = null,
    status = null,
    sidecar = null,
    models = [],
    lastHoraryChartFacts = null,
    selectedModelId = '',
    selectedDownloadId = '',
    sidecarPath = '',
    importPath = '',
} = {}) {
    const running = Boolean(status?.running);
    const sidecarInstalled = Boolean(sidecar?.installed);
    const hasModel = Boolean(selectedModelId);
    const hasDownload = Boolean(selectedDownloadId);
    const hasSidecarPath = Boolean(String(sidecarPath || '').trim());
    const hasImportPath = Boolean(String(importPath || '').trim());
    const canGenerate = tauriRuntime && running && lastHoraryChartFacts && !busy && !streaming;
    const canCancel = tauriRuntime && streaming && Boolean(generationId);

    return {
        running,
        sidecarInstalled,
        hasModel,
        hasDownload,
        hasImportPath,
        canGenerate,
        canCancel,
        refreshDisabled: busy || !tauriRuntime,
        downloadDisabled: busy || !tauriRuntime || running || !hasDownload,
        sidecarImportDisabled: busy || !tauriRuntime || running || !hasSidecarPath,
        importDisabled: busy || !tauriRuntime || running || !hasImportPath,
        startDisabled: busy || !tauriRuntime || running || !sidecarInstalled || !hasModel,
        stopDisabled: busy || !running,
        generateDisabled: !canGenerate,
        cancelDisabled: !canCancel,
        cancelHidden: !canCancel,
        modelSelectDisabled: busy || running || models.length === 0,
        downloadSelectDisabled: busy || running || !hasDownload,
        sidecarInputDisabled: busy || running,
        importInputDisabled: busy || running,
    };
}

export function aiStatusTextForState({
    tauriRuntime = false,
    busy = false,
    streaming = false,
    status = null,
    sidecar = null,
    models = [],
} = {}) {
    if (!tauriRuntime || busy || streaming) return null;

    if (!sidecar) {
        return 'Local AI runtime status unavailable.';
    }
    if (!sidecar.installed) {
        return 'Local AI runtime missing.';
    }
    if (!status) {
        return 'Local model status unavailable.';
    }
    if (status.running) {
        const backend = status.backend === 'native sidecar'
            ? 'local inference'
            : status.backend || 'local inference';
        return `Running ${status.modelId} with ${backend} backend.`;
    }
    if (models.length === 0) {
        return 'No local GGUF models imported.';
    }
    return `Local model stopped. Local AI runtime ready at ${sidecar.path}.`;
}

export function renderAiInterpretationMarkup(interpretation) {
    if (!interpretation) return '';

    const directAnswer = interpretation.directAnswer
        ? `<h4>Direct Answer</h4><p>${escapeHtml(interpretation.directAnswer)}</p>`
        : '';
    const keyFactors = (interpretation.keyFactors || []).map(factor => `
      <div class="ai-factor">
        <strong>${escapeHtml(factor.factor)}</strong>
        <div class="ai-evidence">${escapeHtml(factor.chartEvidence)}</div>
        <p>${escapeHtml(factor.interpretation)}</p>
      </div>
    `).join('');
    const judgementTrace = renderAiTrace(interpretation.judgementTrace);
    const cautions = renderAiList('Cautions', interpretation.cautions);
    const followUps = renderAiList('Follow Up Questions', interpretation.followUpQuestions);

    return `
      <h4>Summary</h4>
      <p>${escapeHtml(interpretation.summary)}</p>
      ${directAnswer}
      <h4>Confidence</h4>
      <p>${escapeHtml(interpretation.confidence)}</p>
      ${judgementTrace}
      <h4>Key Factors</h4>
      ${keyFactors}
      ${cautions}
      ${followUps}
    `;
}

export function renderAiStreamMarkup(text) {
    return `
      <h4>Streaming JSON</h4>
      <pre class="ai-stream-text">${escapeHtml(text || 'Waiting for model output...')}</pre>
    `;
}

export function renderAiList(title, items = []) {
    if (!Array.isArray(items) || items.length === 0) return '';
    return `<h4>${escapeHtml(title)}</h4><ul>${items.map(item => `<li>${escapeHtml(item)}</li>`).join('')}</ul>`;
}

function renderAiTrace(steps = []) {
    if (!Array.isArray(steps) || steps.length === 0) return '';
    return `
      <h4>Judgement Trace</h4>
      ${steps.map(step => `
        <div class="ai-factor">
          <strong>${escapeHtml(step.stepId)} (${escapeHtml(step.confidence)})</strong>
          <div class="ai-evidence">${escapeHtml(step.chartEvidence)}</div>
          <p>${escapeHtml(step.finding)}</p>
        </div>
      `).join('')}
    `;
}
