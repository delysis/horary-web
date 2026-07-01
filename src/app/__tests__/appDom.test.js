import test from 'node:test';
import assert from 'node:assert/strict';

import {
    downloadJson,
    LEGACY_ZODIAC_STORAGE_KEY,
    removeLegacyZodiacSetting,
    renderAppVersion,
    renderInitialChartPlaceholder,
    setDefaultDateInput,
    showHoraryError,
} from '../appDom.js';

test('showHoraryError updates the Horary error surface and tolerates missing markup', () => {
    const documentRef = new FakeDocument();
    const errors = documentRef.add('horaryErrors');

    showHoraryError('Latitude is required.', { documentRef });

    assert.equal(errors.hidden, false);
    assert.equal(errors.textContent, 'Latitude is required.');

    showHoraryError('', { documentRef });

    assert.equal(errors.hidden, true);
    assert.equal(errors.textContent, '');

    assert.doesNotThrow(() => showHoraryError('Ignored', { documentRef: new FakeDocument() }));
});

test('downloadJson creates, clicks, removes, and revokes a browser download link', () => {
    const documentRef = new FakeDocument();
    const urlApi = createUrlApi();

    downloadJson('chart.json', { question: 'Will it work?', ok: true }, {
        documentRef,
        urlApi,
        BlobCtor: FakeBlob,
    });

    const link = documentRef.createdElements[0];
    const blob = urlApi.createdBlobs[0];

    assert.deepEqual(blob.parts, [
        `${JSON.stringify({ question: 'Will it work?', ok: true }, null, 2)}\n`,
    ]);
    assert.deepEqual(blob.options, { type: 'application/json' });
    assert.equal(link.tagName, 'a');
    assert.equal(link.href, 'blob:fake-download');
    assert.equal(link.download, 'chart.json');
    assert.equal(link.rel, 'noopener');
    assert.deepEqual(documentRef.body.appended, [link]);
    assert.equal(link.clicks, 1);
    assert.equal(link.removed, true);
    assert.deepEqual(urlApi.revokedUrls, ['blob:fake-download']);
});

test('removeLegacyZodiacSetting removes the retired zodiac preference key', () => {
    const storage = createStorage();

    removeLegacyZodiacSetting({ storage });

    assert.deepEqual(storage.removedKeys, [LEGACY_ZODIAC_STORAGE_KEY]);
    assert.doesNotThrow(() => removeLegacyZodiacSetting({ storage: null }));
});

test('setDefaultDateInput initializes the date field from the supplied clock', () => {
    const documentRef = new FakeDocument();
    const dateInput = documentRef.add('inputDate');

    const updated = setDefaultDateInput({
        documentRef,
        now: new Date('2026-06-30T23:15:00.000Z'),
    });

    assert.equal(updated, true);
    assert.equal(dateInput.value, '2026-06-30');
    assert.equal(setDefaultDateInput({ documentRef: new FakeDocument() }), false);
});

test('renderAppVersion writes the build version into the About panel', () => {
    const documentRef = new FakeDocument();
    const appVersion = documentRef.add('appVersion');

    const rendered = renderAppVersion({ documentRef, version: ' 2.3.4 ' });

    assert.equal(rendered, true);
    assert.equal(appVersion.textContent, '2.3.4');
    assert.equal(renderAppVersion({ documentRef: new FakeDocument(), version: '2.3.4' }), false);
});

test('renderInitialChartPlaceholder writes the empty chart state when the wheel exists', () => {
    const documentRef = new FakeDocument();
    const chartWheel = documentRef.add('chartWheel');

    const rendered = renderInitialChartPlaceholder({ documentRef });

    assert.equal(rendered, true);
    assert.match(chartWheel.innerHTML, /class="empty-state"/);
    assert.match(chartWheel.innerHTML, /&#10022;/);
    assert.match(chartWheel.innerHTML, /Enter chart data and tap Calculate to cast a chart/);
    assert.equal(renderInitialChartPlaceholder({ documentRef: new FakeDocument() }), false);
});

function createStorage() {
    return {
        removedKeys: [],
        removeItem(key) {
            this.removedKeys.push(key);
        },
    };
}

function createUrlApi() {
    return {
        createdBlobs: [],
        revokedUrls: [],
        createObjectURL(blob) {
            this.createdBlobs.push(blob);
            return 'blob:fake-download';
        },
        revokeObjectURL(url) {
            this.revokedUrls.push(url);
        },
    };
}

class FakeBlob {
    constructor(parts, options) {
        this.parts = parts;
        this.options = options;
    }
}

class FakeDocument {
    constructor() {
        this.elements = new Map();
        this.createdElements = [];
        this.body = new FakeBody();
    }

    add(id) {
        const element = new FakeElement(id);
        this.elements.set(id, element);
        return element;
    }

    getElementById(id) {
        return this.elements.get(id) || null;
    }

    createElement(tagName) {
        const element = new FakeElement('');
        element.tagName = tagName;
        this.createdElements.push(element);
        return element;
    }
}

class FakeBody {
    constructor() {
        this.appended = [];
    }

    append(element) {
        this.appended.push(element);
    }
}

class FakeElement {
    constructor(id) {
        this.id = id;
        this.hidden = false;
        this.textContent = '';
        this.innerHTML = '';
        this.value = '';
        this.href = '';
        this.download = '';
        this.rel = '';
        this.clicks = 0;
        this.removed = false;
        this.tagName = '';
    }

    click() {
        this.clicks++;
    }

    remove() {
        this.removed = true;
    }
}
