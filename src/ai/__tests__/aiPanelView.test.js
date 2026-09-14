import test from 'node:test';
import assert from 'node:assert/strict';

import {
    aiStatusTextForState,
    computeAiControlState,
    downloadableManifestModels,
    formatModelSize,
    renderAiInterpretationMarkup,
    renderAiStreamMarkup,
    renderDownloadModelOptions,
    renderImportedModelOptions,
    selectedImportedModelId,
} from '../aiPanelView.js';

const verifiedCatalogModel = {
    id: 'enabled',
    displayName: 'Enabled',
    source: { url: 'https://example.test/model.gguf' },
    sha256: 'a'.repeat(64),
    sizeBytes: 1024,
    license: 'Apache-2.0',
    minRamGb: 8,
    recommendedRamGb: 16,
    defaultContextTokens: 8192,
    defaultParams: {
        temperature: 0.4,
        top_p: 0.9,
        max_tokens: 1200,
    },
};

test('formatModelSize formats model byte sizes', () => {
    assert.equal(formatModelSize(0), 'unknown size');
    assert.equal(formatModelSize(Number.NaN), 'unknown size');
    assert.equal(formatModelSize(512), '512 B');
    assert.equal(formatModelSize(1536), '1.5 KB');
    assert.equal(formatModelSize(2 * 1024 * 1024 * 1024), '2.0 GB');
});

test('renderImportedModelOptions renders escaped imported model options', () => {
    const html = renderImportedModelOptions([
        { id: 'small', displayName: 'Small <Model>', sizeBytes: 512 },
        { id: 'big"model', displayName: 'Big & Model', sizeBytes: 1536 },
    ], 'big"model');

    assert.match(html, /value="small">Small &lt;Model&gt; \(512 B\)<\/option>/);
    assert.match(html, /value="big&quot;model" selected>Big &amp; Model \(1.5 KB\)<\/option>/);
    assert.equal(renderImportedModelOptions([], ''), '<option value="">No imported models</option>');
});

test('selectedImportedModelId follows preferred, current, running, then first model order', () => {
    const models = [{ id: 'first' }];

    assert.equal(selectedImportedModelId(models, { preferredModelId: 'preferred', currentValue: 'current', runningModelId: 'running' }), 'preferred');
    assert.equal(selectedImportedModelId(models, { currentValue: 'current', runningModelId: 'running' }), 'current');
    assert.equal(selectedImportedModelId(models, { runningModelId: 'running' }), 'running');
    assert.equal(selectedImportedModelId(models), 'first');
});

test('downloadableManifestModels filters disabled and sourceless catalog entries', () => {
    const models = downloadableManifestModels({
        models: [
            verifiedCatalogModel,
            { ...verifiedCatalogModel, id: 'disabled', enabled: false, source: { url: 'https://example.test/disabled.gguf' } },
            { id: 'local-only' },
            { ...verifiedCatalogModel, id: 'incomplete-profile', defaultParams: null },
        ],
    });

    assert.deepEqual(models, [verifiedCatalogModel]);
});

test('renderDownloadModelOptions renders catalog options and empty state', () => {
    const html = renderDownloadModelOptions([
        { id: 'catalog', displayName: 'Catalog & Model', sizeBytes: 2048 },
    ], 'catalog');

    assert.match(html, /value="catalog" selected>Catalog &amp; Model \(2.0 KB\)<\/option>/);
    assert.equal(renderDownloadModelOptions([], ''), '<option value="">No verified catalog models</option>');
});

test('computeAiControlState captures enabled and disabled control states', () => {
    const ready = computeAiControlState({
        tauriRuntime: true,
        status: { running: false },
        sidecar: { installed: true },
        models: [{ id: 'model' }],
        selectedModelId: 'model',
        selectedDownloadId: 'catalog',
        sidecarPath: '/opt/homebrew/bin/llama-server',
        importPath: '/tmp/model.gguf',
    });

    assert.equal(ready.startDisabled, false);
    assert.equal(ready.downloadDisabled, false);
    assert.equal(ready.sidecarImportDisabled, false);
    assert.equal(ready.importDisabled, false);
    assert.equal(ready.sidecarInputDisabled, false);
    assert.equal(ready.stopDisabled, true);
    assert.equal(ready.generateDisabled, true);

    const streaming = computeAiControlState({
        tauriRuntime: true,
        busy: false,
        streaming: true,
        generationId: 'gen-1',
        status: { running: true },
        sidecar: { installed: true },
        models: [{ id: 'model' }],
        selectedModelId: 'model',
    });

    assert.equal(streaming.canCancel, true);
    assert.equal(streaming.cancelHidden, false);
    assert.equal(streaming.generateDisabled, true);
});

test('aiStatusTextForState describes desktop model status', () => {
    assert.equal(aiStatusTextForState({ tauriRuntime: false }), null);
    assert.equal(aiStatusTextForState({ tauriRuntime: true, busy: true }), null);
    assert.equal(aiStatusTextForState({ tauriRuntime: true, sidecar: null }), 'Local AI runtime status unavailable.');
    assert.equal(aiStatusTextForState({ tauriRuntime: true, sidecar: { installed: false } }), 'Local AI runtime missing.');
    assert.equal(aiStatusTextForState({ tauriRuntime: true, sidecar: { installed: true }, status: null }), 'Local model status unavailable.');
    assert.equal(
        aiStatusTextForState({ tauriRuntime: true, sidecar: { installed: true }, status: { running: true, modelId: 'm', port: 1234, backend: 'native sidecar' } }),
        'Running m with local inference backend.'
    );
    assert.equal(
        aiStatusTextForState({ tauriRuntime: true, sidecar: { installed: true, path: '/bin/llama' }, status: { running: false }, models: [] }),
        'No local GGUF models imported.'
    );
    assert.equal(
        aiStatusTextForState({ tauriRuntime: true, sidecar: { installed: true, path: '/bin/llama' }, status: { running: false }, models: [{ id: 'm' }] }),
        'Local model stopped. Local AI runtime ready at /bin/llama.'
    );
});

test('renderAiInterpretationMarkup escapes interpretation content', () => {
    const html = renderAiInterpretationMarkup({
        summary: 'A <summary>',
        directAnswer: 'Yes & no',
        confidence: 'medium',
        judgementTrace: [{
            stepId: 'house_assignment',
            finding: 'Use the 10th <house>.',
            chartEvidence: 'MC & house cusp supplied',
            confidence: 'medium',
        }],
        keyFactors: [{
            factor: 'Moon <Mars>',
            chartEvidence: 'Moon applies & separates',
            interpretation: 'Important <signal>',
        }],
        cautions: ['Do not overstate <certainty>'],
        followUpQuestions: ['What changed & when?'],
    });

    assert.match(html, /A &lt;summary&gt;/);
    assert.match(html, /Yes &amp; no/);
    assert.match(html, /house_assignment/);
    assert.match(html, /Use the 10th &lt;house&gt;\./);
    assert.match(html, /Moon &lt;Mars&gt;/);
    assert.match(html, /Moon applies &amp; separates/);
    assert.match(html, /Do not overstate &lt;certainty&gt;/);
    assert.match(html, /What changed &amp; when\?/);
    assert.equal(renderAiInterpretationMarkup(null), '');
});

test('renderAiStreamMarkup escapes streamed text and renders waiting state', () => {
    assert.match(renderAiStreamMarkup('<json>'), /&lt;json&gt;/);
    assert.match(renderAiStreamMarkup(''), /Waiting for model output/);
});
