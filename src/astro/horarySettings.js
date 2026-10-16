/**
 * Default horary calculation settings.
 *
 * Keep configurable calculation policy here so UI, chart generation, and tests
 * do not encode separate copies of aspect or house defaults.
 */

export const ASPECT_DEFINITIONS = Object.freeze({
    conjunction: Object.freeze({ angle: 0, symbol: '☌', name: 'Conjunction', major: true, color: '#c9a84c' }),
    sextile: Object.freeze({ angle: 60, symbol: '⚹', name: 'Sextile', major: true, color: '#4ca8c9' }),
    square: Object.freeze({ angle: 90, symbol: '□', name: 'Square', major: true, color: '#c94c4c' }),
    trine: Object.freeze({ angle: 120, symbol: '△', name: 'Trine', major: true, color: '#4cc94c' }),
    opposition: Object.freeze({ angle: 180, symbol: '☍', name: 'Opposition', major: true, color: '#c94c8a' }),
    semisextile: Object.freeze({ angle: 30, symbol: '⚺', name: 'Semi-sextile', major: false, color: '#8a8a8a' }),
    semisquare: Object.freeze({ angle: 45, symbol: '∠', name: 'Semi-square', major: false, color: '#8a6060' }),
    sesquiquadrate: Object.freeze({ angle: 135, symbol: '⚼', name: 'Sesquiquadrate', major: false, color: '#8a4040' }),
    quincunx: Object.freeze({ angle: 150, symbol: '⚻', name: 'Quincunx', major: false, color: '#606060' }),
    quintile: Object.freeze({ angle: 72, symbol: 'Q', name: 'Quintile', major: false, color: '#a070c0' }),
    biquintile: Object.freeze({ angle: 144, symbol: 'bQ', name: 'Bi-quintile', major: false, color: '#c070a0' }),
});

export const DEFAULT_ASPECT_ORBS = Object.freeze({
    Sun: Object.freeze({ conjunction: 10, sextile: 6, square: 8, trine: 8, opposition: 10, semisextile: 2, semisquare: 2, sesquiquadrate: 2, quincunx: 3, quintile: 2, biquintile: 2 }),
    Moon: Object.freeze({ conjunction: 10, sextile: 6, square: 8, trine: 8, opposition: 10, semisextile: 2, semisquare: 2, sesquiquadrate: 2, quincunx: 3, quintile: 2, biquintile: 2 }),
    default: Object.freeze({ conjunction: 8, sextile: 5, square: 7, trine: 7, opposition: 8, semisextile: 1.5, semisquare: 1.5, sesquiquadrate: 1.5, quincunx: 2, quintile: 1, biquintile: 1 }),
    Uranus: Object.freeze({ conjunction: 5, sextile: 3, square: 5, trine: 5, opposition: 5, semisextile: 1, semisquare: 1, sesquiquadrate: 1, quincunx: 2, quintile: 1, biquintile: 1 }),
    Neptune: Object.freeze({ conjunction: 5, sextile: 3, square: 5, trine: 5, opposition: 5, semisextile: 1, semisquare: 1, sesquiquadrate: 1, quincunx: 2, quintile: 1, biquintile: 1 }),
    Pluto: Object.freeze({ conjunction: 5, sextile: 3, square: 5, trine: 5, opposition: 5, semisextile: 1, semisquare: 1, sesquiquadrate: 1, quincunx: 2, quintile: 1, biquintile: 1 }),
});

function buildAspectSettings() {
    return Object.freeze(Object.fromEntries(
        Object.entries(ASPECT_DEFINITIONS).map(([key, aspect]) => [
            key,
            Object.freeze({
                enabled: true,
                degree: aspect.angle,
                orb: DEFAULT_ASPECT_ORBS.default[key],
                color: aspect.color,
                major: aspect.major,
                symbol: aspect.symbol,
                name: aspect.name,
            }),
        ])
    ));
}

export const DEFAULT_HORARY_SETTINGS = Object.freeze({
    houseSystem: 'regiomontanus',
    zodiac: 'tropical',
    planetSet: 'modern',
    aspectPhaseEpsilonDays: 1 / 24,
    aspects: buildAspectSettings(),
});
