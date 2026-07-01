import { CLASSICAL_PLANETS, norm180, norm360 } from './utils.js';

export const DEFAULT_ANTISCIA_ORB = 1;

export function antiscionLongitude(longitude) {
    return norm360(180 - longitude);
}

export function contraAntiscionLongitude(longitude) {
    return norm360(antiscionLongitude(longitude) + 180);
}

export function calculateAntisciaContacts(
    positions,
    { planets = CLASSICAL_PLANETS, orb = DEFAULT_ANTISCIA_ORB } = {}
) {
    const availablePlanets = planets.filter(planet => positions[planet]);
    const contacts = [];

    for (let i = 0; i < availablePlanets.length; i++) {
        for (let j = i + 1; j < availablePlanets.length; j++) {
            const planet1 = availablePlanets[i];
            const planet2 = availablePlanets[j];
            const lon1 = positions[planet1].longitude;
            const lon2 = positions[planet2].longitude;

            const antiscionOrb = angularOrb(lon2, antiscionLongitude(lon1));
            if (antiscionOrb <= orb) {
                contacts.push({
                    planet1,
                    planet2,
                    type: 'antiscion',
                    orb: round(antiscionOrb),
                });
            }

            const contraOrb = angularOrb(lon2, contraAntiscionLongitude(lon1));
            if (contraOrb <= orb) {
                contacts.push({
                    planet1,
                    planet2,
                    type: 'contraAntiscion',
                    orb: round(contraOrb),
                });
            }
        }
    }

    return contacts.sort((left, right) =>
        left.orb - right.orb
        || left.planet1.localeCompare(right.planet1)
        || left.planet2.localeCompare(right.planet2)
        || left.type.localeCompare(right.type)
    );
}

function angularOrb(actual, expected) {
    return Math.abs(norm180(actual - expected));
}

function round(value) {
    return Math.round(value * 10000) / 10000;
}
