/**
 * Time conversions for astronomical calculations.
 * All algorithms from Meeus, "Astronomical Algorithms" 2nd ed.
 */

/**
 * Calendar date → Julian Date.
 * @param {number} year - Full year (e.g. 2024)
 * @param {number} month - Month 1-12
 * @param {number} day - Day with fractional part for time (e.g. 1.5 = noon Jan 1)
 */
export function julianDate(year, month, day) {
    if (month <= 2) { year--; month += 12; }
    const A = Math.floor(year / 100);
    const B = 2 - A + Math.floor(A / 4);
    return Math.floor(365.25 * (year + 4716)) +
        Math.floor(30.6001 * (month + 1)) +
        day + B - 1524.5;
}

/**
 * Date object → Julian Date (UT)
 */
export function dateToJD(date) {
    return julianDate(
        date.getUTCFullYear(),
        date.getUTCMonth() + 1,
        date.getUTCDate() +
        date.getUTCHours() / 24 +
        date.getUTCMinutes() / 1440 +
        date.getUTCSeconds() / 86400 +
        date.getUTCMilliseconds() / 86400000
    );
}

/**
 * JD → Date object
 */
export function jdToDate(jd) {
    const z = Math.floor(jd + 0.5);
    const f = jd + 0.5 - z;
    let A;
    if (z < 2299161) {
        A = z;
    } else {
        const alpha = Math.floor((z - 1867216.25) / 36524.25);
        A = z + 1 + alpha - Math.floor(alpha / 4);
    }
    const B = A + 1524;
    const C = Math.floor((B - 122.1) / 365.25);
    const D = Math.floor(365.25 * C);
    const E = Math.floor((B - D) / 30.6001);

    const day = B - D - Math.floor(30.6001 * E) + f;
    const month = E < 14 ? E - 1 : E - 13;
    const year = month > 2 ? C - 4716 : C - 4715;

    const d = Math.floor(day);
    const frac = day - d;
    const h = Math.floor(frac * 24);
    const m = Math.floor((frac * 24 - h) * 60);
    const s = Math.round(((frac * 24 - h) * 60 - m) * 60);

    return new Date(Date.UTC(year, month - 1, d, h, m, s));
}

/**
 * Julian centuries from J2000.0 epoch (JD 2451545.0)
 */
export function julianCenturies(jd) {
    return (jd - 2451545.0) / 36525.0;
}

/**
 * Delta-T approximation (TT - UT) in seconds.
 * Polynomial fits from Espenak & Meeus.
 */
export function deltaT(year) {
    const y = year;
    if (y < -500) {
        const u = (y - 1820) / 100;
        return -20 + 32 * u * u;
    } else if (y < 500) {
        const u = y / 100;
        return 10583.6 + u * (-1014.41 + u * (33.78311 + u * (-5.952053 + u * (-0.1798452 + u * (0.022174192 + u * 0.0090316521)))));
    } else if (y < 1600) {
        const u = (y - 1000) / 100;
        return 1574.2 + u * (-556.01 + u * (71.23472 + u * (0.319781 + u * (-0.8503463 + u * (-0.005050998 + u * 0.0083572073)))));
    } else if (y < 1700) {
        const t = y - 1600;
        return 120 + t * (-0.9808 + t * (-0.01532 + t * 1 / 7129));
    } else if (y < 1800) {
        const t = y - 1700;
        return 8.83 + t * (0.1603 + t * (-0.0059285 + t * (0.00013336 + t * (-1 / 1174000))));
    } else if (y < 1860) {
        const t = y - 1800;
        return 13.72 + t * (-0.332447 + t * (0.0068612 + t * (0.0041116 + t * (-0.00037436 + t * (0.0000121272 + t * (-0.0000001699 + t * 0.000000000875))))));
    } else if (y < 1900) {
        const t = y - 1860;
        return 7.62 + t * (0.5737 + t * (-0.251754 + t * (0.01680668 + t * (-0.0004473624 + t * (1 / 233174)))));
    } else if (y < 1920) {
        const t = y - 1900;
        return -2.79 + t * (1.494119 + t * (-0.0598939 + t * (0.0061966 + t * (-0.000197))));
    } else if (y < 1941) {
        const t = y - 1920;
        return 21.20 + t * (0.84493 + t * (-0.076100 + t * 0.0020936));
    } else if (y < 1961) {
        const t = y - 1950;
        return 29.07 + t * (0.407 + t * (-1 / 233 + t * (1 / 2547)));
    } else if (y < 1986) {
        const t = y - 1975;
        return 45.45 + t * (1.067 + t * (-1 / 260 + t * (-1 / 718)));
    } else if (y < 2005) {
        const t = y - 2000;
        return 63.86 + t * (0.3345 + t * (-0.060374 + t * (0.0017275 + t * (0.000651814 + t * 0.00002373599))));
    } else if (y < 2050) {
        const t = y - 2000;
        return 62.92 + t * (0.32217 + t * 0.005589);
    } else if (y < 2150) {
        return -20 + 32 * ((y - 1820) / 100) ** 2 - 0.5628 * (2150 - y);
    } else {
        const u = (y - 1820) / 100;
        return -20 + 32 * u * u;
    }
}

/**
 * Greenwich Mean Sidereal Time in degrees.
 * @param {number} jd - Julian Date (UT)
 */
export function gmst(jd) {
    const T = julianCenturies(jd);
    // IAU 1982 formula (Meeus eq. 12.4)
    let st = 280.46061837 +
        360.98564736629 * (jd - 2451545.0) +
        0.000387933 * T * T -
        T * T * T / 38710000;
    return ((st % 360) + 360) % 360;
}

/**
 * Local Mean Sidereal Time in degrees.
 * @param {number} jd - Julian Date (UT) 
 * @param {number} longitude - Geographic longitude in degrees (east positive)
 */
export function lmst(jd, longitude) {
    return ((gmst(jd) + longitude) % 360 + 360) % 360;
}

/**
 * Mean obliquity of the ecliptic in degrees.
 * @param {number} T - Julian centuries from J2000.0
 */
export function meanObliquity(T) {
    // IAU formula (Meeus eq. 22.2)
    const U = T / 100;
    return 23.439291111 +
        U * (-1.300258333 +
            U * (-0.000430556 +
                U * (0.555347222 +
                    U * (-0.014272222 +
                        U * (-0.069352778 +
                            U * (-0.010847222 +
                                U * (0.001977778 +
                                    U * (0.007741667 +
                                        U * (0.001608333 +
                                            U * (-0.000680556))))))))));
}

/**
 * Nutation in longitude (Δψ) and obliquity (Δε) in degrees.
 * Simplified model using 5 largest terms.
 */
export function nutation(T) {
    const D = 297.85036 + 445267.11148 * T - 0.0019142 * T * T + T * T * T / 189474;
    const M = 357.52772 + 35999.05034 * T - 0.0001603 * T * T - T * T * T / 300000;
    const Mp = 134.96298 + 477198.867398 * T + 0.0086972 * T * T + T * T * T / 56250;
    const F = 93.27191 + 483202.017538 * T - 0.0036825 * T * T + T * T * T / 327270;
    const O = 125.04452 - 1934.136261 * T + 0.0020708 * T * T + T * T * T / 450000;

    const rad = Math.PI / 180;
    const dPsi = (-17.20 / 3600) * Math.sin(O * rad) +
        (-1.32 / 3600) * Math.sin(2 * (280.4665 + 36000.7698 * T) * rad) +
        (-0.23 / 3600) * Math.sin(2 * Mp * rad) +
        (0.21 / 3600) * Math.sin(2 * O * rad);

    const dEps = (9.20 / 3600) * Math.cos(O * rad) +
        (0.57 / 3600) * Math.cos(2 * (280.4665 + 36000.7698 * T) * rad) +
        (0.10 / 3600) * Math.cos(2 * Mp * rad) +
        (-0.09 / 3600) * Math.cos(2 * O * rad);

    return { dPsi, dEps };
}

/**
 * Apparent sidereal time (includes nutation correction).
 */
export function apparentSiderealTime(jd) {
    const T = julianCenturies(jd);
    const g = gmst(jd);
    const { dPsi } = nutation(T);
    const eps = meanObliquity(T) + nutation(T).dEps;
    return g + dPsi * Math.cos(eps * Math.PI / 180);
}
