import cities from './cities.json' with { type: 'json' };

/**
 * Bundled city database for offline location search.
 */
export const CITIES = cities;

/**
 * Search cities by name prefix (case-insensitive).
 * @param {string} query
 * @param {number} limit
 * @returns {Array}
 */
export function searchCities(query, limit = 8) {
    if (!query || query.length < 2) return [];
    const q = query.toLowerCase();

    return CITIES
        .filter(c => c.name.toLowerCase().includes(q))
        .sort((a, b) => {
            const an = a.name.toLowerCase();
            const bn = b.name.toLowerCase();

            // Priority 1: Exact start match
            const aStarts = an.startsWith(q);
            const bStarts = bn.startsWith(q);
            if (aStarts && !bStarts) return -1;
            if (bStarts && !aStarts) return 1;

            // Priority 2: Word boundary match
            const aWordMatch = an.includes(' ' + q);
            const bWordMatch = bn.includes(' ' + q);
            if (aWordMatch && !bWordMatch) return -1;
            if (bWordMatch && !aWordMatch) return 1;

            // Alphabetical fallback
            return a.name.localeCompare(b.name);
        })
        .slice(0, limit);
}
