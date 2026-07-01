import test from 'node:test';
import assert from 'node:assert/strict';

import {
    renderAspectGrid,
    renderAspectsList,
    renderChartWheel,
    renderCuspsTable,
    renderDignitiesTable,
    renderPositionsTable,
} from '../renderers.js';

test('renderChartWheel writes an accessible SVG and skips the South Node glyph', () => {
    const documentRef = new FakeDocument();
    const container = documentRef.add('chartWheel');

    withDocument(documentRef, () => {
        renderChartWheel(createChartFixture(), 'chartWheel');
    });

    assert.match(container.innerHTML, /<svg[^>]+viewBox="0 0 560 560"/);
    assert.match(container.innerHTML, /role="img"/);
    assert.match(container.innerHTML, /aria-labelledby="chartWheel-title"/);
    assert.match(container.innerHTML, /<title id="chartWheel-title">Astrological chart wheel<\/title>/);
    assert.match(container.innerHTML, /<desc id="chartWheel-desc">A circular chart showing zodiac signs/);
    assert.match(container.innerHTML, /data-planet="Sun"/);
    assert.doesNotMatch(container.innerHTML, /data-planet="SouthNode"/);
});

test('renderPositionsTable renders positions, speeds, dignities, and omits South Node', () => {
    const documentRef = new FakeDocument();
    const body = documentRef.add('positionsBody');

    withDocument(documentRef, () => {
        renderPositionsTable(createChartFixture());
    });

    assert.match(body.innerHTML, /<th scope="row" class="">Sun<\/th>/);
    assert.match(body.innerHTML, /5° ♋ 30'/);
    assert.match(body.innerHTML, /<span class="sign-badge water">♋ Cancer<\/span>/);
    assert.match(body.innerHTML, /0\.986°\/d/);
    assert.match(body.innerHTML, /Dom/);
    assert.match(body.innerHTML, /Angular/);
    assert.match(body.innerHTML, /Acc \+5/);
    assert.doesNotMatch(body.innerHTML, /SouthNode/);
});

test('renderCuspsTable labels angles and renders sign rulers', () => {
    const documentRef = new FakeDocument();
    const body = documentRef.add('cuspsBody');

    withDocument(documentRef, () => {
        renderCuspsTable(createChartFixture());
    });

    assert.match(body.innerHTML, /<th scope="row">I \(ASC\)<\/th>/);
    assert.match(body.innerHTML, /<th scope="row">X \(MC\)<\/th>/);
    assert.match(body.innerHTML, /♄ Saturn/);
});

test('renderAspectGrid renders accessible grid markup and aspect phase titles', () => {
    const documentRef = new FakeDocument();
    const container = documentRef.add('aspectGridContainer');

    withDocument(documentRef, () => {
        renderAspectGrid(createChartFixture());
    });

    assert.match(container.innerHTML, /<caption class="sr-only">Aspect grid between chart bodies<\/caption>/);
    assert.match(container.innerHTML, /title="Sun"/);
    assert.match(container.innerHTML, /title="Moon Square Sun \(1\.25°/);
    assert.match(container.innerHTML, /applying/);
    assert.doesNotMatch(container.innerHTML, /SouthNode/);
});

test('renderAspectsList respects major and applying filters', () => {
    const documentRef = new FakeDocument();
    const body = documentRef.add('aspectsBody');
    documentRef.add('majorOnlyToggle').checked = true;
    documentRef.add('applyingOnlyToggle').checked = true;

    withDocument(documentRef, () => {
        renderAspectsList(createChartFixture());
    });

    assert.match(body.innerHTML, /Square/);
    assert.match(body.innerHTML, /Applying/);
    assert.doesNotMatch(body.innerHTML, /Semisquare/);
});

test('renderAspectsList renders an empty state when filters remove all aspects', () => {
    const documentRef = new FakeDocument();
    const body = documentRef.add('aspectsBody');
    documentRef.add('majorOnlyToggle').checked = false;
    documentRef.add('applyingOnlyToggle').checked = true;
    const chart = {
        ...createChartFixture(),
        aspects: [{
            planet1: 'Sun',
            planet2: 'Moon',
            aspectName: 'Trine',
            symbol: '△',
            color: '#5a9ee0',
            orb: 1.25,
            applying: false,
            separating: true,
            futureOrb: 2,
            major: true,
        }],
    };

    withDocument(documentRef, () => {
        renderAspectsList(chart);
    });

    assert.match(body.innerHTML, /No aspects found/);
});

test('renderDignitiesTable renders positive and negative dignity scores', () => {
    const documentRef = new FakeDocument();
    const body = documentRef.add('dignitiesBody');

    withDocument(documentRef, () => {
        renderDignitiesTable(createChartFixture());
    });

    assert.match(body.innerHTML, /<th scope="row">Sun<\/th>/);
    assert.match(body.innerHTML, /\+5/);
    assert.match(body.innerHTML, /dignity-positive/);
    assert.match(body.innerHTML, /<th scope="row">Moon<\/th>/);
    assert.match(body.innerHTML, /-5/);
    assert.match(body.innerHTML, /dignity-negative/);
});

function withDocument(documentRef, callback) {
    const previous = globalThis.document;
    globalThis.document = documentRef;
    try {
        callback();
    } finally {
        if (previous === undefined) {
            delete globalThis.document;
        } else {
            globalThis.document = previous;
        }
    }
}

function createChartFixture() {
    return {
        positions: {
            Sun: {
                longitude: 95.5,
                speed: 0.986,
                house: 10,
                retrograde: false,
            },
            Moon: {
                longitude: 185.25,
                speed: -12.1,
                house: 1,
                retrograde: true,
            },
            SouthNode: {
                longitude: 20,
                speed: 0,
                house: 3,
                retrograde: false,
            },
        },
        houses: {
            asc: 280,
            mc: 10,
            cusps: [280, 310, 340, 10, 40, 70, 100, 130, 160, 190, 220, 250],
        },
        aspects: [
            {
                planet1: 'Sun',
                planet2: 'Moon',
                lon1: 95.5,
                lon2: 185.25,
                aspectName: 'Square',
                symbol: '□',
                color: '#e05555',
                orb: 1.25,
                applying: true,
                separating: false,
                futureOrb: 0.8,
                major: true,
                exact: false,
            },
            {
                planet1: 'Sun',
                planet2: 'SouthNode',
                lon1: 95.5,
                lon2: 20,
                aspectName: 'Semisquare',
                symbol: '∠',
                color: '#999999',
                orb: 2.5,
                applying: false,
                separating: true,
                futureOrb: 3,
                major: false,
                exact: false,
            },
        ],
        dignities: {
            Sun: {
                domicile: true,
                exaltation: false,
                triplicityRuler: null,
                termRuler: null,
                faceRuler: null,
                detriment: false,
                fall: false,
                peregrine: false,
                score: 5,
            },
            Moon: {
                domicile: false,
                exaltation: false,
                triplicityRuler: null,
                termRuler: null,
                faceRuler: null,
                detriment: true,
                fall: false,
                peregrine: false,
                score: -5,
            },
        },
        accidentalDignities: {
            Sun: {
                angularity: 'angular',
                retrograde: false,
                solarCondition: null,
                inJoy: false,
                score: 5,
            },
            Moon: {
                angularity: 'angular',
                retrograde: true,
                solarCondition: 'combust',
                inJoy: false,
                score: -5,
            },
        },
    };
}

class FakeDocument {
    constructor() {
        this.elements = new Map();
    }

    add(id) {
        const element = new FakeElement(id);
        this.elements.set(id, element);
        return element;
    }

    getElementById(id) {
        return this.elements.get(id) || null;
    }
}

class FakeElement {
    constructor(id) {
        this.id = id;
        this.innerHTML = '';
        this.checked = false;
    }
}
