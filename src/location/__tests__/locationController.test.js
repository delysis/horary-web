import test from 'node:test';
import assert from 'node:assert/strict';

import { setupLocationController } from '../locationController.js';

test('setupLocationController wires autocomplete and reports unsupported geolocation', async () => {
    const documentRef = createLocationDocument();
    const autocompleteCalls = [];
    const statuses = [];
    const horaryErrors = [];

    setupLocationController({
        documentRef,
        navigatorRef: {},
        setLocationStatus: value => statuses.push(value),
        showHoraryError: message => horaryErrors.push(message),
        setupAutocomplete(config) {
            autocompleteCalls.push(config);
            return () => {};
        },
    });

    await documentRef.getElementById('useLocationBtn').dispatch('click').pending;

    assert.equal(autocompleteCalls.length, 1);
    assert.equal(autocompleteCalls[0].input, documentRef.getElementById('inputCity'));
    assert.equal(autocompleteCalls[0].list, documentRef.getElementById('cityList'));
    assert.deepEqual(statuses, ['manualRequired']);
    assert.deepEqual(horaryErrors, ['']);
    assert.equal(documentRef.getElementById('horaryLocation').textContent, 'Geolocation not supported; enter coordinates manually.');
    assert.equal(documentRef.getElementById('horaryLocation').className, 'location-display location-error');
});

test('setupLocationController stores successful browser geolocation and fills coordinate fields', () => {
    const documentRef = createLocationDocument();
    const geoLocationValues = [];
    const statuses = [];
    const errors = [];
    const geolocation = new FakeGeolocation();
    const controller = setupLocationController({
        documentRef,
        navigatorRef: { geolocation },
        setGeoLocation: value => geoLocationValues.push(value),
        setLocationStatus: value => statuses.push(value),
        showHoraryError: message => errors.push(message),
        setupAutocomplete: () => () => {},
    });

    controller.requestGeolocation();
    geolocation.succeed({ latitude: 40.712776, longitude: -74.005974 });

    assert.deepEqual(statuses, ['detecting', 'detected']);
    assert.deepEqual(geoLocationValues, [{ lat: 40.712776, lng: -74.005974 }]);
    assert.deepEqual(errors, ['']);
    assert.deepEqual(geolocation.options, { enableHighAccuracy: false, timeout: 10000 });
    assert.equal(documentRef.getElementById('inputLat').value, '40.7128');
    assert.equal(documentRef.getElementById('inputLng').value, '-74.0060');
    assert.match(documentRef.getElementById('horaryLocation').innerHTML, /Location acquired/);
    assert.equal(documentRef.getElementById('horaryLocation').className, 'location-display location-acquired');
});

test('setupLocationController reports browser geolocation failure', () => {
    const documentRef = createLocationDocument();
    const statuses = [];
    const errors = [];
    const geolocation = new FakeGeolocation();
    const controller = setupLocationController({
        documentRef,
        navigatorRef: { geolocation },
        setLocationStatus: value => statuses.push(value),
        showHoraryError: message => errors.push(message),
        setupAutocomplete: () => () => {},
    });

    controller.requestGeolocation();
    geolocation.fail({ message: 'Permission denied' });

    assert.deepEqual(statuses, ['detecting', 'manualRequired']);
    assert.deepEqual(errors, ['', 'Location unavailable: Permission denied']);
    assert.equal(documentRef.getElementById('horaryLocation').textContent, 'Location unavailable; enter coordinates manually.');
    assert.equal(documentRef.getElementById('horaryLocation').className, 'location-display location-error');
});

test('setupLocationController prefers Tauri geocoder and falls back to local search on failure', async () => {
    const documentRef = createLocationDocument();
    const warnMessages = [];
    const localQueries = [];
    let shouldFail = false;
    const controller = setupLocationController({
        documentRef,
        getTauriRuntime: () => true,
        async geocodeLocation(request) {
            assert.deepEqual(request, { query: 'lon', limit: 8 });
            if (shouldFail) throw new Error('offline');
            return [{ label: 'London, GB' }];
        },
        localGeocoder: {
            search(query) {
                localQueries.push(query);
                return [{ label: 'Local London' }];
            },
        },
        consoleRef: {
            warn(...args) {
                warnMessages.push(args);
            },
        },
        setupAutocomplete: () => () => {},
    });

    assert.deepEqual(await controller.searchLocationCandidates('lon'), [{ label: 'London, GB' }]);

    shouldFail = true;
    assert.deepEqual(await controller.searchLocationCandidates('lon'), [{ label: 'Local London' }]);
    assert.deepEqual(localQueries, ['lon']);
    assert.equal(warnMessages.length, 1);
    assert.match(warnMessages[0][0], /Tauri geocode failed/);
});

test('setupLocationController fills coordinates from a selected city candidate', () => {
    const documentRef = createLocationDocument();
    const geoLocationValues = [];
    const statuses = [];
    const controller = setupLocationController({
        documentRef,
        setGeoLocation: value => geoLocationValues.push(value),
        setLocationStatus: value => statuses.push(value),
        setupAutocomplete: () => () => {},
    });

    controller.selectCityCandidate({
        latitude: 51.5074,
        longitude: -0.1278,
    });

    assert.deepEqual(geoLocationValues, [null]);
    assert.deepEqual(statuses, ['manualValid']);
    assert.equal(documentRef.getElementById('inputLat').value, 51.5074);
    assert.equal(documentRef.getElementById('inputLng').value, -0.1278);
});

test('setupLocationController cleanup removes listeners and autocomplete teardown', async () => {
    const documentRef = createLocationDocument();
    let cleanupCalls = 0;
    let requestCalls = 0;
    const geolocation = new FakeGeolocation(() => requestCalls++);
    const controller = setupLocationController({
        documentRef,
        navigatorRef: { geolocation },
        setupAutocomplete: () => () => {
            cleanupCalls++;
        },
    });

    controller.cleanup();
    await documentRef.getElementById('useLocationBtn').dispatch('click').pending;

    assert.equal(cleanupCalls, 1);
    assert.equal(requestCalls, 0);
});

function createLocationDocument() {
    const documentRef = new FakeDocument();
    [
        'inputCity',
        'cityList',
        'inputLat',
        'inputLng',
        'horaryLocation',
        'useLocationBtn',
    ].forEach(id => documentRef.add(id));
    return documentRef;
}

class FakeGeolocation {
    constructor(onRequest = () => {}) {
        this.onRequest = onRequest;
        this.success = null;
        this.failure = null;
        this.options = null;
    }

    getCurrentPosition(success, failure, options) {
        this.onRequest();
        this.success = success;
        this.failure = failure;
        this.options = options;
    }

    succeed(coords) {
        this.success({ coords });
    }

    fail(error) {
        this.failure(error);
    }
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
        this.value = '';
        this.textContent = '';
        this.innerHTML = '';
        this.className = '';
        this.listeners = new Map();
    }

    addEventListener(type, listener) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(listener);
    }

    removeEventListener(type, listener) {
        this.listeners.set(type, (this.listeners.get(type) ?? []).filter(item => item !== listener));
    }

    dispatch(type, event = {}) {
        const dispatched = { target: this, ...event };
        const pending = Promise.all((this.listeners.get(type) ?? []).map(listener => listener(dispatched)));
        return { pending };
    }
}
