import test from 'node:test';
import assert from 'node:assert/strict';

import {
    aspectPhaseLabel,
} from '../renderers.js';
import {
    renderHoraryAntiscia,
    renderHoraryAspects,
    renderHoraryFactors,
    renderHoraryPositions,
    renderHoraryReceptions,
    renderHoraryTimingPatterns,
} from '../horaryRenderers.js';

test('aspectPhaseLabel reports applying, separating, exact, and unavailable phases', () => {
    assert.equal(aspectPhaseLabel({ applying: true, separating: false, futureOrb: 1.2 }), 'Applying');
    assert.equal(aspectPhaseLabel({ applying: false, separating: true, futureOrb: 1.4 }), 'Separating');
    assert.equal(aspectPhaseLabel({ applying: false, separating: false, futureOrb: 0 }), 'Exact');
    assert.equal(aspectPhaseLabel({ applying: null, separating: null, futureOrb: null }), 'Phase unavailable');
});

test('renderHoraryFactors includes sect, ascendant ruler, and Part of Fortune', () => {
    const html = renderHoraryFactors({
        dayChart: false,
        houses: { asc: 102 },
        lots: {
            partOfFortune: {
                longitude: 155.5,
                house: 11,
                formula: 'ASC + Sun - Moon',
            },
        },
        voidOfCourseMoon: {
            available: true,
            isVoid: false,
            nextApplyingAspect: {
                planet: 'Saturn',
                aspectName: 'Sextile',
                perfectsWithinHours: 4,
            },
        },
        planetaryHour: {
            available: true,
            period: 'day',
            hourNumber: 2,
            planetaryDayRuler: 'Sun',
            planetaryHourRuler: 'Venus',
        },
        solarConditions: [
            { planet: 'Mercury', condition: 'cazimi', separationDegrees: 0.2 },
            { planet: 'Mars', condition: 'underBeams', separationDegrees: 12.25 },
        ],
    });

    assert.match(html, /Chart sect/);
    assert.match(html, /Night/);
    assert.match(html, /Ascendant ruler/);
    assert.match(html, /Moon/);
    assert.match(html, /Part of Fortune/);
    assert.match(html, /5° ♍ 30'/);
    assert.match(html, /house 11/);
    assert.match(html, /ASC \+ Sun - Moon/);
    assert.match(html, /Void-of-course Moon/);
    assert.match(html, /Moon applies to ♄ Saturn by Sextile in 4h/);
    assert.match(html, /Solar condition/);
    assert.match(html, /☿ Mercury cazimi/);
    assert.match(html, /0.20° from Sun/);
    assert.match(html, /♂ Mars under beams/);
    assert.match(html, /Planetary hour/);
    assert.match(html, /♀ Venus hour \(day hour 2 of ☉ Sun day\)/);
    assert.match(html, /<th scope="row">Chart sect<\/th>/);
});

test('renderHoraryReceptions lists receiving and received planets', () => {
    const html = renderHoraryReceptions({
        receptions: [
            { hostPlanet: 'Venus', guestPlanet: 'Mars', dignity: 'domicile', mutual: true },
        ],
    });

    assert.match(html, /Venus/);
    assert.match(html, /receives/);
    assert.match(html, /Mars/);
    assert.match(html, /domicile · mutual/);
    assert.match(html, /<th scope="row">♀ Venus<\/th>/);
});

test('renderHoraryReceptions reports empty state', () => {
    assert.match(renderHoraryReceptions({ receptions: [] }), /No receptions found/);
});

test('renderHoraryAntiscia lists contact type and orb', () => {
    const html = renderHoraryAntiscia({
        antisciaContacts: [
            { planet1: 'Sun', planet2: 'Moon', type: 'contraAntiscion', orb: 0.25 },
        ],
    });

    assert.match(html, /Sun/);
    assert.match(html, /contra-antiscion/);
    assert.match(html, /Moon/);
    assert.match(html, /0.25°/);
    assert.match(html, /<th scope="row">☉ Sun<\/th>/);
});

test('renderHoraryAntiscia reports empty state', () => {
    assert.match(renderHoraryAntiscia({ antisciaContacts: [] }), /No antiscia contacts found/);
});

test('renderHoraryTimingPatterns lists timing candidates', () => {
    const html = renderHoraryTimingPatterns({
        timingPatterns: [
            {
                type: 'translation',
                mediator: 'Moon',
                fromPlanet: 'Saturn',
                toPlanet: 'Venus',
                estimatedPerfectsWithinHours: 3,
            },
        ],
    });

    assert.match(html, /Translation/);
    assert.match(html, /Moon carries light from ♄ Saturn to ♀ Venus/);
    assert.match(html, /3h/);
    assert.match(html, /<th scope="row">Translation<\/th>/);
});

test('renderHoraryTimingPatterns reports empty state', () => {
    assert.match(renderHoraryTimingPatterns({ timingPatterns: [] }), /No timing pattern candidates found/);
});

test('renderHoraryPositions uses planet names as row headers', () => {
    const html = renderHoraryPositions({
        positions: {
            Sun: {
                longitude: 95.5,
                house: 10,
                retrograde: false,
            },
        },
        dignities: {},
        accidentalDignities: {
            Sun: {
                angularity: 'angular',
                retrograde: false,
                solarCondition: null,
                inJoy: false,
                score: 5,
            },
        },
    });

    assert.match(html, /<th scope="row" class="">Sun<\/th>/);
    assert.match(html, /5° ♋ 30'/);
    assert.match(html, /Angular/);
    assert.match(html, /Acc \+5/);
});

test('renderHoraryAspects uses the first planet as a row header', () => {
    const html = renderHoraryAspects({
        aspects: [
            {
                planet1: 'Moon',
                planet2: 'Saturn',
                aspectName: 'Trine',
                symbol: '△',
                color: '#5a9ee0',
                orb: 1.25,
                applying: true,
                major: true,
            },
        ],
    });

    assert.match(html, /<th scope="row">☽ Moon<\/th>/);
    assert.match(html, /Trine/);
    assert.match(html, /1.25°/);
});
