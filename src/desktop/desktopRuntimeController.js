import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import { getHouseSystems } from '../astro/houses.js';
import {
    aspectSettingsEqual,
    normalizeAspectSettings,
} from '../settings/aspectSettings.js';
import {
    currentSettingsPayload as settingsPayloadFromState,
    persistSettingsToStorage,
} from '../settings/settingsPayload.js';
import {
    getAppSettings,
    initializeTauriBridge,
    saveAppSettings,
} from '../tauriBridge.js';

const DEFAULT_COMMANDS = Object.freeze({
    initializeTauriBridge,
    getAppSettings,
    saveAppSettings,
});

export function setupDesktopRuntimeController({
    documentRef = globalThis.document,
    state = {},
    houseSystems = getHouseSystems(),
    defaultSettings = DEFAULT_HORARY_SETTINGS,
    commands = {},
    currentSettingsPayload = () => settingsPayloadFromState(state),
    persistSettings = settings => persistSettingsToStorage(settings),
    updateSettingsControls = () => {},
    recalculate = () => {},
    hasChart = () => Boolean(state.chart),
    loadHoraryHistory = async () => {},
    setupAiStreamListeners = async () => {},
    refreshAiPanel = async () => {},
    consoleRef = globalThis.console,
} = {}) {
    const commandApi = { ...DEFAULT_COMMANDS, ...commands };

    function saveSettings() {
        const settings = currentSettingsPayload();
        persistSettings(settings);
        if (state.tauriRuntime) {
            commandApi.saveAppSettings(settings).catch(error => {
                consoleRef?.warn?.('Tauri settings save failed:', error);
            });
        }
    }

    async function initTauriFeatures() {
        try {
            const appInfo = await commandApi.initializeTauriBridge();
            if (!appInfo) return null;
            state.tauriRuntime = true;
            documentRef?.querySelectorAll?.('[data-tauri-only]')?.forEach(el => {
                el.hidden = false;
            });
            await loadTauriSettings();
            await loadHoraryHistory();
            await setupAiStreamListeners();
            await refreshAiPanel();
            return appInfo;
        } catch (error) {
            consoleRef?.warn?.('Tauri bridge initialization failed:', error);
            return null;
        }
    }

    async function loadTauriSettings() {
        try {
            const settings = await commandApi.getAppSettings();
            const houseSystemKeys = new Set(houseSystems.map(system => system.key));
            const nextHouseSystem = houseSystemKeys.has(settings?.houseSystem)
                ? settings.houseSystem
                : defaultSettings.houseSystem;
            const nextPlanetSet = ['classical', 'modern'].includes(settings?.planetSet)
                ? settings.planetSet
                : defaultSettings.planetSet;
            const nextAspectSettings = normalizeAspectSettings(settings?.aspectSettings || state.aspectSettings);
            const changed = state.houseSystem !== nextHouseSystem
                || state.planetSet !== nextPlanetSet
                || !aspectSettingsEqual(state.aspectSettings, nextAspectSettings);

            state.houseSystem = nextHouseSystem;
            state.planetSet = nextPlanetSet;
            state.aspectSettings = nextAspectSettings;
            persistSettings(currentSettingsPayload());
            updateSettingsControls();
            if (changed && hasChart()) {
                recalculate();
            }
            return true;
        } catch (error) {
            consoleRef?.warn?.('Tauri settings load failed:', error);
            return false;
        }
    }

    return {
        initTauriFeatures,
        loadTauriSettings,
        saveSettings,
    };
}
