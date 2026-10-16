import test from 'node:test';
import assert from 'node:assert/strict';

import { setupTabsController } from '../tabsController.js';

test('setupTabsController initializes the active tab and panel state', () => {
    const documentRef = createTabsDocument('chart');
    const activated = [];

    setupTabsController({
        documentRef,
        onActivateTab: tabName => activated.push(tabName),
    });

    assert.deepEqual(activated, ['chart']);
    assert.equal(documentRef.tab('chart').classList.contains('active'), true);
    assert.equal(documentRef.tab('chart').getAttribute('aria-selected'), 'true');
    assert.equal(documentRef.tab('horary').tabIndex, -1);
    assert.equal(documentRef.pane('chart').hidden, false);
    assert.equal(documentRef.pane('chart').getAttribute('aria-hidden'), 'false');
    assert.equal(documentRef.pane('horary').hidden, true);
    assert.equal(documentRef.pane('horary').getAttribute('aria-hidden'), 'true');
    assert.equal(documentRef.getElementById('settingsBtn').getAttribute('aria-expanded'), 'false');
});

test('setupTabsController activates tabs on click', async () => {
    const documentRef = createTabsDocument('chart');
    const activated = [];
    setupTabsController({
        documentRef,
        onActivateTab: tabName => activated.push(tabName),
    });

    await documentRef.tab('horary').dispatch('click').pending;

    assert.deepEqual(activated, ['chart', 'horary']);
    assert.equal(documentRef.tab('chart').classList.contains('active'), false);
    assert.equal(documentRef.tab('horary').classList.contains('active'), true);
    assert.equal(documentRef.tab('horary').getAttribute('aria-selected'), 'true');
    assert.equal(documentRef.pane('chart').hidden, true);
    assert.equal(documentRef.pane('horary').hidden, false);
});

test('setupTabsController supports arrow, home, and end keyboard navigation', async () => {
    const documentRef = createTabsDocument('chart');
    const controller = setupTabsController({ documentRef });

    let prevented = false;
    await documentRef.tab('chart').dispatch('keydown', {
        key: 'ArrowRight',
        preventDefault() {
            prevented = true;
        },
    }).pending;

    assert.equal(prevented, true);
    assert.equal(documentRef.tab('horary').classList.contains('active'), true);
    assert.equal(documentRef.tab('horary').focused, true);

    await documentRef.tab('horary').dispatch('keydown', { key: 'End', preventDefault() {} }).pending;
    assert.equal(documentRef.tab('settings').classList.contains('active'), true);

    await documentRef.tab('settings').dispatch('keydown', { key: 'Home', preventDefault() {} }).pending;
    assert.equal(documentRef.tab('chart').classList.contains('active'), true);

    await documentRef.tab('chart').dispatch('keydown', { key: 'Tab', preventDefault() { assert.fail('Tab should not be intercepted'); } }).pending;
    assert.equal(controller.activateTab('missing'), false);
});

test('setupTabsController activates settings from the settings shortcut', async () => {
    const documentRef = createTabsDocument('chart');
    setupTabsController({ documentRef });

    await documentRef.getElementById('settingsBtn').dispatch('click').pending;

    assert.equal(documentRef.tab('settings').classList.contains('active'), true);
    assert.equal(documentRef.tab('settings').focused, true);
    assert.equal(documentRef.pane('settings').hidden, false);
    assert.equal(documentRef.getElementById('settingsBtn').getAttribute('aria-expanded'), 'true');
});

test('setupTabsController cleanup removes registered event listeners', async () => {
    const documentRef = createTabsDocument('chart');
    const activated = [];
    const controller = setupTabsController({
        documentRef,
        onActivateTab: tabName => activated.push(tabName),
    });

    controller.cleanup();
    await documentRef.tab('horary').dispatch('click').pending;
    await documentRef.getElementById('settingsBtn').dispatch('click').pending;

    assert.deepEqual(activated, ['chart']);
    assert.equal(documentRef.tab('chart').classList.contains('active'), true);
    assert.equal(documentRef.tab('horary').classList.contains('active'), false);
});

function createTabsDocument(activeTab = 'chart') {
    const documentRef = new FakeDocument();
    for (const tabName of ['chart', 'horary', 'aspects', 'tables', 'settings']) {
        const tab = documentRef.add(`tab-button-${tabName}`);
        tab.dataset.tab = tabName;
        tab.tabIndex = tabName === activeTab ? 0 : -1;
        if (tabName === activeTab) tab.classList.add('active');
        documentRef.tabs.push(tab);

        const pane = documentRef.add(`tab-${tabName}`);
        pane.hidden = tabName !== activeTab;
        if (tabName === activeTab) pane.classList.add('active');
        documentRef.panes.push(pane);
    }
    const settingsButton = documentRef.add('settingsBtn');
    settingsButton.setAttribute('aria-expanded', 'false');
    return documentRef;
}

class FakeDocument {
    constructor() {
        this.elements = new Map();
        this.tabs = [];
        this.panes = [];
    }

    add(id) {
        const element = new FakeElement(id);
        this.elements.set(id, element);
        return element;
    }

    getElementById(id) {
        return this.elements.get(id) || null;
    }

    querySelectorAll(selector) {
        if (selector === '.nav-tab') return this.tabs;
        if (selector === '.tab-pane') return this.panes;
        return [];
    }

    tab(name) {
        return this.getElementById(`tab-button-${name}`);
    }

    pane(name) {
        return this.getElementById(`tab-${name}`);
    }
}

class FakeClassList {
    constructor() {
        this.values = new Set();
    }

    add(value) {
        this.values.add(value);
    }

    contains(value) {
        return this.values.has(value);
    }

    toggle(value, force) {
        if (force) {
            this.values.add(value);
        } else {
            this.values.delete(value);
        }
    }
}

class FakeElement {
    constructor(id) {
        this.id = id;
        this.dataset = {};
        this.hidden = false;
        this.tabIndex = 0;
        this.focused = false;
        this.listeners = new Map();
        this.attributes = new Map();
        this.classList = new FakeClassList();
    }

    addEventListener(type, listener) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(listener);
    }

    removeEventListener(type, listener) {
        this.listeners.set(type, (this.listeners.get(type) ?? []).filter(item => item !== listener));
    }

    setAttribute(name, value) {
        this.attributes.set(name, value);
    }

    getAttribute(name) {
        return this.attributes.get(name);
    }

    focus() {
        this.focused = true;
    }

    dispatch(type, event = {}) {
        const dispatched = { target: this, ...event };
        const pending = Promise.all((this.listeners.get(type) ?? []).map(listener => listener(dispatched)));
        return { pending };
    }
}
