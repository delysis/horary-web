import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import {
    loadWebHoraryHistory,
} from '../history/horaryHistory.js';
import {
    HOUSE_SYSTEM_STORAGE_KEY,
    PLANET_SET_STORAGE_KEY,
    currentSettingsPayload as settingsPayloadFromState,
    loadStoredAspectSettings,
} from '../settings/settingsPayload.js';

export function createInitialAppState({
    storage = globalThis.localStorage,
    defaultSettings = DEFAULT_HORARY_SETTINGS,
    loadHistory = loadWebHoraryHistory,
    loadAspectSettings = loadStoredAspectSettings,
} = {}) {
    return {
        houseSystem: storage?.getItem?.(HOUSE_SYSTEM_STORAGE_KEY) || defaultSettings.houseSystem,
        planetSet: storage?.getItem?.(PLANET_SET_STORAGE_KEY) || defaultSettings.planetSet,
        aspectSettings: loadAspectSettings(storage),
        chart: null,
        horaryHistory: loadHistory(storage),
        geoLocation: null,
        locationStatus: 'unknown',
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
    };
}

export function currentAppSettingsPayload(state) {
    return settingsPayloadFromState(state);
}
