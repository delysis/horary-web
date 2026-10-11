import test from 'node:test';
import assert from 'node:assert/strict';

import {
    ASPECT_SETTINGS_STORAGE_KEY,
    cloneDefaultAspectSettings,
} from '../aspectSettings.js';
import {
    HOUSE_SYSTEM_STORAGE_KEY,
    PLANET_SET_STORAGE_KEY,
    currentSettingsPayload,
    exportableSettingsPayload,
    loadStoredAspectSettings,
    normalizeImportedSettingsPayload,
    persistSettingsToStorage,
} from '../settingsPayload.js';

function memoryStorage(initial = {}) {
    const values = new Map(Object.entries(initial));
    return {
        getItem(key) {
            return values.has(key) ? values.get(key) : null;
        },
        setItem(key, value) {
            values.set(key, value);
        },
    };
}

test('currentSettingsPayload serializes the persisted settings boundary', () => {
    const aspectSettings = cloneDefaultAspectSettings();
    const payload = currentSettingsPayload({
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings,
        ignored: true,
    });

    assert.deepEqual(payload, {
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings,
    });
});

test('exportableSettingsPayload wraps settings with version and timestamp', () => {
    const exported = exportableSettingsPayload({
        houseSystem: 'regiomontanus',
        planetSet: 'modern',
        aspectSettings: { trine: { enabled: false, orb: 5 } },
    }, { now: new Date('2026-06-30T12:00:00.000Z') });

    assert.equal(exported.schemaVersion, 1);
    assert.equal(exported.exportedAt, '2026-06-30T12:00:00.000Z');
    assert.equal(exported.settings.houseSystem, 'regiomontanus');
    assert.equal(exported.settings.planetSet, 'modern');
});

test('normalizeImportedSettingsPayload accepts wrapped and bare settings payloads', () => {
    const houseSystems = [{ key: 'regiomontanus' }, { key: 'whole-sign' }];
    const wrapped = normalizeImportedSettingsPayload({
        settings: {
            houseSystem: 'whole-sign',
            planetSet: 'classical',
            aspectSettings: { sextile: { enabled: false, orb: 2.5 } },
        },
    }, { houseSystems });
    const bare = normalizeImportedSettingsPayload({
        houseSystem: 'regiomontanus',
        planetSet: 'modern',
    }, { houseSystems });

    assert.equal(wrapped.houseSystem, 'whole-sign');
    assert.equal(wrapped.planetSet, 'classical');
    assert.equal(wrapped.aspectSettings.sextile.enabled, false);
    assert.equal(wrapped.aspectSettings.sextile.orb, 2.5);
    assert.equal(bare.houseSystem, 'regiomontanus');
    assert.equal(bare.planetSet, 'modern');
});

test('normalizeImportedSettingsPayload rejects unsupported values', () => {
    const houseSystems = [{ key: 'regiomontanus' }];

    assert.throws(
        () => normalizeImportedSettingsPayload({ houseSystem: 'unknown' }, { houseSystems }),
        /Unsupported house system/
    );
    assert.throws(
        () => normalizeImportedSettingsPayload({ planetSet: 'invented' }, { houseSystems }),
        /Unsupported planet set/
    );
    assert.throws(
        () => normalizeImportedSettingsPayload(null, { houseSystems }),
        /Settings import must be a JSON object/
    );
});

test('settings storage helpers persist and recover aspect policy safely', () => {
    const aspectSettings = cloneDefaultAspectSettings();
    aspectSettings.trine.orb = 4;
    const storage = memoryStorage();

    persistSettingsToStorage({
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings,
    }, storage);

    assert.equal(storage.getItem(HOUSE_SYSTEM_STORAGE_KEY), 'regiomontanus');
    assert.equal(storage.getItem(PLANET_SET_STORAGE_KEY), 'classical');
    assert.equal(JSON.parse(storage.getItem(ASPECT_SETTINGS_STORAGE_KEY)).trine.orb, 4);
    assert.equal(loadStoredAspectSettings(storage).trine.orb, 4);
    assert.equal(loadStoredAspectSettings(memoryStorage({ [ASPECT_SETTINGS_STORAGE_KEY]: '{' })).trine.orb, 7);
});
