import test from 'node:test';
import assert from 'node:assert/strict';

import aspectPhaseFixtures from '../__fixtures__/jpl-aspect-phases.json' with { type: 'json' };
import horizonsFixtures from '../__fixtures__/jpl-horizons-longitudes.json' with { type: 'json' };
import houseFixtures from '../__fixtures__/swiss-regiomontanus-houses.json' with { type: 'json' };
import motionEdgeFixtures from '../__fixtures__/jpl-motion-edge-cases.json' with { type: 'json' };
import { calculateChart } from '../chart.js';
import { getPlanetPosition } from '../planets.js';
import { dateToJD } from '../time.js';
import { norm180, signIndex } from '../utils.js';

function arcMinuteDelta(actual, expected) {
    return Math.abs(norm180(actual - expected)) * 60;
}

function assertWithinArcMinutes(label, actual, expected, toleranceArcMinutes) {
    const deltaArcMinutes = arcMinuteDelta(actual, expected);
    assert.ok(
        deltaArcMinutes <= toleranceArcMinutes,
        `${label} differs by ${deltaArcMinutes.toFixed(3)} arcmin, expected <= ${toleranceArcMinutes}`
    );
}

function aspectOrb(leftLongitude, rightLongitude, aspectAngle) {
    return Math.abs(Math.abs(norm180(leftLongitude - rightLongitude)) - aspectAngle);
}

test('planet longitudes stay within JPL Horizons golden tolerances', () => {
    for (const fixture of horizonsFixtures.cases) {
        const chart = calculateChart({
            date: new Date(fixture.input.utc),
            lat: fixture.input.latitude,
            lng: fixture.input.longitude,
            houseSystem: 'regiomontanus',
            planetSet: 'modern',
        });

        for (const [body, expected] of Object.entries(fixture.expected)) {
            const actual = chart.positions[body]?.longitude;
            assert.equal(typeof actual, 'number', `${fixture.id}: ${body} longitude is missing`);
            const toleranceArcMinutes = expected.toleranceArcMinutes
                ?? horizonsFixtures.tolerancesArcMinutes[body];
            assert.equal(
                typeof toleranceArcMinutes,
                'number',
                `${fixture.id}: ${body} tolerance is missing`
            );
            assertWithinArcMinutes(
                `${fixture.id}: ${body} longitude`,
                actual,
                expected.longitude,
                toleranceArcMinutes
            );

            if (Number.isInteger(expected.signIndex)) {
                assert.equal(
                    signIndex(expected.longitude),
                    expected.signIndex,
                    `${fixture.id}: ${body} JPL sign index`
                );
                assert.equal(
                    signIndex(actual),
                    expected.signIndex,
                    `${fixture.id}: ${body} calculated sign index`
                );
            }
        }
    }
});

test('Regiomontanus angles and house cusps stay within Swiss Ephemeris golden tolerances', () => {
    for (const fixture of houseFixtures.cases) {
        const chart = calculateChart({
            date: new Date(fixture.input.utc),
            lat: fixture.input.latitude,
            lng: fixture.input.longitude,
            houseSystem: 'regiomontanus',
            planetSet: 'modern',
        });

        const angleTolerance = houseFixtures.tolerancesArcMinutes.angle;
        assertWithinArcMinutes(`${fixture.id}: ASC`, chart.houses.asc, fixture.expected.asc, angleTolerance);
        assertWithinArcMinutes(`${fixture.id}: MC`, chart.houses.mc, fixture.expected.mc, angleTolerance);
        assertWithinArcMinutes(
            `${fixture.id}: Vertex`,
            chart.houses.vertex,
            fixture.expected.vertex,
            angleTolerance
        );

        fixture.expected.cusps.forEach((expectedCusp, index) => {
            assertWithinArcMinutes(
                `${fixture.id}: house ${index + 1} cusp`,
                chart.houses.cusps[index],
                expectedCusp,
                houseFixtures.tolerancesArcMinutes.cusp
            );
        });
    }
});

test('aspect phase agrees with JPL one-hour reference fixtures', () => {
    const orbTolerance = aspectPhaseFixtures.tolerancesArcMinutes.orb;

    for (const fixture of aspectPhaseFixtures.cases) {
        const chart = calculateChart({
            date: new Date(fixture.input.utc),
            lat: fixture.input.latitude,
            lng: fixture.input.longitude,
            houseSystem: 'regiomontanus',
            planetSet: 'classical',
        });

        const { planet1, planet2, aspectName, angle } = fixture.aspect;
        const actual = chart.aspects.find(aspect =>
            aspect.planet1 === planet1
            && aspect.planet2 === planet2
            && aspect.aspectName === aspectName
        );

        assert.ok(actual, `${fixture.id}: expected chart aspect is missing`);
        assert.equal(actual.applying, fixture.reference.applying, `${fixture.id}: applying phase`);
        assert.equal(actual.separating, fixture.reference.separating, `${fixture.id}: separating phase`);

        const referenceCurrentOrb = aspectOrb(
            fixture.reference.current[planet1],
            fixture.reference.current[planet2],
            angle
        );
        const referenceFutureOrb = aspectOrb(
            fixture.reference.future[planet1],
            fixture.reference.future[planet2],
            angle
        );

        assertWithinArcMinutes(`${fixture.id}: fixture current orb`, fixture.reference.currentOrb, referenceCurrentOrb, 0.001);
        assertWithinArcMinutes(`${fixture.id}: fixture future orb`, fixture.reference.futureOrb, referenceFutureOrb, 0.001);
        assertWithinArcMinutes(`${fixture.id}: current orb`, actual.orb, fixture.reference.currentOrb, orbTolerance);
        assertWithinArcMinutes(`${fixture.id}: future orb`, actual.futureOrb, fixture.reference.futureOrb, orbTolerance);
    }
});

test('station-adjacent motion agrees with JPL daily direction fixtures', () => {
    for (const fixture of motionEdgeFixtures.cases) {
        const position = getPlanetPosition(fixture.body, dateToJD(new Date(fixture.input.utc)));

        assertWithinArcMinutes(
            `${fixture.id}: longitude`,
            position.longitude,
            fixture.reference.currentLongitude,
            motionEdgeFixtures.tolerancesArcMinutes.longitude
        );
        assertWithinArcMinutes(
            `${fixture.id}: daily motion`,
            position.speed,
            fixture.reference.dailyMotion,
            motionEdgeFixtures.tolerancesArcMinutes.dailyMotion
        );
        assert.equal(position.retrograde, fixture.reference.retrograde, `${fixture.id}: retrograde flag`);
    }
});
