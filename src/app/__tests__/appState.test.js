import test from 'node:test';
import assert from 'node:assert/strict';

import { ASPECT_SETTINGS_STORAGE_KEY } from '../../settings/aspectSettings.js';
import {
    HORARY_HISTORY_STORAGE_KEY,
} from '../../history/horaryHistory.js';
import {
    HOUSE_SYSTEM_STORAGE_KEY,
    PLANET_SET_STORAGE_KEY,
} from '../../settings/settingsPayload.js';
import {
    createInitialAppState,
    currentAppSettingsPayload,
} from '../appState.js';

test('createInitialAppState builds default web app state', () => {
    const state = createInitialAppState({
        storage: memoryStorage(),
        defaultSettings: {
            houseSystem: 'regiomontanus',
            planetSet: 'classical',
        },
    });

    assert.equal(state.houseSystem, 'regiomontanus');
    assert.equal(state.planetSet, 'classical');
    assert.equal(state.chart, null);
    assert.deepEqual(state.horaryHistory, []);
    assert.equal(state.geoLocation, null);
    assert.equal(state.locationStatus, 'unknown');
    assert.equal(state.tauriRuntime, false);
    assert.deepEqual(state.aiModels, []);
    assert.equal(state.aiBusy, false);
    assert.equal(state.aiStreaming, false);
    assert.equal(state.aiGenerationId, null);
    assert.equal(state.lastHoraryQuestion, '');
    assert.equal(state.lastHoraryChartFacts, null);
});

test('createInitialAppState reads stored settings, aspect policy, and history', () => {
    const history = [{ id: 'saved', question: 'Saved chart?' }];
    const storage = memoryStorage({
        [HOUSE_SYSTEM_STORAGE_KEY]: 'whole-sign',
        [PLANET_SET_STORAGE_KEY]: 'modern',
        [ASPECT_SETTINGS_STORAGE_KEY]: JSON.stringify({
            trine: { enabled: false, orb: 4 },
        }),
        [HORARY_HISTORY_STORAGE_KEY]: JSON.stringify(history),
    });

    const state = createInitialAppState({ storage });

    assert.equal(state.houseSystem, 'whole-sign');
    assert.equal(state.planetSet, 'modern');
    assert.equal(state.aspectSettings.trine.enabled, false);
    assert.equal(state.aspectSettings.trine.orb, 4);
    assert.deepEqual(state.horaryHistory, history);
});

test('createInitialAppState accepts injected loaders for isolated tests', () => {
    const calls = [];
    const storage = memoryStorage();
    const state = createInitialAppState({
        storage,
        defaultSettings: {
            houseSystem: 'regiomontanus',
            planetSet: 'classical',
        },
        loadAspectSettings(nextStorage) {
            calls.push(['loadAspectSettings', nextStorage]);
            return { sextile: { enabled: true, orb: 3 } };
        },
        loadHistory(nextStorage) {
            calls.push(['loadHistory', nextStorage]);
            return [{ id: 'history' }];
        },
    });

    assert.deepEqual(state.aspectSettings, { sextile: { enabled: true, orb: 3 } });
    assert.deepEqual(state.horaryHistory, [{ id: 'history' }]);
    assert.deepEqual(calls, [
        ['loadAspectSettings', storage],
        ['loadHistory', storage],
    ]);
});

test('currentAppSettingsPayload serializes only persisted settings', () => {
    const aspectSettings = { trine: { enabled: true, orb: 7 } };
    const payload = currentAppSettingsPayload({
        houseSystem: 'regiomontanus',
        planetSet: 'modern',
        aspectSettings,
        chart: { id: 'ignored' },
    });

    assert.deepEqual(payload, {
        houseSystem: 'regiomontanus',
        planetSet: 'modern',
        aspectSettings,
    });
});

function memoryStorage(initial = {}) {
    const values = new Map(Object.entries(initial));
    return {
        getItem(key) {
            return values.has(key) ? values.get(key) : null;
        },
        setItem(key, value) {
            values.set(key, value);
        },
    };
}
