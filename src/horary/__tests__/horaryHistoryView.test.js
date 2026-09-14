import test from 'node:test';
import assert from 'node:assert/strict';

import {
    renderHoraryHistoryMarkup,
    safeFilePart,
    safeHistoryExportFilename,
} from '../horaryHistoryView.js';

test('renderHoraryHistoryMarkup escapes saved chart text and disables legacy loads', () => {
    const html = renderHoraryHistoryMarkup([
        {
            id: 'entry"1',
            question: 'Will <it> work & soon?',
            time: 'ignored',
            chart: { legacy: true },
        },
    ], {
        tauriRuntime: false,
        canRenderSavedChart: () => false,
        entryDate: () => fakeDate('<date>', '10:30 "pm"'),
    });

    assert.match(html, /data-id="entry&quot;1"/);
    assert.match(html, /Will &lt;it&gt; work &amp; soon\?/);
    assert.match(html, /&lt;date&gt; 10:30 &quot;pm&quot;/);
    assert.match(html, /data-history-action="load"[^>]*disabled/);
    assert.match(html, /data-history-action="export"/);
    assert.match(html, /data-history-action="delete"/);
});

test('renderHoraryHistoryMarkup enables load when desktop storage or renderable chart is available', () => {
    const webHtml = renderHoraryHistoryMarkup([
        { id: 'web', question: 'Web chart', time: 'ignored', chart: { renderable: true } },
    ], {
        canRenderSavedChart: chart => chart.renderable,
        entryDate: () => fakeDate('6/30/2026', '10:30 AM'),
    });
    const tauriHtml = renderHoraryHistoryMarkup([
        { id: 'native', question: 'Native chart', time: 'ignored', chart: null },
    ], {
        tauriRuntime: true,
        canRenderSavedChart: () => false,
        entryDate: () => fakeDate('6/30/2026', '10:30 AM'),
    });

    assert.doesNotMatch(webHtml, /data-history-action="load"[^>]*disabled/);
    assert.doesNotMatch(tauriHtml, /data-history-action="load"[^>]*disabled/);
});

test('history export filename helpers normalize unsafe and empty question text', () => {
    assert.equal(safeFilePart('  Will <it> work?!  '), 'will-it-work');
    assert.equal(safeHistoryExportFilename('  Will <it> work?!  '), 'horary-will-it-work.json');
    assert.equal(safeHistoryExportFilename('***'), 'horary-chart.json');
    assert.equal(
        safeFilePart('a'.repeat(100)),
        'a'.repeat(80),
    );
});

function fakeDate(dateText, timeText) {
    return {
        toLocaleDateString() {
            return dateText;
        },
        toLocaleTimeString() {
            return timeText;
        },
    };
}
