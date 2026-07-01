/**
 * Planetary position calculations.
 * 
 * Sun: Truncated VSOP87 (Meeus Chapter 25)
 * Moon: Truncated ELP2000 (Meeus Chapter 47)
 * Planets: Truncated VSOP87 / orbital elements (Meeus Chapter 31-36)
 * Pluto: Polynomial fit (Meeus Chapter 37)
 * Nodes: Mean lunar node
 * 
 * All positions returned as geometric ecliptic longitude (degrees, J2000 ecliptic).
 * Precision: ~0.01° for Sun, ~0.05° for Moon, ~0.1° for planets.
 */

import { norm360, sind, cosd, tand, asind, atan2d, DEG, RAD } from './utils.js';
import { julianCenturies, meanObliquity, nutation } from './time.js';

// ╔══════════════════════════════════════╗
//   SUN (Meeus Ch. 25 — low accuracy)
// ╚══════════════════════════════════════╝

function sunPosition(T) {
    // Geometric mean longitude
    const L0 = norm360(280.46646 + 36000.76983 * T + 0.0003032 * T * T);
    // Mean anomaly
    const M = norm360(357.52911 + 35999.05029 * T - 0.0001537 * T * T);
    const Mr = M * DEG;
    // Equation of center
    const C = (1.914602 - 0.004817 * T - 0.000014 * T * T) * Math.sin(Mr)
        + (0.019993 - 0.000101 * T) * Math.sin(2 * Mr)
        + 0.000289 * Math.sin(3 * Mr);
    // Sun's true longitude
    const sunLon = norm360(L0 + C);
    // Sun's true anomaly
    const v = M + C;
    // Distance (AU)
    const e = 0.016708634 - 0.000042037 * T - 0.0000001267 * T * T;
    const R = (1.000001018 * (1 - e * e)) / (1 + e * cosd(v));
    // Apparent longitude (nutation + aberration)
    const omega = 125.04 - 1934.136 * T;
    const apparent = sunLon - 0.00569 - 0.00478 * sind(omega);

    // Daily motion ~0.9856°/day
    const speed = 0.9856 + 0.0200 * Math.cos(Mr);

    return { longitude: norm360(apparent), latitude: 0, distance: R, speed, retrograde: false, name: 'Sun' };
}

// ╔══════════════════════════════════════╗
//   MOON (Meeus Ch. 47 — truncated)
// ╚══════════════════════════════════════╝

function moonPosition(T) {
    // Fundamental arguments
    const Lp = norm360(218.3164477 + 481267.88123421 * T - 0.0015786 * T * T + T * T * T / 538841 - T * T * T * T / 65194000);
    const D = norm360(297.8501921 + 445267.1114034 * T - 0.0018819 * T * T + T * T * T / 545868 - T * T * T * T / 113065000);
    const M = norm360(357.5291092 + 35999.0502909 * T - 0.0001536 * T * T + T * T * T / 24490000);
    const Mp = norm360(134.9633964 + 477198.8675055 * T + 0.0087414 * T * T + T * T * T / 69699 - T * T * T * T / 14712000);
    const F = norm360(93.2720950 + 483202.0175233 * T - 0.0036539 * T * T - T * T * T / 3526000 + T * T * T * T / 863310000);

    const A1 = norm360(119.75 + 131.849 * T);
    const A2 = norm360(53.09 + 479264.290 * T);
    const A3 = norm360(313.45 + 481266.484 * T);
    const E = 1 - 0.002516 * T - 0.0000074 * T * T;
    const E2 = E * E;

    // Longitude terms (largest 60)
    const lonTerms = [
        [0, 0, 1, 0, 6288774], [2, 0, -1, 0, 1274027], [2, 0, 0, 0, 658314],
        [0, 0, 2, 0, 213618], [0, 1, 0, 0, -185116], [0, 0, 0, 2, -114332],
        [2, 0, -2, 0, 58793], [2, -1, -1, 0, 57066], [2, 0, 1, 0, 53322],
        [2, -1, 0, 0, 45758], [0, 1, -1, 0, -40923], [1, 0, 0, 0, -34720],
        [0, 1, 1, 0, -30383], [2, 0, 0, -2, 15327], [0, 0, 1, 2, -12528],
        [0, 0, 1, -2, 10980], [4, 0, -1, 0, 10675], [0, 0, 3, 0, 10034],
        [4, 0, -2, 0, 8548], [2, 1, -1, 0, -7888], [2, 1, 0, 0, -6766],
        [1, 0, -1, 0, -5163], [1, 1, 0, 0, 4987], [2, -1, 1, 0, 4036],
        [2, 0, 2, 0, 3994], [4, 0, 0, 0, 3861], [2, 0, -3, 0, 3665],
        [0, 1, -2, 0, -2689], [2, 0, -1, 2, -2602], [2, -1, -2, 0, 2390],
        [1, 0, 1, 0, -2348], [2, -2, 0, 0, 2236], [0, 1, 2, 0, -2120],
        [0, 2, 0, 0, -2069], [2, -2, -1, 0, 2048], [2, 0, 1, -2, -1773],
        [2, 0, 0, 2, -1595], [4, -1, -1, 0, 1215], [0, 0, 2, 2, -1110],
        [3, 0, -1, 0, -892], [2, 1, 1, 0, -810], [4, -1, -2, 0, 759],
        [0, 2, -1, 0, -713], [2, 2, -1, 0, -700], [2, 1, -2, 0, 691],
        [2, -1, 0, -2, 596], [4, 0, 1, 0, 549], [0, 0, 4, 0, 537],
        [4, -1, 0, 0, 520], [1, 0, -2, 0, -487], [2, 1, 0, -2, -399],
        [0, 0, 2, -2, -381], [1, 1, 1, 0, 351], [3, 0, -2, 0, -340],
        [4, 0, -3, 0, 330], [2, -1, 2, 0, 327], [0, 2, 1, 0, -323],
        [1, 1, -1, 0, 299], [2, 0, 3, 0, 294]
    ];

    let sumL = 0;
    for (const [d, m, mp, f, coeff] of lonTerms) {
        let c = coeff;
        if (Math.abs(m) === 1) c *= E;
        else if (Math.abs(m) === 2) c *= E2;
        sumL += c * sind(d * D + m * M + mp * Mp + f * F);
    }
    sumL += 3958 * sind(A1) + 1962 * sind(Lp - F) + 318 * sind(A2);

    // Latitude terms (largest 60)
    const latTerms = [
        [0, 0, 0, 1, 5128122], [0, 0, 1, 1, 280602], [0, 0, 1, -1, 277693],
        [2, 0, 0, -1, 173237], [2, 0, -1, 1, 55413], [2, 0, -1, -1, 46271],
        [2, 0, 0, 1, 32573], [0, 0, 2, 1, 17198], [2, 0, 1, -1, 9266],
        [0, 0, 2, -1, 8822], [2, -1, 0, -1, 8216], [2, 0, -2, -1, 4324],
        [2, 0, 1, 1, 4200], [2, 1, 0, -1, -3359], [2, -1, -1, 1, 2463],
        [2, -1, 0, 1, 2211], [2, -1, -1, -1, 2065], [0, 1, -1, -1, -1870],
        [4, 0, -1, -1, 1828], [0, 1, 0, 1, -1794], [0, 0, 0, 3, -1749],
        [0, 1, -1, 1, -1565], [1, 0, 0, 1, -1491], [0, 1, 1, 1, -1475],
        [0, 1, 1, -1, -1410], [0, 1, 0, -1, -1344], [1, 0, 0, -1, -1335],
        [0, 0, 3, 1, 1107], [4, 0, 0, -1, 1021], [4, 0, -1, 1, 833],
        [0, 0, 1, -3, 777], [4, 0, -2, 1, 671], [2, 0, 0, -3, 607],
        [2, 0, 2, -1, 596], [2, -1, 1, -1, 491], [2, 0, -2, 1, -451],
        [0, 0, 3, -1, 439], [2, 0, 2, 1, 422], [2, 0, -3, -1, 421],
        [2, 1, -1, 1, -366], [2, 1, 0, 1, -351], [4, 0, 0, 1, 331],
        [2, -1, 1, 1, 315], [2, -2, 0, -1, 302], [0, 0, 1, 3, -283],
        [2, 1, 1, -1, -229], [1, 1, 0, -1, 223], [1, 1, 0, 1, 223],
        [0, 1, -2, -1, -220], [2, 1, -1, -1, -220], [1, 0, 1, 1, -185],
        [2, -1, -2, -1, 181], [0, 1, 2, 1, -177], [4, 0, -2, -1, 176],
        [4, -1, -1, -1, 166], [1, 0, 1, -1, -164], [4, 0, 1, -1, 132],
        [1, 0, -1, -1, -119], [4, -1, 0, -1, 115], [2, -2, 0, 1, 107]
    ];

    let sumB = 0;
    for (const [d, m, mp, f, coeff] of latTerms) {
        let c = coeff;
        if (Math.abs(m) === 1) c *= E;
        else if (Math.abs(m) === 2) c *= E2;
        sumB += c * sind(d * D + m * M + mp * Mp + f * F);
    }
    sumB += -2235 * sind(Lp) + 382 * sind(A3) + 175 * sind(A1 - F) +
        175 * sind(A1 + F) + 127 * sind(Lp - Mp) - 115 * sind(Lp + Mp);

    // Distance terms (largest)
    const distTerms = [
        [0, 0, 1, 0, -20905355], [2, 0, -1, 0, -3699111], [2, 0, 0, 0, -2955968],
        [0, 0, 2, 0, -569925], [0, 1, 0, 0, 48888], [0, 0, 0, 2, -3149],
        [2, 0, -2, 0, 246158], [2, -1, -1, 0, -152138], [2, 0, 1, 0, -170733],
        [2, -1, 0, 0, -204586], [0, 1, -1, 0, -129620], [1, 0, 0, 0, 108743],
        [0, 1, 1, 0, 104755], [2, 0, 0, -2, 10321], [0, 0, 1, -2, 79661],
        [4, 0, -1, 0, -34782], [0, 0, 3, 0, -23210], [4, 0, -2, 0, -21636],
        [2, 1, -1, 0, 24208], [2, 1, 0, 0, 30824], [1, 0, -1, 0, -8379],
        [1, 1, 0, 0, -16675], [2, -1, 1, 0, -12831]
    ];

    let sumR = 0;
    for (const [d, m, mp, f, coeff] of distTerms) {
        let c = coeff;
        if (Math.abs(m) === 1) c *= E;
        else if (Math.abs(m) === 2) c *= E2;
        sumR += c * cosd(d * D + m * M + mp * Mp + f * F);
    }

    const longitude = norm360(Lp + sumL / 1000000);
    const latitude = sumB / 1000000;
    const distance = 385000.56 + sumR / 1000; // km

    // Moon's mean daily motion ~13.176°
    const speed = 13.176358 + 1.3437 * cosd(Mp) + 0.2136 * cosd(2 * D) +
        0.1154 * cosd(2 * D - Mp);

    return { longitude, latitude, distance, speed, retrograde: false, name: 'Moon' };
}

// ╔══════════════════════════════════════╗
//   PLANETS (Truncated orbital elements)
// ╚══════════════════════════════════════╝

// Mean orbital elements at J2000.0 and rates per century
// From Meeus Table 31.A and JPL
const PLANET_ELEMENTS = {
    Mercury: {
        L: [252.250832, 149472.6746358], a: 0.387098310,
        e: [0.20563593, 0.00001906], i: [7.00497902, -0.00594749],
        O: [48.33076593, -0.12534081], w: [77.45779628, 0.16047689]
    },
    Venus: {
        L: [181.979801, 58517.8156760], a: 0.723329820,
        e: [0.00677672, -0.00004107], i: [3.39467605, -0.00078890],
        O: [76.67984255, -0.27769418], w: [131.60246718, 0.00268329]
    },
    Earth: {
        L: [100.466449, 35999.3728519], a: 1.000001018,
        e: [0.01670862, -0.00004204], i: [0, 0],
        O: [0, 0], w: [102.93768193, 0.32327364]
    },
    Mars: {
        L: [355.433275, 19140.2993313], a: 1.523679342,
        e: [0.09340062, 0.00007882], i: [1.84969142, -0.00813131],
        O: [49.55953891, -0.29257343], w: [336.05637041, 0.44441088]
    },
    Jupiter: {
        L: [34.351484, 3034.9056746], a: 5.202603191,
        e: [0.04849485, 0.00016322], i: [1.30326698, -0.00183714],
        O: [100.46444064, 0.09266985], w: [14.33130930, 0.21852106]
    },
    Saturn: {
        L: [50.077471, 1222.1137943], a: 9.554909596,
        e: [0.05550862, -0.00034664], i: [2.48599187, 0.00193609],
        O: [113.66242448, -0.28867794], w: [93.05678728, 0.56654090]
    },
    Uranus: {
        L: [314.055005, 428.4669983], a: 19.218446062,
        e: [0.04629590, -0.00002729], i: [0.77263783, -0.00242939],
        O: [74.00595590, 0.04240477], w: [173.00529106, 0.09266985]
    },
    Neptune: {
        L: [304.348665, 218.4862002], a: 30.110386869,
        e: [0.00898809, 0.00000603], i: [1.76995259, 0.00035372],
        O: [131.78422574, -0.00508664], w: [48.12027554, 0.00609160]
    }
};

/**
 * Solve Kepler's equation M = E - e*sin(E) iteratively.
 */
function solveKepler(M_deg, e) {
    const Mr = norm360(M_deg) * DEG;
    let E = Mr;
    for (let i = 0; i < 30; i++) {
        const dE = (Mr - E + e * Math.sin(E)) / (1 - e * Math.cos(E));
        E += dE;
        if (Math.abs(dE) < 1e-12) break;
    }
    return E * RAD;
}

/**
 * Compute heliocentric ecliptic coordinates for a planet.
 */
function heliocentricPosition(name, T) {
    const el = PLANET_ELEMENTS[name];
    const L = norm360(el.L[0] + el.L[1] * T);
    const e = el.e[0] + el.e[1] * T;
    const i = el.i[0] + el.i[1] * T;
    const O = norm360(el.O[0] + el.O[1] * T);
    const wBar = norm360(el.w[0] + el.w[1] * T);
    const w = norm360(wBar - O);
    const M = norm360(L - wBar);
    const a = el.a;

    const E = solveKepler(M, e);
    const xp = a * (cosd(E) - e);
    const yp = a * Math.sqrt(1 - e * e) * sind(E);

    const cosO = cosd(O), sinO = sind(O);
    const cosI = cosd(i), sinI = sind(i);
    const cosW = cosd(w), sinW = sind(w);

    const x = (cosO * cosW - sinO * sinW * cosI) * xp + (-cosO * sinW - sinO * cosW * cosI) * yp;
    const y = (sinO * cosW + cosO * sinW * cosI) * xp + (-sinO * sinW + cosO * cosW * cosI) * yp;
    const z = (sinW * sinI) * xp + (cosW * sinI) * yp;

    return { x, y, z };
}

/**
 * Compute geocentric ecliptic longitude/latitude for a planet.
 */
function planetPosition(name, T) {
    const earth = heliocentricPosition('Earth', T);
    const planet = heliocentricPosition(name, T);

    const dx = planet.x - earth.x;
    const dy = planet.y - earth.y;
    const dz = planet.z - earth.z;

    const lon = norm360(atan2d(dy, dx));
    const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
    const lat = asind(dz / dist);

    // Compute speed by finite difference (1 day ~ T+0.0000274)
    const dT = 1 / 36525;
    const earth2 = heliocentricPosition('Earth', T + dT);
    const planet2 = heliocentricPosition(name, T + dT);
    const dx2 = planet2.x - earth2.x;
    const dy2 = planet2.y - earth2.y;
    const lon2 = norm360(atan2d(dy2, dx2));

    let speed = lon2 - lon;
    if (speed > 180) speed -= 360;
    if (speed < -180) speed += 360;

    return {
        longitude: lon, latitude: lat, distance: dist,
        speed, retrograde: speed < 0, name
    };
}

// ╔══════════════════════════════════════╗
//   PLUTO (Meeus Ch. 37 polynomial)
// ╚══════════════════════════════════════╝

function plutoPosition(T) {
    const S = 50.03 + 0.033459652 * 36525 * T;
    const P = 238.95 + 0.003968789 * 36525 * T;

    // Truncated series for longitude, latitude, radius
    const plutoLonTerms = [
        [0, 0, 1, -19799805, 19850055], [0, 0, 2, 897144, -4954829],
        [0, 0, 3, 611149, 1211027], [0, 0, 4, -341243, -189585],
        [0, 0, 5, 129027, -34863], [0, 0, 6, -38215, 31061],
        [0, 1, -1, 20349, -9886], [0, 1, 0, -4045, -4904],
        [0, 1, 1, -5885, -3238], [0, 1, 2, -3812, 3011],
        [0, 1, 3, -601, 3468], [0, 2, -2, 1237, 463],
        [0, 2, -1, 1086, -911], [0, 2, 0, 595, -1229],
        [1, -1, 0, 4814, 5858], [1, -1, 1, 543, -1532],
        [1, 0, -3, -3116, 7055], [1, 0, -2, 18026, 7404],
        [1, 0, -1, -4584, -7825], [1, 0, 0, -6348, 8378],
        [1, 0, 1, -3454, -3613], [1, 0, 2, 1091, -1262],
        [1, 0, 3, -191, 1459], [1, 1, -1, -5765, 3150],
        [1, 1, 0, -2028, 1592], [1, 1, 1, -1528, -178],
        [2, 0, -6, -1606, 1023], [2, 0, -5, 6554, 7335],
        [2, 0, -4, -3127, -6419], [2, 0, -3, -7669, 506],
        [2, 0, -2, 11043, 3813], [2, 0, -1, 5765, -2507],
        [2, 0, 0, -1992, 3650], [3, 0, -2, 4908, 4145],
        [3, 0, -1, -2, 3379], [3, 0, 0, -1271, 1322]
    ];

    let sumL = 0;
    for (const [j, s, p, a, b] of plutoLonTerms) {
        const alpha = j * T * 36525 * 0.0000005 + s * S + p * P; // approximate
        const arg = (s * S + p * P) * DEG;
        sumL += a * Math.sin(arg) + b * Math.cos(arg);
    }

    const lon = norm360(238.958116 + 144.9600 * T + sumL / 1000000);

    // Simplified latitude
    const lat = -3.9082 - 5.453 * Math.sin((238.95 + 0.003968789 * 36525 * T) * DEG);

    // Simplified distance
    const dist = 40.7241346 - 0.12 * Math.cos(P * DEG);

    // Speed ~0.0124°/day
    const speed = 0.01245;

    return { longitude: lon, latitude: lat, distance: dist, speed, retrograde: false, name: 'Pluto' };
}

// ╔══════════════════════════════════════╗
//   LUNAR NODE (Mean)
// ╚══════════════════════════════════════╝

function northNodePosition(T) {
    const lon = norm360(125.0445479 - 1934.1362891 * T + 0.0020754 * T * T + T * T * T / 467441 - T * T * T * T / 60616000);
    return { longitude: lon, latitude: 0, distance: 0, speed: -0.053, retrograde: true, name: 'NorthNode' };
}

function southNodePosition(T) {
    const n = northNodePosition(T);
    return { longitude: norm360(n.longitude + 180), latitude: 0, distance: 0, speed: n.speed, retrograde: true, name: 'SouthNode' };
}

// ╔══════════════════════════════════════╗
//   PUBLIC API
// ╚══════════════════════════════════════╝

const PLANET_FNS = {
    Sun: sunPosition,
    Moon: moonPosition,
    Mercury: (T) => planetPosition('Mercury', T),
    Venus: (T) => planetPosition('Venus', T),
    Mars: (T) => planetPosition('Mars', T),
    Jupiter: (T) => planetPosition('Jupiter', T),
    Saturn: (T) => planetPosition('Saturn', T),
    Uranus: (T) => planetPosition('Uranus', T),
    Neptune: (T) => planetPosition('Neptune', T),
    Pluto: plutoPosition,
    NorthNode: northNodePosition,
    SouthNode: southNodePosition,
};

/**
 * Get the position of a single planet.
 * @param {string} name - Planet name (Sun, Moon, Mercury, etc.)
 * @param {number} jd - Julian Date
 */
export function getPlanetPosition(name, jd) {
    const T = julianCenturies(jd);
    const fn = PLANET_FNS[name];
    if (!fn) throw new Error(`Unknown planet: ${name}`);
    return fn(T);
}

/**
 * Get positions of all planets.
 * @param {number} jd - Julian Date
 * @param {string[]} planets - List of planet names (default: all)
 */
export function getAllPositions(jd, planets) {
    const names = planets || Object.keys(PLANET_FNS);
    const T = julianCenturies(jd);
    const positions = {};
    for (const name of names) {
        positions[name] = PLANET_FNS[name](T);
    }
    return positions;
}

/**
 * Compute Ascendant from LMST and obliquity.
 */
export function computeAscendant(lmstDeg, obliquity, latitude) {
    const eps = obliquity * DEG;
    const phi = latitude * DEG;
    const theta = lmstDeg * DEG;

    const y = -Math.cos(theta);
    const x = Math.sin(theta) * Math.cos(eps) + Math.tan(phi) * Math.sin(eps);
    return norm360(atan2d(y, x));
}

/**
 * Compute Midheaven (MC) from LMST and obliquity.
 */
export function computeMC(lmstDeg, obliquity) {
    const eps = obliquity * DEG;
    const theta = lmstDeg * DEG;
    return norm360(atan2d(Math.sin(theta), Math.cos(theta) * Math.cos(eps)));
}

export { PLANET_FNS };
