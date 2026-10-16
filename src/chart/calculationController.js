import { calculateChart } from '../astro/chart.js';
import { validateChartInput } from '../astro/chartInput.js';
import {
    clearChartErrors,
    showChartErrors,
} from '../ui/fieldErrors.js';
import {
    renderAspectGrid,
    renderAspectsList,
    renderChartWheel,
    renderCuspsTable,
    renderDignitiesTable,
    renderPositionsTable,
} from '../ui/renderers.js';

const DEFAULT_RENDERERS = Object.freeze({
    renderAspectGrid,
    renderAspectsList,
    renderChartWheel,
    renderCuspsTable,
    renderDignitiesTable,
    renderPositionsTable,
});

export function setupChartCalculationController({
    documentRef = globalThis.document,
    getSettings = () => ({}),
    getChart = () => null,
    setChart = () => {},
    onManualCoordinateEdit = () => {},
    validateInput = validateChartInput,
    calculate = calculateChart,
    showErrors = showChartErrors,
    clearErrors = clearChartErrors,
    renderers = DEFAULT_RENDERERS,
} = {}) {
    if (!documentRef?.getElementById) {
        return emptyChartCalculationController();
    }

    const calculateButton = documentRef.getElementById('calcBtn');
    const dateInput = documentRef.getElementById('inputDate');
    const timeInput = documentRef.getElementById('inputTime');
    const offsetInput = documentRef.getElementById('inputOffset');
    const latitudeInput = documentRef.getElementById('inputLat');
    const longitudeInput = documentRef.getElementById('inputLng');
    const majorOnlyToggle = documentRef.getElementById('majorOnlyToggle');
    const applyingOnlyToggle = documentRef.getElementById('applyingOnlyToggle');

    const recalculate = () => {
        const input = validateInput(
            {
                date: dateInput?.value,
                time: timeInput?.value || '12:00',
                offset: offsetInput?.value || '0',
                lat: latitudeInput?.value,
                lng: longitudeInput?.value,
            },
            { allowFuture: true }
        );

        if (!input.ok) {
            showErrors(input.errors);
            return null;
        }

        clearErrors();

        const settings = getSettings() || {};
        const chart = calculate({
            date: input.value.utcDate,
            lat: input.value.latitude,
            lng: input.value.longitude,
            localDate: input.value.localDate,
            localTime: input.value.localTime,
            offsetHours: input.value.offsetHours,
            houseSystem: settings.houseSystem,
            planetSet: settings.planetSet,
            aspectConfig: settings.aspectSettings,
        });
        setChart(chart);

        renderers.renderChartWheel?.(chart, 'chartWheel');
        renderers.renderPositionsTable?.(chart);
        renderers.renderCuspsTable?.(chart);
        renderers.renderAspectGrid?.(chart);
        renderers.renderAspectsList?.(chart);
        renderers.renderDignitiesTable?.(chart);
        return chart;
    };

    const onRecalculateClick = () => {
        recalculate();
    };

    const onInputKeydown = event => {
        if (event.key === 'Enter') recalculate();
    };

    const onManualCoordinateInput = () => {
        onManualCoordinateEdit();
    };

    const onAspectFilterChange = () => {
        const chart = getChart();
        if (chart) renderers.renderAspectsList?.(chart);
    };

    calculateButton?.addEventListener?.('click', onRecalculateClick);
    [dateInput, timeInput, latitudeInput, longitudeInput].forEach(input => {
        input?.addEventListener?.('keydown', onInputKeydown);
    });
    [latitudeInput, longitudeInput].forEach(input => {
        input?.addEventListener?.('input', onManualCoordinateInput);
    });
    majorOnlyToggle?.addEventListener?.('change', onAspectFilterChange);
    applyingOnlyToggle?.addEventListener?.('change', onAspectFilterChange);

    return {
        recalculate,
        cleanup() {
            calculateButton?.removeEventListener?.('click', onRecalculateClick);
            [dateInput, timeInput, latitudeInput, longitudeInput].forEach(input => {
                input?.removeEventListener?.('keydown', onInputKeydown);
            });
            [latitudeInput, longitudeInput].forEach(input => {
                input?.removeEventListener?.('input', onManualCoordinateInput);
            });
            majorOnlyToggle?.removeEventListener?.('change', onAspectFilterChange);
            applyingOnlyToggle?.removeEventListener?.('change', onAspectFilterChange);
        },
    };
}

function emptyChartCalculationController() {
    return {
        recalculate() {
            return null;
        },
        cleanup() {},
    };
}
