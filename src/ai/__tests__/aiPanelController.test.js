import test from 'node:test';
import assert from 'node:assert/strict';

import { setupAiPanelController } from '../aiPanelController.js';

test('setupAiPanelController disables local AI controls outside Tauri', () => {
    const documentRef = createAiDocument();
    setupAiPanelController({
        documentRef,
        state: createAiState({ tauriRuntime: false }),
    });

    assert.equal(documentRef.getElementById('refreshAiModelsBtn').disabled, true);
    assert.equal(documentRef.getElementById('downloadAiModelBtn').disabled, true);
    assert.equal(documentRef.getElementById('importAiSidecarBtn').disabled, true);
    assert.equal(documentRef.getElementById('importAiModelBtn').disabled, true);
    assert.equal(documentRef.getElementById('startAiModelBtn').disabled, true);
    assert.equal(documentRef.getElementById('generateAiBtn').disabled, true);
    assert.equal(documentRef.getElementById('cancelAiBtn').hidden, true);
});

test('setupAiPanelController refreshes model state and renders model options', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({ tauriRuntime: true });
    const controller = setupAiPanelController({
        documentRef,
        state,
        commands: {
            listModels: async () => [{
                id: 'local-model',
                displayName: 'Local <Model>',
                sizeBytes: 1024,
            }],
            getModelStatus: async () => ({ running: false }),
            getModelManifest: async () => ({
                models: [{
                    id: 'catalog-model',
                    displayName: 'Catalog Model',
                    sizeBytes: 2048,
                    source: { url: 'https://example.test/model.gguf' },
                    sha256: 'a'.repeat(64),
                    license: 'Apache-2.0',
                    minRamGb: 8,
                    recommendedRamGb: 16,
                    defaultContextTokens: 8192,
                    defaultParams: {
                        temperature: 0.4,
                        top_p: 0.9,
                        max_tokens: 1200,
                    },
                }],
            }),
            getLlamaSidecarInfo: async () => ({ installed: true, path: '/bin/llama-server' }),
        },
    });

    await controller.refreshAiPanel();

    assert.deepEqual(state.aiModels, [{
        id: 'local-model',
        displayName: 'Local <Model>',
        sizeBytes: 1024,
    }]);
    assert.match(documentRef.getElementById('aiModelSelect').innerHTML, /Local &lt;Model&gt;/);
    assert.match(documentRef.getElementById('aiDownloadSelect').innerHTML, /Catalog Model/);
    assert.equal(documentRef.getElementById('aiModelStatus').textContent, 'Local model stopped. Local AI runtime ready at /bin/llama-server.');
    assert.equal(documentRef.getElementById('startAiModelBtn').disabled, false);
    assert.equal(documentRef.getElementById('downloadAiModelBtn').disabled, false);
});

test('setupAiPanelController installs a native llama-server sidecar path', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({ tauriRuntime: true });
    const calls = [];
    setupAiPanelController({
        documentRef,
        state,
        commands: {
            importLlamaSidecar: async request => {
                calls.push(['importLlamaSidecar', request]);
                return { installed: true, path: '/app-data/binaries/llama-server-aarch64-apple-darwin' };
            },
            listModels: async () => [],
            getModelStatus: async () => ({ running: false }),
            getModelManifest: async () => ({ models: [] }),
            getLlamaSidecarInfo: async () => ({ installed: true, path: '/app-data/binaries/llama-server-aarch64-apple-darwin' }),
        },
    });

    await documentRef.getElementById('importAiSidecarBtn').dispatch('click').pending;
    assert.equal(documentRef.getElementById('aiErrors').textContent, 'Enter a local llama-server binary path before installing.');

    documentRef.getElementById('aiSidecarPath').value = '/opt/homebrew/bin/llama-server';
    await documentRef.getElementById('importAiSidecarBtn').dispatch('click').pending;

    assert.deepEqual(calls, [['importLlamaSidecar', { path: '/opt/homebrew/bin/llama-server' }]]);
    assert.equal(documentRef.getElementById('aiSidecarPath').value, '');
    assert.equal(state.aiSidecar.installed, true);
    assert.match(documentRef.getElementById('aiAcquireStatus').textContent, /Installed native local AI runtime/);
});

test('setupAiPanelController imports a local model path and refreshes selection', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({ tauriRuntime: true });
    const calls = [];
    setupAiPanelController({
        documentRef,
        state,
        commands: {
            importModel: async request => {
                calls.push(['importModel', request]);
                return { id: 'imported', displayName: 'Imported Model' };
            },
            listModels: async () => [{ id: 'imported', displayName: 'Imported Model', sizeBytes: 4096 }],
            getModelStatus: async () => ({ running: false }),
            getModelManifest: async () => ({ models: [] }),
            getLlamaSidecarInfo: async () => ({ installed: true, path: '/bin/llama-server' }),
        },
    });

    await documentRef.getElementById('importAiModelBtn').dispatch('click').pending;
    assert.equal(documentRef.getElementById('aiErrors').textContent, 'Enter a local .gguf file path before importing.');

    documentRef.getElementById('aiImportPath').value = '/models/local.gguf';
    await documentRef.getElementById('importAiModelBtn').dispatch('click').pending;

    assert.deepEqual(calls, [['importModel', { path: '/models/local.gguf' }]]);
    assert.equal(documentRef.getElementById('aiImportPath').value, '');
    assert.match(documentRef.getElementById('aiModelSelect').innerHTML, /value="imported" selected/);
    assert.equal(documentRef.getElementById('aiAcquireStatus').textContent, 'Imported Imported Model.');
});

test('setupAiPanelController starts and stops the selected model', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({
        tauriRuntime: true,
        aiModels: [{ id: 'model-1', displayName: 'Model 1', sizeBytes: 1024 }],
        aiSidecar: { installed: true, path: '/bin/llama-server' },
        aiStatus: { running: false },
    });
    const calls = [];
    documentRef.getElementById('aiModelSelect').value = 'model-1';
    setupAiPanelController({
        documentRef,
        state,
        commands: {
            startLlama: async request => {
                calls.push(['startLlama', request]);
                return { running: true, modelId: request.modelId, port: 8080 };
            },
            stopLlama: async () => {
                calls.push(['stopLlama']);
            },
            getModelStatus: async () => ({ running: false }),
        },
    });

    await documentRef.getElementById('startAiModelBtn').dispatch('click').pending;
    await documentRef.getElementById('stopAiModelBtn').dispatch('click').pending;

    assert.deepEqual(calls, [
        ['startLlama', {
            modelId: 'model-1',
            ctxSize: 16384,
            nGpuLayers: 'auto',
            parallel: 4,
            continuousBatching: true,
            cacheRamMb: 4096,
            cacheIdleSlots: true,
            coldKvCache: true,
            specDraftNMax: 3,
        }],
        ['stopLlama'],
    ]);
    assert.deepEqual(state.aiStatus, { running: false });
});

test('setupAiPanelController streams and completes an AI interpretation', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({
        tauriRuntime: true,
        aiStatus: { running: true, modelId: 'model-1', port: 8080 },
        aiSidecar: { installed: true, path: '/bin/llama-server' },
        lastHoraryQuestion: 'Will it work?',
        lastHoraryChartFacts: { ascendant: { sign: 'Cancer' } },
    });
    let streamRequest = null;
    let handlers = null;
    const controller = setupAiPanelController({
        documentRef,
        state,
        horarySettings: { houseSystem: 'whole-sign', zodiac: 'tropical' },
        commands: {
            listenAiInterpretationEvents: async nextHandlers => {
                handlers = nextHandlers;
                return () => {};
            },
            startInterpretationStream: async request => {
                streamRequest = request;
                return { generationId: 'gen-1' };
            },
            getModelStatus: async () => ({ running: true, modelId: 'model-1', port: 8080 }),
        },
    });

    await controller.setupAiStreamListeners();
    await documentRef.getElementById('generateAiBtn').dispatch('click').pending;

    assert.deepEqual(streamRequest, {
        question: 'Will it work?',
        chart: { ascendant: { sign: 'Cancer' } },
        settings: {
            tradition: 'traditional',
            traditionProfile: 'traditional-horary-textbook-v1',
            tone: 'technical',
            houseSystem: 'whole-sign',
            zodiac: 'tropical',
        },
    });
    assert.equal(state.aiBusy, true);
    assert.equal(state.aiStreaming, true);
    assert.equal(state.aiGenerationId, 'gen-1');

    handlers.token({ generationId: 'other', text: 'ignored' });
    handlers.token({ generationId: 'gen-1', text: '{"summary":' });
    handlers.token({ generationId: 'gen-1', text: '"yes"}' });
    assert.match(documentRef.getElementById('aiOutput').innerHTML, /\{&quot;summary&quot;:&quot;yes&quot;\}/);

    await handlers.complete({
        generationId: 'gen-1',
        interpretation: {
            summary: 'It works.',
            confidence: 'high',
            judgementTrace: [{
                stepId: 'synthesis_pass',
                finding: 'The answer is positive.',
                chartEvidence: 'Ascendant in Cancer',
                confidence: 'high',
            }],
            keyFactors: [],
        },
    });

    assert.equal(state.aiBusy, false);
    assert.equal(state.aiStreaming, false);
    assert.equal(state.aiGenerationId, null);
    assert.match(documentRef.getElementById('aiOutput').innerHTML, /It works\./);
    assert.equal(documentRef.getElementById('aiModelStatus').textContent, 'Running model-1 with local inference backend.');
});

test('setupAiPanelController cleanup removes registered event listeners', async () => {
    const documentRef = createAiDocument();
    const state = createAiState({ tauriRuntime: true });
    let refreshCalls = 0;
    const controller = setupAiPanelController({
        documentRef,
        state,
        commands: {
            listModels: async () => {
                refreshCalls++;
                return [];
            },
        },
    });

    controller.cleanup();
    await documentRef.getElementById('refreshAiModelsBtn').dispatch('click').pending;

    assert.equal(refreshCalls, 0);
});

function createAiState(overrides = {}) {
    return {
        tauriRuntime: false,
        aiModels: [],
        aiManifest: null,
        aiSidecar: null,
        aiStatus: null,
        aiBusy: false,
        aiStreaming: false,
        aiGenerationId: null,
        aiStreamText: '',
        aiUnlisten: null,
        lastHoraryQuestion: '',
        lastHoraryChartFacts: null,
        ...overrides,
    };
}

function createAiDocument() {
    const documentRef = new FakeDocument();
    [
        'refreshAiModelsBtn',
        'downloadAiModelBtn',
        'importAiSidecarBtn',
        'importAiModelBtn',
        'startAiModelBtn',
        'stopAiModelBtn',
        'generateAiBtn',
        'cancelAiBtn',
        'aiModelSelect',
        'aiDownloadSelect',
        'aiSidecarPath',
        'aiImportPath',
        'aiModelStatus',
        'aiAcquireStatus',
        'aiErrors',
        'aiOutput',
    ].forEach(id => documentRef.add(id));
    return documentRef;
}

class FakeDocument {
    constructor() {
        this.elements = new Map();
    }

    add(id) {
        const element = new FakeElement(id);
        this.elements.set(id, element);
        return element;
    }

    getElementById(id) {
        return this.elements.get(id) || null;
    }
}

class FakeElement {
    constructor(id) {
        this.id = id;
        this.value = '';
        this.textContent = '';
        this._innerHTML = '';
        this.className = '';
        this.hidden = false;
        this.disabled = false;
        this.listeners = new Map();
    }

    get innerHTML() {
        return this._innerHTML;
    }

    set innerHTML(value) {
        this._innerHTML = value;
        const selectedOption = /<option[^>]*value="([^"]*)"[^>]* selected/.exec(value);
        const firstOption = /<option[^>]*value="([^"]*)"/.exec(value);
        const optionValue = selectedOption?.[1] ?? firstOption?.[1];
        if (optionValue !== undefined) {
            this.value = optionValue.replace(/&quot;/g, '"').replace(/&amp;/g, '&');
        }
    }

    addEventListener(type, listener) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(listener);
    }

    removeEventListener(type, listener) {
        this.listeners.set(type, (this.listeners.get(type) ?? []).filter(item => item !== listener));
    }

    dispatch(type, event = {}) {
        const dispatched = { target: this, ...event };
        const pending = Promise.all((this.listeners.get(type) ?? []).map(listener => listener(dispatched)));
        return { pending };
    }
}
