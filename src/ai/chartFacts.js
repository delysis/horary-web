import { signRuler } from '../astro/dignities.js';
import { norm360, signInfo } from '../astro/utils.js';

export function buildHoraryChartFacts(chart, options = {}) {
    const castDate = chart.date instanceof Date ? chart.date : null;
    const locationLabel = options.locationLabel || `${formatNumber(chart.lat, 4)}, ${formatNumber(chart.lng, 4)}`;
    const timezone = chartFactTimezone(chart, options);

    return {
        castLocalTime: chartFactLocalTime(chart, options, { castDate, timezone }),
        castUtcTime: castDate ? castDate.toISOString() : null,
        timezone,
        location: {
            label: locationLabel,
            latitude: round(chart.lat, 6),
            longitude: round(chart.lng, 6),
        },
        ascendant: angleFact(chart.houses.asc),
        midheaven: angleFact(chart.houses.mc),
        houses: buildHouseFacts(chart),
        bodies: buildBodyFacts(chart),
        aspects: buildAspectFacts(chart),
        derived: {
            zodiac: chart.zodiac || 'tropical',
            houseSystem: chart.houseSystem || 'regiomontanus',
            chartSect: chart.dayChart ? 'day' : 'night',
            ascendantRuler: signRuler(signInfo(chart.houses.asc).index),
            partOfFortune: lotFact(chart.lots?.partOfFortune),
            accidentalDignities: buildAccidentalDignityFacts(chart),
            receptions: buildReceptionFacts(chart),
            voidOfCourseMoon: voidOfCourseFact(chart.voidOfCourseMoon),
            antisciaContacts: buildAntisciaFacts(chart),
            solarConditions: buildSolarConditionFacts(chart),
            planetaryHour: planetaryHourFact(chart.planetaryHour),
            timingPatterns: buildTimingPatternFacts(chart),
        },
    };
}

function chartFactTimezone(chart, options) {
    if (options.timezone) return options.timezone;
    if (chart.timezone) return chart.timezone;
    if (chart.time?.timezone) return chart.time.timezone;

    const offsetHours = firstFinite(
        options.offsetHours,
        chart.input?.offsetHours,
        chart.offsetHours,
        chart.time?.offsetHours,
    );
    return Number.isFinite(offsetHours) ? formatUtcOffset(offsetHours) : 'UTC';
}

function chartFactLocalTime(chart, options, { castDate, timezone }) {
    if (options.localTime) return options.localTime;
    if (chart.localTime) return chart.localTime;
    if (chart.time?.local) return chart.time.local;
    if (chart.input?.localDate && chart.input?.localTime) {
        return `${chart.input.localDate} ${chart.input.localTime} ${timezone}`;
    }
    return castDate ? castDate.toISOString() : null;
}

function buildHouseFacts(chart) {
    return chart.houses.cusps.map((longitude, index) => {
        const info = signInfo(longitude);
        return {
            number: index + 1,
            sign: info.name,
            glyph: info.glyph,
            degree: round(info.totalDegrees, 4),
            absoluteLongitude: round(norm360(longitude), 4),
            ruler: signRuler(info.index),
        };
    });
}

function buildBodyFacts(chart) {
    return Object.entries(chart.positions)
        .filter(([name]) => name !== 'SouthNode')
        .map(([name, position]) => {
            const info = signInfo(position.longitude);
            return {
                name,
                sign: info.name,
                glyph: info.glyph,
                degree: round(info.totalDegrees, 4),
                absoluteLongitude: round(norm360(position.longitude), 4),
                house: position.house || null,
                retrograde: Boolean(position.retrograde),
                speed: typeof position.speed === 'number' ? round(position.speed, 6) : null,
                dignity: summarizeDignity(chart.dignities?.[name]),
                accidentalDignity: summarizeAccidentalDignity(chart.accidentalDignities?.[name]),
            };
        });
}

function buildAspectFacts(chart) {
    return chart.aspects.map(aspect => ({
        planet1: aspect.planet1,
        aspectName: aspect.aspectName,
        planet2: aspect.planet2,
        orb: round(aspect.orb, 4),
        applying: aspect.applying === true,
        separating: aspect.separating === true,
        exact: aspect.exact === true,
        major: aspect.major === true,
    }));
}

function buildReceptionFacts(chart) {
    return (chart.receptions || []).map(reception => ({
        hostPlanet: reception.hostPlanet,
        guestPlanet: reception.guestPlanet,
        dignity: reception.dignity,
        mutual: reception.mutual === true,
    }));
}

function buildAntisciaFacts(chart) {
    return (chart.antisciaContacts || []).map(contact => ({
        planet1: contact.planet1,
        planet2: contact.planet2,
        type: contact.type,
        orb: round(contact.orb, 4),
    }));
}

function buildSolarConditionFacts(chart) {
    return (chart.solarConditions || []).map(condition => ({
        planet: condition.planet,
        condition: condition.condition,
        separationDegrees: round(condition.separationDegrees, 4),
    }));
}

function buildAccidentalDignityFacts(chart) {
    return Object.fromEntries(Object.entries(chart.accidentalDignities || {}).map(([planet, dignity]) => [
        planet,
        summarizeAccidentalDignity(dignity),
    ]));
}

function buildTimingPatternFacts(chart) {
    return (chart.timingPatterns || []).map(pattern => ({
        ...pattern,
    }));
}

function angleFact(longitude) {
    const info = signInfo(longitude);
    return {
        sign: info.name,
        glyph: info.glyph,
        degree: round(info.totalDegrees, 4),
        absoluteLongitude: round(norm360(longitude), 4),
    };
}

function lotFact(lot) {
    if (!lot) return null;
    const info = signInfo(lot.longitude);
    return {
        name: lot.name,
        sign: info.name,
        glyph: info.glyph,
        degree: round(info.totalDegrees, 4),
        absoluteLongitude: round(norm360(lot.longitude), 4),
        house: lot.house || null,
        formula: lot.formula,
    };
}

function voidOfCourseFact(voidOfCourseMoon) {
    if (!voidOfCourseMoon) return null;
    return {
        available: voidOfCourseMoon.available === true,
        isVoid: voidOfCourseMoon.isVoid,
        checkedUntilHours: voidOfCourseMoon.checkedUntilHours ?? null,
        nextApplyingAspect: voidOfCourseMoon.nextApplyingAspect || null,
        reason: voidOfCourseMoon.reason || null,
    };
}

function planetaryHourFact(planetaryHour) {
    if (!planetaryHour) return null;
    return {
        available: planetaryHour.available === true,
        period: planetaryHour.period || null,
        hourNumber: planetaryHour.hourNumber || null,
        planetaryDay: planetaryHour.planetaryDay || null,
        planetaryDayRuler: planetaryHour.planetaryDayRuler || null,
        planetaryHourRuler: planetaryHour.planetaryHourRuler || null,
        startsAtUtc: planetaryHour.startsAtUtc || null,
        endsAtUtc: planetaryHour.endsAtUtc || null,
        method: planetaryHour.method || null,
        reason: planetaryHour.reason || null,
    };
}

function summarizeDignity(dignity) {
    if (!dignity) return null;
    return {
        domicile: Boolean(dignity.domicile),
        exaltation: Boolean(dignity.exaltation),
        triplicityRuler: Boolean(dignity.triplicityRuler),
        termRuler: Boolean(dignity.termRuler),
        faceRuler: Boolean(dignity.faceRuler),
        detriment: Boolean(dignity.detriment),
        fall: Boolean(dignity.fall),
        peregrine: Boolean(dignity.peregrine),
        score: dignity.score,
    };
}

function summarizeAccidentalDignity(dignity) {
    if (!dignity) return null;
    return {
        house: dignity.house || null,
        angularity: dignity.angularity || null,
        retrograde: Boolean(dignity.retrograde),
        solarCondition: dignity.solarCondition || null,
        inJoy: Boolean(dignity.inJoy),
        joyHouse: dignity.joyHouse || null,
        score: dignity.score,
        factors: (dignity.factors || []).map(factor => ({
            type: factor.type,
            label: factor.label,
            score: factor.score,
        })),
    };
}

function round(value, precision = 4) {
    return Number(value.toFixed(precision));
}

function formatNumber(value, precision) {
    return Number.isFinite(value) ? value.toFixed(precision) : '';
}

function firstFinite(...values) {
    return values.find(value => Number.isFinite(value));
}

export function formatUtcOffset(offsetHours) {
    if (!Number.isFinite(offsetHours)) return 'UTC';
    const sign = offsetHours < 0 ? '-' : '+';
    const totalMinutes = Math.round(Math.abs(offsetHours) * 60);
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return `UTC${sign}${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}`;
}
