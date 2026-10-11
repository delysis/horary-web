import test from 'node:test';
import assert from 'node:assert/strict';

import { calculateAccidentalDignities } from '../accidentalDignity.js';

test('calculateAccidentalDignities scores angularity, motion, solar condition, and planetary joy', () => {
    const dignities = calculateAccidentalDignities({
        Mercury: { house: 1, retrograde: false },
        Mars: { house: 6, retrograde: true },
        Venus: { house: 8, retrograde: false },
        Uranus: { house: 10, retrograde: false },
    }, {
        planets: ['Mercury', 'Mars', 'Venus', 'Uranus'],
        solarConditions: [
            { planet: 'Mercury', condition: 'cazimi', separationDegrees: 0.1 },
            { planet: 'Mars', condition: 'combust', separationDegrees: 3 },
        ],
    });

    assert.equal(dignities.Mercury.angularity, 'angular');
    assert.equal(dignities.Mercury.inJoy, true);
    assert.equal(dignities.Mercury.solarCondition, 'cazimi');
    assert.equal(dignities.Mercury.score, 12);

    assert.equal(dignities.Mars.angularity, 'cadent');
    assert.equal(dignities.Mars.retrograde, true);
    assert.equal(dignities.Mars.inJoy, true);
    assert.equal(dignities.Mars.solarCondition, 'combust');
    assert.equal(dignities.Mars.score, -13);

    assert.equal(dignities.Venus.angularity, 'succedent');
    assert.equal(dignities.Venus.score, 2);
    assert.equal(dignities.Uranus, undefined);
});
