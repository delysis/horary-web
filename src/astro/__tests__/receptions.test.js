import test from 'node:test';
import assert from 'node:assert/strict';

import { getDignityRulersForLongitude } from '../dignities.js';
import { calculateReceptions } from '../receptions.js';

test('getDignityRulersForLongitude reports traditional rulers for a degree', () => {
    const rulers = getDignityRulersForLongitude(10, true);

    assert.ok(rulers.some(ruler => ruler.dignity === 'domicile' && ruler.planet === 'Mars'));
    assert.ok(rulers.some(ruler => ruler.dignity === 'exaltation' && ruler.planet === 'Sun'));
    assert.ok(rulers.some(ruler => ruler.dignity === 'triplicity' && ruler.planet === 'Sun'));
    assert.ok(rulers.some(ruler => ruler.dignity === 'term' && ruler.planet === 'Venus'));
    assert.ok(rulers.some(ruler => ruler.dignity === 'face' && ruler.planet));
});

test('calculateReceptions detects domicile mutual reception', () => {
    const receptions = calculateReceptions({
        Mars: { longitude: 35 },
        Venus: { longitude: 5 },
    });

    assert.ok(receptions.some(reception =>
        reception.hostPlanet === 'Mars'
        && reception.guestPlanet === 'Venus'
        && reception.dignity === 'domicile'
        && reception.mutual
    ));
    assert.ok(receptions.some(reception =>
        reception.hostPlanet === 'Venus'
        && reception.guestPlanet === 'Mars'
        && reception.dignity === 'domicile'
        && reception.mutual
    ));
});

test('calculateReceptions skips unavailable host planets and self-rulership', () => {
    const receptions = calculateReceptions({
        Sun: { longitude: 125 },
        Moon: { longitude: 10 },
    });

    assert.ok(receptions.every(reception => reception.hostPlanet !== reception.guestPlanet));
    assert.ok(receptions.every(reception => ['Sun', 'Moon'].includes(reception.hostPlanet)));
});
