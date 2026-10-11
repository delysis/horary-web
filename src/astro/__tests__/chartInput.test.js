import test from 'node:test';
import assert from 'node:assert/strict';

import {
    localDateTimeToUtcDate,
    validateChartInput,
    validateCoordinates,
} from '../chartInput.js';

test('validateChartInput rejects blank date', () => {
    const result = validateChartInput({
        date: '',
        time: '12:00',
        offset: '0',
        lat: '51.5',
        lng: '-0.1',
    });

    assert.equal(result.ok, false);
    assert.deepEqual(result.errors.map(error => error.field), ['date']);
});

test('validateChartInput rejects invalid calendar date without normalizing', () => {
    const result = validateChartInput({
        date: '2024-02-31',
        time: '12:00',
        offset: '0',
        lat: '51.5',
        lng: '-0.1',
    });

    assert.equal(result.ok, false);
    assert.equal(result.errors[0].message, 'Use a real calendar date.');
});

test('validateChartInput rejects invalid hour and minute', () => {
    const badHour = validateChartInput({
        date: '2024-06-21',
        time: '24:00',
        offset: '0',
        lat: '51.5',
        lng: '-0.1',
    });
    const badMinute = validateChartInput({
        date: '2024-06-21',
        time: '12:60',
        offset: '0',
        lat: '51.5',
        lng: '-0.1',
    });

    assert.equal(badHour.ok, false);
    assert.equal(badMinute.ok, false);
    assert.equal(badHour.errors[0].field, 'time');
    assert.equal(badMinute.errors[0].field, 'time');
});

test('validateCoordinates rejects blank and out-of-range coordinates', () => {
    const blank = validateCoordinates({ lat: '', lng: '' });
    const outOfRange = validateCoordinates({ lat: '91', lng: '-181' });

    assert.equal(blank.ok, false);
    assert.deepEqual(blank.errors.map(error => error.field), ['lat', 'lng']);
    assert.equal(outOfRange.ok, false);
    assert.deepEqual(outOfRange.errors.map(error => error.field), ['lat', 'lng']);
});

test('localDateTimeToUtcDate applies manual UTC offset exactly', () => {
    const utc = localDateTimeToUtcDate(
        { year: 2024, month: 6, day: 21 },
        { hour: 12, minute: 30 },
        -4
    );

    assert.equal(utc.toISOString(), '2024-06-21T16:30:00.000Z');
});

test('validateChartInput rejects future chart time unless allowed', () => {
    const raw = {
        date: '2024-06-21',
        time: '12:00',
        offset: '0',
        lat: '51.5',
        lng: '-0.1',
    };
    const now = new Date('2024-06-21T11:59:00.000Z');

    const rejected = validateChartInput(raw, { now });
    const accepted = validateChartInput(raw, { now, allowFuture: true });

    assert.equal(rejected.ok, false);
    assert.equal(rejected.errors[0].field, 'date');
    assert.equal(accepted.ok, true);
    assert.equal(accepted.value.utcDate.toISOString(), '2024-06-21T12:00:00.000Z');
});
