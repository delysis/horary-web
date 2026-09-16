export function isTauriRuntime() {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export async function initializeTauriBridge() {
    if (!isTauriRuntime()) return null;

    const { invoke } = await import('@tauri-apps/api/core');
    const appInfo = await invoke('get_app_info');
    document.documentElement.dataset.runtime = appInfo.runtime;
    return appInfo;
}

async function invokeTauri(command, payload) {
    if (!isTauriRuntime()) {
        throw new Error(`Tauri command unavailable in web runtime: ${command}`);
    }

    const { invoke } = await import('@tauri-apps/api/core');
    return invoke(command, payload);
}

export async function saveChart(req) {
    return invokeTauri('save_chart', { req });
}

export async function listCharts() {
    return invokeTauri('list_charts');
}

export async function getChart(id) {
    return invokeTauri('get_chart', { id });
}

export async function deleteChart(id) {
    return invokeTauri('delete_chart', { id });
}

export async function getAppSettings() {
    return invokeTauri('get_settings');
}

export async function saveAppSettings(settings) {
    return invokeTauri('save_settings', { settings });
}

export async function geocodeLocation(req) {
    return invokeTauri('geocode_location', { req });
}

export async function reverseGeocodeLocation(req) {
    return invokeTauri('reverse_geocode_location', { req });
}

export async function listModels() {
    return invokeTauri('list_models');
}

export async function importModel(req) {
    return invokeTauri('import_model', { req });
}

export async function importLlamaSidecar(req) {
    return invokeTauri('import_llama_sidecar', { req });
}

export async function getModelInfo(modelId) {
    return invokeTauri('get_model_info', { modelId });
}

export async function getModelManifest() {
    return invokeTauri('get_model_manifest');
}

export async function downloadModel(req) {
    return invokeTauri('download_model', { req });
}

export async function getModelStatus() {
    return invokeTauri('get_model_status');
}

export async function getLlamaSidecarInfo() {
    return invokeTauri('get_llama_sidecar_info');
}

export async function getNativeLlamaRuntimeInfo() {
    return invokeTauri('get_native_llama_runtime_info');
}

export async function getNativeLlamaHealth() {
    return invokeTauri('get_native_llama_health');
}

export async function startLlama(req) {
    return invokeTauri('start_llama', { req });
}

export async function stopLlama() {
    return invokeTauri('stop_llama');
}

export async function generateInterpretation(req) {
    return invokeTauri('generate_interpretation', { req });
}

export async function startInterpretationStream(req) {
    return invokeTauri('start_interpretation_stream', { req });
}

export async function cancelInterpretationStream(generationId) {
    return invokeTauri('cancel_interpretation_stream', { generationId });
}

export async function listenAiInterpretationEvents(handlers) {
    if (!isTauriRuntime()) {
        throw new Error('Tauri event listeners unavailable in web runtime');
    }

    const { listen } = await import('@tauri-apps/api/event');
    const unlisteners = await Promise.all([
        listen('ai-interpretation-token', event => handlers.token?.(event.payload)),
        listen('ai-interpretation-complete', event => handlers.complete?.(event.payload)),
        listen('ai-interpretation-error', event => handlers.error?.(event.payload)),
        listen('ai-interpretation-cancelled', event => handlers.cancelled?.(event.payload)),
    ]);

    return async () => {
        for (const unlisten of unlisteners) {
            unlisten();
        }
    };
}
