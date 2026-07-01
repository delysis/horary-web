import test from 'node:test';
import assert from 'node:assert/strict';

import { setupPwaInstallPrompt } from '../installPrompt.js';

test('setupPwaInstallPrompt shows the banner and defers the install event', () => {
    const windowRef = new FakeTarget();
    const banner = new FakeElement();
    const installButton = new FakeTarget();
    const dismissButton = new FakeTarget();
    let prevented = false;

    setupPwaInstallPrompt({ windowRef, banner, installButton, dismissButton });

    windowRef.dispatch('beforeinstallprompt', {
        preventDefault() {
            prevented = true;
        },
        prompt() {},
        userChoice: Promise.resolve({ outcome: 'dismissed' }),
    });

    assert.equal(prevented, true);
    assert.equal(banner.classList.contains('show'), true);
});

test('setupPwaInstallPrompt prompts once and hides the banner after user choice', async () => {
    const windowRef = new FakeTarget();
    const banner = new FakeElement();
    const installButton = new FakeTarget();
    const dismissButton = new FakeTarget();
    let promptCalls = 0;

    setupPwaInstallPrompt({ windowRef, banner, installButton, dismissButton });

    windowRef.dispatch('beforeinstallprompt', {
        preventDefault() {},
        prompt() {
            promptCalls++;
        },
        userChoice: Promise.resolve({ outcome: 'accepted' }),
    });

    await installButton.dispatch('click').pending;
    await installButton.dispatch('click').pending;

    assert.equal(promptCalls, 1);
    assert.equal(banner.classList.contains('show'), false);
});

test('setupPwaInstallPrompt dismisses and unregisters listeners during cleanup', async () => {
    const windowRef = new FakeTarget();
    const banner = new FakeElement();
    const installButton = new FakeTarget();
    const dismissButton = new FakeTarget();
    let promptCalls = 0;

    const cleanup = setupPwaInstallPrompt({ windowRef, banner, installButton, dismissButton });

    windowRef.dispatch('beforeinstallprompt', {
        preventDefault() {},
        prompt() {
            promptCalls++;
        },
        userChoice: Promise.resolve({ outcome: 'accepted' }),
    });
    assert.equal(banner.classList.contains('show'), true);

    await dismissButton.dispatch('click').pending;
    assert.equal(banner.classList.contains('show'), false);

    cleanup();
    windowRef.dispatch('beforeinstallprompt', {
        preventDefault() {},
        prompt() {
            promptCalls++;
        },
        userChoice: Promise.resolve({ outcome: 'accepted' }),
    });
    await installButton.dispatch('click').pending;

    assert.equal(promptCalls, 0);
    assert.equal(banner.classList.contains('show'), false);
});

class FakeTarget {
    constructor() {
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
        const pending = Promise.all((this.listeners.get(type) ?? []).map(listener => listener(event)));
        return { pending };
    }
}

class FakeElement extends FakeTarget {
    constructor() {
        super();
        this.classList = new FakeClassList();
    }
}

class FakeClassList {
    constructor() {
        this.values = new Set();
    }

    add(value) {
        this.values.add(value);
    }

    remove(value) {
        this.values.delete(value);
    }

    contains(value) {
        return this.values.has(value);
    }
}
