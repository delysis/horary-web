import test from 'node:test';
import assert from 'node:assert/strict';

import {
    buildHoraryChartFacts,
    formatUtcOffset,
} from '../chartFacts.js';
import { calculateChart } from '../../astro/chart.js';

const chart = {
    date: new Date('2026-06-30T16:00:00.000Z'),
    lat: 40.7128,
    lng: -74.006,
    zodiac: 'tropical',
    houseSystem: 'regiomontanus',
    dayChart: true,
    houses: {
        asc: 192.3,
        mc: 105.1,
        cusps: [192.3, 220, 250, 285, 315, 340, 12.3, 40, 70, 105.1, 135, 160],
    },
    positions: {
        Moon: { longitude: 284.2, house: 4, speed: 12.4, retrograde: false },
        Venus: { longitude: 46.4, house: 7, speed: 1.2, retrograde: false },
        SouthNode: { longitude: 200, house: 1, speed: 0, retrograde: false },
    },
    aspects: [
        {
            planet1: 'Moon',
            aspectName: 'Trine',
            planet2: 'Venus',
            orb: 2.123456,
            applying: true,
            separating: false,
            exact: false,
            major: true,
        },
    ],
    receptions: [
        { hostPlanet: 'Venus', guestPlanet: 'Mars', dignity: 'domicile', mutual: true },
    ],
    voidOfCourseMoon: {
        available: true,
        isVoid: false,
        checkedUntilHours: 4,
        nextApplyingAspect: {
            planet: 'Saturn',
            aspectName: 'Sextile',
            angle: 60,
            orb: 0,
            perfectsWithinHours: 4,
        },
    },
    antisciaContacts: [
        { planet1: 'Moon', planet2: 'Venus', type: 'antiscion', orb: 0.25 },
    ],
    solarConditions: [
        { planet: 'Mercury', condition: 'cazimi', separationDegrees: 0.2 },
    ],
    planetaryHour: {
        available: true,
        period: 'day',
        hourNumber: 2,
        planetaryDay: 'Sunday',
        planetaryDayRuler: 'Sun',
        planetaryHourRuler: 'Venus',
        startsAtUtc: '2024-06-23T07:00:00.000Z',
        endsAtUtc: '2024-06-23T08:00:00.000Z',
        method: 'NOAA sunrise/sunset with local mean solar date from longitude',
    },
    timingPatterns: [
        {
            type: 'translation',
            mediator: 'Moon',
            fromPlanet: 'Saturn',
            toPlanet: 'Venus',
            estimatedPerfectsWithinHours: 3,
            method: 'Candidate detection from major aspect phase and one-hour orb delta',
        },
    ],
    lots: {
        partOfFortune: {
            name: 'Part of Fortune',
            longitude: 155.5,
            house: 11,
            formula: 'ASC + Moon - Sun',
        },
    },
    dignities: {
        Moon: { domicile: false, exaltation: false, triplicityRuler: true, termRuler: false, faceRuler: false, detriment: true, fall: false, peregrine: false, score: -2 },
    },
    accidentalDignities: {
        Moon: {
            house: 4,
            angularity: 'angular',
            retrograde: false,
            solarCondition: null,
            inJoy: false,
            joyHouse: 3,
            score: 5,
            factors: [
                { type: 'angularity', label: 'angular', score: 5 },
            ],
        },
    },
};

test('buildHoraryChartFacts emits deterministic model evidence', () => {
    const facts = buildHoraryChartFacts(chart, {
        timezone: 'America/New_York',
        locationLabel: 'New York, US',
    });

    assert.equal(facts.castUtcTime, '2026-06-30T16:00:00.000Z');
    assert.equal(facts.timezone, 'America/New_York');
    assert.equal(facts.location.label, 'New York, US');
    assert.equal(facts.ascendant.sign, 'Libra');
    assert.equal(facts.derived.ascendantRuler, 'Venus');
    assert.equal(facts.derived.houseSystem, 'regiomontanus');
    assert.equal(facts.derived.partOfFortune.sign, 'Virgo');
    assert.equal(facts.derived.partOfFortune.house, 11);
    assert.equal(facts.derived.partOfFortune.formula, 'ASC + Moon - Sun');
    assert.deepEqual(facts.derived.receptions[0], {
        hostPlanet: 'Venus',
        guestPlanet: 'Mars',
        dignity: 'domicile',
        mutual: true,
    });
    assert.equal(facts.derived.voidOfCourseMoon.isVoid, false);
    assert.equal(facts.derived.voidOfCourseMoon.nextApplyingAspect.planet, 'Saturn');
    assert.deepEqual(facts.derived.antisciaContacts[0], {
        planet1: 'Moon',
        planet2: 'Venus',
        type: 'antiscion',
        orb: 0.25,
    });
    assert.deepEqual(facts.derived.solarConditions[0], {
        planet: 'Mercury',
        condition: 'cazimi',
        separationDegrees: 0.2,
    });
    assert.equal(facts.derived.planetaryHour.planetaryDayRuler, 'Sun');
    assert.equal(facts.derived.planetaryHour.planetaryHourRuler, 'Venus');
    assert.equal(facts.derived.timingPatterns[0].type, 'translation');
    assert.equal(facts.derived.timingPatterns[0].mediator, 'Moon');
    assert.equal(facts.bodies.length, 2);
    assert.equal(facts.bodies[0].name, 'Moon');
    assert.equal(facts.bodies[0].dignity.triplicityRuler, true);
    assert.equal(facts.bodies[0].accidentalDignity.angularity, 'angular');
    assert.equal(facts.bodies[0].accidentalDignity.score, 5);
    assert.equal(facts.aspects[0].orb, 2.1235);
    assert.equal(facts.aspects[0].applying, true);
});

test('buildHoraryChartFacts keeps native judgement payload compact', () => {
    const generatedChart = calculateChart({
        date: new Date('2026-07-04T18:46:00.000Z'),
        lat: 38.8048,
        lng: -77.0469,
        localDate: '2026-07-04',
        localTime: '14:46',
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
    });
    const facts = buildHoraryChartFacts(generatedChart, {
        timezone: 'America/New_York',
        locationLabel: 'Alexandria, VA',
        localTime: '2026-07-04 14:46 America/New_York',
    });
    const encoded = JSON.stringify(facts);

    assert.ok(Buffer.byteLength(encoded) < 8_000);
    assert.deepEqual(facts.bodies.map(body => body.name), [
        'Sun',
        'Moon',
        'Mercury',
        'Venus',
        'Mars',
        'Jupiter',
        'Saturn',
    ]);
    assert.ok(facts.aspects.every(aspect => aspect.major === true));
    assert.ok(!encoded.includes('"perfection"'));
    assert.ok(!encoded.includes('"method"'));
});

test('buildHoraryChartFacts derives local time and timezone from chart input metadata', () => {
    const facts = buildHoraryChartFacts({
        ...chart,
        input: {
            localDate: '2026-06-30',
            localTime: '09:30',
            offsetHours: -4,
        },
    });

    assert.equal(facts.castLocalTime, '2026-06-30 09:30 UTC-04:00');
    assert.equal(facts.castUtcTime, '2026-06-30T16:00:00.000Z');
    assert.equal(facts.timezone, 'UTC-04:00');
});

test('buildHoraryChartFacts defaults to UTC rather than the browser timezone', () => {
    const facts = buildHoraryChartFacts(chart);

    assert.equal(facts.castLocalTime, '2026-06-30T16:00:00.000Z');
    assert.equal(facts.timezone, 'UTC');
});

test('formatUtcOffset formats fractional UTC offsets', () => {
    assert.equal(formatUtcOffset(5.5), 'UTC+05:30');
    assert.equal(formatUtcOffset(-3.75), 'UTC-03:45');
    assert.equal(formatUtcOffset(Number.NaN), 'UTC');
});
