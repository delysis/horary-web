import { buildHoraryChartFacts } from '../ai/chartFacts.js';
import { calculateChart } from '../astro/chart.js';
import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import { resolveHoraryCoordinates } from '../location/coordinates.js';
import {
    clearChartErrors,
    showChartErrors,
} from '../ui/fieldErrors.js';
import { renderHoraryDashboard } from '../ui/horaryDashboard.js';
import {
    createHoraryHistoryEntry,
    currentSavedChartLocationLabel,
    historyEntryDate,
    isRenderableSavedChart,
    normalizeSavedChart,
    persistWebHoraryHistory,
    prependHoraryHistoryEntry,
    tauriSummaryToHistoryEntry,
} from '../history/horaryHistory.js';
import {
    renderHoraryHistoryMarkup,
    safeHistoryExportFilename,
} from './horaryHistoryView.js';

const DEFAULT_COMMANDS = Object.freeze({
    listCharts: async () => [],
    saveChart: async () => {},
    getChart: async () => null,
    deleteChart: async () => {},
});

export function setupHoraryController({
    documentRef = globalThis.document,
    state = {},
    horarySettings = DEFAULT_HORARY_SETTINGS,
    commands = {},
    calculate = calculateChart,
    buildChartFacts = buildHoraryChartFacts,
    resolveCoordinates = resolveHoraryCoordinates,
    renderDashboard = renderHoraryDashboard,
    showErrors = showChartErrors,
    clearErrors = clearChartErrors,
    showHoraryError = () => {},
    persistWebHistory = persistWebHoraryHistory,
    createHistoryEntry = createHoraryHistoryEntry,
    prependHistoryEntry = prependHoraryHistoryEntry,
    toTauriHistoryEntry = tauriSummaryToHistoryEntry,
    normalizeChart = normalizeSavedChart,
    canRenderSavedChart = isRenderableSavedChart,
    savedChartLocationLabel = currentSavedChartLocationLabel,
    entryDate = historyEntryDate,
    renderHistoryMarkup = renderHoraryHistoryMarkup,
    exportFilename = safeHistoryExportFilename,
    downloadJson = () => {},
    setAiOutput = () => {},
    setAiError = () => {},
    updateAiControls = () => {},
    now = () => new Date(),
    consoleRef = globalThis.console,
} = {}) {
    if (!documentRef?.getElementById) {
        return emptyHoraryController();
    }

    const commandApi = { ...DEFAULT_COMMANDS, ...commands };
    let horaryClockInterval = null;
    const castButton = documentRef.getElementById('castNowBtn');
    const historyList = documentRef.getElementById('horaryHistory');

    function startHoraryClock() {
        function tick() {
            const currentTime = now();
            const timeElement = documentRef.getElementById('horaryTime');
            const dateElement = documentRef.getElementById('horaryDate');
            if (timeElement) timeElement.textContent = currentTime.toLocaleTimeString();
            if (dateElement) {
                dateElement.textContent = currentTime.toLocaleDateString(undefined, {
                    weekday: 'long',
                    year: 'numeric',
                    month: 'long',
                    day: 'numeric',
                });
            }
        }
        tick();
        if (horaryClockInterval) clearInterval(horaryClockInterval);
        horaryClockInterval = setInterval(tick, 1000);
    }

    function castHoraryChart() {
        const question = documentRef.getElementById('horaryQuestion')?.value?.trim() || '(No question recorded)';
        const coordinates = resolveCoordinates({
            geoLocation: state.geoLocation,
            manualLat: documentRef.getElementById('inputLat')?.value,
            manualLng: documentRef.getElementById('inputLng')?.value,
        });
        if (!coordinates.ok) {
            showHoraryError('Enter a valid latitude and longitude, choose a saved city, or click Use Browser Location before casting.');
            showErrors(coordinates.errors);
            const container = documentRef.getElementById('horaryChart');
            if (container) container.innerHTML = '';
            return;
        }
        showHoraryError('');
        clearErrors();

        const lat = coordinates.value.latitude;
        const lng = coordinates.value.longitude;
        const castTime = now();
        const chart = calculate({
            date: castTime,
            lat,
            lng,
            houseSystem: horarySettings.houseSystem,
            planetSet: state.planetSet,
            aspectConfig: state.aspectSettings,
        });

        state.chart = chart;
        state.lastHoraryQuestion = question;
        state.lastHoraryChartFacts = buildChartFacts(chart, {
            locationLabel: currentLocationLabel(lat, lng),
        });
        setAiOutput(null);
        setAiError('');
        renderDashboard(chart);

        const entry = createHistoryEntry({
            id: castTime.getTime(),
            question,
            time: castTime.toISOString(),
            lat,
            lng,
            chart,
            planetSet: state.planetSet,
            aspectSettings: state.aspectSettings,
        });
        state.horaryHistory = prependHistoryEntry(state.horaryHistory, entry);
        persistWebHistory(state.horaryHistory);
        renderHoraryHistory();
        persistHoraryChartToTauri(entry);
        updateAiControls();
    }

    function renderHoraryHistory() {
        const list = documentRef.getElementById('horaryHistory');
        const empty = documentRef.getElementById('emptyHistory');
        if (!list) return;

        if (!Array.isArray(state.horaryHistory) || state.horaryHistory.length === 0) {
            list.innerHTML = '';
            if (empty) empty.style.display = '';
            return;
        }

        if (empty) empty.style.display = 'none';
        list.innerHTML = renderHistoryMarkup(state.horaryHistory, {
            tauriRuntime: state.tauriRuntime,
            canRenderSavedChart,
            entryDate,
        });
    }

    async function handleHoraryHistoryAction(event) {
        const button = event.target?.closest?.('[data-history-action]');
        if (!button) return;
        const action = button.dataset.historyAction;
        const id = button.dataset.id;
        if (!id) return;

        button.disabled = true;
        showHoraryError('');
        try {
            if (action === 'load') {
                await loadHoraryHistoryEntry(id);
            } else if (action === 'export') {
                await exportHoraryHistoryEntry(id);
            } else if (action === 'delete') {
                await deleteHoraryHistoryEntry(id);
            }
        } catch (error) {
            showHoraryError(error?.message || String(error));
        } finally {
            if (button.isConnected) button.disabled = false;
        }
    }

    async function loadTauriHoraryHistory() {
        if (!state.tauriRuntime) return;
        try {
            const charts = await commandApi.listCharts();
            state.horaryHistory = charts.map(toTauriHistoryEntry);
            renderHoraryHistory();
        } catch (error) {
            consoleRef?.warn?.('Tauri chart history load failed:', error);
        }
    }

    function persistHoraryChartToTauri(entry) {
        if (!state.tauriRuntime) return;
        commandApi.saveChart({
            id: String(entry.id),
            question: entry.question,
            chart: entry.chart,
        }).catch(error => {
            consoleRef?.warn?.('Tauri chart save failed:', error);
        });
    }

    async function loadHoraryHistoryEntry(id) {
        const savedChart = await savedChartForHistoryEntry(id);
        const chart = normalizeChart(savedChart);
        if (!canRenderSavedChart(chart)) {
            throw new Error('Saved chart is a legacy summary and cannot be loaded into the full chart view.');
        }

        state.chart = chart;
        state.lastHoraryQuestion = chart.question || '(No question recorded)';
        state.lastHoraryChartFacts = buildChartFacts(chart, {
            locationLabel: chart.location?.label || savedChartLocationLabel(chart),
        });
        setAiOutput(null);
        setAiError('');
        renderDashboard(chart);
        updateAiControls();
    }

    async function exportHoraryHistoryEntry(id) {
        const entry = getHoraryHistoryEntry(id);
        if (!entry) throw new Error('Saved chart was not found.');
        const savedChart = await savedChartForHistoryEntry(id, { allowSummaryFallback: true });
        downloadJson(exportFilename(entry.question || id), {
            exportedAt: now().toISOString(),
            source: state.tauriRuntime ? 'tauri' : 'web',
            entry: {
                id: String(entry.id),
                question: entry.question,
                time: entry.time,
            },
            chart: savedChart,
        });
    }

    async function deleteHoraryHistoryEntry(id) {
        if (state.tauriRuntime) {
            await commandApi.deleteChart(String(id));
        }
        state.horaryHistory = state.horaryHistory.filter(entry => String(entry.id) !== String(id));
        persistWebHistory(state.horaryHistory);
        renderHoraryHistory();
    }

    async function savedChartForHistoryEntry(id, { allowSummaryFallback = false } = {}) {
        const entry = getHoraryHistoryEntry(id);
        if (!entry) throw new Error('Saved chart was not found.');

        if (state.tauriRuntime) {
            const saved = await commandApi.getChart(String(id));
            if (saved?.chart) return saved.chart;
        }
        if (entry.chart) return entry.chart;
        if (allowSummaryFallback) return { type: 'horary-summary', ...entry };
        return null;
    }

    function getHoraryHistoryEntry(id) {
        return state.horaryHistory.find(entry => String(entry.id) === String(id));
    }

    function currentLocationLabel(lat, lng) {
        const city = documentRef.getElementById('inputCity')?.value?.trim();
        if (city) return city;
        if (state.locationStatus === 'detected') return 'Browser location';
        return `${lat.toFixed(4)}, ${lng.toFixed(4)}`;
    }

    function cleanup() {
        castButton?.removeEventListener?.('click', castHoraryChart);
        historyList?.removeEventListener?.('click', handleHoraryHistoryAction);
        if (horaryClockInterval) clearInterval(horaryClockInterval);
    }

    castButton?.addEventListener?.('click', castHoraryChart);
    historyList?.addEventListener?.('click', handleHoraryHistoryAction);
    renderHoraryHistory();

    return {
        startHoraryClock,
        castHoraryChart,
        renderHoraryHistory,
        handleHoraryHistoryAction,
        loadTauriHoraryHistory,
        loadHoraryHistoryEntry,
        exportHoraryHistoryEntry,
        deleteHoraryHistoryEntry,
        cleanup,
    };
}

function emptyHoraryController() {
    return {
        startHoraryClock() {},
        castHoraryChart() {},
        renderHoraryHistory() {},
        async handleHoraryHistoryAction() {},
        async loadTauriHoraryHistory() {},
        async loadHoraryHistoryEntry() {},
        async exportHoraryHistoryEntry() {},
        async deleteHoraryHistoryEntry() {},
        cleanup() {},
    };
}
