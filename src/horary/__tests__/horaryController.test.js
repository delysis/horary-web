import test from 'node:test';
import assert from 'node:assert/strict';

import { setupHoraryController } from '../horaryController.js';

test('setupHoraryController reports coordinate errors without casting', () => {
    const documentRef = createHoraryDocument();
    const state = createHoraryState();
    const calls = {
        horaryErrors: [],
        fieldErrors: null,
        calculated: false,
    };
    const controller = setupHoraryController({
        documentRef,
        state,
        resolveCoordinates: () => ({
            ok: false,
            errors: { inputLat: 'Enter a latitude.' },
        }),
        calculate() {
            calls.calculated = true;
        },
        showHoraryError: message => calls.horaryErrors.push(message),
        showErrors: errors => {
            calls.fieldErrors = errors;
        },
    });

    controller.castHoraryChart();

    assert.deepEqual(calls.horaryErrors, [
        'Enter a valid latitude and longitude, choose a saved city, or click Use Browser Location before casting.',
    ]);
    assert.deepEqual(calls.fieldErrors, { inputLat: 'Enter a latitude.' });
    assert.equal(calls.calculated, false);
    assert.equal(documentRef.getElementById('horaryChart').innerHTML, '');
});

test('setupHoraryController casts, renders, persists, and resets AI output', () => {
    const documentRef = createHoraryDocument();
    documentRef.getElementById('horaryQuestion').value = 'Will it work?';
    documentRef.getElementById('inputCity').value = 'London';
    const castTime = new Date('2026-06-30T14:15:00.000Z');
    const chart = { id: 'chart-1' };
    const state = createHoraryState({
        tauriRuntime: true,
        planetSet: 'classical',
        aspectSettings: { trine: { orb: 7 } },
    });
    const calls = {
        calculation: null,
        facts: null,
        historyPayload: null,
        persistedHistory: null,
        renderedChart: null,
        savedChart: null,
        aiOutput: ['untouched'],
        aiErrors: ['untouched'],
        controls: 0,
        clearedErrors: 0,
        horaryErrors: [],
    };
    const controller = setupHoraryController({
        documentRef,
        state,
        horarySettings: { houseSystem: 'regiomontanus' },
        now: () => castTime,
        resolveCoordinates: () => ({
            ok: true,
            value: { latitude: 51.5, longitude: -0.1 },
        }),
        calculate(args) {
            calls.calculation = args;
            return chart;
        },
        buildChartFacts(nextChart, options) {
            calls.facts = { chart: nextChart, options };
            return { fact: 'chart-facts' };
        },
        createHistoryEntry(payload) {
            calls.historyPayload = payload;
            return { id: payload.id, question: payload.question, time: payload.time, chart: payload.chart };
        },
        prependHistoryEntry(history, entry) {
            return [entry, ...history];
        },
        persistWebHistory(history) {
            calls.persistedHistory = history;
        },
        renderDashboard(nextChart) {
            calls.renderedChart = nextChart;
        },
        clearErrors() {
            calls.clearedErrors++;
        },
        showHoraryError(message) {
            calls.horaryErrors.push(message);
        },
        setAiOutput(value) {
            calls.aiOutput.push(value);
        },
        setAiError(value) {
            calls.aiErrors.push(value);
        },
        updateAiControls() {
            calls.controls++;
        },
        commands: {
            saveChart: async request => {
                calls.savedChart = request;
            },
        },
    });

    controller.castHoraryChart();

    assert.deepEqual(calls.calculation, {
        date: castTime,
        lat: 51.5,
        lng: -0.1,
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectConfig: { trine: { orb: 7 } },
    });
    assert.deepEqual(calls.facts, {
        chart,
        options: { locationLabel: 'London' },
    });
    assert.equal(state.chart, chart);
    assert.equal(state.lastHoraryQuestion, 'Will it work?');
    assert.deepEqual(state.lastHoraryChartFacts, { fact: 'chart-facts' });
    assert.deepEqual(calls.aiOutput, ['untouched', null]);
    assert.deepEqual(calls.aiErrors, ['untouched', '']);
    assert.equal(calls.controls, 1);
    assert.equal(calls.clearedErrors, 1);
    assert.deepEqual(calls.horaryErrors, ['']);
    assert.deepEqual(calls.persistedHistory, state.horaryHistory);
    assert.match(documentRef.getElementById('horaryHistory').innerHTML, /Will it work\?/);
    assert.deepEqual(calls.savedChart, {
        id: String(castTime.getTime()),
        question: 'Will it work?',
        chart,
    });
});

test('setupHoraryController loads Tauri chart history into the rendered list', async () => {
    const documentRef = createHoraryDocument();
    const state = createHoraryState({ tauriRuntime: true });
    const controller = setupHoraryController({
        documentRef,
        state,
        commands: {
            listCharts: async () => [{ id: 'native-1', question: 'Native chart', createdAt: '2026-06-30T12:00:00.000Z' }],
        },
        toTauriHistoryEntry(summary) {
            return {
                id: summary.id,
                question: summary.question,
                time: summary.createdAt,
                chart: { id: 'chart' },
            };
        },
    });

    await controller.loadTauriHoraryHistory();

    assert.deepEqual(state.horaryHistory, [{
        id: 'native-1',
        question: 'Native chart',
        time: '2026-06-30T12:00:00.000Z',
        chart: { id: 'chart' },
    }]);
    assert.match(documentRef.getElementById('horaryHistory').innerHTML, /Native chart/);
});

test('setupHoraryController loads and exports saved horary history entries', async () => {
    const documentRef = createHoraryDocument();
    const savedChart = {
        id: 'chart-1',
        question: 'Saved question',
        location: { label: 'Paris' },
    };
    const state = createHoraryState({
        horaryHistory: [{
            id: 'entry-1',
            question: 'Will <it> work?',
            time: '2026-06-30T12:00:00.000Z',
            chart: savedChart,
        }],
    });
    const calls = {
        facts: null,
        renderedChart: null,
        downloaded: null,
        controls: 0,
    };
    const controller = setupHoraryController({
        documentRef,
        state,
        now: () => new Date('2026-06-30T15:00:00.000Z'),
        normalizeChart: chart => ({ ...chart, normalized: true }),
        canRenderSavedChart: chart => Boolean(chart?.normalized),
        buildChartFacts(chart, options) {
            calls.facts = { chart, options };
            return { loaded: true };
        },
        renderDashboard(chart) {
            calls.renderedChart = chart;
        },
        downloadJson(filename, payload) {
            calls.downloaded = { filename, payload };
        },
        updateAiControls() {
            calls.controls++;
        },
    });

    await controller.loadHoraryHistoryEntry('entry-1');
    await controller.exportHoraryHistoryEntry('entry-1');

    assert.equal(state.chart.normalized, true);
    assert.equal(state.lastHoraryQuestion, 'Saved question');
    assert.deepEqual(state.lastHoraryChartFacts, { loaded: true });
    assert.deepEqual(calls.facts.options, { locationLabel: 'Paris' });
    assert.equal(calls.renderedChart.normalized, true);
    assert.equal(calls.controls, 1);
    assert.equal(calls.downloaded.filename, 'horary-will-it-work.json');
    assert.deepEqual(calls.downloaded.payload, {
        exportedAt: '2026-06-30T15:00:00.000Z',
        source: 'web',
        entry: {
            id: 'entry-1',
            question: 'Will <it> work?',
            time: '2026-06-30T12:00:00.000Z',
        },
        chart: savedChart,
    });
});

test('setupHoraryController deletes desktop history entries through native storage', async () => {
    const documentRef = createHoraryDocument();
    const state = createHoraryState({
        tauriRuntime: true,
        horaryHistory: [
            { id: 'keep', question: 'Keep', time: '2026-06-30T11:00:00.000Z', chart: {} },
            { id: 'drop', question: 'Drop', time: '2026-06-30T12:00:00.000Z', chart: {} },
        ],
    });
    const calls = {
        deleted: [],
        persisted: null,
    };
    const controller = setupHoraryController({
        documentRef,
        state,
        commands: {
            deleteChart: async id => calls.deleted.push(id),
        },
        persistWebHistory(history) {
            calls.persisted = history;
        },
    });

    await controller.deleteHoraryHistoryEntry('drop');

    assert.deepEqual(calls.deleted, ['drop']);
    assert.deepEqual(state.horaryHistory.map(entry => entry.id), ['keep']);
    assert.deepEqual(calls.persisted.map(entry => entry.id), ['keep']);
    assert.match(documentRef.getElementById('horaryHistory').innerHTML, /Keep/);
    assert.doesNotMatch(documentRef.getElementById('horaryHistory').innerHTML, /Drop/);
});

function createHoraryState(overrides = {}) {
    return {
        planetSet: 'classical',
        aspectSettings: {},
        chart: null,
        horaryHistory: [],
        geoLocation: null,
        locationStatus: 'unknown',
        tauriRuntime: false,
        lastHoraryQuestion: '',
        lastHoraryChartFacts: null,
        ...overrides,
    };
}

function createHoraryDocument() {
    const documentRef = new FakeDocument();
    [
        'castNowBtn',
        'horaryHistory',
        'emptyHistory',
        'horaryQuestion',
        'inputLat',
        'inputLng',
        'inputCity',
        'horaryChart',
        'horaryTime',
        'horaryDate',
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
        this.innerHTML = '';
        this.hidden = false;
        this.disabled = false;
        this.isConnected = true;
        this.style = {};
        this.listeners = new Map();
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
