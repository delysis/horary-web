/** Astrology utility functions */

export const DEG = Math.PI / 180;
export const RAD = 180 / Math.PI;

/** Normalize angle to 0–360 */
export function norm360(a) {
    return ((a % 360) + 360) % 360;
}

/** Normalize angle to -180–180 */
export function norm180(a) {
    let v = norm360(a);
    return v > 180 ? v - 360 : v;
}

/** sin in degrees */
export function sind(d) { return Math.sin(d * DEG); }
/** cos in degrees */
export function cosd(d) { return Math.cos(d * DEG); }
/** tan in degrees */
export function tand(d) { return Math.tan(d * DEG); }
/** asin returning degrees */
export function asind(x) { return Math.asin(Math.max(-1, Math.min(1, x))) * RAD; }
/** acos returning degrees */
export function acosd(x) { return Math.acos(Math.max(-1, Math.min(1, x))) * RAD; }
/** atan2 returning degrees */
export function atan2d(y, x) { return Math.atan2(y, x) * RAD; }

/** Decimal degrees → { degrees, minutes, seconds, sign } */
export function dms(dd) {
    const neg = dd < 0;
    dd = Math.abs(dd);
    const d = Math.floor(dd);
    const m = Math.floor((dd - d) * 60);
    const s = Math.round(((dd - d) * 60 - m) * 60);
    return { degrees: d, minutes: m, seconds: s, sign: neg ? -1 : 1 };
}

/** DMS → decimal degrees */
export function fromDms(d, m, s, sign = 1) {
    return sign * (Math.abs(d) + m / 60 + s / 3600);
}

const SIGN_NAMES = [
    'Aries', 'Taurus', 'Gemini', 'Cancer', 'Leo', 'Virgo',
    'Libra', 'Scorpio', 'Sagittarius', 'Capricorn', 'Aquarius', 'Pisces'
];

const SIGN_GLYPHS = ['♈', '♉', '♊', '♋', '♌', '♍', '♎', '♏', '♐', '♑', '♒', '♓'];

const SIGN_ELEMENTS = [
    'fire', 'earth', 'air', 'water', 'fire', 'earth',
    'air', 'water', 'fire', 'earth', 'air', 'water'
];

const SIGN_MODALITIES = [
    'cardinal', 'fixed', 'mutable', 'cardinal', 'fixed', 'mutable',
    'cardinal', 'fixed', 'mutable', 'cardinal', 'fixed', 'mutable'
];

/** Get zodiac sign index (0-11) from ecliptic longitude */
export function signIndex(lon) {
    return Math.floor(norm360(lon) / 30);
}

/** Get sign info from ecliptic longitude */
export function signInfo(lon) {
    const l = norm360(lon);
    const idx = Math.floor(l / 30);
    const degInSign = l - idx * 30;
    const d = dms(degInSign);
    return {
        index: idx,
        name: SIGN_NAMES[idx],
        glyph: SIGN_GLYPHS[idx],
        element: SIGN_ELEMENTS[idx],
        modality: SIGN_MODALITIES[idx],
        degree: d.degrees,
        minutes: d.minutes,
        seconds: d.seconds,
        totalDegrees: degInSign,
        longitude: l
    };
}

/** Format longitude as "15° ♈ 23'" */
export function fmtLon(lon) {
    const info = signInfo(lon);
    return `${info.degree}° ${info.glyph} ${String(info.minutes).padStart(2, '0')}'`;
}

/** Planet glyphs */
export const PLANET_GLYPHS = {
    Sun: '☉', Moon: '☽', Mercury: '☿', Venus: '♀', Mars: '♂',
    Jupiter: '♃', Saturn: '♄', Uranus: '♅', Neptune: '♆', Pluto: '♇',
    NorthNode: '☊', SouthNode: '☋', Chiron: '⚷'
};

export const PLANET_NAMES = Object.keys(PLANET_GLYPHS);
export const CLASSICAL_PLANETS = ['Sun', 'Moon', 'Mercury', 'Venus', 'Mars', 'Jupiter', 'Saturn'];

export { SIGN_NAMES, SIGN_GLYPHS, SIGN_ELEMENTS, SIGN_MODALITIES };
