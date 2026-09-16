import test from 'node:test';
import assert from 'node:assert/strict';

import {
    calculateSolarConditions,
    classifySolarCondition,
    solarElongationDegrees,
} from '../solarCondition.js';

test('solarElongationDegrees handles wraparound from the Sun', () => {
    assert.equal(solarElongationDegrees(1, 359), 2);
    assert.equal(solarElongationDegrees(350, 10), 20);
});

test('classifySolarCondition reports cazimi, combust, and under beams exclusively', () => {
    assert.equal(classifySolarCondition(0.2), 'cazimi');
    assert.equal(classifySolarCondition(2), 'combust');
    assert.equal(classifySolarCondition(12), 'underBeams');
    assert.equal(classifySolarCondition(18), null);
});

test('calculateSolarConditions returns notable non-Sun solar conditions sorted by closeness', () => {
    const conditions = calculateSolarConditions({
        Sun: { longitude: 359 },
        Mercury: { longitude: 359.1 },
        Venus: { longitude: 3 },
        Mars: { longitude: 345 },
        Jupiter: { longitude: 40 },
    }, {
        planets: ['Sun', 'Mercury', 'Venus', 'Mars', 'Jupiter'],
    });

    assert.deepEqual(conditions.map(condition => [condition.planet, condition.condition]), [
        ['Mercury', 'cazimi'],
        ['Venus', 'combust'],
        ['Mars', 'underBeams'],
    ]);
    assert.ok(Math.abs(conditions[0].separationDegrees - 0.1) < 1e-9);
    assert.equal(conditions[1].separationDegrees, 4);
    assert.equal(conditions[2].separationDegrees, 14);
});
