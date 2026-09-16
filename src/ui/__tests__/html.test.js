import test from 'node:test';
import assert from 'node:assert/strict';

import { escapeHtml } from '../html.js';

test('escapeHtml escapes markup-significant characters consistently', () => {
    assert.equal(
        escapeHtml('&<>"\''),
        '&amp;&lt;&gt;&quot;&#39;',
    );
    assert.equal(escapeHtml(42), '42');
});
