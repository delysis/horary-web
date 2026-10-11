import { searchCities } from '../data/cities.js';

export const LOCAL_CITY_GEOCODER_PROVIDER = {
    id: 'bundled-cities',
    displayName: 'Bundled city list',
    online: false,
    attribution: 'Offline bundled city list',
};

export class LocalCityGeocoder {
    constructor({ search = searchCities, cacheSize = 50 } = {}) {
        this.searchCities = search;
        this.cacheSize = Math.max(1, cacheSize);
        this.cache = new Map();
        this.provider = LOCAL_CITY_GEOCODER_PROVIDER;
    }

    search(query, { limit = 8 } = {}) {
        const normalizedQuery = normalizeQuery(query);
        if (normalizedQuery.length < 2) return [];

        const normalizedLimit = normalizeLimit(limit);
        const key = `${normalizedQuery}|${normalizedLimit}`;
        const cached = this.cache.get(key);
        if (cached) {
            this.cache.delete(key);
            this.cache.set(key, cached);
            return cloneCandidates(cached);
        }

        const results = this.searchCities(normalizedQuery, normalizedLimit)
            .map(cityToCandidate);
        this.cache.set(key, results);
        trimCache(this.cache, this.cacheSize);
        return cloneCandidates(results);
    }
}

export function createLocalCityGeocoder(options) {
    return new LocalCityGeocoder(options);
}

function normalizeQuery(query) {
    return String(query || '').trim().toLowerCase();
}

function normalizeLimit(limit) {
    const value = Number(limit);
    if (!Number.isFinite(value) || value <= 0) return 8;
    return Math.min(50, Math.floor(value));
}

function cityToCandidate(city) {
    const label = `${city.name}, ${city.country}`;
    return {
        id: label.toLowerCase(),
        label,
        name: city.name,
        country: city.country,
        latitude: city.lat,
        longitude: city.lng,
        timezone: city.tz,
        provider: LOCAL_CITY_GEOCODER_PROVIDER.id,
    };
}

function cloneCandidates(candidates) {
    return candidates.map(candidate => ({ ...candidate }));
}

function trimCache(cache, maxSize) {
    while (cache.size > maxSize) {
        const oldestKey = cache.keys().next().value;
        cache.delete(oldestKey);
    }
}
