import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';
import { getHouseSystems } from '../astro/houses.js';
import {
    ASPECT_SETTINGS_STORAGE_KEY,
    normalizeAspectSettings,
} from './aspectSettings.js';

export const HOUSE_SYSTEM_STORAGE_KEY = 'horary_house';
export const PLANET_SET_STORAGE_KEY = 'horary_planets';
export const SETTINGS_SCHEMA_VERSION = 1;
export const SUPPORTED_PLANET_SETS = Object.freeze(['classical', 'modern']);

export function currentSettingsPayload(settings) {
    return {
        houseSystem: settings.houseSystem,
        planetSet: settings.planetSet,
        aspectSettings: settings.aspectSettings,
    };
}

export function exportableSettingsPayload(settings, { now = new Date() } = {}) {
    return {
        schemaVersion: SETTINGS_SCHEMA_VERSION,
        exportedAt: now.toISOString(),
        settings: currentSettingsPayload(settings),
    };
}

export function normalizeImportedSettingsPayload(payload, {
    houseSystems = getHouseSystems(),
    defaultSettings = DEFAULT_HORARY_SETTINGS,
} = {}) {
    const candidate = payload?.settings && typeof payload.settings === 'object'
        ? payload.settings
        : payload;
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate)) {
        throw new Error('Settings import must be a JSON object.');
    }

    const houseSystemKeys = new Set(houseSystems.map(system => system.key));
    const houseSystem = candidate.houseSystem || defaultSettings.houseSystem;
    const planetSet = candidate.planetSet || defaultSettings.planetSet;
    if (!houseSystemKeys.has(houseSystem)) {
        throw new Error(`Unsupported house system in settings import: ${houseSystem}`);
    }
    if (!SUPPORTED_PLANET_SETS.includes(planetSet)) {
        throw new Error(`Unsupported planet set in settings import: ${planetSet}`);
    }

    return {
        houseSystem,
        planetSet,
        aspectSettings: normalizeAspectSettings(candidate.aspectSettings),
    };
}

export function loadStoredAspectSettings(storage = globalThis.localStorage) {
    try {
        return normalizeAspectSettings(JSON.parse(storage?.getItem?.(ASPECT_SETTINGS_STORAGE_KEY) || 'null'));
    } catch {
        return normalizeAspectSettings(null);
    }
}

export function persistSettingsToStorage(settings, storage = globalThis.localStorage) {
    storage?.setItem?.(HOUSE_SYSTEM_STORAGE_KEY, settings.houseSystem);
    storage?.setItem?.(PLANET_SET_STORAGE_KEY, settings.planetSet);
    storage?.setItem?.(ASPECT_SETTINGS_STORAGE_KEY, JSON.stringify(settings.aspectSettings));
}
