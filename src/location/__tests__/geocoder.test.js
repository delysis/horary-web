import test from 'node:test';
import assert from 'node:assert/strict';

import {
    createLocalCityGeocoder,
    LOCAL_CITY_GEOCODER_PROVIDER,
} from '../geocoder.js';

const london = {
    name: 'London',
    country: 'GB',
    lat: 51.5074,
    lng: -0.1278,
    tz: 'Europe/London',
};

test('local city geocoder exposes an offline provider boundary', () => {
    const geocoder = createLocalCityGeocoder();

    assert.equal(geocoder.provider.id, LOCAL_CITY_GEOCODER_PROVIDER.id);
    assert.equal(geocoder.provider.online, false);
});

test('local city geocoder normalizes bundled city records into location candidates', () => {
    const geocoder = createLocalCityGeocoder({
        search: () => [london],
    });
    const [candidate] = geocoder.search('lon');

    assert.deepEqual(candidate, {
        id: 'london, gb',
        label: 'London, GB',
        name: 'London',
        country: 'GB',
        latitude: 51.5074,
        longitude: -0.1278,
        timezone: 'Europe/London',
        provider: 'bundled-cities',
    });
});

test('local city geocoder caches repeated normalized queries', () => {
    let calls = 0;
    const geocoder = createLocalCityGeocoder({
        search: () => {
            calls++;
            return [london];
        },
    });

    geocoder.search(' London ', { limit: 8 });
    const cached = geocoder.search('london', { limit: 8 });

    assert.equal(calls, 1);
    assert.equal(cached[0].label, 'London, GB');
});

test('local city geocoder returns cloned cached candidates', () => {
    const geocoder = createLocalCityGeocoder({
        search: () => [london],
    });
    const first = geocoder.search('lon');
    first[0].label = 'Mutated';

    const second = geocoder.search('lon');

    assert.equal(second[0].label, 'London, GB');
});

test('local city geocoder ignores short queries without searching', () => {
    let calls = 0;
    const geocoder = createLocalCityGeocoder({
        search: () => {
            calls++;
            return [london];
        },
    });

    assert.deepEqual(geocoder.search('l'), []);
    assert.equal(calls, 0);
});
