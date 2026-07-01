import test from 'node:test';
import assert from 'node:assert/strict';

import { cloneDefaultAspectSettings } from '../../settings/aspectSettings.js';
import { setupDesktopRuntimeController } from '../desktopRuntimeController.js';

test('setupDesktopRuntimeController leaves web runtime unchanged when Tauri bridge is absent', async () => {
    const documentRef = createDesktopDocument();
    const state = createDesktopState();
    const calls = [];
    const controller = setupDesktopRuntimeController({
        documentRef,
        state,
        commands: {
            initializeTauriBridge: async () => null,
            getAppSettings: async () => {
                calls.push('getAppSettings');
                return {};
            },
        },
        loadHoraryHistory: async () => calls.push('loadHoraryHistory'),
        setupAiStreamListeners: async () => calls.push('setupAiStreamListeners'),
        refreshAiPanel: async () => calls.push('refreshAiPanel'),
    });

    const result = await controller.initTauriFeatures();

    assert.equal(result, null);
    assert.equal(state.tauriRuntime, false);
    assert.equal(documentRef.tauriOnly[0].hidden, true);
    assert.deepEqual(calls, []);
});

test('setupDesktopRuntimeController initializes Tauri runtime and startup workflows', async () => {
    const documentRef = createDesktopDocument();
    const state = createDesktopState({
        houseSystem: 'placidus',
        planetSet: 'modern',
        chart: { id: 'chart' },
    });
    const calls = [];
    const persisted = [];
    const savedSettings = {
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings: {
            trine: { enabled: false, orb: 6 },
        },
    };
    const controller = setupDesktopRuntimeController({
        documentRef,
        state,
        houseSystems: [{ key: 'regiomontanus' }, { key: 'placidus' }],
        commands: {
            initializeTauriBridge: async () => {
                calls.push('initializeTauriBridge');
                return { runtime: 'tauri' };
            },
            getAppSettings: async () => {
                calls.push('getAppSettings');
                return savedSettings;
            },
        },
        persistSettings(settings) {
            calls.push('persistSettings');
            persisted.push(settings);
        },
        updateSettingsControls() {
            calls.push('updateSettingsControls');
        },
        recalculate() {
            calls.push('recalculate');
        },
        loadHoraryHistory: async () => calls.push('loadHoraryHistory'),
        setupAiStreamListeners: async () => calls.push('setupAiStreamListeners'),
        refreshAiPanel: async () => calls.push('refreshAiPanel'),
    });

    const result = await controller.initTauriFeatures();

    assert.deepEqual(result, { runtime: 'tauri' });
    assert.equal(state.tauriRuntime, true);
    assert.equal(documentRef.tauriOnly[0].hidden, false);
    assert.equal(state.houseSystem, 'regiomontanus');
    assert.equal(state.planetSet, 'classical');
    assert.equal(state.aspectSettings.trine.enabled, false);
    assert.equal(state.aspectSettings.trine.orb, 6);
    assert.deepEqual(persisted, [{
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings: state.aspectSettings,
    }]);
    assert.deepEqual(calls, [
        'initializeTauriBridge',
        'getAppSettings',
        'persistSettings',
        'updateSettingsControls',
        'recalculate',
        'loadHoraryHistory',
        'setupAiStreamListeners',
        'refreshAiPanel',
    ]);
});

test('setupDesktopRuntimeController falls back invalid desktop settings without recalculating unchanged charts', async () => {
    const state = createDesktopState({
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        chart: { id: 'chart' },
    });
    const calls = [];
    const controller = setupDesktopRuntimeController({
        state,
        houseSystems: [{ key: 'regiomontanus' }],
        defaultSettings: { houseSystem: 'regiomontanus', planetSet: 'classical' },
        commands: {
            getAppSettings: async () => ({
                houseSystem: 'unknown',
                planetSet: 'invalid',
                aspectSettings: null,
            }),
        },
        persistSettings() {
            calls.push('persistSettings');
        },
        updateSettingsControls() {
            calls.push('updateSettingsControls');
        },
        recalculate() {
            calls.push('recalculate');
        },
    });

    const loaded = await controller.loadTauriSettings();

    assert.equal(loaded, true);
    assert.equal(state.houseSystem, 'regiomontanus');
    assert.equal(state.planetSet, 'classical');
    assert.deepEqual(calls, ['persistSettings', 'updateSettingsControls']);
});

test('setupDesktopRuntimeController saves settings locally and to Tauri only in desktop runtime', async () => {
    const state = createDesktopState();
    const calls = [];
    const controller = setupDesktopRuntimeController({
        state,
        currentSettingsPayload: () => ({ houseSystem: 'regiomontanus', planetSet: 'classical', aspectSettings: {} }),
        persistSettings(settings) {
            calls.push(['persistSettings', settings]);
        },
        commands: {
            saveAppSettings: async settings => {
                calls.push(['saveAppSettings', settings]);
            },
        },
    });

    controller.saveSettings();
    state.tauriRuntime = true;
    controller.saveSettings();
    await Promise.resolve();

    assert.deepEqual(calls, [
        ['persistSettings', { houseSystem: 'regiomontanus', planetSet: 'classical', aspectSettings: {} }],
        ['persistSettings', { houseSystem: 'regiomontanus', planetSet: 'classical', aspectSettings: {} }],
        ['saveAppSettings', { houseSystem: 'regiomontanus', planetSet: 'classical', aspectSettings: {} }],
    ]);
});

test('setupDesktopRuntimeController reports initialization and settings failures', async () => {
    const warnings = [];
    const initController = setupDesktopRuntimeController({
        commands: {
            initializeTauriBridge: async () => {
                throw new Error('bridge failed');
            },
        },
        consoleRef: {
            warn(...args) {
                warnings.push(args);
            },
        },
    });
    const settingsController = setupDesktopRuntimeController({
        commands: {
            getAppSettings: async () => {
                throw new Error('settings failed');
            },
        },
        consoleRef: {
            warn(...args) {
                warnings.push(args);
            },
        },
    });

    assert.equal(await initController.initTauriFeatures(), null);
    assert.equal(await settingsController.loadTauriSettings(), false);
    assert.match(warnings[0][0], /Tauri bridge initialization failed/);
    assert.match(warnings[1][0], /Tauri settings load failed/);
});

function createDesktopState(overrides = {}) {
    return {
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings: cloneDefaultAspectSettings(),
        chart: null,
        tauriRuntime: false,
        ...overrides,
    };
}

function createDesktopDocument() {
    return {
        tauriOnly: [{ hidden: true }, { hidden: true }],
        querySelectorAll(selector) {
            if (selector === '[data-tauri-only]') return this.tauriOnly;
            return [];
        },
    };
}
