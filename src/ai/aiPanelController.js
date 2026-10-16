import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import { INTERPRETATION_TRADITION_PROFILE } from './interpretationSchema.js';
import {
    aiStatusTextForState,
    computeAiControlState,
    downloadableManifestModels,
    renderAiInterpretationMarkup,
    renderAiStreamMarkup,
    renderDownloadModelOptions,
    renderImportedModelOptions,
    selectedImportedModelId,
} from './aiPanelView.js';

const DEFAULT_COMMANDS = Object.freeze({
    listModels: async () => [],
    getModelStatus: async () => null,
    getModelManifest: async () => null,
    getLlamaSidecarInfo: async () => null,
    downloadModel: async () => null,
    importLlamaSidecar: async () => null,
    importModel: async () => null,
    startLlama: async () => null,
    stopLlama: async () => {},
    startInterpretationStream: async () => ({ generationId: null }),
    cancelInterpretationStream: async () => false,
    listenAiInterpretationEvents: async () => () => {},
});

export function setupAiPanelController({
    documentRef = globalThis.document,
    state = {},
    horarySettings = DEFAULT_HORARY_SETTINGS,
    commands = {},
    consoleRef = globalThis.console,
} = {}) {
    if (!documentRef?.getElementById) {
        return emptyAiPanelController();
    }

    const commandApi = { ...DEFAULT_COMMANDS, ...commands };
    const elements = {
        refreshButton: documentRef.getElementById('refreshAiModelsBtn'),
        downloadButton: documentRef.getElementById('downloadAiModelBtn'),
        sidecarImportButton: documentRef.getElementById('importAiSidecarBtn'),
        importButton: documentRef.getElementById('importAiModelBtn'),
        startButton: documentRef.getElementById('startAiModelBtn'),
        stopButton: documentRef.getElementById('stopAiModelBtn'),
        generateButton: documentRef.getElementById('generateAiBtn'),
        cancelButton: documentRef.getElementById('cancelAiBtn'),
        modelSelect: documentRef.getElementById('aiModelSelect'),
        downloadSelect: documentRef.getElementById('aiDownloadSelect'),
        sidecarInput: documentRef.getElementById('aiSidecarPath'),
        importInput: documentRef.getElementById('aiImportPath'),
        modelStatus: documentRef.getElementById('aiModelStatus'),
        acquireStatus: documentRef.getElementById('aiAcquireStatus'),
        errors: documentRef.getElementById('aiErrors'),
        output: documentRef.getElementById('aiOutput'),
    };

    async function refreshAiPanel() {
        if (!state.tauriRuntime || state.aiBusy) return;
        setAiError('');
        setAiStatusText('Refreshing local model status...');
        setAiBusy(true);
        try {
            await refreshAiPanelAfterMutation();
            updateAiControls();
        } catch (error) {
            setAiError(error?.message || String(error));
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function downloadSelectedAiModel() {
        if (!state.tauriRuntime || state.aiBusy) return;
        const selectedModel = elements.downloadSelect?.value;
        if (!selectedModel) {
            setAiError('Select a catalog model first.');
            return;
        }

        setAiError('');
        setAiAcquireStatus('Downloading and verifying model...');
        setAiBusy(true);
        try {
            const model = await commandApi.downloadModel({ modelId: selectedModel });
            await refreshAiPanelAfterMutation(model?.id);
            setAiAcquireStatus(`Installed ${model?.displayName || selectedModel}.`);
        } catch (error) {
            setAiError(error?.message || String(error));
            setAiAcquireStatus('Model download failed.');
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function importAiModelFromPath() {
        if (!state.tauriRuntime || state.aiBusy) return;
        const path = elements.importInput?.value?.trim() || '';
        if (!path) {
            setAiError('Enter a local .gguf file path before importing.');
            return;
        }

        setAiError('');
        setAiAcquireStatus('Importing local model...');
        setAiBusy(true);
        try {
            const model = await commandApi.importModel({ path });
            if (elements.importInput) elements.importInput.value = '';
            await refreshAiPanelAfterMutation(model?.id);
            setAiAcquireStatus(`Imported ${model?.displayName || path}.`);
        } catch (error) {
            setAiError(error?.message || String(error));
            setAiAcquireStatus('Model import failed.');
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function importAiSidecarFromPath() {
        if (!state.tauriRuntime || state.aiBusy) return;
        const path = elements.sidecarInput?.value?.trim() || '';
        if (!path) {
            setAiError('Enter a local llama-server binary path before installing.');
            return;
        }

        setAiError('');
        setAiAcquireStatus('Installing native local AI runtime...');
        setAiBusy(true);
        try {
            const sidecar = await commandApi.importLlamaSidecar({ path });
            state.aiSidecar = sidecar;
            if (elements.sidecarInput) elements.sidecarInput.value = '';
            await refreshAiPanelAfterMutation(elements.modelSelect?.value);
            setAiAcquireStatus(`Installed native local AI runtime at ${sidecar?.path || 'app data'}.`);
        } catch (error) {
            setAiError(error?.message || String(error));
            setAiAcquireStatus('Local AI runtime install failed.');
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function refreshAiPanelAfterMutation(selectedModelId) {
        const [models, status, manifest, sidecar] = await Promise.all([
            commandApi.listModels(),
            commandApi.getModelStatus(),
            commandApi.getModelManifest(),
            commandApi.getLlamaSidecarInfo(),
        ]);
        state.aiModels = models;
        state.aiStatus = status;
        state.aiManifest = manifest;
        state.aiSidecar = sidecar;
        renderAiModelOptions(selectedModelId);
        renderAiDownloadOptions();
    }

    async function startSelectedAiModel() {
        if (!state.tauriRuntime || state.aiBusy) return;
        const selectedModel = elements.modelSelect?.value;
        if (!selectedModel) {
            setAiError('Select an imported local model first.');
            return;
        }

        setAiError('');
        setAiStatusText('Starting local model...');
        setAiBusy(true);
        try {
            state.aiStatus = await commandApi.startLlama({
                modelId: selectedModel,
                ctxSize: 16384,
                nGpuLayers: 'auto',
                parallel: 4,
                continuousBatching: true,
                cacheRamMb: 4096,
                cacheIdleSlots: true,
                coldKvCache: true,
                specDraftNMax: 3,
            });
        } catch (error) {
            setAiError(error?.message || String(error));
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function stopAiModel() {
        if (!state.tauriRuntime || state.aiBusy) return;
        setAiError('');
        setAiStatusText('Stopping local model...');
        setAiBusy(true);
        try {
            await commandApi.stopLlama();
            state.aiStatus = await commandApi.getModelStatus();
        } catch (error) {
            setAiError(error?.message || String(error));
        } finally {
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function generateAiInterpretation() {
        if (!state.tauriRuntime || state.aiBusy) return;
        if (!state.lastHoraryChartFacts) {
            setAiError('Cast a horary chart before generating an interpretation.');
            return;
        }
        if (!state.aiStatus?.running) {
            setAiError('Start a local model before generating an interpretation.');
            return;
        }

        setAiError('');
        state.aiStreamText = '';
        setAiStreamOutput('');
        setAiStatusText('Generating streamed interpretation...');
        setAiBusy(true);
        try {
            const started = await commandApi.startInterpretationStream({
                question: state.lastHoraryQuestion || '(No question recorded)',
                chart: state.lastHoraryChartFacts,
                settings: {
                    tradition: 'traditional',
                    traditionProfile: INTERPRETATION_TRADITION_PROFILE,
                    tone: 'technical',
                    houseSystem: horarySettings.houseSystem,
                    zodiac: horarySettings.zodiac,
                },
            });
            state.aiGenerationId = started.generationId;
            state.aiStreaming = true;
            setAiStatusText(`Streaming interpretation ${started.generationId}...`);
            updateAiControls();
        } catch (error) {
            setAiError(error?.message || String(error));
            state.aiGenerationId = null;
            state.aiStreaming = false;
            setAiBusy(false);
            updateAiControls();
        }
    }

    async function cancelAiGeneration() {
        if (!state.tauriRuntime || !state.aiGenerationId || !state.aiStreaming) return;
        setAiStatusText('Cancelling streamed interpretation...');
        try {
            const cancelled = await commandApi.cancelInterpretationStream(state.aiGenerationId);
            if (!cancelled) {
                finishAiStream('No matching active generation to cancel.');
            }
        } catch (error) {
            setAiError(error?.message || String(error));
            finishAiStream('Cancellation failed.');
        }
    }

    async function setupAiStreamListeners() {
        if (state.aiUnlisten) return;
        state.aiUnlisten = await commandApi.listenAiInterpretationEvents({
            token: handleAiStreamToken,
            complete: handleAiStreamComplete,
            error: handleAiStreamError,
            cancelled: handleAiStreamCancelled,
        });
    }

    function handleAiStreamToken(payload) {
        if (!isCurrentAiGeneration(payload)) return;
        state.aiStreamText += payload.text || '';
        setAiStreamOutput(state.aiStreamText);
    }

    async function handleAiStreamComplete(payload) {
        if (!isCurrentAiGeneration(payload)) return;
        setAiOutput(payload.interpretation);
        finishAiStream('Interpretation complete.');
        await refreshAiStatusOnly();
    }

    function handleAiStreamError(payload) {
        if (!isCurrentAiGeneration(payload)) return;
        setAiError(payload.message || 'Interpretation stream failed.');
        finishAiStream('Interpretation failed.');
    }

    function handleAiStreamCancelled(payload) {
        if (!isCurrentAiGeneration(payload)) return;
        finishAiStream(payload.message || 'Generation cancelled.');
    }

    function isCurrentAiGeneration(payload) {
        return payload?.generationId && payload.generationId === state.aiGenerationId;
    }

    function finishAiStream(statusMessage) {
        state.aiBusy = false;
        state.aiStreaming = false;
        state.aiGenerationId = null;
        setAiStatusText(statusMessage);
        updateAiControls();
    }

    async function refreshAiStatusOnly() {
        try {
            state.aiStatus = await commandApi.getModelStatus();
        } catch (error) {
            consoleRef?.warn?.('Failed to refresh model status:', error);
        }
    }

    function renderAiModelOptions(preferredModelId) {
        const select = elements.modelSelect;
        if (!select) return;

        const currentValue = selectedImportedModelId(state.aiModels, {
            preferredModelId,
            currentValue: select.value,
            runningModelId: state.aiStatus?.modelId,
        });
        select.innerHTML = renderImportedModelOptions(state.aiModels, currentValue);
    }

    function renderAiDownloadOptions() {
        const select = elements.downloadSelect;
        if (!select) return;

        const downloadable = downloadableManifestModels(state.aiManifest);
        if (downloadable.length === 0) {
            select.innerHTML = renderDownloadModelOptions([], '');
            setAiAcquireStatus('No verified downloadable models in bundled manifest.');
            return;
        }

        const currentValue = select.value || downloadable[0].id;
        select.innerHTML = renderDownloadModelOptions(downloadable, currentValue);
    }

    function updateAiControls() {
        const controls = computeAiControlState({
            tauriRuntime: state.tauriRuntime,
            busy: state.aiBusy,
            streaming: state.aiStreaming,
            generationId: state.aiGenerationId,
            status: state.aiStatus,
            sidecar: state.aiSidecar,
            models: state.aiModels,
            lastHoraryChartFacts: state.lastHoraryChartFacts,
            selectedModelId: elements.modelSelect?.value,
            selectedDownloadId: elements.downloadSelect?.value,
            sidecarPath: elements.sidecarInput?.value,
            importPath: elements.importInput?.value,
        });

        if (elements.refreshButton) elements.refreshButton.disabled = controls.refreshDisabled;
        if (elements.downloadButton) elements.downloadButton.disabled = controls.downloadDisabled;
        if (elements.sidecarImportButton) elements.sidecarImportButton.disabled = controls.sidecarImportDisabled;
        if (elements.importButton) elements.importButton.disabled = controls.importDisabled;
        if (elements.startButton) elements.startButton.disabled = controls.startDisabled;
        if (elements.stopButton) elements.stopButton.disabled = controls.stopDisabled;
        if (elements.generateButton) elements.generateButton.disabled = controls.generateDisabled;
        if (elements.cancelButton) {
            elements.cancelButton.disabled = controls.cancelDisabled;
            elements.cancelButton.hidden = controls.cancelHidden;
        }
        if (elements.modelSelect) elements.modelSelect.disabled = controls.modelSelectDisabled;
        if (elements.downloadSelect) elements.downloadSelect.disabled = controls.downloadSelectDisabled;
        if (elements.sidecarInput) elements.sidecarInput.disabled = controls.sidecarInputDisabled;
        if (elements.importInput) elements.importInput.disabled = controls.importInputDisabled;

        const statusText = aiStatusTextForState({
            tauriRuntime: state.tauriRuntime,
            busy: state.aiBusy,
            streaming: state.aiStreaming,
            status: state.aiStatus,
            sidecar: state.aiSidecar,
            models: state.aiModels,
        });
        if (statusText !== null) setAiStatusText(statusText);
    }

    function setAiBusy(isBusy) {
        state.aiBusy = isBusy;
        updateAiControls();
    }

    function setAiStatusText(message) {
        if (elements.modelStatus) elements.modelStatus.textContent = message;
    }

    function setAiAcquireStatus(message) {
        if (elements.acquireStatus) elements.acquireStatus.textContent = message;
    }

    function setAiError(message) {
        if (!elements.errors) return;
        elements.errors.hidden = !message;
        elements.errors.textContent = message || '';
    }

    function setAiOutput(interpretation) {
        if (!elements.output) return;
        if (!interpretation) {
            elements.output.hidden = true;
            elements.output.innerHTML = '';
            return;
        }

        elements.output.innerHTML = renderAiInterpretationMarkup(interpretation);
        elements.output.hidden = false;
    }

    function setAiStreamOutput(text) {
        if (!elements.output) return;
        elements.output.innerHTML = renderAiStreamMarkup(text);
        elements.output.hidden = false;
    }

    function cleanup() {
        elements.refreshButton?.removeEventListener?.('click', refreshAiPanel);
        elements.downloadButton?.removeEventListener?.('click', downloadSelectedAiModel);
        elements.sidecarImportButton?.removeEventListener?.('click', importAiSidecarFromPath);
        elements.importButton?.removeEventListener?.('click', importAiModelFromPath);
        elements.startButton?.removeEventListener?.('click', startSelectedAiModel);
        elements.stopButton?.removeEventListener?.('click', stopAiModel);
        elements.generateButton?.removeEventListener?.('click', generateAiInterpretation);
        elements.cancelButton?.removeEventListener?.('click', cancelAiGeneration);
        elements.modelSelect?.removeEventListener?.('change', updateAiControls);
        elements.downloadSelect?.removeEventListener?.('change', updateAiControls);
        elements.sidecarInput?.removeEventListener?.('input', updateAiControls);
        elements.importInput?.removeEventListener?.('input', updateAiControls);
    }

    elements.refreshButton?.addEventListener?.('click', refreshAiPanel);
    elements.downloadButton?.addEventListener?.('click', downloadSelectedAiModel);
    elements.sidecarImportButton?.addEventListener?.('click', importAiSidecarFromPath);
    elements.importButton?.addEventListener?.('click', importAiModelFromPath);
    elements.startButton?.addEventListener?.('click', startSelectedAiModel);
    elements.stopButton?.addEventListener?.('click', stopAiModel);
    elements.generateButton?.addEventListener?.('click', generateAiInterpretation);
    elements.cancelButton?.addEventListener?.('click', cancelAiGeneration);
    elements.modelSelect?.addEventListener?.('change', updateAiControls);
    elements.downloadSelect?.addEventListener?.('change', updateAiControls);
    elements.sidecarInput?.addEventListener?.('input', updateAiControls);
    elements.importInput?.addEventListener?.('input', updateAiControls);
    updateAiControls();

    return {
        refreshAiPanel,
        refreshAiPanelAfterMutation,
        setupAiStreamListeners,
        updateAiControls,
        setAiError,
        setAiOutput,
        setAiStreamOutput,
        cleanup,
    };
}

function emptyAiPanelController() {
    return {
        async refreshAiPanel() {},
        async refreshAiPanelAfterMutation() {},
        async setupAiStreamListeners() {},
        updateAiControls() {},
        setAiError() {},
        setAiOutput() {},
        setAiStreamOutput() {},
        cleanup() {},
    };
}
