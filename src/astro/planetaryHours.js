import { acosd, cosd, norm360, sind, tand } from './utils.js';

export const CHALDEAN_HOUR_SEQUENCE = Object.freeze([
    'Saturn',
    'Jupiter',
    'Mars',
    'Sun',
    'Venus',
    'Mercury',
    'Moon',
]);

export const WEEKDAY_RULERS = Object.freeze([
    'Sun',
    'Moon',
    'Mars',
    'Mercury',
    'Jupiter',
    'Venus',
    'Saturn',
]);

const WEEKDAY_NAMES = Object.freeze([
    'Sunday',
    'Monday',
    'Tuesday',
    'Wednesday',
    'Thursday',
    'Friday',
    'Saturday',
]);

export function calculatePlanetaryHour({
    date,
    lat,
    lng,
    solarEventsProvider = solarEventsForLocalDate,
}) {
    if (!(date instanceof Date) || !Number.isFinite(date.getTime())) {
        return unavailable('Chart date unavailable');
    }
    if (!Number.isFinite(lat) || !Number.isFinite(lng)) {
        return unavailable('Coordinates unavailable');
    }

    const localDate = localMeanDateParts(date, lng);
    const today = solarEventsProvider(localDate, lat, lng);
    const previousDate = addLocalDays(localDate, -1);
    const nextDate = addLocalDays(localDate, 1);
    const previous = solarEventsProvider(previousDate, lat, lng);
    const next = solarEventsProvider(nextDate, lat, lng);

    if (!today.available || !previous.available || !next.available) {
        return unavailable(today.reason || previous.reason || next.reason || 'Solar events unavailable');
    }

    if (date >= today.sunrise && date < today.sunset) {
        return buildPlanetaryHourResult({
            date,
            planetaryDate: localDate,
            period: 'day',
            periodStart: today.sunrise,
            periodEnd: today.sunset,
            sunrise: today.sunrise,
            sunset: today.sunset,
            hourOffset: 0,
        });
    }

    if (date >= today.sunset) {
        return buildPlanetaryHourResult({
            date,
            planetaryDate: localDate,
            period: 'night',
            periodStart: today.sunset,
            periodEnd: next.sunrise,
            sunrise: today.sunrise,
            sunset: today.sunset,
            hourOffset: 12,
        });
    }

    return buildPlanetaryHourResult({
        date,
        planetaryDate: previousDate,
        period: 'night',
        periodStart: previous.sunset,
        periodEnd: today.sunrise,
        sunrise: today.sunrise,
        sunset: today.sunset,
        hourOffset: 12,
    });
}

function buildPlanetaryHourResult({
    date,
    planetaryDate,
    period,
    periodStart,
    periodEnd,
    sunrise,
    sunset,
    hourOffset,
}) {
    const periodMs = periodEnd.getTime() - periodStart.getTime();
    if (periodMs <= 0) return unavailable('Invalid planetary hour interval');

    const planetaryDayIndex = weekdayIndex(planetaryDate);
    const planetaryDayRuler = WEEKDAY_RULERS[planetaryDayIndex];
    const hourLengthMs = periodMs / 12;
    const periodHourIndex = clamp(Math.floor((date.getTime() - periodStart.getTime()) / hourLengthMs), 0, 11);
    const ruler = planetaryHourRuler(planetaryDayRuler, hourOffset + periodHourIndex);

    return {
        available: true,
        period,
        hourNumber: periodHourIndex + 1,
        planetaryDay: WEEKDAY_NAMES[planetaryDayIndex],
        planetaryDayRuler,
        planetaryHourRuler: ruler,
        startsAtUtc: new Date(periodStart.getTime() + periodHourIndex * hourLengthMs).toISOString(),
        endsAtUtc: new Date(periodStart.getTime() + (periodHourIndex + 1) * hourLengthMs).toISOString(),
        sunriseUtc: sunrise.toISOString(),
        sunsetUtc: sunset.toISOString(),
        method: 'NOAA sunrise/sunset with local mean solar date from longitude',
    };
}

export function planetaryHourRuler(dayRuler, hourOffset) {
    const startIndex = CHALDEAN_HOUR_SEQUENCE.indexOf(dayRuler);
    if (startIndex === -1) return null;
    return CHALDEAN_HOUR_SEQUENCE[(startIndex + hourOffset) % CHALDEAN_HOUR_SEQUENCE.length];
}

export function solarEventsForLocalDate(localDate, lat, lng) {
    const sunrise = solarEventUtc(localDate, lat, lng, 'sunrise');
    const sunset = solarEventUtc(localDate, lat, lng, 'sunset');
    if (!sunrise || !sunset) {
        return {
            available: false,
            reason: 'Sunrise or sunset unavailable at this latitude/date',
        };
    }

    return {
        available: true,
        sunrise,
        sunset,
    };
}

function solarEventUtc(localDate, lat, lng, event) {
    const dayOfYear = localDayOfYear(localDate);
    const lngHour = lng / 15;
    const approximateHour = event === 'sunrise' ? 6 : 18;
    const t = dayOfYear + ((approximateHour - lngHour) / 24);
    const meanAnomaly = (0.9856 * t) - 3.289;
    const trueLongitude = norm360(
        meanAnomaly
        + (1.916 * sind(meanAnomaly))
        + (0.020 * sind(2 * meanAnomaly))
        + 282.634
    );
    let rightAscension = atanHours(0.91764 * tand(trueLongitude));
    const longitudeQuadrant = Math.floor(trueLongitude / 90) * 90;
    const raQuadrant = Math.floor((rightAscension * 15) / 90) * 90;
    rightAscension = (rightAscension + (longitudeQuadrant - raQuadrant) / 15) % 24;
    if (rightAscension < 0) rightAscension += 24;

    const sinDeclination = 0.39782 * sind(trueLongitude);
    const cosDeclination = Math.cos(Math.asin(sinDeclination));
    const zenith = 90.833;
    const cosHourAngle = (cosd(zenith) - (sinDeclination * sind(lat))) / (cosDeclination * cosd(lat));
    if (cosHourAngle > 1 || cosHourAngle < -1) return null;

    const hourAngle = event === 'sunrise'
        ? (360 - acosd(cosHourAngle)) / 15
        : acosd(cosHourAngle) / 15;
    const localMeanTime = hourAngle + rightAscension - (0.06571 * t) - 6.622;
    const utcHour = ((localMeanTime - lngHour) % 24 + 24) % 24;

    return utcEventForLocalDate(localDate, utcHour, lng);
}

function atanHours(value) {
    return norm360(Math.atan(value) * 180 / Math.PI) / 15;
}

function utcEventForLocalDate(localDate, utcHour, lng) {
    let utcMs = Date.UTC(localDate.year, localDate.month - 1, localDate.day) + utcHour * 60 * 60 * 1000;
    const targetKey = localDateKey(localDate);

    for (let i = 0; i < 3; i++) {
        const meanDate = localMeanDateParts(new Date(utcMs), lng);
        const comparison = localDateKey(meanDate).localeCompare(targetKey);
        if (comparison === 0) break;
        utcMs += comparison < 0 ? 86400000 : -86400000;
    }

    return new Date(utcMs);
}

function localMeanDateParts(date, lng) {
    const shifted = new Date(date.getTime() + (lng / 15) * 60 * 60 * 1000);
    return {
        year: shifted.getUTCFullYear(),
        month: shifted.getUTCMonth() + 1,
        day: shifted.getUTCDate(),
    };
}

function addLocalDays(localDate, days) {
    const date = new Date(Date.UTC(localDate.year, localDate.month - 1, localDate.day + days));
    return {
        year: date.getUTCFullYear(),
        month: date.getUTCMonth() + 1,
        day: date.getUTCDate(),
    };
}

function localDayOfYear(localDate) {
    const start = Date.UTC(localDate.year, 0, 1);
    const current = Date.UTC(localDate.year, localDate.month - 1, localDate.day);
    return Math.floor((current - start) / 86400000) + 1;
}

function weekdayIndex(localDate) {
    return new Date(Date.UTC(localDate.year, localDate.month - 1, localDate.day)).getUTCDay();
}

function localDateKey(localDate) {
    return [
        String(localDate.year).padStart(4, '0'),
        String(localDate.month).padStart(2, '0'),
        String(localDate.day).padStart(2, '0'),
    ].join('-');
}

function unavailable(reason) {
    return {
        available: false,
        reason,
    };
}

function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
}
