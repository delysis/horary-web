import test from 'node:test';
import assert from 'node:assert/strict';

import {
    calculateAspectPerfections,
    findFirstPerfection,
} from '../aspectPerfections.js';

function currentAspect(overrides = {}) {
    return {
        planet1: 'Moon',
        planet2: 'Sun',
        aspect: 'sextile',
        aspectName: 'Sextile',
        angle: 60,
        major: true,
        applying: true,
        orb: 2,
        ...overrides,
    };
}

test('findFirstPerfection refines the first exact future aspect', () => {
    const event = findFirstPerfection({
        aspect: currentAspect(),
        maxHours: 12,
        positionsAtHours: hours => ({
            Moon: { longitude: 78 + hours, speed: 1 },
            Sun: { longitude: 20, speed: 0 },
        }),
    });

    assert.equal(event.usable, true);
    assert.equal(event.blockedBySignBoundary, false);
    assert.ok(Math.abs(event.perfectsWithinHours - 2) < 0.001);
    assert.ok(event.minimumOrb <= 0.01);
});

test('findFirstPerfection marks events blocked by sign boundary before perfection', () => {
    const event = findFirstPerfection({
        aspect: currentAspect({
            planet2: 'Venus',
            aspect: 'square',
            aspectName: 'Square',
            angle: 90,
            orb: 3,
        }),
        maxHours: 12,
        positionsAtHours: hours => ({
            Moon: { longitude: 29 + hours, speed: 1 },
            Venus: { longitude: 122, speed: 0 },
        }),
    });

    assert.equal(event.usable, false);
    assert.equal(event.blockedBySignBoundary, true);
    assert.equal(event.signBoundary.planet, 'Moon');
    assert.ok(event.signBoundary.hours < event.perfectsWithinHours);
});

test('findFirstPerfection flags a station before perfection', () => {
    const event = findFirstPerfection({
        aspect: currentAspect(),
        maxHours: 12,
        positionsAtHours: hours => ({
            Moon: { longitude: 78 + hours, speed: hours < 1 ? 1 : -1 },
            Sun: { longitude: 20, speed: 0.5 },
        }),
    });

    assert.equal(event.usable, true);
    assert.equal(event.stationBeforePerfection, true);
    assert.equal(event.station.planet, 'Moon');
    assert.ok(event.station.hours < event.perfectsWithinHours);
});

test('calculateAspectPerfections only searches applying major configured planets', () => {
    const events = calculateAspectPerfections({
        jd: 0,
        aspects: [
            currentAspect(),
            currentAspect({ planet1: 'Moon', planet2: 'Pluto', major: true }),
            currentAspect({ planet1: 'Mercury', planet2: 'Venus', major: false }),
            currentAspect({ planet1: 'Mars', planet2: 'Jupiter', applying: false }),
        ],
        maxHours: 12,
        positionsAtHours: hours => ({
            Moon: { longitude: 78 + hours, speed: 1 },
            Sun: { longitude: 20, speed: 0 },
            Pluto: { longitude: 20, speed: 0 },
            Mercury: { longitude: 78 + hours, speed: 1 },
            Venus: { longitude: 20, speed: 0 },
            Mars: { longitude: 78 + hours, speed: 1 },
            Jupiter: { longitude: 20, speed: 0 },
        }),
    });

    assert.equal(events.length, 1);
    assert.equal(events[0].planet1, 'Moon');
    assert.equal(events[0].planet2, 'Sun');
});
