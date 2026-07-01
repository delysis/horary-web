import test from 'node:test';
import assert from 'node:assert/strict';

import {
    candidateFromAutocompleteItem,
    nextAutocompleteIndex,
    renderCityAutocompleteItems,
    setupCityAutocomplete,
} from '../autocomplete.js';

test('renderCityAutocompleteItems escapes display text and data attributes', () => {
    const html = renderCityAutocompleteItems([{
        label: 'A "Town" <Place>',
        name: 'A <Town>',
        country: 'G&B',
        latitude: '1.23',
        longitude: '-4.56',
        timezone: 'Etc/"Quoted"',
    }], 'cityList');

    assert.match(html, /id="cityList-option-0"/);
    assert.match(html, /role="option"/);
    assert.match(html, /data-name="A &quot;Town&quot; &lt;Place&gt;"/);
    assert.match(html, /data-tz="Etc\/&quot;Quoted&quot;"/);
    assert.match(html, />\s*A &lt;Town&gt; <span class="country">G&amp;B<\/span>/);
});

test('candidateFromAutocompleteItem returns a cloned candidate by rendered index', () => {
    const candidates = [
        { label: 'Paris, FR', name: 'Paris', country: 'FR', latitude: 48.8566, longitude: 2.3522 },
        { label: 'London, GB', name: 'London', country: 'GB', latitude: 51.5074, longitude: -0.1278 },
    ];

    const selected = candidateFromAutocompleteItem({ dataset: { index: '1' } }, candidates);
    selected.label = 'Mutated';

    assert.equal(candidates[1].label, 'London, GB');
    assert.deepEqual(candidateFromAutocompleteItem({ dataset: { index: 'bad' } }, candidates), null);
});

test('nextAutocompleteIndex follows keyboard bounds', () => {
    assert.equal(nextAutocompleteIndex(-1, 0, 'ArrowDown'), -1);
    assert.equal(nextAutocompleteIndex(-1, 3, 'ArrowDown'), 0);
    assert.equal(nextAutocompleteIndex(2, 3, 'ArrowDown'), 2);
    assert.equal(nextAutocompleteIndex(-1, 3, 'ArrowUp'), 0);
    assert.equal(nextAutocompleteIndex(0, 3, 'ArrowUp'), 0);
});

test('setupCityAutocomplete ignores stale searches and selects highlighted candidates', async () => {
    const input = new FakeElement({ id: 'inputCity' });
    const list = new FakeElement({ id: 'cityList' });
    const documentRef = new FakeElement({ id: 'document' });
    const firstSearch = deferred();
    const secondSearch = deferred();
    const selected = [];
    let searches = 0;

    setupCityAutocomplete({
        input,
        list,
        documentRef,
        searchCandidates: () => {
            searches++;
            return searches === 1 ? firstSearch.promise : secondSearch.promise;
        },
        onSelect: candidate => selected.push(candidate),
    });

    input.value = 'lo';
    const firstInput = input.dispatch('input');
    input.value = 'lon';
    const secondInput = input.dispatch('input');

    secondSearch.resolve([
        { label: 'London, GB', name: 'London', country: 'GB', latitude: 51.5074, longitude: -0.1278 },
    ]);
    await secondInput.pending;

    assert.equal(list.classList.contains('open'), true);
    assert.equal(list.children.length, 1);
    assert.equal(list.children[0].dataset.index, '0');

    firstSearch.resolve([
        { label: 'Los Angeles, US', name: 'Los Angeles', country: 'US', latitude: 34.0522, longitude: -118.2437 },
    ]);
    await firstInput.pending;

    assert.match(list.innerHTML, /London/);
    assert.doesNotMatch(list.innerHTML, /Los Angeles/);

    const down = input.dispatch('keydown', { key: 'ArrowDown' }).event;
    assert.equal(down.defaultPrevented, true);
    assert.equal(list.children[0].classList.contains('selected'), true);

    const enter = input.dispatch('keydown', { key: 'Enter' }).event;
    assert.equal(enter.defaultPrevented, true);
    assert.equal(input.value, 'London, GB');
    assert.equal(list.classList.contains('open'), false);
    assert.deepEqual(selected, [{
        label: 'London, GB',
        name: 'London',
        country: 'GB',
        latitude: 51.5074,
        longitude: -0.1278,
        timezone: '',
    }]);
});

class FakeElement {
    constructor({ id = '', dataset = {}, parent = null, autocompleteItem = false } = {}) {
        this.id = id;
        this.dataset = dataset;
        this.parent = parent;
        this.autocompleteItem = autocompleteItem;
        this.attributes = new Map();
        this.children = [];
        this.classList = new FakeClassList();
        this.listeners = new Map();
        this.value = '';
        this.html = '';
    }

    get innerHTML() {
        return this.html;
    }

    set innerHTML(value) {
        this.html = String(value);
        this.children = Array.from(this.html.matchAll(/<div\b[^>]*class="autocomplete-item"[^>]*data-index="([^"]+)"/g))
            .map(match => new FakeElement({
                id: `option-${match[1]}`,
                dataset: { index: match[1] },
                parent: this,
                autocompleteItem: true,
            }));
    }

    addEventListener(type, listener) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(listener);
    }

    removeEventListener(type, listener) {
        this.listeners.set(type, (this.listeners.get(type) ?? []).filter(item => item !== listener));
    }

    dispatch(type, event = {}) {
        const dispatchEvent = {
            target: this,
            key: undefined,
            defaultPrevented: false,
            preventDefault() {
                this.defaultPrevented = true;
            },
            ...event,
        };
        const pending = Promise.all((this.listeners.get(type) ?? []).map(listener => listener(dispatchEvent)));
        return { event: dispatchEvent, pending };
    }

    querySelectorAll(selector) {
        return selector === '.autocomplete-item' ? this.children : [];
    }

    contains(target) {
        return target === this || this.children.includes(target);
    }

    closest(selector) {
        return selector === '.autocomplete-item' && this.autocompleteItem ? this : null;
    }

    setAttribute(name, value) {
        this.attributes.set(name, String(value));
    }

    removeAttribute(name) {
        this.attributes.delete(name);
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

    toggle(value, force) {
        if (force) {
            this.add(value);
        } else {
            this.remove(value);
        }
    }

    contains(value) {
        return this.values.has(value);
    }
}

function deferred() {
    let resolve;
    const promise = new Promise(resolvePromise => {
        resolve = resolvePromise;
    });
    return { promise, resolve };
}
