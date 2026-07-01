import test from 'node:test';
import assert from 'node:assert/strict';

import {
    calculatePlanetaryHour,
    planetaryHourRuler,
    solarEventsForLocalDate,
} from '../planetaryHours.js';

function fixedSolarEvents(localDate) {
    const midnight = Date.UTC(localDate.year, localDate.month - 1, localDate.day);
    return {
        available: true,
        sunrise: new Date(midnight + 6 * 60 * 60 * 1000),
        sunset: new Date(midnight + 18 * 60 * 60 * 1000),
    };
}

test('planetaryHourRuler follows the Chaldean sequence from the day ruler', () => {
    assert.equal(planetaryHourRuler('Sun', 0), 'Sun');
    assert.equal(planetaryHourRuler('Sun', 1), 'Venus');
    assert.equal(planetaryHourRuler('Sun', 2), 'Mercury');
    assert.equal(planetaryHourRuler('Moon', 0), 'Moon');
});

test('calculatePlanetaryHour identifies daylight planetary hour', () => {
    const result = calculatePlanetaryHour({
        date: new Date('2024-06-23T07:30:00.000Z'),
        lat: 51.5,
        lng: 0,
        solarEventsProvider: fixedSolarEvents,
    });

    assert.equal(result.available, true);
    assert.equal(result.period, 'day');
    assert.equal(result.planetaryDay, 'Sunday');
    assert.equal(result.planetaryDayRuler, 'Sun');
    assert.equal(result.hourNumber, 2);
    assert.equal(result.planetaryHourRuler, 'Venus');
});

test('calculatePlanetaryHour identifies night planetary hour after sunset', () => {
    const result = calculatePlanetaryHour({
        date: new Date('2024-06-23T19:30:00.000Z'),
        lat: 51.5,
        lng: 0,
        solarEventsProvider: fixedSolarEvents,
    });

    assert.equal(result.period, 'night');
    assert.equal(result.planetaryDay, 'Sunday');
    assert.equal(result.hourNumber, 2);
    assert.equal(result.planetaryHourRuler, 'Mars');
});

test('calculatePlanetaryHour uses previous planetary day before sunrise', () => {
    const result = calculatePlanetaryHour({
        date: new Date('2024-06-24T05:30:00.000Z'),
        lat: 51.5,
        lng: 0,
        solarEventsProvider: fixedSolarEvents,
    });

    assert.equal(result.period, 'night');
    assert.equal(result.planetaryDay, 'Sunday');
    assert.equal(result.hourNumber, 12);
    assert.equal(result.planetaryHourRuler, 'Mercury');
});

test('solarEventsForLocalDate returns ordered sunrise and sunset for ordinary latitude', () => {
    const events = solarEventsForLocalDate({ year: 2024, month: 6, day: 21 }, 51.5, -0.1);

    assert.equal(events.available, true);
    assert.ok(events.sunrise < events.sunset);
    assert.match(events.sunrise.toISOString(), /^2024-06-21T/);
});
