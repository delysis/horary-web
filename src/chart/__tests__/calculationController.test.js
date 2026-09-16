import test from 'node:test';
import assert from 'node:assert/strict';

import { setupChartCalculationController } from '../calculationController.js';

test('setupChartCalculationController validates input, calculates, stores, and renders a chart', async () => {
    const documentRef = createChartDocument({
        inputDate: '2026-06-30',
        inputTime: '09:30',
        inputOffset: '-4',
        inputLat: '40.7',
        inputLng: '-74',
    });
    const utcDate = new Date('2026-06-30T13:30:00.000Z');
    const chart = { id: 'chart' };
    const calls = {
        validation: null,
        calculation: null,
        clearErrors: 0,
        rendered: [],
        storedChart: null,
    };
    setupChartCalculationController({
        documentRef,
        getSettings: () => ({
            houseSystem: 'regiomontanus',
            planetSet: 'classical',
            aspectSettings: { trine: { orb: 7 } },
        }),
        setChart(nextChart) {
            calls.storedChart = nextChart;
        },
        validateInput(raw, options) {
            calls.validation = { raw, options };
            return {
                ok: true,
                value: {
                    utcDate,
                    localDate: '2026-06-30',
                    localTime: '09:30',
                    offsetHours: -4,
                    latitude: 40.7,
                    longitude: -74,
                },
            };
        },
        calculate(args) {
            calls.calculation = args;
            return chart;
        },
        clearErrors() {
            calls.clearErrors++;
        },
        showErrors() {
            assert.fail('showErrors should not be called for valid input');
        },
        renderers: recordingRenderers(calls.rendered),
    });

    await documentRef.getElementById('calcBtn').dispatch('click').pending;

    assert.deepEqual(calls.validation.raw, {
        date: '2026-06-30',
        time: '09:30',
        offset: '-4',
        lat: '40.7',
        lng: '-74',
    });
    assert.deepEqual(calls.validation.options, { allowFuture: true });
    assert.deepEqual(calls.calculation, {
        date: utcDate,
        lat: 40.7,
        lng: -74,
        localDate: '2026-06-30',
        localTime: '09:30',
        offsetHours: -4,
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectConfig: { trine: { orb: 7 } },
    });
    assert.equal(calls.storedChart, chart);
    assert.equal(calls.clearErrors, 1);
    assert.deepEqual(calls.rendered, [
        ['renderChartWheel', chart, 'chartWheel'],
        ['renderPositionsTable', chart],
        ['renderCuspsTable', chart],
        ['renderAspectGrid', chart],
        ['renderAspectsList', chart],
        ['renderDignitiesTable', chart],
    ]);
});

test('setupChartCalculationController reports validation errors without calculating', () => {
    const documentRef = createChartDocument();
    const calls = {
        errors: null,
        calculated: false,
        cleared: false,
    };
    const controller = setupChartCalculationController({
        documentRef,
        validateInput: () => ({
            ok: false,
            errors: { inputDate: 'Enter a date.' },
        }),
        calculate() {
            calls.calculated = true;
        },
        showErrors(errors) {
            calls.errors = errors;
        },
        clearErrors() {
            calls.cleared = true;
        },
        renderers: recordingRenderers([]),
    });

    const result = controller.recalculate();

    assert.equal(result, null);
    assert.deepEqual(calls.errors, { inputDate: 'Enter a date.' });
    assert.equal(calls.calculated, false);
    assert.equal(calls.cleared, false);
});

test('setupChartCalculationController recalculates on Enter in chart inputs', async () => {
    const documentRef = createChartDocument();
    let calculationCount = 0;
    setupChartCalculationController({
        documentRef,
        validateInput: () => ({
            ok: true,
            value: {
                utcDate: new Date('2026-06-30T12:00:00.000Z'),
                latitude: 0,
                longitude: 0,
            },
        }),
        calculate() {
            calculationCount++;
            return {};
        },
        renderers: recordingRenderers([]),
    });

    await documentRef.getElementById('inputDate').dispatch('keydown', { key: 'Tab' }).pending;
    await documentRef.getElementById('inputDate').dispatch('keydown', { key: 'Enter' }).pending;

    assert.equal(calculationCount, 1);
});

test('setupChartCalculationController marks manual coordinate edits', async () => {
    const documentRef = createChartDocument();
    let editCount = 0;
    setupChartCalculationController({
        documentRef,
        onManualCoordinateEdit() {
            editCount++;
        },
    });

    await documentRef.getElementById('inputLat').dispatch('input').pending;
    await documentRef.getElementById('inputLng').dispatch('input').pending;

    assert.equal(editCount, 2);
});

test('setupChartCalculationController rerenders aspects for filter changes only when a chart exists', async () => {
    const documentRef = createChartDocument();
    const chart = { id: 'chart' };
    const rendered = [];
    let currentChart = chart;
    setupChartCalculationController({
        documentRef,
        getChart: () => currentChart,
        renderers: recordingRenderers(rendered),
    });

    await documentRef.getElementById('majorOnlyToggle').dispatch('change').pending;
    currentChart = null;
    await documentRef.getElementById('applyingOnlyToggle').dispatch('change').pending;

    assert.deepEqual(rendered, [['renderAspectsList', chart]]);
});

function createChartDocument(values = {}) {
    const documentRef = new FakeDocument();
    [
        'calcBtn',
        'inputDate',
        'inputTime',
        'inputOffset',
        'inputLat',
        'inputLng',
        'majorOnlyToggle',
        'applyingOnlyToggle',
    ].forEach(id => {
        const element = documentRef.add(id);
        element.value = values[id] || '';
    });
    return documentRef;
}

function recordingRenderers(rendered) {
    return {
        renderChartWheel(chart, target) {
            rendered.push(['renderChartWheel', chart, target]);
        },
        renderPositionsTable(chart) {
            rendered.push(['renderPositionsTable', chart]);
        },
        renderCuspsTable(chart) {
            rendered.push(['renderCuspsTable', chart]);
        },
        renderAspectGrid(chart) {
            rendered.push(['renderAspectGrid', chart]);
        },
        renderAspectsList(chart) {
            rendered.push(['renderAspectsList', chart]);
        },
        renderDignitiesTable(chart) {
            rendered.push(['renderDignitiesTable', chart]);
        },
    };
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
