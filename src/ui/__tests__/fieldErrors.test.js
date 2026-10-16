import test from 'node:test';
import assert from 'node:assert/strict';

import {
    clearChartErrors,
    setFieldError,
    showChartErrors,
} from '../fieldErrors.js';

test('showChartErrors marks invalid fields and escapes the summary', () => {
    const documentRef = createFakeDocument([
        'calcErrors',
        'inputDate',
        'inputDateError',
        'inputTime',
        'inputTimeError',
        'inputOffset',
        'inputOffsetError',
        'inputLat',
        'inputLatError',
        'inputLng',
        'inputLngError',
    ]);

    showChartErrors([
        { field: 'lat', message: 'Latitude is required.' },
        { field: 'lng', message: 'Longitude <bad> & required.' },
    ], { documentRef });

    const latitude = documentRef.getElementById('inputLat');
    const latitudeError = documentRef.getElementById('inputLatError');
    const longitude = documentRef.getElementById('inputLng');
    const longitudeError = documentRef.getElementById('inputLngError');
    const summary = documentRef.getElementById('calcErrors');

    assert.equal(latitude.getAttribute('aria-invalid'), 'true');
    assert.equal(latitude.getAttribute('aria-describedby'), 'inputLatError');
    assert.equal(latitudeError.hidden, false);
    assert.equal(latitudeError.textContent, 'Latitude is required.');
    assert.equal(longitude.getAttribute('aria-invalid'), 'true');
    assert.equal(longitude.getAttribute('aria-describedby'), 'inputLngError');
    assert.equal(longitudeError.hidden, false);
    assert.equal(longitudeError.textContent, 'Longitude <bad> & required.');
    assert.equal(summary.hidden, false);
    assert.match(summary.innerHTML, /Latitude is required\./);
    assert.match(summary.innerHTML, /Longitude &lt;bad&gt; &amp; required\./);
});

test('clearChartErrors clears all field and summary state', () => {
    const documentRef = createFakeDocument([
        'calcErrors',
        'inputDate',
        'inputDateError',
        'inputTime',
        'inputTimeError',
        'inputOffset',
        'inputOffsetError',
        'inputLat',
        'inputLatError',
        'inputLng',
        'inputLngError',
    ]);

    showChartErrors([{ field: 'date', message: 'Date is required.' }], { documentRef });
    clearChartErrors({ documentRef });

    const date = documentRef.getElementById('inputDate');
    const dateError = documentRef.getElementById('inputDateError');
    const summary = documentRef.getElementById('calcErrors');

    assert.equal(date.getAttribute('aria-invalid'), null);
    assert.equal(date.getAttribute('aria-describedby'), null);
    assert.equal(dateError.hidden, true);
    assert.equal(dateError.textContent, '');
    assert.equal(summary.hidden, true);
    assert.equal(summary.innerHTML, '');
});

test('setFieldError creates a missing field error node', () => {
    const input = new FakeElement('inputCity');
    const created = [];
    const documentRef = {
        getElementById: () => null,
        createElement: tagName => {
            const element = new FakeElement(tagName);
            created.push(element);
            return element;
        },
    };

    setFieldError(input, 'Choose a valid city.', { documentRef });

    assert.equal(created.length, 1);
    assert.equal(created[0].id, 'inputCityError');
    assert.equal(created[0].className, 'field-error');
    assert.equal(created[0].hidden, false);
    assert.equal(created[0].textContent, 'Choose a valid city.');
    assert.equal(input.insertedAfter, created[0]);
    assert.equal(input.getAttribute('aria-invalid'), 'true');
    assert.equal(input.getAttribute('aria-describedby'), 'inputCityError');
});

function createFakeDocument(ids) {
    const elements = new Map(ids.map(id => [id, new FakeElement(id)]));
    return {
        getElementById(id) {
            return elements.get(id) || null;
        },
        createElement(tagName) {
            return new FakeElement(tagName);
        },
    };
}

class FakeElement {
    constructor(id) {
        this.id = id;
        this.className = '';
        this.hidden = true;
        this.innerHTML = '';
        this.textContent = '';
        this.attributes = new Map();
        this.insertedAfter = null;
    }

    setAttribute(name, value) {
        this.attributes.set(name, String(value));
    }

    getAttribute(name) {
        return this.attributes.get(name) ?? null;
    }

    removeAttribute(name) {
        this.attributes.delete(name);
    }

    insertAdjacentElement(position, element) {
        assert.equal(position, 'afterend');
        this.insertedAfter = element;
    }
}
