import { getDignityRulersForLongitude } from './dignities.js';
import { CLASSICAL_PLANETS } from './utils.js';

export function calculateReceptions(positions, { isDayChart = true, planets = CLASSICAL_PLANETS } = {}) {
    const availablePlanets = planets.filter(planet => positions[planet]);
    const receptions = [];

    for (const guestPlanet of availablePlanets) {
        const guestPosition = positions[guestPlanet];
        const dignityRulers = getDignityRulersForLongitude(guestPosition.longitude, isDayChart);

        for (const ruler of dignityRulers) {
            const hostPlanet = ruler.planet;
            if (hostPlanet === guestPlanet || !positions[hostPlanet]) continue;

            receptions.push({
                hostPlanet,
                guestPlanet,
                dignity: ruler.dignity,
                mutual: false,
            });
        }
    }

    for (const reception of receptions) {
        reception.mutual = receptions.some(other =>
            other.hostPlanet === reception.guestPlanet
            && other.guestPlanet === reception.hostPlanet
        );
    }

    return receptions.sort((left, right) =>
        Number(right.mutual) - Number(left.mutual)
        || left.hostPlanet.localeCompare(right.hostPlanet)
        || left.guestPlanet.localeCompare(right.guestPlanet)
        || left.dignity.localeCompare(right.dignity)
    );
}
