import { CLASSICAL_PLANETS, norm180 } from './utils.js';

export const SOLAR_CONDITION_THRESHOLDS = Object.freeze({
    cazimiDegrees: 17 / 60,
    combustDegrees: 8.5,
    underBeamsDegrees: 17,
});

export function solarElongationDegrees(planetLongitude, sunLongitude) {
    return Math.abs(norm180(planetLongitude - sunLongitude));
}

export function classifySolarCondition(elongationDegrees) {
    if (!Number.isFinite(elongationDegrees)) return null;
    if (elongationDegrees <= SOLAR_CONDITION_THRESHOLDS.cazimiDegrees) return 'cazimi';
    if (elongationDegrees <= SOLAR_CONDITION_THRESHOLDS.combustDegrees) return 'combust';
    if (elongationDegrees <= SOLAR_CONDITION_THRESHOLDS.underBeamsDegrees) return 'underBeams';
    return null;
}

export function calculateSolarConditions(positions, { planets = CLASSICAL_PLANETS } = {}) {
    const sun = positions?.Sun;
    if (!sun || !Number.isFinite(sun.longitude)) return [];

    return planets
        .filter(planet => planet !== 'Sun')
        .map(planet => solarConditionForPlanet(planet, positions?.[planet], sun.longitude))
        .filter(Boolean)
        .sort((left, right) =>
            left.separationDegrees - right.separationDegrees
            || left.planet.localeCompare(right.planet)
        );
}

function solarConditionForPlanet(planet, position, sunLongitude) {
    if (!position || !Number.isFinite(position.longitude)) return null;
    const separationDegrees = solarElongationDegrees(position.longitude, sunLongitude);
    const condition = classifySolarCondition(separationDegrees);
    if (!condition) return null;

    return {
        planet,
        condition,
        separationDegrees,
        sunLongitude,
        planetLongitude: position.longitude,
    };
}
