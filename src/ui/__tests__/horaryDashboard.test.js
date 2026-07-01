import test from 'node:test';
import assert from 'node:assert/strict';

import { renderHoraryDashboard } from '../horaryDashboard.js';

test('renderHoraryDashboard renders horary sections and delegates chart/table renderers', () => {
    const documentRef = new FakeDocument();
    const chart = { houseSystem: 'custom', id: 'chart' };
    const calls = [];
    const rendered = renderHoraryDashboard(chart, {
        documentRef,
        houseSystems: [{ key: 'custom', name: 'Custom <House>' }],
        renderers: recordingRenderers(calls),
    });
    const html = documentRef.getElementById('horaryChart').innerHTML;

    assert.equal(rendered, true);
    assert.match(html, /Horary Positions \(Custom &lt;House&gt;\)/);
    assert.match(html, /<caption class="sr-only">Horary positions<\/caption>/);
    assert.match(html, /<caption class="sr-only">Horary factors<\/caption>/);
    assert.match(html, /<caption class="sr-only">Receptions<\/caption>/);
    assert.match(html, /<caption class="sr-only">Antiscia contacts<\/caption>/);
    assert.match(html, /<caption class="sr-only">Timing pattern candidates<\/caption>/);
    assert.match(html, /<caption class="sr-only">Applying major aspects<\/caption>/);
    assert.match(html, /<tbody><tr><td>positions<\/td><\/tr><\/tbody>/);
    assert.match(html, /<tbody><tr><td>factors<\/td><\/tr><\/tbody>/);
    assert.match(html, /<tbody><tr><td>aspects<\/td><\/tr><\/tbody>/);
    assert.deepEqual(calls, [
        ['renderHoraryPositions', chart],
        ['renderHoraryFactors', chart],
        ['renderHoraryReceptions', chart],
        ['renderHoraryAntiscia', chart],
        ['renderHoraryTimingPatterns', chart],
        ['renderHoraryAspects', chart],
        ['renderChartWheel', chart, 'horaryWheelContainer'],
        ['renderPositionsTable', chart],
        ['renderCuspsTable', chart],
        ['renderAspectGrid', chart],
        ['renderAspectsList', chart],
        ['renderDignitiesTable', chart],
    ]);
});

test('renderHoraryDashboard returns false when the horary container is absent', () => {
    const calls = [];
    const rendered = renderHoraryDashboard({}, {
        documentRef: { getElementById: () => null },
        renderers: recordingRenderers(calls),
    });

    assert.equal(rendered, false);
    assert.deepEqual(calls, []);
});

function recordingRenderers(calls) {
    return {
        renderHoraryPositions(chart) {
            calls.push(['renderHoraryPositions', chart]);
            return '<tr><td>positions</td></tr>';
        },
        renderHoraryFactors(chart) {
            calls.push(['renderHoraryFactors', chart]);
            return '<tr><td>factors</td></tr>';
        },
        renderHoraryReceptions(chart) {
            calls.push(['renderHoraryReceptions', chart]);
            return '<tr><td>receptions</td></tr>';
        },
        renderHoraryAntiscia(chart) {
            calls.push(['renderHoraryAntiscia', chart]);
            return '<tr><td>antiscia</td></tr>';
        },
        renderHoraryTimingPatterns(chart) {
            calls.push(['renderHoraryTimingPatterns', chart]);
            return '<tr><td>timing</td></tr>';
        },
        renderHoraryAspects(chart) {
            calls.push(['renderHoraryAspects', chart]);
            return '<tr><td>aspects</td></tr>';
        },
        renderChartWheel(chart, target) {
            calls.push(['renderChartWheel', chart, target]);
        },
        renderPositionsTable(chart) {
            calls.push(['renderPositionsTable', chart]);
        },
        renderCuspsTable(chart) {
            calls.push(['renderCuspsTable', chart]);
        },
        renderAspectGrid(chart) {
            calls.push(['renderAspectGrid', chart]);
        },
        renderAspectsList(chart) {
            calls.push(['renderAspectsList', chart]);
        },
        renderDignitiesTable(chart) {
            calls.push(['renderDignitiesTable', chart]);
        },
    };
}

class FakeDocument {
    constructor() {
        this.container = { innerHTML: '' };
    }

    getElementById(id) {
        return id === 'horaryChart' ? this.container : null;
    }
}
