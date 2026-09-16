import { getAllPositions } from './planets.js';
import { CLASSICAL_PLANETS, norm180, signIndex } from './utils.js';

export const VOID_OF_COURSE_MAJOR_ASPECTS = Object.freeze([
    Object.freeze({ angle: 0, name: 'Conjunction' }),
    Object.freeze({ angle: 60, name: 'Sextile' }),
    Object.freeze({ angle: 90, name: 'Square' }),
    Object.freeze({ angle: 120, name: 'Trine' }),
    Object.freeze({ angle: 180, name: 'Opposition' }),
]);

export function calculateVoidOfCourseMoon({
    jd,
    positions,
    planets = CLASSICAL_PLANETS,
    stepHours = 1,
    maxHours = 72,
    exactOrb = 0.5,
    positionProvider = getAllPositions,
}) {
    if (!positions?.Moon || !Number.isFinite(jd)) {
        return {
            available: false,
            isVoid: null,
            reason: 'Moon position or Julian Date unavailable',
        };
    }

    const moonSign = signIndex(positions.Moon.longitude);
    const candidatePlanets = planets.filter(planet => planet !== 'Moon' && positions[planet]);
    const initialAspects = buildMoonAspectMap(positions, candidatePlanets);

    for (let hours = stepHours; hours <= maxHours; hours += stepHours) {
        const futurePositions = positionProvider(jd + hours / 24, planets);
        const futureMoon = futurePositions.Moon;
        if (!futureMoon) continue;

        if (signIndex(futureMoon.longitude) !== moonSign) {
            return {
                available: true,
                isVoid: true,
                checkedUntilHours: round(hours),
                moonSign,
                nextApplyingAspect: null,
            };
        }

        for (const planet of candidatePlanets) {
            if (!futurePositions[planet]) continue;
            for (const aspect of VOID_OF_COURSE_MAJOR_ASPECTS) {
                const currentOrb = initialAspects.get(`${planet}:${aspect.angle}`);
                const futureOrb = aspectOrb(futureMoon.longitude, futurePositions[planet].longitude, aspect.angle);
                if (futureOrb <= exactOrb && futureOrb < currentOrb) {
                    return {
                        available: true,
                        isVoid: false,
                        checkedUntilHours: round(hours),
                        moonSign,
                        nextApplyingAspect: {
                            planet,
                            aspectName: aspect.name,
                            angle: aspect.angle,
                            orb: round(futureOrb),
                            perfectsWithinHours: round(hours),
                        },
                    };
                }
            }
        }
    }

    return {
        available: true,
        isVoid: null,
        checkedUntilHours: maxHours,
        moonSign,
        nextApplyingAspect: null,
        reason: 'Moon did not leave sign within scan window',
    };
}

function buildMoonAspectMap(positions, planets) {
    const result = new Map();
    for (const planet of planets) {
        for (const aspect of VOID_OF_COURSE_MAJOR_ASPECTS) {
            result.set(
                `${planet}:${aspect.angle}`,
                aspectOrb(positions.Moon.longitude, positions[planet].longitude, aspect.angle)
            );
        }
    }
    return result;
}

function aspectOrb(moonLongitude, planetLongitude, aspectAngle) {
    return Math.abs(Math.abs(norm180(moonLongitude - planetLongitude)) - aspectAngle);
}

function round(value) {
    return Math.round(value * 10000) / 10000;
}
