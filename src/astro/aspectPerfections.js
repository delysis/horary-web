import { getAllPositions } from './planets.js';
import { CLASSICAL_PLANETS, norm180, signIndex } from './utils.js';

export const ASPECT_PERFECTION_METHOD = 'Bounded future event search with sign-boundary and station flags';

const DEFAULT_MAX_HOURS = 168;
const DEFAULT_STEP_HOURS = 1;
const DEFAULT_EXACT_ORB = 0.01;
const ROUND_FACTOR = 10000;
const STATION_SPEED_EPSILON = 0.00001;

export function calculateAspectPerfections({
    jd,
    aspects,
    planets = CLASSICAL_PLANETS,
    maxHours = DEFAULT_MAX_HOURS,
    stepHours = DEFAULT_STEP_HOURS,
    exactOrb = DEFAULT_EXACT_ORB,
    positionsAtHours,
}) {
    if (!Array.isArray(aspects) || aspects.length === 0) return [];
    const planetSet = new Set(planets);
    const getPositions = positionsAtHours || (hours => getAllPositions(jd + hours / 24, planets));
    if (typeof getPositions !== 'function') return [];

    return aspects.flatMap(aspect => {
        if (
            aspect.major !== true
            || aspect.applying !== true
            || !planetSet.has(aspect.planet1)
            || !planetSet.has(aspect.planet2)
            || !Number.isFinite(aspect.angle)
        ) {
            return [];
        }

        const event = findFirstPerfection({
            aspect,
            positionsAtHours: getPositions,
            maxHours,
            stepHours,
            exactOrb,
        });
        return event ? [event] : [];
    });
}

export function findFirstPerfection({
    aspect,
    positionsAtHours,
    maxHours = DEFAULT_MAX_HOURS,
    stepHours = DEFAULT_STEP_HOURS,
    exactOrb = DEFAULT_EXACT_ORB,
}) {
    const samples = sampleOrbCurve({
        aspect,
        positionsAtHours,
        maxHours,
        stepHours,
    });
    if (samples.length < 2) return null;

    if (samples[0].orb <= exactOrb) {
        return buildEvent(aspect, { hours: 0, orb: samples[0].orb }, positionsAtHours, stepHours, exactOrb);
    }

    for (let i = 1; i < samples.length - 1; i++) {
        const previous = samples[i - 1];
        const current = samples[i];
        const next = samples[i + 1];
        if (current.orb <= previous.orb && current.orb <= next.orb) {
            const refined = refineMinimum(aspect, positionsAtHours, previous.hours, next.hours);
            if (refined.orb <= exactOrb) {
                return buildEvent(aspect, refined, positionsAtHours, stepHours, exactOrb);
            }
        }
    }

    const finalSample = samples[samples.length - 1];
    if (finalSample.orb <= exactOrb) {
        return buildEvent(aspect, finalSample, positionsAtHours, stepHours, exactOrb);
    }

    return null;
}

export function aspectPerfectionKey(aspect) {
    return [
        aspect.planet1,
        aspect.planet2,
        aspect.aspect || aspect.aspectName || '',
        Number.isFinite(aspect.angle) ? aspect.angle : '',
    ].join('|');
}

function sampleOrbCurve({ aspect, positionsAtHours, maxHours, stepHours }) {
    const samples = [];
    for (let hours = 0; hours <= maxHours + 0.000001; hours += stepHours) {
        const orb = orbAt(aspect, positionsAtHours, hours);
        if (Number.isFinite(orb)) {
            samples.push({ hours: round(hours), orb });
        }
    }
    return samples;
}

function refineMinimum(aspect, positionsAtHours, leftHours, rightHours) {
    let left = leftHours;
    let right = rightHours;

    for (let i = 0; i < 48; i++) {
        const span = right - left;
        const m1 = left + span / 3;
        const m2 = right - span / 3;
        if (orbAt(aspect, positionsAtHours, m1) <= orbAt(aspect, positionsAtHours, m2)) {
            right = m2;
        } else {
            left = m1;
        }
    }

    const hours = (left + right) / 2;
    return {
        hours: round(hours),
        orb: orbAt(aspect, positionsAtHours, hours),
    };
}

function buildEvent(aspect, refined, positionsAtHours, stepHours, exactOrb) {
    const signBoundary = firstSignBoundaryBefore(aspect, refined.hours, positionsAtHours, stepHours);
    const station = firstStationBefore(aspect, refined.hours, positionsAtHours, stepHours);
    const blockedBySignBoundary = Boolean(signBoundary && signBoundary.hours <= refined.hours + 0.0001);

    return {
        planet1: aspect.planet1,
        planet2: aspect.planet2,
        aspect: aspect.aspect,
        aspectName: aspect.aspectName,
        angle: aspect.angle,
        currentOrb: Number.isFinite(aspect.orb) ? aspect.orb : null,
        perfectsWithinHours: round(refined.hours),
        minimumOrb: round(refined.orb),
        exactOrb,
        signBoundary,
        station,
        blockedBySignBoundary,
        stationBeforePerfection: Boolean(station && station.hours <= refined.hours + 0.0001),
        usable: refined.orb <= exactOrb && !blockedBySignBoundary,
        method: ASPECT_PERFECTION_METHOD,
    };
}

function firstSignBoundaryBefore(aspect, eventHours, positionsAtHours, stepHours) {
    const start = positionsAtHours(0);
    const boundaries = [aspect.planet1, aspect.planet2]
        .map(planet => firstPlanetSignBoundaryBefore(planet, eventHours, positionsAtHours, stepHours, start))
        .filter(Boolean)
        .sort((left, right) => left.hours - right.hours);
    return boundaries[0] || null;
}

function firstPlanetSignBoundaryBefore(planet, eventHours, positionsAtHours, stepHours, startPositions) {
    const startPosition = startPositions?.[planet];
    if (!startPosition) return null;

    const startSign = safeSignIndex(startPosition.longitude);
    if (!Number.isFinite(startSign)) return null;
    let previousHours = 0;
    let previousSign = startSign;

    for (let hours = Math.min(stepHours, eventHours); hours <= eventHours + 0.000001; hours += stepHours) {
        const currentHours = Math.min(hours, eventHours);
        const currentSign = safeSignIndex(positionsAtHours(currentHours)?.[planet]?.longitude);
        if (!Number.isFinite(currentSign)) return null;
        if (currentSign !== previousSign) {
            return {
                planet,
                hours: round(refineSignBoundary(planet, startSign, positionsAtHours, previousHours, currentHours)),
                fromSign: startSign,
                toSign: currentSign,
            };
        }
        previousHours = currentHours;
        previousSign = currentSign;
        if (currentHours >= eventHours) break;
    }

    return null;
}

function refineSignBoundary(planet, startSign, positionsAtHours, leftHours, rightHours) {
    let left = leftHours;
    let right = rightHours;
    for (let i = 0; i < 40; i++) {
        const mid = (left + right) / 2;
        const midSign = safeSignIndex(positionsAtHours(mid)?.[planet]?.longitude);
        if (midSign === startSign) {
            left = mid;
        } else {
            right = mid;
        }
    }
    return right;
}

function firstStationBefore(aspect, eventHours, positionsAtHours, stepHours) {
    const stations = [aspect.planet1, aspect.planet2]
        .map(planet => firstPlanetStationBefore(planet, eventHours, positionsAtHours, stepHours))
        .filter(Boolean)
        .sort((left, right) => left.hours - right.hours);
    return stations[0] || null;
}

function firstPlanetStationBefore(planet, eventHours, positionsAtHours, stepHours) {
    const startSpeed = speedSign(positionsAtHours(0)?.[planet]?.speed);
    if (startSpeed == null) return null;
    if (startSpeed === 0) return { planet, hours: 0 };

    let previousHours = 0;
    let previousSign = startSpeed;
    for (let hours = Math.min(stepHours, eventHours); hours <= eventHours + 0.000001; hours += stepHours) {
        const currentHours = Math.min(hours, eventHours);
        const currentSign = speedSign(positionsAtHours(currentHours)?.[planet]?.speed);
        if (currentSign == null) return null;
        if (currentSign === 0 || (previousSign !== 0 && currentSign !== previousSign)) {
            return {
                planet,
                hours: round(refineStation(planet, positionsAtHours, previousHours, currentHours, previousSign)),
            };
        }
        previousHours = currentHours;
        previousSign = currentSign;
        if (currentHours >= eventHours) break;
    }

    return null;
}

function refineStation(planet, positionsAtHours, leftHours, rightHours, startSign) {
    let left = leftHours;
    let right = rightHours;
    for (let i = 0; i < 40; i++) {
        const mid = (left + right) / 2;
        const midSign = speedSign(positionsAtHours(mid)?.[planet]?.speed);
        if (midSign === startSign) {
            left = mid;
        } else {
            right = mid;
        }
    }
    return right;
}

function speedSign(speed) {
    if (!Number.isFinite(speed)) return null;
    if (Math.abs(speed) <= STATION_SPEED_EPSILON) return 0;
    return speed > 0 ? 1 : -1;
}

function safeSignIndex(longitude) {
    return Number.isFinite(longitude) ? signIndex(longitude) : Number.NaN;
}

function orbAt(aspect, positionsAtHours, hours) {
    const positions = positionsAtHours(hours);
    const left = positions?.[aspect.planet1];
    const right = positions?.[aspect.planet2];
    if (!left || !right) return Number.NaN;
    return Math.abs(Math.abs(norm180(left.longitude - right.longitude)) - aspect.angle);
}

function round(value) {
    return Math.round(value * ROUND_FACTOR) / ROUND_FACTOR;
}
