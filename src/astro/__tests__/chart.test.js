import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';

import {
    ASPECT_PHASE_EPSILON_DAYS,
    calculateChart,
    getHouseNumber,
    planetListForSet,
} from '../chart.js';

test('planetListForSet returns classical or modern bodies explicitly', () => {
    assert.deepEqual(planetListForSet('classical'), ['Sun', 'Moon', 'Mercury', 'Venus', 'Mars', 'Jupiter', 'Saturn']);
    assert.ok(planetListForSet('modern').includes('NorthNode'));
    assert.ok(planetListForSet('modern').includes('Pluto'));
});

test('getHouseNumber assigns wrapped houses without normalizing to zero coordinates', () => {
    const cusps = [350, 20, 50, 80, 110, 140, 170, 200, 230, 260, 290, 320];

    assert.equal(getHouseNumber(355, cusps), 1);
    assert.equal(getHouseNumber(15, cusps), 1);
    assert.equal(getHouseNumber(45, cusps), 2);
});

test('calculateChart returns a tropical chart with future-position aspect phase metadata', () => {
    const chart = calculateChart({
        date: new Date(Date.UTC(2024, 5, 21, 12, 0, 0)),
        lat: 51.5074,
        lng: -0.1278,
        localDate: '2024-06-21',
        localTime: '08:00',
        offsetHours: -4,
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
    });

    assert.equal(chart.zodiac, 'tropical');
    assert.equal(chart.aspectPhaseEpsilonDays, ASPECT_PHASE_EPSILON_DAYS);
    assert.deepEqual(Object.keys(chart.positions), planetListForSet('classical'));
    assert.ok(Number.isFinite(chart.houses.asc));
    assert.ok(chart.aspects.every(aspect => aspect.futureOrb !== null));
    assert.ok(Object.values(chart.positions).every(position => Number.isInteger(position.house)));
    assert.equal(typeof chart.accidentalDignities.Moon.score, 'number');
    assert.equal(chart.lots.partOfFortune.name, 'Part of Fortune');
    assert.equal(typeof chart.lots.partOfFortune.longitude, 'number');
    assert.ok(Number.isInteger(chart.lots.partOfFortune.house));
    assert.ok(Array.isArray(chart.receptions));
    assert.equal(chart.voidOfCourseMoon.available, true);
    assert.equal(typeof chart.voidOfCourseMoon.checkedUntilHours, 'number');
    assert.ok(Array.isArray(chart.antisciaContacts));
    assert.ok(Array.isArray(chart.solarConditions));
    assert.equal(chart.planetaryHour.available, true);
    assert.equal(typeof chart.planetaryHour.planetaryHourRuler, 'string');
    assert.ok(Array.isArray(chart.timingPatterns));
    assert.deepEqual(chart.input, {
        localDate: '2024-06-21',
        localTime: '08:00',
        offsetHours: -4,
    });
});

test('calculateChart output is independent of process timezone for UTC date inputs', () => {
    const script = `
        import { calculateChart } from './src/astro/chart.js';
        const chart = calculateChart({
            date: new Date(Date.UTC(2024, 5, 21, 12, 0, 0)),
            lat: 51.5074,
            lng: -0.1278,
            houseSystem: 'regiomontanus',
            planetSet: 'classical',
        });
        console.log(JSON.stringify({
            jd: chart.jd,
            asc: chart.houses.asc,
            mc: chart.houses.mc,
            sun: chart.positions.Sun.longitude,
            moon: chart.positions.Moon.longitude,
        }));
    `;

    const london = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
        cwd: process.cwd(),
        env: { ...process.env, TZ: 'Europe/London' },
        encoding: 'utf8',
    });
    const tokyo = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
        cwd: process.cwd(),
        env: { ...process.env, TZ: 'Asia/Tokyo' },
        encoding: 'utf8',
    });

    assert.deepEqual(JSON.parse(london), JSON.parse(tokyo));
});
