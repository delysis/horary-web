import test from 'node:test';
import assert from 'node:assert/strict';

import {
    renderAspectSettingsMarkup,
    renderHouseSystemOptions,
    renderHouseSystemPills,
} from '../settingsMarkup.js';

test('renderHouseSystemOptions marks the selected house system and escapes values', () => {
    const html = renderHouseSystemOptions([
        { key: 'regiomontanus', name: 'Regiomontanus' },
        { key: 'bad"key', name: '<Unsafe>' },
    ], 'regiomontanus');

    assert.match(html, /<option value="regiomontanus" selected>Regiomontanus<\/option>/);
    assert.match(html, /value="bad&quot;key"/);
    assert.match(html, />&lt;Unsafe&gt;<\/option>/);
});

test('renderHouseSystemPills marks the selected house system and escapes text', () => {
    const html = renderHouseSystemPills([
        { key: 'regiomontanus', name: 'Regiomontanus' },
        { key: 'whole-sign', name: 'Whole & Sign' },
    ], 'whole-sign');

    assert.match(html, /class="radio-pill " data-value="regiomontanus">Regiomontanus<\/button>/);
    assert.match(html, /class="radio-pill active" data-value="whole-sign">Whole &amp; Sign<\/button>/);
});

test('renderAspectSettingsMarkup renders controls and escapes labels', () => {
    const html = renderAspectSettingsMarkup({
        trine: {
            enabled: true,
            color: '#4cc94c',
            symbol: '△',
            name: 'Tri<ne>',
            orb: 7,
        },
        square: {
            enabled: false,
            color: 'red"bad',
            symbol: '□',
            name: 'Square',
            orb: 3.5,
        },
    });

    assert.match(html, /data-aspect-enabled="trine" checked/);
    assert.match(html, /Tri&lt;ne&gt;/);
    assert.match(html, /aria-label="Tri&lt;ne&gt; orb in degrees"/);
    assert.match(html, /data-aspect-orb="square"/);
    assert.match(html, /style="color:red&quot;bad"/);
    assert.doesNotMatch(html, /data-aspect-enabled="square" checked/);
});
