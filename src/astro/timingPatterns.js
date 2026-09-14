import { CLASSICAL_PLANETS } from './utils.js';
import {
    ASPECT_PERFECTION_METHOD,
    aspectPerfectionKey,
    calculateAspectPerfections,
} from './aspectPerfections.js';

export const TIMING_PATTERN_METHOD = ASPECT_PERFECTION_METHOD;
export const TIMING_PATTERN_ESTIMATE_METHOD = 'Fallback estimate from one-hour orb delta';

export function calculateHoraryTimingPatterns({
    aspects,
    positions,
    planets = CLASSICAL_PLANETS,
    jd,
    maxSearchHours,
    positionsAtHours,
}) {
    const planetSet = new Set(planets);
    const precisionAvailable = Number.isFinite(jd) || typeof positionsAtHours === 'function';
    const perfections = precisionAvailable
        ? perfectionMap(calculateAspectPerfections({
            jd,
            aspects,
            planets,
            maxHours: maxSearchHours,
            positionsAtHours,
        }))
        : new Map();
    const timedAspects = aspects
        .filter(aspect =>
            aspect.major === true
            && planetSet.has(aspect.planet1)
            && planetSet.has(aspect.planet2)
        )
        .map(aspect => {
            const perfection = perfections.get(aspectPerfectionKey(aspect)) || null;
            return {
                ...aspect,
                perfection,
                estimatedPerfectsWithinHours: perfection
                    ? (perfection.usable ? perfection.perfectsWithinHours : null)
                    : (precisionAvailable ? null : estimateHoursToPerfection(aspect)),
                timingMethod: perfection ? perfection.method : TIMING_PATTERN_ESTIMATE_METHOD,
            };
        });
    const applying = timedAspects.filter(aspect => aspect.applying === true);
    const separating = timedAspects.filter(aspect => aspect.separating === true);

    return [
        ...detectTranslationCandidates(applying, separating),
        ...detectCollectionCandidates(applying, positions),
        ...detectProhibitionCandidates(applying),
    ].sort((left, right) =>
        patternSortTime(left) - patternSortTime(right)
        || left.type.localeCompare(right.type)
    );
}

export function estimateHoursToPerfection(aspect) {
    if (aspect.applying !== true) return null;
    if (!Number.isFinite(aspect.orb) || !Number.isFinite(aspect.futureOrb)) return null;

    const closingPerHour = aspect.orb - aspect.futureOrb;
    if (closingPerHour <= 0) return null;
    return round(aspect.orb / closingPerHour);
}

function detectTranslationCandidates(applying, separating) {
    const candidates = [];

    for (const separatedAspect of separating) {
        for (const applyingAspect of applying) {
            for (const mediator of commonPlanets(separatedAspect, applyingAspect)) {
                const fromPlanet = otherPlanet(separatedAspect, mediator);
                const toPlanet = otherPlanet(applyingAspect, mediator);
                if (!fromPlanet || !toPlanet || fromPlanet === toPlanet) continue;

                candidates.push({
                    type: 'translation',
                    mediator,
                    fromPlanet,
                    toPlanet,
                    separatedAspect: aspectEvidence(separatedAspect),
                    applyingAspect: aspectEvidence(applyingAspect),
                    estimatedPerfectsWithinHours: applyingAspect.estimatedPerfectsWithinHours,
                    method: TIMING_PATTERN_METHOD,
                });
            }
        }
    }

    return candidates;
}

function detectCollectionCandidates(applying, positions) {
    const candidates = [];

    for (const collector of CLASSICAL_PLANETS) {
        const collectorAspects = applying.filter(aspect => includesPlanet(aspect, collector));
        for (let i = 0; i < collectorAspects.length; i++) {
            for (let j = i + 1; j < collectorAspects.length; j++) {
                const aspect1 = collectorAspects[i];
                const aspect2 = collectorAspects[j];
                const planet1 = otherPlanet(aspect1, collector);
                const planet2 = otherPlanet(aspect2, collector);
                if (!planet1 || !planet2 || planet1 === planet2) continue;
                if (!isSlowerThan(collector, planet1, positions) || !isSlowerThan(collector, planet2, positions)) {
                    continue;
                }

                candidates.push({
                    type: 'collection',
                    collector,
                    planet1,
                    planet2,
                    aspect1: aspectEvidence(aspect1),
                    aspect2: aspectEvidence(aspect2),
                    estimatedFirstPerfectsWithinHours: minKnown(
                        aspect1.estimatedPerfectsWithinHours,
                        aspect2.estimatedPerfectsWithinHours
                    ),
                    estimatedSecondPerfectsWithinHours: maxKnown(
                        aspect1.estimatedPerfectsWithinHours,
                        aspect2.estimatedPerfectsWithinHours
                    ),
                    method: TIMING_PATTERN_METHOD,
                });
            }
        }
    }

    return candidates;
}

function detectProhibitionCandidates(applying) {
    const candidates = [];

    for (const directAspect of applying) {
        if (!Number.isFinite(directAspect.estimatedPerfectsWithinHours)) continue;

        for (const interveningAspect of applying) {
            if (directAspect === interveningAspect) continue;
            if (!Number.isFinite(interveningAspect.estimatedPerfectsWithinHours)) continue;
            if (interveningAspect.estimatedPerfectsWithinHours >= directAspect.estimatedPerfectsWithinHours) continue;

            const shared = commonPlanets(directAspect, interveningAspect)[0];
            if (!shared) continue;
            const interveningPlanet = otherPlanet(interveningAspect, shared);
            if (!interveningPlanet) continue;

            candidates.push({
                type: 'prohibitionFrustration',
                planet1: directAspect.planet1,
                planet2: directAspect.planet2,
                sharedPlanet: shared,
                interveningPlanet,
                directAspect: aspectEvidence(directAspect),
                interveningAspect: aspectEvidence(interveningAspect),
                directEstimatedPerfectsWithinHours: directAspect.estimatedPerfectsWithinHours,
                interveningEstimatedPerfectsWithinHours: interveningAspect.estimatedPerfectsWithinHours,
                method: TIMING_PATTERN_METHOD,
            });
        }
    }

    return dedupeProhibitions(candidates);
}

function aspectEvidence(aspect) {
    return {
        planet1: aspect.planet1,
        planet2: aspect.planet2,
        aspectName: aspect.aspectName,
        orb: aspect.orb,
        applying: aspect.applying === true,
        separating: aspect.separating === true,
        estimatedPerfectsWithinHours: aspect.estimatedPerfectsWithinHours ?? null,
        perfection: aspect.perfection || null,
        method: aspect.timingMethod || TIMING_PATTERN_ESTIMATE_METHOD,
    };
}

function includesPlanet(aspect, planet) {
    return aspect.planet1 === planet || aspect.planet2 === planet;
}

function commonPlanets(left, right) {
    return [left.planet1, left.planet2].filter(planet => includesPlanet(right, planet));
}

function otherPlanet(aspect, planet) {
    if (aspect.planet1 === planet) return aspect.planet2;
    if (aspect.planet2 === planet) return aspect.planet1;
    return null;
}

function isSlowerThan(candidate, planet, positions) {
    const candidateSpeed = Math.abs(positions?.[candidate]?.speed ?? Number.NaN);
    const planetSpeed = Math.abs(positions?.[planet]?.speed ?? Number.NaN);
    if (!Number.isFinite(candidateSpeed) || !Number.isFinite(planetSpeed)) return true;
    return candidateSpeed <= planetSpeed;
}

function patternSortTime(pattern) {
    return pattern.estimatedPerfectsWithinHours
        ?? pattern.estimatedFirstPerfectsWithinHours
        ?? pattern.interveningEstimatedPerfectsWithinHours
        ?? Number.POSITIVE_INFINITY;
}

function minKnown(left, right) {
    const values = [left, right].filter(Number.isFinite);
    return values.length ? Math.min(...values) : null;
}

function maxKnown(left, right) {
    const values = [left, right].filter(Number.isFinite);
    return values.length ? Math.max(...values) : null;
}

function dedupeProhibitions(candidates) {
    const seen = new Set();
    return candidates.filter(candidate => {
        const key = [
            candidate.planet1,
            candidate.planet2,
            candidate.sharedPlanet,
            candidate.interveningPlanet,
            candidate.directAspect.aspectName,
            candidate.interveningAspect.aspectName,
        ].join('|');
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
    });
}

function round(value) {
    return Math.round(value * 10000) / 10000;
}

function perfectionMap(perfections) {
    return new Map(perfections.map(perfection => [aspectPerfectionKey(perfection), perfection]));
}
