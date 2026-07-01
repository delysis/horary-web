export const HORARY_HISTORY_STORAGE_KEY = 'whorary_horary_history';
export const MAX_HORARY_HISTORY_ENTRIES = 50;

export function loadWebHoraryHistory(storage = globalThis.localStorage) {
    try {
        const parsed = JSON.parse(storage?.getItem(HORARY_HISTORY_STORAGE_KEY) || '[]');
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

export function persistWebHoraryHistory(history, storage = globalThis.localStorage) {
    storage?.setItem(HORARY_HISTORY_STORAGE_KEY, JSON.stringify(history));
}

export function prependHoraryHistoryEntry(history, entry, maxEntries = MAX_HORARY_HISTORY_ENTRIES) {
    return [entry, ...history].slice(0, maxEntries);
}

export function createHoraryHistoryEntry({
    id = Date.now(),
    question,
    time,
    lat,
    lng,
    chart,
    planetSet,
    aspectSettings,
}) {
    const entry = { id, question, time, lat, lng };
    return {
        ...entry,
        chart: buildSavedHoraryChart(entry, chart, { planetSet, aspectSettings }),
    };
}

export function buildSavedHoraryChart(entry, chart, { planetSet, aspectSettings } = {}) {
    return {
        type: 'horary',
        schemaVersion: 1,
        castAt: entry.time,
        date: entry.time,
        lat: entry.lat,
        lng: entry.lng,
        question: entry.question,
        input: chart.input || null,
        location: {
            latitude: entry.lat,
            longitude: entry.lng,
        },
        settings: {
            houseSystem: chart.houseSystem,
            zodiac: chart.zodiac,
            planetSet,
            aspectSettings,
        },
        houses: chart.houses,
        positions: serializePositions(chart.positions),
        aspects: chart.aspects,
        dignities: chart.dignities,
        accidentalDignities: chart.accidentalDignities,
        receptions: chart.receptions,
        voidOfCourseMoon: chart.voidOfCourseMoon,
        antisciaContacts: chart.antisciaContacts,
        solarConditions: chart.solarConditions,
        planetaryHour: chart.planetaryHour,
        timingPatterns: chart.timingPatterns,
        lots: chart.lots,
        dayChart: chart.dayChart,
    };
}

export function serializePositions(positions) {
    const result = {};
    for (const [key, value] of Object.entries(positions || {})) {
        result[key] = {
            longitude: value.longitude,
            latitude: value.latitude,
            distance: value.distance,
            speed: value.speed,
            house: value.house,
            retrograde: value.retrograde,
        };
    }
    return result;
}

export function tauriSummaryToHistoryEntry(summary) {
    return {
        id: summary.id,
        question: summary.question || '(No question recorded)',
        time: timestampToIso(summary.updatedAt || summary.createdAt),
        source: 'tauri',
    };
}

export function normalizeSavedChart(savedChart) {
    if (!savedChart || typeof savedChart !== 'object') return null;
    return {
        ...savedChart,
        date: new Date(savedChart.date || savedChart.castAt || Date.now()),
        lat: savedChart.lat ?? savedChart.location?.latitude,
        lng: savedChart.lng ?? savedChart.location?.longitude,
        aspects: savedChart.aspects || [],
        accidentalDignities: savedChart.accidentalDignities || {},
        receptions: savedChart.receptions || [],
        antisciaContacts: savedChart.antisciaContacts || [],
        solarConditions: savedChart.solarConditions || [],
        timingPatterns: savedChart.timingPatterns || [],
        lots: savedChart.lots || {},
    };
}

export function isRenderableSavedChart(chart) {
    return Boolean(
        chart
        && typeof chart === 'object'
        && chart.houses?.cusps
        && Number.isFinite(chart.houses.asc)
        && chart.positions
        && chart.dignities
        && Array.isArray(chart.aspects)
        && chart.lots?.partOfFortune
    );
}

export function currentSavedChartLocationLabel(chart) {
    if (chart.location?.label) return chart.location.label;
    if (Number.isFinite(chart.lat) && Number.isFinite(chart.lng)) {
        return `${chart.lat.toFixed(4)}, ${chart.lng.toFixed(4)}`;
    }
    return 'Saved chart location';
}

export function historyEntryDate(entry) {
    return new Date(timestampToIso(entry.time || entry.updatedAt || entry.createdAt));
}

export function timestampToIso(value) {
    if (typeof value === 'number') return new Date(value).toISOString();
    if (typeof value === 'string' && /^\d+$/.test(value)) {
        return new Date(Number(value)).toISOString();
    }
    return value || new Date(0).toISOString();
}
