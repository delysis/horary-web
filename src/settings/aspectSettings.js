import { DEFAULT_HORARY_SETTINGS } from '../astro/horarySettings.js';

export const ASPECT_SETTINGS_STORAGE_KEY = 'horary_aspects';
export const MIN_ASPECT_ORB = 0;
export const MAX_ASPECT_ORB = 30;

export function cloneDefaultAspectSettings() {
    return Object.fromEntries(
        Object.entries(DEFAULT_HORARY_SETTINGS.aspects).map(([key, aspect]) => [
            key,
            { ...aspect },
        ])
    );
}

export function normalizeAspectSettings(candidate) {
    const defaults = cloneDefaultAspectSettings();
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate)) {
        return defaults;
    }

    for (const [key, aspect] of Object.entries(defaults)) {
        const override = candidate[key];
        if (!override || typeof override !== 'object' || Array.isArray(override)) continue;

        if (typeof override.enabled === 'boolean') {
            aspect.enabled = override.enabled;
        }

        const orb = Number(override.orb);
        if (Number.isFinite(orb)) {
            aspect.orb = clamp(orb, MIN_ASPECT_ORB, MAX_ASPECT_ORB);
        }
    }

    return defaults;
}

export function aspectSettingsEqual(left, right) {
    return JSON.stringify(normalizeAspectSettings(left)) === JSON.stringify(normalizeAspectSettings(right));
}

function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
}
