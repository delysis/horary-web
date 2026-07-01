import test from 'node:test';
import assert from 'node:assert/strict';

import { calculateVoidOfCourseMoon } from '../voidOfCourse.js';

function providerWithMoonMotion({ moonStart = 10, moonDegreesPerHour = 1, fixedPlanets = {} }) {
    return (jd) => {
        const hours = jd * 24;
        return {
            Moon: { longitude: moonStart + hours * moonDegreesPerHour },
            ...fixedPlanets,
        };
    };
}

test('calculateVoidOfCourseMoon reports unavailable input', () => {
    const result = calculateVoidOfCourseMoon({ jd: 0, positions: {} });

    assert.equal(result.available, false);
    assert.equal(result.isVoid, null);
});

test('calculateVoidOfCourseMoon reports void when Moon leaves sign without applying aspect', () => {
    const positions = {
        Moon: { longitude: 10 },
        Saturn: { longitude: 210 },
    };
    const result = calculateVoidOfCourseMoon({
        jd: 0,
        positions,
        planets: ['Moon', 'Saturn'],
        positionProvider: providerWithMoonMotion({
            moonStart: 10,
            fixedPlanets: { Saturn: { longitude: 210 } },
        }),
    });

    assert.equal(result.available, true);
    assert.equal(result.isVoid, true);
    assert.equal(result.nextApplyingAspect, null);
});

test('calculateVoidOfCourseMoon reports next applying major aspect before sign change', () => {
    const positions = {
        Moon: { longitude: 10 },
        Saturn: { longitude: 74 },
    };
    const result = calculateVoidOfCourseMoon({
        jd: 0,
        positions,
        planets: ['Moon', 'Saturn'],
        positionProvider: providerWithMoonMotion({
            moonStart: 10,
            fixedPlanets: { Saturn: { longitude: 74 } },
        }),
    });

    assert.equal(result.available, true);
    assert.equal(result.isVoid, false);
    assert.deepEqual(result.nextApplyingAspect, {
        planet: 'Saturn',
        aspectName: 'Sextile',
        angle: 60,
        orb: 0,
        perfectsWithinHours: 4,
    });
});
