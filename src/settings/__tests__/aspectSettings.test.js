import test from 'node:test';
import assert from 'node:assert/strict';

import { DEFAULT_HORARY_SETTINGS } from '../../astro/horarySettings.js';
import {
    aspectSettingsEqual,
    cloneDefaultAspectSettings,
    normalizeAspectSettings,
} from '../aspectSettings.js';

test('cloneDefaultAspectSettings returns mutable copies of default aspect policy', () => {
    const clone = cloneDefaultAspectSettings();
    clone.sextile.enabled = false;

    assert.equal(DEFAULT_HORARY_SETTINGS.aspects.sextile.enabled, true);
    assert.equal(clone.sextile.enabled, false);
});

test('normalizeAspectSettings applies persisted enabled flags and orbs', () => {
    const settings = normalizeAspectSettings({
        sextile: { enabled: false, orb: 2.5 },
    });

    assert.equal(settings.sextile.enabled, false);
    assert.equal(settings.sextile.orb, 2.5);
    assert.equal(settings.trine.enabled, DEFAULT_HORARY_SETTINGS.aspects.trine.enabled);
});

test('normalizeAspectSettings ignores unknown aspects and clamps orb ranges', () => {
    const settings = normalizeAspectSettings({
        sextile: { orb: -10 },
        square: { orb: 99 },
        invented: { enabled: true, orb: 12 },
    });

    assert.equal(settings.sextile.orb, 0);
    assert.equal(settings.square.orb, 30);
    assert.equal(settings.invented, undefined);
});

test('aspectSettingsEqual compares normalized policy values', () => {
    assert.equal(aspectSettingsEqual(null, cloneDefaultAspectSettings()), true);
    assert.equal(aspectSettingsEqual({ trine: { enabled: false } }, cloneDefaultAspectSettings()), false);
});
