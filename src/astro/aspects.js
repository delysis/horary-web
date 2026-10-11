/**
 * Aspect Calculator
 * 
 * Detects aspects between planets with configurable orbs.
 * Distinguishes between applying and separating aspects when future
 * positions are provided.
 */

import {
    ASPECT_DEFINITIONS,
    DEFAULT_ASPECT_ORBS,
} from './horarySettings.js';
import { norm180 } from './utils.js';

export const ASPECTS = ASPECT_DEFINITIONS;
export const DEFAULT_ORBS = DEFAULT_ASPECT_ORBS;

function angularSeparation(lon1, lon2) {
    return Math.abs(norm180(lon1 - lon2));
}

function aspectOrb(pos1, pos2, aspectAngle) {
    return Math.abs(angularSeparation(pos1.longitude, pos2.longitude) - aspectAngle);
}

/**
 * Get the effective orb for a pair of planets and aspect.
 */
function getOrb(planet1, planet2, aspectKey, customOrbs, aspectConfig) {
    const configuredOrb = aspectConfig?.[aspectKey]?.orb;
    if (Number.isFinite(configuredOrb)) {
        return configuredOrb;
    }

    const orbs = customOrbs || DEFAULT_ORBS;
    const o1 = orbs[planet1] || orbs.default;
    const o2 = orbs[planet2] || orbs.default;
    // Use the tighter of the two orbs
    const a1 = o1[aspectKey] ?? o1.conjunction ?? 5;
    const a2 = o2[aspectKey] ?? o2.conjunction ?? 5;
    return Math.min(a1, a2);
}

function configuredAspectEntries(aspectConfig, majorOnly) {
    return Object.entries(ASPECTS).flatMap(([key, aspect]) => {
        const config = aspectConfig?.[key];
        if (config?.enabled === false) return [];

        const configuredAspect = {
            ...aspect,
            angle: Number.isFinite(config?.degree) ? config.degree : aspect.angle,
            color: config?.color || aspect.color,
            symbol: config?.symbol || aspect.symbol,
            name: config?.name || aspect.name,
            major: typeof config?.major === 'boolean' ? config.major : aspect.major,
        };
        if (majorOnly && !configuredAspect.major) return [];
        return [[key, configuredAspect]];
    });
}

/**
 * Calculate all aspects between a set of planets.
 * @param {Object} positions - { planetName: { longitude, speed } }
 * @param {Object} options - { majorOnly, customOrbs, aspectConfig, planets, futurePositions }
 * @returns {Array} aspects
 */
export function calculateAspects(positions, options = {}) {
    const { majorOnly = false, customOrbs, aspectConfig, planets, futurePositions } = options;
    const names = planets || Object.keys(positions);
    const aspectEntries = configuredAspectEntries(aspectConfig, majorOnly);
    const aspects = [];

    for (let i = 0; i < names.length; i++) {
        for (let j = i + 1; j < names.length; j++) {
            const p1 = names[i], p2 = names[j];
            const pos1 = positions[p1], pos2 = positions[p2];
            if (!pos1 || !pos2) continue;

            const diff = angularSeparation(pos1.longitude, pos2.longitude);

            for (const [key, aspect] of aspectEntries) {
                const maxOrb = getOrb(p1, p2, key, customOrbs, aspectConfig);
                const orb = Math.abs(diff - aspect.angle);

                if (orb <= maxOrb) {
                    let applying = null;
                    let separating = null;
                    let futureOrb = null;

                    if (futurePositions?.[p1] && futurePositions?.[p2]) {
                        futureOrb = aspectOrb(futurePositions[p1], futurePositions[p2], aspect.angle);
                        const epsilon = 0.000001;
                        applying = futureOrb < orb - epsilon;
                        separating = futureOrb > orb + epsilon;
                    }

                    aspects.push({
                        planet1: p1,
                        planet2: p2,
                        aspect: key,
                        aspectName: aspect.name,
                        symbol: aspect.symbol,
                        color: aspect.color,
                        major: aspect.major,
                        angle: aspect.angle,
                        orb: Math.round(orb * 100) / 100,
                        futureOrb: futureOrb == null ? null : Math.round(futureOrb * 10000) / 10000,
                        applying,
                        separating,
                        exact: orb < 0.1,
                        lon1: pos1.longitude,
                        lon2: pos2.longitude,
                    });
                    break; // Only one aspect per pair
                }
            }
        }
    }

    return aspects.sort((a, b) => a.orb - b.orb);
}

/**
 * Filter for only applying aspects (used in horary).
 */
export function applyingAspects(aspects) {
    return aspects.filter(a => a.applying);
}

/**
 * Get aspects to a specific planet.
 */
export function aspectsTo(aspects, planetName) {
    return aspects.filter(a => a.planet1 === planetName || a.planet2 === planetName);
}
