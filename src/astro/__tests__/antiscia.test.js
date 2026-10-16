import test from 'node:test';
import assert from 'node:assert/strict';

import {
    antiscionLongitude,
    calculateAntisciaContacts,
    contraAntiscionLongitude,
} from '../antiscia.js';

test('antiscionLongitude reflects across the Cancer-Capricorn axis', () => {
    assert.equal(antiscionLongitude(10), 170);
    assert.equal(antiscionLongitude(40), 140);
    assert.equal(antiscionLongitude(280), 260);
});

test('contraAntiscionLongitude opposes the antiscion', () => {
    assert.equal(contraAntiscionLongitude(10), 350);
    assert.equal(contraAntiscionLongitude(200), 160);
});

test('calculateAntisciaContacts finds antiscion and contra-antiscion contacts', () => {
    const contacts = calculateAntisciaContacts(
        {
            Sun: { longitude: 10 },
            Moon: { longitude: 170.4 },
            Mars: { longitude: 349.75 },
        },
        { planets: ['Sun', 'Moon', 'Mars'], orb: 0.5 }
    );

    assert.deepEqual(contacts, [
        { planet1: 'Sun', planet2: 'Mars', type: 'contraAntiscion', orb: 0.25 },
        { planet1: 'Sun', planet2: 'Moon', type: 'antiscion', orb: 0.4 },
    ]);
});
