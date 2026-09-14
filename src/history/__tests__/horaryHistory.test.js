import test from 'node:test';
import assert from 'node:assert/strict';

import {
    HORARY_HISTORY_STORAGE_KEY,
    createHoraryHistoryEntry,
    currentSavedChartLocationLabel,
    historyEntryDate,
    isRenderableSavedChart,
    loadWebHoraryHistory,
    normalizeSavedChart,
    persistWebHoraryHistory,
    prependHoraryHistoryEntry,
    serializePositions,
    tauriSummaryToHistoryEntry,
    timestampToIso,
} from '../horaryHistory.js';

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

function chartFixture() {
    return {
        houseSystem: 'regiomontanus',
        zodiac: 'tropical',
        houses: {
            asc: 104.5,
            cusps: [104.5, 130, 155, 180, 205, 230, 284.5, 310, 335, 0, 25, 50],
        },
        positions: {
            Moon: {
                longitude: 211.25,
                latitude: 2.1,
                distance: 0.0025,
                speed: 12.4,
                house: 5,
                retrograde: false,
                privateScratch: 'not persisted',
            },
        },
        aspects: [{ planet1: 'Moon', planet2: 'Saturn', aspectName: 'Trine' }],
        dignities: { Moon: { domicileRuler: 'Mars' } },
        accidentalDignities: { Moon: { angularity: 'succedent', score: 2 } },
        receptions: [],
        voidOfCourseMoon: { available: true, isVoid: false },
        antisciaContacts: [],
        solarConditions: [{ planet: 'Mercury', condition: 'combust', separationDegrees: 4 }],
        planetaryHour: { available: true, planetaryHourRuler: 'Venus' },
        timingPatterns: [],
        lots: { partOfFortune: { longitude: 142.25 } },
        input: {
            localDate: '2026-06-30',
            localTime: '09:30',
            offsetHours: -4,
        },
        dayChart: false,
    };
}

test('web horary history loads and persists through injected storage', () => {
    const storage = memoryStorage();
    assert.deepEqual(loadWebHoraryHistory(storage), []);

    const history = [{ id: 1, question: 'Will it work?' }];
    persistWebHoraryHistory(history, storage);

    assert.deepEqual(loadWebHoraryHistory(storage), history);
    assert.equal(storage.getItem(HORARY_HISTORY_STORAGE_KEY), JSON.stringify(history));
});

test('web horary history ignores malformed persisted values', () => {
    assert.deepEqual(loadWebHoraryHistory(memoryStorage({ [HORARY_HISTORY_STORAGE_KEY]: '{' })), []);
    assert.deepEqual(loadWebHoraryHistory(memoryStorage({ [HORARY_HISTORY_STORAGE_KEY]: '{"id":1}' })), []);
});

test('prependHoraryHistoryEntry keeps newest first and caps history length', () => {
    const existing = Array.from({ length: 50 }, (_, index) => ({ id: index }));
    const next = prependHoraryHistoryEntry(existing, { id: 'new' });

    assert.equal(next.length, 50);
    assert.equal(next[0].id, 'new');
    assert.equal(next.at(-1).id, 48);
});

test('createHoraryHistoryEntry serializes a full saved chart payload', () => {
    const entry = createHoraryHistoryEntry({
        id: 'chart-1',
        question: 'Where is the ring?',
        time: '2026-06-30T12:00:00.000Z',
        lat: 40.7,
        lng: -74,
        chart: chartFixture(),
        planetSet: 'classical',
        aspectSettings: { trine: { enabled: true, orb: 8 } },
    });

    assert.equal(entry.id, 'chart-1');
    assert.equal(entry.chart.type, 'horary');
    assert.equal(entry.chart.schemaVersion, 1);
    assert.equal(entry.chart.question, 'Where is the ring?');
    assert.equal(entry.chart.location.latitude, 40.7);
    assert.equal(entry.chart.settings.planetSet, 'classical');
    assert.deepEqual(entry.chart.input, chartFixture().input);
    assert.equal(entry.chart.positions.Moon.longitude, 211.25);
    assert.equal(entry.chart.positions.Moon.privateScratch, undefined);
    assert.equal(entry.chart.accidentalDignities.Moon.score, 2);
    assert.deepEqual(entry.chart.lots, { partOfFortune: { longitude: 142.25 } });
    assert.equal(entry.chart.solarConditions[0].condition, 'combust');
});

test('serializePositions keeps only renderable ephemeris fields', () => {
    assert.deepEqual(serializePositions(chartFixture().positions), {
        Moon: {
            longitude: 211.25,
            latitude: 2.1,
            distance: 0.0025,
            speed: 12.4,
            house: 5,
            retrograde: false,
        },
    });
});

test('tauri summaries normalize into history entries', () => {
    assert.deepEqual(tauriSummaryToHistoryEntry({
        id: 'abc',
        question: '',
        updatedAt: 1782820800000,
    }), {
        id: 'abc',
        question: '(No question recorded)',
        time: '2026-06-30T12:00:00.000Z',
        source: 'tauri',
    });
});

test('saved chart normalization restores renderable defaults', () => {
    const normalized = normalizeSavedChart({
        castAt: '2026-06-30T12:00:00.000Z',
        location: { latitude: 51.5, longitude: -0.12 },
        houses: chartFixture().houses,
        positions: chartFixture().positions,
        dignities: chartFixture().dignities,
        lots: chartFixture().lots,
    });

    assert.equal(normalized.date.toISOString(), '2026-06-30T12:00:00.000Z');
    assert.equal(normalized.lat, 51.5);
    assert.equal(normalized.lng, -0.12);
    assert.deepEqual(normalized.aspects, []);
    assert.equal(isRenderableSavedChart(normalized), true);
});

test('history date and saved location labels are stable', () => {
    assert.equal(timestampToIso('1782820800000'), '2026-06-30T12:00:00.000Z');
    assert.equal(historyEntryDate({ updatedAt: 1782820800000 }).toISOString(), '2026-06-30T12:00:00.000Z');
    assert.equal(currentSavedChartLocationLabel({ location: { label: 'London, GB' } }), 'London, GB');
    assert.equal(currentSavedChartLocationLabel({ lat: 51.5, lng: -0.12 }), '51.5000, -0.1200');
    assert.equal(currentSavedChartLocationLabel({}), 'Saved chart location');
});
