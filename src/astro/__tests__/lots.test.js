import test from 'node:test';
import assert from 'node:assert/strict';

import { calculatePartOfFortune } from '../lots.js';

test('calculatePartOfFortune uses the day formula for day charts', () => {
    const lot = calculatePartOfFortune({
        ascendantLongitude: 100,
        sunLongitude: 20,
        moonLongitude: 50,
        isDayChart: true,
    });

    assert.equal(lot.longitude, 130);
    assert.equal(lot.formula, 'ASC + Moon - Sun');
});

test('calculatePartOfFortune uses the night formula for night charts', () => {
    const lot = calculatePartOfFortune({
        ascendantLongitude: 100,
        sunLongitude: 20,
        moonLongitude: 50,
        isDayChart: false,
    });

    assert.equal(lot.longitude, 70);
    assert.equal(lot.formula, 'ASC + Sun - Moon');
});

test('calculatePartOfFortune normalizes wrapped longitudes', () => {
    const lot = calculatePartOfFortune({
        ascendantLongitude: 350,
        sunLongitude: 280,
        moonLongitude: 10,
        isDayChart: true,
    });

    assert.equal(lot.longitude, 80);
});
