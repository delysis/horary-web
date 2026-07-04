import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const indexHtml = readFileSync(new URL('../../../index.html', import.meta.url), 'utf8');
const indexCss = readFileSync(new URL('../../index.css', import.meta.url), 'utf8');
const appCss = readFileSync(new URL('../../App.css', import.meta.url), 'utf8');
const css = `${indexCss}\n${appCss}`;
const fieldErrorsJs = readFileSync(new URL('../fieldErrors.js', import.meta.url), 'utf8');
const horaryDashboardJs = readFileSync(new URL('../horaryDashboard.js', import.meta.url), 'utf8');

test('React shell loads the upstream app entrypoint', () => {
    assert.match(indexHtml, /<div id="root"><\/div>/);
    assert.match(indexHtml, /<script type="module" src="\/src\/main\.tsx"><\/script>/);
    assert.match(indexHtml, /<title>Horary Calculator<\/title>/);
    assert.equal(indexHtml.toLowerCase().includes(`w${'horary'}`), false);
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

test('keyboard focus remains visible on buttons and form controls', () => {
    for (const selector of [
        'button:focus-visible',
        'input:not([type="radio"]):not([type="checkbox"])',
        'textarea',
    ]) {
        assert.match(css, new RegExp(escapeRegExp(selector)));
    }

    assert.match(css, /outline:\s*4px auto -webkit-focus-ring-color/);
});

test('field error helper associates validation errors with fields', () => {
    assert.match(fieldErrorsJs, /input\.setAttribute\?\.\('aria-invalid', 'true'\)/);
    assert.match(fieldErrorsJs, /input\.setAttribute\?\.\('aria-describedby', errorId\)/);
    assert.match(fieldErrorsJs, /input\.removeAttribute\?\.\('aria-invalid'\)/);
    assert.match(fieldErrorsJs, /input\.removeAttribute\?\.\('aria-describedby'\)/);
});

function escapeRegExp(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
