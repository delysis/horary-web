import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveHoraryCoordinates } from '../coordinates.js';

test('resolveHoraryCoordinates rejects blank manual coordinates instead of applying a fallback', () => {
    const result = resolveHoraryCoordinates({
        geoLocation: null,
        manualLat: '',
        manualLng: '',
    });

    assert.equal(result.ok, false);
    assert.deepEqual(result.errors.map(error => error.field), ['lat', 'lng']);
});

test('resolveHoraryCoordinates accepts manual coordinates', () => {
    const result = resolveHoraryCoordinates({
        geoLocation: null,
        manualLat: '51.5074',
        manualLng: '-0.1278',
    });

    assert.equal(result.ok, true);
    assert.deepEqual(result.value, { latitude: 51.5074, longitude: -0.1278 });
});

test('resolveHoraryCoordinates prefers explicit browser geolocation over stale manual fields', () => {
    const result = resolveHoraryCoordinates({
        geoLocation: { lat: 40.7128, lng: -74.006 },
        manualLat: '',
        manualLng: '',
    });

    assert.equal(result.ok, true);
    assert.deepEqual(result.value, { latitude: 40.7128, longitude: -74.006 });
});
