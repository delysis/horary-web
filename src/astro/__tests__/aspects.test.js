import test from 'node:test';
import assert from 'node:assert/strict';

import { calculateAspects } from '../aspects.js';
import { DEFAULT_HORARY_SETTINGS } from '../horarySettings.js';

test('calculateAspects marks applying when future orb is smaller', () => {
    const aspects = calculateAspects(
        {
            Moon: { longitude: 0, speed: 13 },
            Saturn: { longitude: 58, speed: 0 },
        },
        {
            majorOnly: true,
            futurePositions: {
                Moon: { longitude: 0 },
                Saturn: { longitude: 59 },
            },
        }
    );

    assert.equal(aspects.length, 1);
    assert.equal(aspects[0].aspect, 'sextile');
    assert.equal(aspects[0].orb, 2);
    assert.equal(aspects[0].applying, true);
    assert.equal(aspects[0].separating, false);
});

test('calculateAspects marks separating when future orb is larger', () => {
    const aspects = calculateAspects(
        {
            Moon: { longitude: 0, speed: 13 },
            Saturn: { longitude: 58, speed: 0 },
        },
        {
            majorOnly: true,
            futurePositions: {
                Moon: { longitude: 0 },
                Saturn: { longitude: 57 },
            },
        }
    );

    assert.equal(aspects.length, 1);
    assert.equal(aspects[0].aspect, 'sextile');
    assert.equal(aspects[0].applying, false);
    assert.equal(aspects[0].separating, true);
});

test('calculateAspects ignores misleading speed values when future positions are available', () => {
    const aspects = calculateAspects(
        {
            FastBody: { longitude: 0, speed: 20 },
            SlowBody: { longitude: 58, speed: 0 },
        },
        {
            majorOnly: true,
            customOrbs: {
                default: { sextile: 5 },
            },
            futurePositions: {
                FastBody: { longitude: 0 },
                SlowBody: { longitude: 57 },
            },
        }
    );

    assert.equal(aspects.length, 1);
    assert.equal(aspects[0].applying, false);
    assert.equal(aspects[0].separating, true);
});

test('calculateAspects reports unavailable phase without future positions', () => {
    const aspects = calculateAspects(
        {
            Moon: { longitude: 0, speed: 13 },
            Saturn: { longitude: 58, speed: 0 },
        },
        { majorOnly: true }
    );

    assert.equal(aspects.length, 1);
    assert.equal(aspects[0].applying, null);
    assert.equal(aspects[0].separating, null);
    assert.equal(aspects[0].futureOrb, null);
});

test('calculateAspects honors disabled configured aspects', () => {
    const aspectConfig = {
        ...DEFAULT_HORARY_SETTINGS.aspects,
        sextile: { ...DEFAULT_HORARY_SETTINGS.aspects.sextile, enabled: false },
    };
    const aspects = calculateAspects(
        {
            Moon: { longitude: 0 },
            Saturn: { longitude: 58 },
        },
        { majorOnly: true, aspectConfig }
    );

    assert.equal(aspects.length, 0);
});

test('calculateAspects honors configured aspect orb widths', () => {
    const narrowAspectConfig = {
        ...DEFAULT_HORARY_SETTINGS.aspects,
        sextile: { ...DEFAULT_HORARY_SETTINGS.aspects.sextile, orb: 1 },
    };
    const wideAspectConfig = {
        ...DEFAULT_HORARY_SETTINGS.aspects,
        sextile: { ...DEFAULT_HORARY_SETTINGS.aspects.sextile, orb: 3 },
    };

    assert.equal(calculateAspects(
        {
            Moon: { longitude: 0 },
            Saturn: { longitude: 58 },
        },
        { majorOnly: true, aspectConfig: narrowAspectConfig }
    ).length, 0);

    const aspects = calculateAspects(
        {
            Moon: { longitude: 0 },
            Saturn: { longitude: 58 },
        },
        { majorOnly: true, aspectConfig: wideAspectConfig }
    );

    assert.equal(aspects.length, 1);
    assert.equal(aspects[0].aspect, 'sextile');
    assert.equal(aspects[0].orb, 2);
});
