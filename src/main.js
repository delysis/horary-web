/**
 * Whorary — Main Application Controller
 * 
 * Wires up the astronomy engine to the UI components.
 */

import './index.css';
import { setupAiPanelController } from './ai/aiPanelController.js';
import {
    downloadJson,
    removeLegacyZodiacSetting,
    renderAppVersion,
    renderInitialChartPlaceholder,
    setDefaultDateInput,
    showHoraryError,
} from './app/appDom.js';
import {
    createInitialAppState,
    currentAppSettingsPayload,
} from './app/appState.js';
import { DEFAULT_HORARY_SETTINGS } from './astro/horarySettings.js';
import { getHouseSystems } from './astro/houses.js';
import { setupSettingsController } from './settings/settingsController.js';
import { setupPwaInstallPrompt } from './pwa/installPrompt.js';
import {
    cancelInterpretationStream,
    deleteChart,
    downloadModel,
    geocodeLocation,
    getChart,
    getLlamaSidecarInfo,
    getModelManifest,
    getModelStatus,
    importLlamaSidecar,
    importModel,
    listenAiInterpretationEvents,
    listCharts,
    listModels,
    saveChart,
    startLlama,
    startInterpretationStream,
    stopLlama,
} from './tauriBridge.js';
import { createLocalCityGeocoder } from './location/geocoder.js';
import { setupLocationController } from './location/locationController.js';
import { setupChartCalculationController } from './chart/calculationController.js';
import { setupHoraryController } from './horary/horaryController.js';
import { setupTabsController } from './ui/tabsController.js';
import { setupDesktopRuntimeController } from './desktop/desktopRuntimeController.js';

const locationGeocoder = createLocalCityGeocoder();

// ╔══════════════════════════════════════╗
//    STATE
// ╚══════════════════════════════════════╝

const state = createInitialAppState();

let settingsController = null;
let chartCalculationController = null;
let locationController = null;
let aiPanelController = null;
let horaryController = null;
let tabsController = null;
let desktopRuntimeController = null;

function saveSettings() {
    desktopRuntimeController?.saveSettings();
}

function currentSettingsPayload() {
    return currentAppSettingsPayload(state);
}

// ╔══════════════════════════════════════╗
//    LOCAL AI
// ╚══════════════════════════════════════╝

function initAiPanel() {
    aiPanelController = setupAiPanelController({
        documentRef: document,
        state,
        horarySettings: DEFAULT_HORARY_SETTINGS,
        commands: {
            listModels,
            getModelStatus,
            getModelManifest,
            getLlamaSidecarInfo,
            downloadModel,
            importLlamaSidecar,
            importModel,
            startLlama,
            stopLlama,
            startInterpretationStream,
            cancelInterpretationStream,
            listenAiInterpretationEvents,
        },
    });
}

async function initTauriFeatures() {
    await desktopRuntimeController?.initTauriFeatures();
}

function setAiError(message) {
    aiPanelController?.setAiError(message);
}

function setAiOutput(interpretation) {
    aiPanelController?.setAiOutput(interpretation);
}

function updateAiControls() {
    aiPanelController?.updateAiControls();
}

// ╔══════════════════════════════════════╗
//    HORARY
// ╚══════════════════════════════════════╝

function startHoraryClock() {
    horaryController?.startHoraryClock();
}

async function loadTauriHoraryHistory() {
    await horaryController?.loadTauriHoraryHistory();
}

// ╔══════════════════════════════════════╗
//    UI INITIALIZATION
// ╚══════════════════════════════════════╝

function initTabs() {
    tabsController = setupTabsController({
        documentRef: document,
        onActivateTab(tabName) {
            if (tabName === 'horary') {
                startHoraryClock();
            }
        },
    });
}

function initDesktopRuntimeController() {
    desktopRuntimeController = setupDesktopRuntimeController({
        documentRef: document,
        state,
        houseSystems: getHouseSystems(),
        currentSettingsPayload,
        updateSettingsControls() {
            settingsController?.updateSettingsControls();
        },
        recalculate,
        hasChart() {
            return Boolean(state.chart);
        },
        loadHoraryHistory: loadTauriHoraryHistory,
        setupAiStreamListeners() {
            return aiPanelController?.setupAiStreamListeners();
        },
        refreshAiPanel() {
            return aiPanelController?.refreshAiPanel();
        },
    });
}

function initSettingsController() {
    settingsController = setupSettingsController({
        documentRef: document,
        houseSystems: getHouseSystems(),
        getSettings: currentSettingsPayload,
        setHouseSystem(value) {
            state.houseSystem = value;
        },
        setPlanetSet(value) {
            state.planetSet = value;
        },
        setAspectSettings(value) {
            state.aspectSettings = value;
        },
        saveSettings,
        recalculate,
        hasChart() {
            return Boolean(state.chart);
        },
        downloadJson,
    });
}

function initCalculation() {
    chartCalculationController = setupChartCalculationController({
        documentRef: document,
        getSettings: currentSettingsPayload,
        getChart() {
            return state.chart;
        },
        setChart(chart) {
            state.chart = chart;
        },
        onManualCoordinateEdit() {
            state.geoLocation = null;
            state.locationStatus = 'manualEditing';
        },
    });
}

function recalculate() {
    return chartCalculationController?.recalculate() || null;
}

function initLocationController() {
    locationController = setupLocationController({
        documentRef: document,
        navigatorRef: navigator,
        getTauriRuntime() {
            return state.tauriRuntime;
        },
        geocodeLocation,
        localGeocoder: locationGeocoder,
        setGeoLocation(value) {
            state.geoLocation = value;
        },
        setLocationStatus(value) {
            state.locationStatus = value;
        },
        showHoraryError,
    });
}

function initHorary() {
    horaryController = setupHoraryController({
        documentRef: document,
        state,
        horarySettings: DEFAULT_HORARY_SETTINGS,
        commands: {
            listCharts,
            saveChart,
            getChart,
            deleteChart,
        },
        showHoraryError,
        downloadJson,
        setAiOutput,
        setAiError,
        updateAiControls,
    });
}

// ╔══════════════════════════════════════╗
//    BOOT
// ╚══════════════════════════════════════╝

function init() {
    removeLegacyZodiacSetting();
    setDefaultDateInput();
    renderAppVersion();

    initHorary();
    initTabs();
    initDesktopRuntimeController();
    initSettingsController();
    initCalculation();
    initLocationController();
    initAiPanel();
    setupPwaInstallPrompt();
    initTauriFeatures();

    renderInitialChartPlaceholder();

    // Restore settings controls
    settingsController?.updateSettingsControls();
}

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
} else {
    init();
}
