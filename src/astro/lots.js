import { norm360 } from './utils.js';

export function calculatePartOfFortune({
    ascendantLongitude,
    sunLongitude,
    moonLongitude,
    isDayChart,
}) {
    const longitude = isDayChart
        ? norm360(ascendantLongitude + moonLongitude - sunLongitude)
        : norm360(ascendantLongitude + sunLongitude - moonLongitude);

    return {
        key: 'partOfFortune',
        name: 'Part of Fortune',
        longitude,
        formula: isDayChart ? 'ASC + Moon - Sun' : 'ASC + Sun - Moon',
    };
}
