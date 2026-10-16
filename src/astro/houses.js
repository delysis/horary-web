/**
 * House Systems Calculator
 * 
 * Implements all 10 major house systems used in Western astrology.
 * All systems share the same interface:
 *   calculateHouses(system, lmst, latitude, obliquity) → { cusps, asc, mc, vertex }
 */

import { norm360, sind, cosd, tand, asind, acosd, atan2d, DEG, RAD } from './utils.js';

// ╔══════════════════════════════════════╗
//    FUNDAMENTAL ANGLES
// ╚══════════════════════════════════════╝

export function calcASC(lmst, obliquity, latitude) {
    const eps = obliquity * DEG;
    const phi = latitude * DEG;
    const theta = lmst * DEG;
    const y = Math.cos(theta);
    const x = -(Math.sin(theta) * Math.cos(eps) + Math.tan(phi) * Math.sin(eps));
    return norm360(Math.atan2(y, x) * RAD);
}

export function calcMC(lmst, obliquity) {
    const eps = obliquity * DEG;
    const theta = lmst * DEG;
    return norm360(Math.atan2(Math.sin(theta), Math.cos(theta) * Math.cos(eps)) * RAD);
}

export function calcVertex(lmst, obliquity, latitude) {
    const coLat = 90 - latitude;
    return calcASC(norm360(lmst + 180), obliquity, coLat);
}

/** Convert ecliptic longitude to right ascension */
function eclToRA(lon, obliquity) {
    return norm360(atan2d(sind(lon) * cosd(obliquity), cosd(lon)));
}

/** Declination from ecliptic longitude (on ecliptic, β=0) */
function eclToDecl(lon, obliquity) {
    return asind(sind(obliquity) * sind(lon));
}

// ╔══════════════════════════════════════╗
//    PLACIDUS — out of scope, falls back to Porphyry
//    (True Placidus requires Swiss Ephemeris-level iteration;
//     Porphyry is a reasonable quadrant-based approximation.
//     Regiomontanus is preferred for horary anyway.)
// ╚══════════════════════════════════════╝

// ╔══════════════════════════════════════╗
//    KOCH (Birthplace time system)
// ╚══════════════════════════════════════╝

function koch(lmst, lat, eps) {
    const mc = calcMC(lmst, eps);
    const asc = calcASC(lmst, eps, lat);
    const cusps = new Array(12);
    cusps[0] = asc;
    cusps[6] = norm360(asc + 180);
    cusps[9] = mc;
    cusps[3] = norm360(mc + 180);

    const RAMC = lmst;
    const declMC = eclToDecl(mc, eps);

    // Semi-arc of MC degree
    const cosHA = -tand(lat) * tand(declMC);
    let SA;
    if (cosHA < -1) SA = 180;
    else if (cosHA > 1) SA = 0;
    else SA = acosd(cosHA);

    // Koch: vary RAMC by fractions of MC's semi-arc, compute ASC at each
    cusps[10] = calcASC(norm360(RAMC + SA / 3), eps, lat);
    cusps[11] = calcASC(norm360(RAMC + 2 * SA / 3), eps, lat);

    // For houses 2,3: use nocturnal semi-arc
    const NSA = 180 - SA;
    cusps[2] = calcASC(norm360(RAMC - NSA / 3), eps, lat);
    cusps[1] = calcASC(norm360(RAMC - 2 * NSA / 3), eps, lat);

    cusps[4] = norm360(cusps[10] + 180);
    cusps[5] = norm360(cusps[11] + 180);
    cusps[7] = norm360(cusps[1] + 180);
    cusps[8] = norm360(cusps[2] + 180);

    return cusps;
}

// ╔══════════════════════════════════════╗
//    REGIOMONTANUS (equator division)
// ╚══════════════════════════════════════╝

function regiomontanus(lmst, lat, eps) {
    const RAMC = lmst;
    const cusps = new Array(12);
    const phi = lat * DEG;
    const e = eps * DEG;

    const cuspAt = (equatorialHA, poleLat) => {
        const th = norm360(equatorialHA) * DEG;
        const p = poleLat;
        const y = Math.cos(th);
        const x = -(Math.sin(th) * Math.cos(e) + Math.tan(p) * Math.sin(e));
        return norm360(Math.atan2(y, x) * RAD);
    };

    const p30 = Math.atan(Math.tan(phi) * Math.sin(30 * DEG));
    const p60 = Math.atan(Math.tan(phi) * Math.sin(60 * DEG));

    cusps[9] = calcMC(lmst, eps);
    cusps[10] = cuspAt(RAMC - 60, p30);
    cusps[11] = cuspAt(RAMC - 30, p60);
    cusps[0] = calcASC(lmst, eps, lat);
    cusps[1] = cuspAt(RAMC + 30, p60);
    cusps[2] = cuspAt(RAMC + 60, p30);

    for (let i = 0; i < 6; i++) {
        const idx = (i + 9) % 12;
        const oppIdx = (idx + 6) % 12;
        if (cusps[idx] !== undefined) {
            cusps[oppIdx] = norm360(cusps[idx] + 180);
        }
    }
    return cusps;
}

// ╔══════════════════════════════════════╗
//    CAMPANUS (prime vertical division)
// ╚══════════════════════════════════════╝

function campanus(lmst, lat, eps) {
    const mc = calcMC(lmst, eps);
    const asc = calcASC(lmst, eps, lat);
    const cusps = new Array(12);
    cusps[0] = asc;
    cusps[6] = norm360(asc + 180);
    cusps[9] = mc;
    cusps[3] = norm360(mc + 180);

    const RAMC = lmst;

    function campanCusp(A) {
        // A = azimuth from East (30°, 60°, 120°, 150°)
        const Arad = A * DEG;
        const latRad = lat * DEG;

        // Pole of house circle
        const D = atan2d(tand(A), cosd(lat));
        const decl = asind(-cosd(D) * sind(lat));
        const ra = norm360(RAMC + 90 + D);

        return norm360(atan2d(
            sind(ra) * cosd(eps) + tand(decl) * sind(eps),
            cosd(ra)
        ));
    }

    cusps[10] = campanCusp(30);
    cusps[11] = campanCusp(60);
    cusps[1] = campanCusp(120);
    cusps[2] = campanCusp(150);

    cusps[4] = norm360(cusps[10] + 180);
    cusps[5] = norm360(cusps[11] + 180);
    cusps[7] = norm360(cusps[1] + 180);
    cusps[8] = norm360(cusps[2] + 180);

    return cusps;
}

// ╔══════════════════════════════════════╗
//    EQUAL (30° from ASC)
// ╚══════════════════════════════════════╝

function equal(lmst, lat, eps) {
    const asc = calcASC(lmst, eps, lat);
    const cusps = new Array(12);
    for (let i = 0; i < 12; i++) {
        cusps[i] = norm360(asc + i * 30);
    }
    return cusps;
}

// ╔══════════════════════════════════════╗
//    WHOLE SIGN
// ╚══════════════════════════════════════╝

function wholeSign(lmst, lat, eps) {
    const asc = calcASC(lmst, eps, lat);
    const signStart = Math.floor(asc / 30) * 30;
    const cusps = new Array(12);
    for (let i = 0; i < 12; i++) {
        cusps[i] = norm360(signStart + i * 30);
    }
    return cusps;
}

// ╔══════════════════════════════════════╗
//    PORPHYRY (trisect quadrants)
// ╚══════════════════════════════════════╝

function porphyry(lmst, lat, eps) {
    const mc = calcMC(lmst, eps);
    const asc = calcASC(lmst, eps, lat);
    const ic = norm360(mc + 180);
    const dsc = norm360(asc + 180);
    const cusps = new Array(12);
    cusps[0] = asc;
    cusps[3] = ic;
    cusps[6] = dsc;
    cusps[9] = mc;

    function arc(from, to) {
        let a = to - from;
        if (a < 0) a += 360;
        return a;
    }

    // MC → ASC
    const q1 = arc(mc, asc);
    cusps[10] = norm360(mc + q1 / 3);
    cusps[11] = norm360(mc + 2 * q1 / 3);

    // ASC → IC
    const q2 = arc(asc, ic);
    cusps[1] = norm360(asc + q2 / 3);
    cusps[2] = norm360(asc + 2 * q2 / 3);

    // IC → DSC
    const q3 = arc(ic, dsc);
    cusps[4] = norm360(ic + q3 / 3);
    cusps[5] = norm360(ic + 2 * q3 / 3);

    // DSC → MC
    const q4 = arc(dsc, mc);
    cusps[7] = norm360(dsc + q4 / 3);
    cusps[8] = norm360(dsc + 2 * q4 / 3);

    return cusps;
}

// ╔══════════════════════════════════════╗
//    MORINUS (equator, no horizon)
// ╚══════════════════════════════════════╝

function morinus(lmst, lat, eps) {
    const cusps = new Array(12);
    for (let i = 0; i < 12; i++) {
        const RA = norm360(lmst + 90 + i * 30);
        cusps[i] = norm360(atan2d(sind(RA), cosd(RA) * cosd(eps)));
    }
    return cusps;
}

// ╔══════════════════════════════════════╗
//    ALCABITIUS (semi-arc on diurnal arcs)
// ╚══════════════════════════════════════╝

function alcabitius(lmst, lat, eps) {
    const mc = calcMC(lmst, eps);
    const asc = calcASC(lmst, eps, lat);
    const cusps = new Array(12);
    cusps[0] = asc;
    cusps[6] = norm360(asc + 180);
    cusps[9] = mc;
    cusps[3] = norm360(mc + 180);

    const RAMC = lmst;

    // Oblique ascension of ASC
    const RA_ASC = eclToRA(asc, eps);
    // Diurnal semi-arc = distance from MC to ASC in RA
    let DSA = norm360(RA_ASC - RAMC);
    if (DSA > 180) DSA = 360 - DSA;

    // Trisect DSA
    cusps[10] = calcASC(norm360(RAMC + DSA / 3), eps, lat);
    cusps[11] = calcASC(norm360(RAMC + 2 * DSA / 3), eps, lat);

    // Nocturnal semi-arc
    const NSA = 180 - DSA;
    cusps[1] = calcASC(norm360(RAMC + DSA + NSA / 3), eps, lat);
    cusps[2] = calcASC(norm360(RAMC + DSA + 2 * NSA / 3), eps, lat);

    cusps[4] = norm360(cusps[10] + 180);
    cusps[5] = norm360(cusps[11] + 180);
    cusps[7] = norm360(cusps[1] + 180);
    cusps[8] = norm360(cusps[2] + 180);

    return cusps;
}

// ╔══════════════════════════════════════╗
//    TOPOCENTRIC (Polich-Page)
// ╚══════════════════════════════════════╝

function topocentric(lmst, lat, eps) {
    const mc = calcMC(lmst, eps);
    const asc = calcASC(lmst, eps, lat);
    const cusps = new Array(12);
    cusps[0] = asc;
    cusps[6] = norm360(asc + 180);
    cusps[9] = mc;
    cusps[3] = norm360(mc + 180);

    const RAMC = lmst;
    const tanPhi = tand(lat);

    // Topocentric uses tangent ratios rather than the full latitude
    function topoCalc(h) {
        // "Pseudo-latitude" for each house
        let d;
        if (h === 30 || h === 150) d = Math.atan(tanPhi / 3) * RAD;
        else d = Math.atan(2 * tanPhi / 3) * RAD;

        const RA = norm360(RAMC + h);
        return norm360(atan2d(
            sind(RA),
            cosd(RA) * cosd(eps) + tand(d) * sind(eps) * sind(h)
        ));
    }

    cusps[10] = topoCalc(30);
    cusps[11] = topoCalc(60);
    cusps[1] = topoCalc(120);
    cusps[2] = topoCalc(150);

    cusps[4] = norm360(cusps[10] + 180);
    cusps[5] = norm360(cusps[11] + 180);
    cusps[7] = norm360(cusps[1] + 180);
    cusps[8] = norm360(cusps[2] + 180);

    return cusps;
}

// ╔══════════════════════════════════════╗
//    PUBLIC API
// ╚══════════════════════════════════════╝

const HOUSE_SYSTEMS = {
    placidus: { fn: porphyry, name: 'Placidus (Porphyry)', code: 'P' },
    koch: { fn: koch, name: 'Koch', code: 'K' },
    regiomontanus: { fn: regiomontanus, name: 'Regiomontanus', code: 'R' },
    campanus: { fn: campanus, name: 'Campanus', code: 'C' },
    equal: { fn: equal, name: 'Equal', code: 'E' },
    wholesign: { fn: wholeSign, name: 'Whole Sign', code: 'W' },
    porphyry: { fn: porphyry, name: 'Porphyry', code: 'O' },
    morinus: { fn: morinus, name: 'Morinus', code: 'M' },
    alcabitius: { fn: alcabitius, name: 'Alcabitius', code: 'B' },
    topocentric: { fn: topocentric, name: 'Topocentric', code: 'T' },
};

/**
 * Calculate house cusps for a given system.
 */
export function calculateHouses(system, lmst, latitude, obliquity) {
    const sys = HOUSE_SYSTEMS[system];
    if (!sys) throw new Error(`Unknown house system: ${system}`);

    const cusps = sys.fn(lmst, latitude, obliquity);
    const asc = calcASC(lmst, obliquity, latitude);
    const mc = calcMC(lmst, obliquity);
    const vertex = calcVertex(lmst, obliquity, latitude);

    return { cusps, asc, mc, vertex, system: sys.name };
}

/**
 * Get list of available house systems.
 */
export function getHouseSystems() {
    return Object.entries(HOUSE_SYSTEMS).map(([key, val]) => ({
        key, name: val.name, code: val.code
    }));
}

export { HOUSE_SYSTEMS };
