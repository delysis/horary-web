import test from 'node:test';
import assert from 'node:assert/strict';

import {
    APP_VERSION,
    FALLBACK_APP_VERSION,
    resolveAppVersion,
} from '../version.js';

test('resolveAppVersion normalizes injected build versions', () => {
    assert.equal(resolveAppVersion('1.2.3'), '1.2.3');
    assert.equal(resolveAppVersion(' 2026.6.30 '), '2026.6.30');
});

test('resolveAppVersion falls back for missing or invalid versions', () => {
    assert.equal(resolveAppVersion(''), FALLBACK_APP_VERSION);
    assert.equal(resolveAppVersion('   '), FALLBACK_APP_VERSION);
    assert.equal(resolveAppVersion(null), FALLBACK_APP_VERSION);
});

test('APP_VERSION is always a non-empty string under direct Node test execution', () => {
    assert.equal(typeof APP_VERSION, 'string');
    assert.notEqual(APP_VERSION, '');
});
