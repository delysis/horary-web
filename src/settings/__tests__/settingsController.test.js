import test from 'node:test';
import assert from 'node:assert/strict';

import { cloneDefaultAspectSettings } from '../aspectSettings.js';
import { setupSettingsController } from '../settingsController.js';

const HOUSE_SYSTEMS = [
    { key: 'regiomontanus', name: 'Regiomontanus' },
    { key: 'whole-sign', name: 'Whole Sign' },
];

test('setupSettingsController renders house and aspect settings controls', () => {
    const { documentRef } = createSettingsHarness();
    const select = documentRef.getElementById('houseSystem');
    const houseGroup = documentRef.getElementById('settingsHouseSystem');
    const aspects = documentRef.getElementById('settingsAspects');

    assert.equal(select.value, 'regiomontanus');
    assert.match(select.innerHTML, /<option value="regiomontanus" selected>Regiomontanus<\/option>/);
    assert.equal(houseGroup.querySelectorAll('.radio-pill')[0].classList.contains('active'), true);
    assert.match(aspects.innerHTML, /data-aspect-enabled="trine"/);
});

test('setupSettingsController applies house system and planet pill changes', async () => {
    const harness = createSettingsHarness();
    const select = harness.documentRef.getElementById('houseSystem');
    const houseGroup = harness.documentRef.getElementById('settingsHouseSystem');
    const planetGroup = harness.documentRef.getElementById('settingsPlanets');

    select.value = 'whole-sign';
    await select.dispatch('change').pending;

    assert.equal(harness.state.houseSystem, 'whole-sign');
    assert.equal(houseGroup.querySelectorAll('.radio-pill')[1].classList.contains('active'), true);
    assert.equal(harness.calls.saves, 1);
    assert.equal(harness.calls.recalculations, 1);

    const modernPill = planetGroup.querySelectorAll('.radio-pill')[1];
    await planetGroup.dispatch('click', { target: modernPill }).pending;

    assert.equal(harness.state.planetSet, 'modern');
    assert.equal(modernPill.classList.contains('active'), true);
    assert.equal(harness.calls.saves, 2);
    assert.equal(harness.calls.recalculations, 2);
});

test('setupSettingsController normalizes aspect orb edits and rerenders aspect controls', async () => {
    const harness = createSettingsHarness();
    const aspects = harness.documentRef.getElementById('settingsAspects');
    const orbInput = new FakeElement();
    orbInput.dataset.aspectOrb = 'trine';
    orbInput.value = '99';
    orbInput.parentElement = aspects;

    await aspects.dispatch('change', { target: orbInput }).pending;

    assert.equal(harness.state.aspectSettings.trine.orb, 30);
    assert.match(aspects.innerHTML, /data-aspect-orb="trine"/);
    assert.match(aspects.innerHTML, /value="30"/);
    assert.equal(harness.calls.saves, 1);
    assert.equal(harness.calls.recalculations, 1);
});

test('setupSettingsController exports settings through injected download handler', async () => {
    const harness = createSettingsHarness();
    const exportButton = harness.documentRef.getElementById('exportSettingsBtn');
    const status = harness.documentRef.getElementById('settingsImportStatus');

    await exportButton.dispatch('click').pending;

    assert.equal(harness.downloads[0].filename, 'horary-settings.json');
    assert.equal(harness.downloads[0].payload.schemaVersion, 1);
    assert.equal(harness.downloads[0].payload.settings.houseSystem, 'regiomontanus');
    assert.equal(status.textContent, 'Settings exported.');
    assert.equal(status.classList.contains('status-error'), false);
});

test('setupSettingsController imports normalized settings and recalculates only with a chart', async () => {
    const harness = createSettingsHarness({ hasChart: true });
    const input = harness.documentRef.getElementById('importSettingsFile');
    const status = harness.documentRef.getElementById('settingsImportStatus');
    input.value = '/tmp/horary-settings.json';
    input.files = [{
        text: async () => JSON.stringify({
            settings: {
                houseSystem: 'whole-sign',
                planetSet: 'modern',
                aspectSettings: {
                    trine: { enabled: false, orb: 3 },
                },
            },
        }),
    }];

    await input.dispatch('change').pending;

    assert.equal(harness.state.houseSystem, 'whole-sign');
    assert.equal(harness.state.planetSet, 'modern');
    assert.equal(harness.state.aspectSettings.trine.enabled, false);
    assert.equal(harness.state.aspectSettings.trine.orb, 3);
    assert.equal(harness.calls.saves, 1);
    assert.equal(harness.calls.recalculations, 1);
    assert.equal(status.textContent, 'Settings imported.');
    assert.equal(input.value, '');
});

test('setupSettingsController reports import errors without mutating settings', async () => {
    const harness = createSettingsHarness({ hasChart: true });
    const input = harness.documentRef.getElementById('importSettingsFile');
    const status = harness.documentRef.getElementById('settingsImportStatus');
    input.files = [{
        text: async () => JSON.stringify({ houseSystem: 'invented' }),
    }];

    await input.dispatch('change').pending;

    assert.equal(harness.state.houseSystem, 'regiomontanus');
    assert.equal(harness.calls.saves, 0);
    assert.equal(harness.calls.recalculations, 0);
    assert.match(status.textContent, /Unsupported house system/);
    assert.equal(status.classList.contains('status-error'), true);
});

function createSettingsHarness({ hasChart = false } = {}) {
    const documentRef = createSettingsDocument();
    const state = {
        houseSystem: 'regiomontanus',
        planetSet: 'classical',
        aspectSettings: cloneDefaultAspectSettings(),
    };
    const calls = {
        saves: 0,
        recalculations: 0,
    };
    const downloads = [];

    const controller = setupSettingsController({
        documentRef,
        houseSystems: HOUSE_SYSTEMS,
        getSettings: () => state,
        setHouseSystem(value) {
            state.houseSystem = value;
        },
        setPlanetSet(value) {
            state.planetSet = value;
        },
        setAspectSettings(value) {
            state.aspectSettings = value;
        },
        saveSettings() {
            calls.saves++;
        },
        recalculate() {
            calls.recalculations++;
        },
        hasChart: () => hasChart,
        downloadJson(filename, payload) {
            downloads.push({ filename, payload });
        },
    });

    return {
        documentRef,
        state,
        calls,
        downloads,
        controller,
    };
}

function createSettingsDocument() {
    const documentRef = new FakeDocument();
    [
        'houseSystem',
        'settingsHouseSystem',
        'settingsAspects',
        'resetAspectSettingsBtn',
        'exportSettingsBtn',
        'importSettingsBtn',
        'importSettingsFile',
        'settingsImportStatus',
    ].forEach(id => documentRef.add(id));

    const planetGroup = documentRef.add('settingsPlanets');
    planetGroup.setChildren([
        createRadioPill('classical', true),
        createRadioPill('modern', false),
    ]);

    return documentRef;
}

function createRadioPill(value, active = false) {
    const pill = new FakeElement();
    pill.dataset.value = value;
    pill.classList.add('radio-pill');
    if (active) pill.classList.add('active');
    return pill;
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
    constructor(id = '') {
        this.id = id;
        this.dataset = {};
        this.listeners = new Map();
        this.children = [];
        this.classList = new FakeClassList();
        this.textContent = '';
        this.value = '';
        this.checked = false;
        this.files = [];
        this.parentElement = null;
        this.clicks = 0;
        this._innerHTML = '';
    }

    get innerHTML() {
        return this._innerHTML;
    }

    set innerHTML(value) {
        this._innerHTML = String(value);
        this.setChildren(parseRadioPills(this._innerHTML));
    }

    setChildren(children) {
        this.children = children;
        this.children.forEach(child => {
            child.parentElement = this;
        });
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

    click() {
        this.clicks++;
        return this.dispatch('click');
    }

    querySelectorAll(selector) {
        if (selector === '.radio-pill') {
            return this.children.filter(child => child.classList.contains('radio-pill'));
        }
        return [];
    }

    closest(selector) {
        if (selector === '.radio-pill' && this.classList.contains('radio-pill')) return this;
        if (selector === '[data-aspect-enabled]' && Object.hasOwn(this.dataset, 'aspectEnabled')) return this;
        if (selector === '[data-aspect-orb]' && Object.hasOwn(this.dataset, 'aspectOrb')) return this;
        return this.parentElement?.closest?.(selector) || null;
    }

    contains(target) {
        let current = target;
        while (current) {
            if (current === this) return true;
            current = current.parentElement;
        }
        return false;
    }
}

class FakeClassList {
    constructor(values = []) {
        this.values = new Set(values);
    }

    add(value) {
        this.values.add(value);
    }

    toggle(value, force) {
        if (force === undefined) {
            if (this.values.has(value)) {
                this.values.delete(value);
                return false;
            }
            this.values.add(value);
            return true;
        }
        if (force) this.values.add(value);
        else this.values.delete(value);
        return Boolean(force);
    }

    contains(value) {
        return this.values.has(value);
    }
}

function parseRadioPills(html) {
    const pills = [];
    const pattern = /<button class="([^"]*)" data-value="([^"]*)">/g;
    let match = pattern.exec(html);
    while (match) {
        const pill = new FakeElement();
        pill.classList = new FakeClassList(match[1].split(/\s+/).filter(Boolean));
        pill.dataset.value = match[2];
        pills.push(pill);
        match = pattern.exec(html);
    }
    return pills;
}
