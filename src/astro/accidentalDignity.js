import { CLASSICAL_PLANETS } from './utils.js';

export const HOUSE_ANGULARITY = Object.freeze({
    1: { label: 'angular', score: 5 },
    2: { label: 'succedent', score: 2 },
    3: { label: 'cadent', score: -5 },
    4: { label: 'angular', score: 5 },
    5: { label: 'succedent', score: 2 },
    6: { label: 'cadent', score: -5 },
    7: { label: 'angular', score: 5 },
    8: { label: 'succedent', score: 2 },
    9: { label: 'cadent', score: -5 },
    10: { label: 'angular', score: 5 },
    11: { label: 'succedent', score: 2 },
    12: { label: 'cadent', score: -5 },
});

export const PLANETARY_JOY_HOUSES = Object.freeze({
    Mercury: 1,
    Moon: 3,
    Venus: 5,
    Mars: 6,
    Sun: 9,
    Jupiter: 11,
    Saturn: 12,
});

export const ACCIDENTAL_DIGNITY_SCORES = Object.freeze({
    retrograde: -5,
    cazimi: 5,
    combust: -5,
    underBeams: -4,
    planetaryJoy: 2,
});

export function calculateAccidentalDignities(
    positions,
    {
        planets = CLASSICAL_PLANETS,
        solarConditions = [],
    } = {},
) {
    const solarConditionByPlanet = new Map(
        solarConditions.map(condition => [condition.planet, condition])
    );

    const result = {};
    for (const planet of planets) {
        if (!CLASSICAL_PLANETS.includes(planet)) continue;
        const position = positions?.[planet];
        if (!position) continue;
        result[planet] = accidentalDignityForPlanet(
            planet,
            position,
            solarConditionByPlanet.get(planet),
        );
    }
    return result;
}

function accidentalDignityForPlanet(planet, position, solarCondition) {
    const house = Number.isInteger(position.house) ? position.house : null;
    const angularity = HOUSE_ANGULARITY[house]?.label || 'unknown';
    let score = HOUSE_ANGULARITY[house]?.score || 0;
    const factors = [];

    if (HOUSE_ANGULARITY[house]) {
        factors.push({ type: 'angularity', label: angularity, score: HOUSE_ANGULARITY[house].score });
    }

    if (position.retrograde === true) {
        score += ACCIDENTAL_DIGNITY_SCORES.retrograde;
        factors.push({ type: 'motion', label: 'retrograde', score: ACCIDENTAL_DIGNITY_SCORES.retrograde });
    }

    if (solarCondition?.condition) {
        const solarScore = ACCIDENTAL_DIGNITY_SCORES[solarCondition.condition] || 0;
        score += solarScore;
        factors.push({
            type: 'solarCondition',
            label: solarCondition.condition,
            score: solarScore,
            separationDegrees: solarCondition.separationDegrees,
        });
    }

    const joyHouse = PLANETARY_JOY_HOUSES[planet] || null;
    const inJoy = joyHouse === house;
    if (inJoy) {
        score += ACCIDENTAL_DIGNITY_SCORES.planetaryJoy;
        factors.push({ type: 'planetaryJoy', label: `joy in house ${joyHouse}`, score: ACCIDENTAL_DIGNITY_SCORES.planetaryJoy });
    }

    return {
        planet,
        house,
        angularity,
        retrograde: position.retrograde === true,
        solarCondition: solarCondition?.condition || null,
        inJoy,
        joyHouse,
        score,
        factors,
    };
}
