import test from 'node:test';
import assert from 'node:assert/strict';

import { createPwaManifest, normalizePwaBase } from '../pwaManifest.js';

test('normalizePwaBase keeps root deployment at root', () => {
    assert.equal(normalizePwaBase('/'), '/');
    assert.equal(normalizePwaBase(''), '/');
});

test('normalizePwaBase normalizes subpath deployments', () => {
    assert.equal(normalizePwaBase('/horary-web'), '/horary-web/');
    assert.equal(normalizePwaBase('horary-web'), '/horary-web/');
    assert.equal(normalizePwaBase('/horary-web/'), '/horary-web/');
});

test('createPwaManifest scopes start URL to the configured web base', () => {
    const manifest = createPwaManifest('/horary-web');

    assert.equal(manifest.start_url, '/horary-web/');
    assert.equal(manifest.scope, '/horary-web/');
    assert.equal(manifest.icons.length, 3);
});
