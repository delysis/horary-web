import test from 'node:test';
import assert from 'node:assert/strict';

import {
    calculateHoraryTimingPatterns,
    estimateHoursToPerfection,
} from '../timingPatterns.js';

function aspect(planet1, planet2, aspectName, orb, futureOrb, phase = 'applying') {
    return {
        planet1,
        planet2,
        aspectName,
        major: true,
        orb,
        futureOrb,
        applying: phase === 'applying',
        separating: phase === 'separating',
    };
}

test('estimateHoursToPerfection uses current and one-hour future orb deltas', () => {
    assert.equal(estimateHoursToPerfection(aspect('Moon', 'Venus', 'Trine', 3, 2)), 3);
    assert.equal(estimateHoursToPerfection(aspect('Moon', 'Venus', 'Trine', 3, 4)), null);
    assert.equal(estimateHoursToPerfection(aspect('Moon', 'Venus', 'Trine', 3, 4, 'separating')), null);
});

test('calculateHoraryTimingPatterns detects translation candidates', () => {
    const patterns = calculateHoraryTimingPatterns({
        positions: {
            Moon: { speed: 13 },
            Saturn: { speed: 0.1 },
            Venus: { speed: 1 },
        },
        aspects: [
            aspect('Moon', 'Saturn', 'Sextile', 2, 3, 'separating'),
            aspect('Moon', 'Venus', 'Trine', 3, 2, 'applying'),
        ],
    });

    const translation = patterns.find(pattern => pattern.type === 'translation');
    assert.equal(translation.mediator, 'Moon');
    assert.equal(translation.fromPlanet, 'Saturn');
    assert.equal(translation.toPlanet, 'Venus');
    assert.equal(translation.estimatedPerfectsWithinHours, 3);
});

test('calculateHoraryTimingPatterns detects collection candidates', () => {
    const patterns = calculateHoraryTimingPatterns({
        positions: {
            Mercury: { speed: 1.5 },
            Venus: { speed: 1.2 },
            Saturn: { speed: 0.1 },
        },
        aspects: [
            aspect('Mercury', 'Saturn', 'Square', 2, 1, 'applying'),
            aspect('Venus', 'Saturn', 'Trine', 4, 3, 'applying'),
        ],
    });

    const collection = patterns.find(pattern => pattern.type === 'collection');
    assert.equal(collection.collector, 'Saturn');
    assert.equal(collection.planet1, 'Mercury');
    assert.equal(collection.planet2, 'Venus');
    assert.equal(collection.estimatedFirstPerfectsWithinHours, 2);
    assert.equal(collection.estimatedSecondPerfectsWithinHours, 4);
});

test('calculateHoraryTimingPatterns detects prohibition/frustration candidates', () => {
    const patterns = calculateHoraryTimingPatterns({
        positions: {
            Moon: { speed: 13 },
            Venus: { speed: 1.2 },
            Mars: { speed: 0.7 },
        },
        aspects: [
            aspect('Moon', 'Venus', 'Trine', 5, 4, 'applying'),
            aspect('Moon', 'Mars', 'Square', 1, 0.5, 'applying'),
        ],
    });

    const prohibition = patterns.find(pattern => pattern.type === 'prohibitionFrustration');
    assert.equal(prohibition.planet1, 'Moon');
    assert.equal(prohibition.planet2, 'Venus');
    assert.equal(prohibition.interveningPlanet, 'Mars');
    assert.equal(prohibition.directEstimatedPerfectsWithinHours, 5);
    assert.equal(prohibition.interveningEstimatedPerfectsWithinHours, 2);
});

test('calculateHoraryTimingPatterns uses searched perfection times when available', () => {
    const patterns = calculateHoraryTimingPatterns({
        jd: 0,
        positions: {
            Moon: { speed: 13 },
            Saturn: { speed: 0.1 },
            Venus: { speed: 1 },
        },
        aspects: [
            aspect('Moon', 'Saturn', 'Sextile', 2, 3, 'separating'),
            {
                ...aspect('Moon', 'Venus', 'Trine', 3, 2, 'applying'),
                aspect: 'trine',
                angle: 120,
            },
        ],
        positionsAtHours: hours => ({
            Moon: { longitude: 137 + hours, speed: 1 },
            Venus: { longitude: 20, speed: 0 },
            Saturn: { longitude: 55, speed: 0 },
        }),
    });

    const translation = patterns.find(pattern => pattern.type === 'translation');
    assert.equal(translation.estimatedPerfectsWithinHours, 3);
    assert.equal(translation.applyingAspect.perfection.usable, true);
    assert.equal(translation.applyingAspect.perfection.method, 'Bounded future event search with sign-boundary and station flags');
});

test('calculateHoraryTimingPatterns does not use sign-blocked perfection for prohibition order', () => {
    const patterns = calculateHoraryTimingPatterns({
        jd: 0,
        positions: {
            Moon: { speed: 13 },
            Venus: { speed: 1.2 },
            Mars: { speed: 0.7 },
        },
        aspects: [
            {
                ...aspect('Moon', 'Venus', 'Square', 3, 2, 'applying'),
                aspect: 'square',
                angle: 90,
            },
            {
                ...aspect('Moon', 'Mars', 'Sextile', 1, 0.5, 'applying'),
                aspect: 'sextile',
                angle: 60,
            },
        ],
        positionsAtHours: hours => ({
            Moon: { longitude: 29 + hours, speed: 1 },
            Venus: { longitude: 122, speed: 0 },
            Mars: { longitude: 90, speed: 0 },
        }),
    });

    assert.equal(patterns.some(pattern => pattern.type === 'prohibitionFrustration'), false);
});
