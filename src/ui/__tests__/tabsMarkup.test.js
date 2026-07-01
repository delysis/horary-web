import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const indexHtml = readFileSync(new URL('../../../index.html', import.meta.url), 'utf8');
const indexCss = readFileSync(new URL('../../index.css', import.meta.url), 'utf8');
const fieldErrorsJs = readFileSync(new URL('../fieldErrors.js', import.meta.url), 'utf8');
const horaryDashboardJs = readFileSync(new URL('../horaryDashboard.js', import.meta.url), 'utf8');

test('tab buttons and panels expose accessible relationships', () => {
    const tabNames = ['chart', 'horary', 'aspects', 'tables', 'settings'];

    for (const name of tabNames) {
        assert.match(indexHtml, new RegExp(`id="tab-button-${name}"[^>]+role="tab"`));
        assert.match(indexHtml, new RegExp(`aria-controls="tab-${name}"`));
        assert.match(indexHtml, new RegExp(`id="tab-${name}"[^>]+role="tabpanel"`));
        assert.match(indexHtml, new RegExp(`id="tab-${name}"[^>]+aria-labelledby="tab-button-${name}"`));
    }
});

test('inactive tab panels are initially hidden from assistive technology', () => {
    assert.match(indexHtml, /id="tab-chart"[^>]+role="tabpanel"/);
    assert.doesNotMatch(indexHtml, /id="tab-chart"[^>]+hidden/);

    for (const name of ['horary', 'aspects', 'tables', 'settings']) {
        assert.match(indexHtml, new RegExp(`id="tab-${name}"[^>]+aria-hidden="true"[^>]+hidden`));
    }
});

test('settings shortcut advertises controlled panel expansion state', () => {
    assert.match(indexHtml, /id="settingsBtn"[^>]+aria-controls="tab-settings"[^>]+aria-expanded="false"/);
});

test('static data tables expose captions and scoped column headers', () => {
    for (const caption of [
        'Planetary positions',
        'House cusps',
        'Aspect list',
        'Essential dignities',
        'Traditional dignity reference',
    ]) {
        assert.match(indexHtml, new RegExp(`<caption class="sr-only">${caption}</caption>`));
    }

    assert.match(indexHtml, /<th scope="col">Planet<\/th>/);
    assert.match(indexHtml, /<th scope="col">House<\/th>/);
    assert.match(indexHtml, /<th scope="row">☉ Sun<\/th>/);
});

test('generated horary data tables expose captions and scoped headers', () => {
    for (const caption of [
        'Horary positions',
        'Horary factors',
        'Receptions',
        'Antiscia contacts',
        'Timing pattern candidates',
        'Applying major aspects',
    ]) {
        assert.match(horaryDashboardJs, new RegExp(`<caption class="sr-only">${caption}</caption>`));
    }

    assert.match(horaryDashboardJs, /<th scope="col">Receiving Planet<\/th>/);
    assert.match(horaryDashboardJs, /<th scope="col">Candidate<\/th>/);
});

test('install banner dismiss button has an accessible name', () => {
    assert.match(indexHtml, /id="dismissInstall"[^>]+aria-label="Dismiss install banner"/);
});

test('About panel exposes a build-backed app version target', () => {
    assert.match(indexHtml, /Whorary v<span id="appVersion">1\.0\.0<\/span>/);
    assert.doesNotMatch(indexHtml, /Whorary v1\.0<\/strong>/);
});

test('keyboard focus remains visible on buttons and form controls', () => {
    for (const selector of [
        '.btn:focus-visible',
        '.nav-tab:focus-visible',
        '.radio-pill:focus-visible',
        '.form-input:focus-visible',
        '.form-select:focus-visible',
        '.form-row input[type="checkbox"]:focus-visible',
        '.question-input:focus-visible',
        '.autocomplete-item:focus-visible',
    ]) {
        assert.match(indexCss, new RegExp(escapeRegExp(selector)));
    }

    assert.match(indexCss, /outline:\s*2px solid var\(--gold\)/);
    assert.match(indexCss, /outline-offset:\s*2px/);
});

test('chart input validation errors are associated with fields', () => {
    for (const inputId of ['inputDate', 'inputTime', 'inputOffset', 'inputLat', 'inputLng']) {
        assert.match(indexHtml, new RegExp(`id="${inputId}Error" class="field-error"[^>]+aria-live="polite"[^>]+hidden`));
    }

    assert.match(fieldErrorsJs, /input\.setAttribute\?\.\('aria-invalid', 'true'\)/);
    assert.match(fieldErrorsJs, /input\.setAttribute\?\.\('aria-describedby', errorId\)/);
    assert.match(fieldErrorsJs, /input\.removeAttribute\?\.\('aria-invalid'\)/);
    assert.match(fieldErrorsJs, /input\.removeAttribute\?\.\('aria-describedby'\)/);
});

function escapeRegExp(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
