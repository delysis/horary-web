import test from 'node:test';
import assert from 'node:assert/strict';

import {
    ASPECT_DEFINITIONS,
    DEFAULT_ASPECT_ORBS,
    DEFAULT_HORARY_SETTINGS,
} from '../horarySettings.js';

test('default horary settings expose calculation policy in one module', () => {
    assert.equal(DEFAULT_HORARY_SETTINGS.houseSystem, 'regiomontanus');
    assert.equal(DEFAULT_HORARY_SETTINGS.zodiac, 'tropical');
    assert.equal(DEFAULT_HORARY_SETTINGS.planetSet, 'modern');
    assert.equal(DEFAULT_HORARY_SETTINGS.aspectPhaseEpsilonDays, 1 / 24);

    for (const [key, aspect] of Object.entries(ASPECT_DEFINITIONS)) {
        assert.deepEqual(
            {
                enabled: DEFAULT_HORARY_SETTINGS.aspects[key].enabled,
                degree: DEFAULT_HORARY_SETTINGS.aspects[key].degree,
                orb: DEFAULT_HORARY_SETTINGS.aspects[key].orb,
                color: DEFAULT_HORARY_SETTINGS.aspects[key].color,
                major: DEFAULT_HORARY_SETTINGS.aspects[key].major,
            },
            {
                enabled: true,
                degree: aspect.angle,
                orb: DEFAULT_ASPECT_ORBS.default[key],
                color: aspect.color,
                major: aspect.major,
            }
        );
    }
});

test('default aspect settings are immutable at runtime', () => {
    assert.throws(() => {
        DEFAULT_HORARY_SETTINGS.aspects.square.enabled = false;
    }, TypeError);

    assert.equal(DEFAULT_HORARY_SETTINGS.aspects.square.enabled, true);
});
