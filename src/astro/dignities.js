/**
 * Essential Dignities & Debilities
 * 
 * Complete traditional dignity system including:
 * - Domicile / Detriment
 * - Exaltation / Fall  
 * - Triplicity Rulers (Dorothean: day, night, participating)
 * - Egyptian Terms/Bounds
 * - Faces/Decans (Chaldean order)
 * - Peregrine detection
 * - Mutual reception
 */

import { signIndex } from './utils.js';

// ╔══════════════════════════════════════╗
//    DOMICILE (RULERSHIP)
// ╚══════════════════════════════════════╝

// Sign index → ruling planet(s)
const DOMICILE = {
    0: ['Mars'],       // Aries
    1: ['Venus'],      // Taurus
    2: ['Mercury'],    // Gemini
    3: ['Moon'],       // Cancer
    4: ['Sun'],        // Leo
    5: ['Mercury'],    // Virgo
    6: ['Venus'],      // Libra
    7: ['Mars'],       // Scorpio (traditional: Mars, modern: Pluto)
    8: ['Jupiter'],    // Sagittarius
    9: ['Saturn'],     // Capricorn
    10: ['Saturn'],    // Aquarius (traditional: Saturn, modern: Uranus)
    11: ['Jupiter'],   // Pisces (traditional: Jupiter, modern: Neptune)
};

// Modern co-rulers
const MODERN_RULERS = {
    7: 'Pluto',     // Scorpio
    10: 'Uranus',    // Aquarius
    11: 'Neptune',   // Pisces
};

// Detriment = opposite sign's ruler
const DETRIMENT_SIGNS = {
    Sun: [10],           // Aquarius
    Moon: [9],           // Capricorn
    Mercury: [8, 11],    // Sagittarius, Pisces
    Venus: [0, 7],       // Aries, Scorpio
    Mars: [1, 6],        // Taurus, Libra
    Jupiter: [2, 5],     // Gemini, Virgo
    Saturn: [3, 4],      // Cancer, Leo
};

// ╔══════════════════════════════════════╗
//    EXALTATION / FALL
// ╚══════════════════════════════════════╝

const EXALTATION = {
    Sun: { sign: 0, degree: 19 },    // Aries 19°
    Moon: { sign: 1, degree: 3 },     // Taurus 3°
    Mercury: { sign: 5, degree: 15 },    // Virgo 15°
    Venus: { sign: 11, degree: 27 },   // Pisces 27°
    Mars: { sign: 9, degree: 28 },    // Capricorn 28°
    Jupiter: { sign: 3, degree: 15 },    // Cancer 15°
    Saturn: { sign: 6, degree: 21 },    // Libra 21°
    NorthNode: { sign: 2, degree: 3 },   // Gemini (Dragon's Head)
};

const EXALTATION_BY_SIGN = Object.fromEntries(
    Object.entries(EXALTATION).map(([planet, value]) => [value.sign, planet])
);

// Fall = opposite sign of exaltation
const FALL = {
    Sun: { sign: 6 },   // Libra
    Moon: { sign: 7 },   // Scorpio
    Mercury: { sign: 11 },  // Pisces
    Venus: { sign: 5 },   // Virgo
    Mars: { sign: 3 },   // Cancer
    Jupiter: { sign: 9 },   // Capricorn
    Saturn: { sign: 0 },   // Aries
};

// ╔══════════════════════════════════════╗
//    TRIPLICITY RULERS (Dorothean)
// ╚══════════════════════════════════════╝

// element → { day, night, participating }
const TRIPLICITY = {
    fire: { day: 'Sun', night: 'Jupiter', participating: 'Saturn' },
    earth: { day: 'Venus', night: 'Moon', participating: 'Mars' },
    air: { day: 'Saturn', night: 'Mercury', participating: 'Jupiter' },
    water: { day: 'Venus', night: 'Mars', participating: 'Moon' },
};

const ELEMENTS = ['fire', 'earth', 'air', 'water', 'fire', 'earth', 'air', 'water', 'fire', 'earth', 'air', 'water'];

// ╔══════════════════════════════════════╗
//    EGYPTIAN TERMS/BOUNDS
// ╚══════════════════════════════════════╝

// Each sign: array of [endDegree, planet] pairs
const TERMS = [
    // Aries
    [[6, 'Jupiter'], [12, 'Venus'], [20, 'Mercury'], [25, 'Mars'], [30, 'Saturn']],
    // Taurus
    [[8, 'Venus'], [14, 'Mercury'], [22, 'Jupiter'], [27, 'Saturn'], [30, 'Mars']],
    // Gemini
    [[6, 'Mercury'], [12, 'Jupiter'], [17, 'Venus'], [24, 'Mars'], [30, 'Saturn']],
    // Cancer
    [[7, 'Mars'], [13, 'Venus'], [19, 'Mercury'], [26, 'Jupiter'], [30, 'Saturn']],
    // Leo
    [[6, 'Jupiter'], [11, 'Venus'], [18, 'Saturn'], [24, 'Mercury'], [30, 'Mars']],
    // Virgo
    [[7, 'Mercury'], [17, 'Venus'], [21, 'Jupiter'], [28, 'Mars'], [30, 'Saturn']],
    // Libra
    [[6, 'Saturn'], [14, 'Mercury'], [21, 'Jupiter'], [28, 'Venus'], [30, 'Mars']],
    // Scorpio
    [[7, 'Mars'], [11, 'Venus'], [19, 'Mercury'], [24, 'Jupiter'], [30, 'Saturn']],
    // Sagittarius
    [[12, 'Jupiter'], [17, 'Venus'], [21, 'Mercury'], [26, 'Saturn'], [30, 'Mars']],
    // Capricorn
    [[7, 'Mercury'], [14, 'Jupiter'], [22, 'Venus'], [26, 'Saturn'], [30, 'Mars']],
    // Aquarius
    [[7, 'Mercury'], [13, 'Venus'], [20, 'Jupiter'], [25, 'Mars'], [30, 'Saturn']],
    // Pisces
    [[12, 'Venus'], [16, 'Jupiter'], [19, 'Mercury'], [28, 'Mars'], [30, 'Saturn']],
];

// ╔══════════════════════════════════════╗
//    FACES / DECANS (Chaldean order)
// ╚══════════════════════════════════════╝

const CHALDEAN_ORDER = ['Saturn', 'Jupiter', 'Mars', 'Sun', 'Venus', 'Mercury', 'Moon'];

function faceRuler(signIdx, decan) {
    // decan 0, 1, 2 for each sign
    // Mars rules first decan of Aries (index 0), then cycle Chaldean
    const startIdx = 4; // Mars in Chaldean order
    const totalDecan = signIdx * 3 + decan;
    return CHALDEAN_ORDER[(startIdx + totalDecan) % 7];
}

// ╔══════════════════════════════════════╗
//    PUBLIC API
// ╚══════════════════════════════════════╝

/**
 * Get all dignities for a planet at a given ecliptic longitude.
 * @param {string} planet - Planet name
 * @param {number} longitude - Ecliptic longitude in degrees
 * @param {boolean} isDayChart - Whether the Sun is above the horizon
 * @returns {Object} dignity information
 */
export function getDignities(planet, longitude, isDayChart = true) {
    const sign = signIndex(longitude);
    const degInSign = longitude - sign * 30;
    const decan = Math.floor(degInSign / 10);

    const result = {
        planet,
        longitude,
        sign,
        domicile: false,
        detriment: false,
        exaltation: false,
        fall: false,
        triplicityRuler: false,
        termRuler: false,
        faceRuler: false,
        peregrine: false,
        score: 0,
    };

    // Domicile (+5)
    if (DOMICILE[sign] && DOMICILE[sign].includes(planet)) {
        result.domicile = true;
        result.score += 5;
    }

    // Detriment (-5)
    if (DETRIMENT_SIGNS[planet] && DETRIMENT_SIGNS[planet].includes(sign)) {
        result.detriment = true;
        result.score -= 5;
    }

    // Exaltation (+4)
    if (EXALTATION[planet] && EXALTATION[planet].sign === sign) {
        result.exaltation = true;
        result.score += 4;
    }

    // Fall (-4)
    if (FALL[planet] && FALL[planet].sign === sign) {
        result.fall = true;
        result.score -= 4;
    }

    // Triplicity (+3)
    const element = ELEMENTS[sign];
    const trip = TRIPLICITY[element];
    if (trip) {
        const ruler = isDayChart ? trip.day : trip.night;
        if (ruler === planet) {
            result.triplicityRuler = true;
            result.score += 3;
        }
    }

    // Terms (+2)
    const signTerms = TERMS[sign];
    if (signTerms) {
        for (const [endDeg, ruler] of signTerms) {
            if (degInSign < endDeg) {
                result.termRulerPlanet = ruler;
                if (ruler === planet) {
                    result.termRuler = true;
                    result.score += 2;
                }
                break;
            }
        }
    }

    // Face (+1)
    const decanRuler = faceRuler(sign, decan);
    result.faceRulerPlanet = decanRuler;
    if (decanRuler === planet) {
        result.faceRuler = true;
        result.score += 1;
    }

    // Peregrine (no essential dignity at all)
    result.peregrine = !result.domicile && !result.exaltation &&
        !result.triplicityRuler && !result.termRuler && !result.faceRuler;

    return result;
}

export function getDignityRulersForLongitude(longitude, isDayChart = true) {
    const sign = signIndex(longitude);
    const degInSign = longitude - sign * 30;
    const decan = Math.floor(degInSign / 10);
    const element = ELEMENTS[sign];
    const triplicity = TRIPLICITY[element];
    const rulers = [];

    for (const ruler of DOMICILE[sign] || []) {
        rulers.push({ dignity: 'domicile', planet: ruler });
    }

    if (EXALTATION_BY_SIGN[sign]) {
        rulers.push({ dignity: 'exaltation', planet: EXALTATION_BY_SIGN[sign] });
    }

    if (triplicity) {
        rulers.push({
            dignity: 'triplicity',
            planet: isDayChart ? triplicity.day : triplicity.night,
        });
    }

    const termRuler = termRulerForDegree(sign, degInSign);
    if (termRuler) {
        rulers.push({ dignity: 'term', planet: termRuler });
    }

    rulers.push({ dignity: 'face', planet: faceRuler(sign, decan) });

    return rulers;
}

function termRulerForDegree(sign, degreeInSign) {
    for (const [endDeg, ruler] of TERMS[sign] || []) {
        if (degreeInSign < endDeg) return ruler;
    }
    return null;
}

/**
 * Get the traditional sign ruler.
 */
export function signRuler(signIdx) {
    return DOMICILE[signIdx]?.[0] || null;
}

/**
 * Get the modern ruler (includes outer planets).
 */
export function modernRuler(signIdx) {
    return MODERN_RULERS[signIdx] || DOMICILE[signIdx]?.[0] || null;
}

/**
 * Detect mutual receptions between two planets.
 */
export function isMutualReception(planet1, lon1, planet2, lon2) {
    const sign1 = signIndex(lon1);
    const sign2 = signIndex(lon2);

    // By domicile
    const ruler1 = DOMICILE[sign1] || [];
    const ruler2 = DOMICILE[sign2] || [];

    return ruler1.includes(planet2) && ruler2.includes(planet1);
}

/**
 * Get complete dignity table for all planets in a chart.
 */
export function getChartDignities(positions, isDayChart = true) {
    const result = {};
    for (const [name, pos] of Object.entries(positions)) {
        if (['NorthNode', 'SouthNode'].includes(name)) continue;
        result[name] = getDignities(name, pos.longitude, isDayChart);
    }
    return result;
}

/**
 * Determine if a chart is a day or night chart.
 * Day = Sun above horizon (houses 7-12).
 */
export function isDayChart(sunLon, ascLon) {
    let diff = sunLon - ascLon;
    if (diff < 0) diff += 360;
    // Sun is above horizon when its longitude places it in houses 7-12
    // Simplified: Sun is above horizon if within 180° ahead of ASC (counter-clockwise)
    return diff > 180;
}

export { DOMICILE, EXALTATION, FALL, TRIPLICITY, TERMS, DETRIMENT_SIGNS };
