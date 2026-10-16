const DATE_RE = /^(\d{4})-(\d{2})-(\d{2})$/;
const TIME_RE = /^(\d{2}):(\d{2})$/;

export const YEAR_MIN = 1600;
export const YEAR_MAX = 2399;
export const OFFSET_MIN = -14;
export const OFFSET_MAX = 14;

function blank(value) {
    return value == null || String(value).trim() === '';
}

function fieldError(field, message) {
    return { field, message };
}

function parseIsoDate(value) {
    if (blank(value)) {
        return { ok: false, error: fieldError('date', 'Date is required.') };
    }

    const match = DATE_RE.exec(String(value).trim());
    if (!match) {
        return { ok: false, error: fieldError('date', 'Use a valid date in YYYY-MM-DD format.') };
    }

    const year = Number(match[1]);
    const month = Number(match[2]);
    const day = Number(match[3]);

    if (year < YEAR_MIN || year > YEAR_MAX) {
        return { ok: false, error: fieldError('date', `Year must be between ${YEAR_MIN} and ${YEAR_MAX}.`) };
    }

    const utc = new Date(Date.UTC(year, month - 1, day));
    const valid =
        utc.getUTCFullYear() === year &&
        utc.getUTCMonth() === month - 1 &&
        utc.getUTCDate() === day;

    if (!valid) {
        return { ok: false, error: fieldError('date', 'Use a real calendar date.') };
    }

    return { ok: true, value: { year, month, day } };
}

function parseLocalTime(value) {
    if (blank(value)) {
        return { ok: false, error: fieldError('time', 'Time is required.') };
    }

    const match = TIME_RE.exec(String(value).trim());
    if (!match) {
        return { ok: false, error: fieldError('time', 'Use a valid time in HH:MM format.') };
    }

    const hour = Number(match[1]);
    const minute = Number(match[2]);

    if (hour < 0 || hour > 23) {
        return { ok: false, error: fieldError('time', 'Hour must be between 0 and 23.') };
    }

    if (minute < 0 || minute > 59) {
        return { ok: false, error: fieldError('time', 'Minute must be between 0 and 59.') };
    }

    return { ok: true, value: { hour, minute } };
}

function parseNumber(value, field, label, min, max) {
    if (blank(value)) {
        return { ok: false, error: fieldError(field, `${label} is required.`) };
    }

    const number = Number(String(value).trim());
    if (!Number.isFinite(number)) {
        return { ok: false, error: fieldError(field, `${label} must be a number.`) };
    }

    if (number < min || number > max) {
        return { ok: false, error: fieldError(field, `${label} must be between ${min} and ${max}.`) };
    }

    return { ok: true, value: number };
}

export function localDateTimeToUtcDate(dateParts, timeParts, offsetHours) {
    const utcMs = Date.UTC(
        dateParts.year,
        dateParts.month - 1,
        dateParts.day,
        timeParts.hour,
        timeParts.minute,
        0,
        0
    ) - offsetHours * 60 * 60 * 1000;

    return new Date(utcMs);
}

export function validateCoordinates(raw) {
    const errors = [];
    const lat = parseNumber(raw.lat, 'lat', 'Latitude', -90, 90);
    const lng = parseNumber(raw.lng, 'lng', 'Longitude', -180, 180);

    if (!lat.ok) errors.push(lat.error);
    if (!lng.ok) errors.push(lng.error);

    if (errors.length > 0) {
        return { ok: false, errors };
    }

    return {
        ok: true,
        value: {
            latitude: lat.value,
            longitude: lng.value,
        },
    };
}

export function validateChartInput(raw, options = {}) {
    const { allowFuture = false, now = new Date() } = options;
    const errors = [];

    const date = parseIsoDate(raw.date);
    const time = parseLocalTime(raw.time);
    const offset = parseNumber(raw.offset ?? '0', 'offset', 'UTC offset', OFFSET_MIN, OFFSET_MAX);
    const coordinates = validateCoordinates({ lat: raw.lat, lng: raw.lng });

    if (!date.ok) errors.push(date.error);
    if (!time.ok) errors.push(time.error);
    if (!offset.ok) errors.push(offset.error);
    if (!coordinates.ok) errors.push(...coordinates.errors);

    if (errors.length > 0) {
        return { ok: false, errors };
    }

    const utcDate = localDateTimeToUtcDate(date.value, time.value, offset.value);
    if (!allowFuture && utcDate.getTime() > now.getTime()) {
        return {
            ok: false,
            errors: [fieldError('date', 'The chart date and time cannot be in the future.')],
        };
    }

    return {
        ok: true,
        value: {
            localDate: String(raw.date).trim(),
            localTime: String(raw.time).trim(),
            offsetHours: offset.value,
            latitude: coordinates.value.latitude,
            longitude: coordinates.value.longitude,
            utcDate,
        },
    };
}
