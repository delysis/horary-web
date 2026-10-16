const AUTOCOMPLETE_ITEM_SELECTOR = '.autocomplete-item';

export function escapeAutocompleteHtml(value) {
    return String(value).replace(/[&<>"']/g, char => ({
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
    })[char]);
}

export function normalizeAutocompleteCandidate(candidate = {}) {
    const name = candidate.name ?? '';
    const country = candidate.country ?? '';
    const label = candidate.label ?? [name, country].filter(Boolean).join(', ');

    return {
        ...candidate,
        label: String(label),
        name: String(name || label),
        country: String(country),
        latitude: candidate.latitude ?? '',
        longitude: candidate.longitude ?? '',
        timezone: String(candidate.timezone ?? ''),
    };
}

export function normalizeAutocompleteCandidates(candidates) {
    if (!Array.isArray(candidates)) return [];
    return candidates.map(candidate => normalizeAutocompleteCandidate(candidate));
}

export function renderCityAutocompleteItems(candidates, listId = 'city-autocomplete') {
    return normalizeAutocompleteCandidates(candidates).map((candidate, index) => {
        const optionId = `${listId}-option-${index}`;
        return `<div id="${escapeAutocompleteHtml(optionId)}" class="autocomplete-item" role="option" aria-selected="false" tabindex="-1" data-index="${index}" data-lat="${escapeAutocompleteHtml(candidate.latitude)}" data-lng="${escapeAutocompleteHtml(candidate.longitude)}" data-tz="${escapeAutocompleteHtml(candidate.timezone)}" data-name="${escapeAutocompleteHtml(candidate.label)}">
        ${escapeAutocompleteHtml(candidate.name)} <span class="country">${escapeAutocompleteHtml(candidate.country)}</span>
      </div>`;
    }).join('');
}

export function candidateFromAutocompleteItem(item, candidates) {
    const index = Number(item?.dataset?.index);
    if (!Number.isInteger(index) || index < 0) return null;

    const candidate = normalizeAutocompleteCandidates(candidates)[index];
    return candidate ? { ...candidate } : null;
}

export function nextAutocompleteIndex(currentIndex, itemCount, key) {
    if (itemCount <= 0) return -1;
    if (key === 'ArrowDown') return Math.min(currentIndex + 1, itemCount - 1);
    if (key === 'ArrowUp') return Math.max(currentIndex - 1, 0);
    return currentIndex;
}

export function highlightAutocompleteItems(items, selectedIndex) {
    items.forEach((item, index) => {
        const selected = index === selectedIndex;
        item.classList.toggle('selected', selected);
        item.setAttribute?.('aria-selected', String(selected));
    });
}

export function setupCityAutocomplete({
    input,
    list,
    searchCandidates,
    onSelect,
    documentRef = globalThis.document,
} = {}) {
    if (!input || !list || typeof searchCandidates !== 'function' || typeof onSelect !== 'function') {
        return () => {};
    }

    let selectedIndex = -1;
    let searchRequestId = 0;
    let renderedCandidates = [];

    setupAutocompleteAccessibility(input, list);

    const closeList = () => {
        selectedIndex = -1;
        list.classList.remove('open');
        input.setAttribute?.('aria-expanded', 'false');
        input.removeAttribute?.('aria-activedescendant');
    };

    const clearList = () => {
        renderedCandidates = [];
        list.innerHTML = '';
        closeList();
    };

    const openList = () => {
        list.classList.add('open');
        input.setAttribute?.('aria-expanded', 'true');
    };

    const itemId = index => `${list.id || 'city-autocomplete'}-option-${index}`;

    const updateHighlight = () => {
        const items = Array.from(list.querySelectorAll(AUTOCOMPLETE_ITEM_SELECTOR));
        highlightAutocompleteItems(items, selectedIndex);
        if (selectedIndex >= 0 && items[selectedIndex]) {
            input.setAttribute?.('aria-activedescendant', items[selectedIndex].id || itemId(selectedIndex));
        } else {
            input.removeAttribute?.('aria-activedescendant');
        }
    };

    const selectItem = item => {
        const candidate = candidateFromAutocompleteItem(item, renderedCandidates);
        if (!candidate) return;

        input.value = candidate.label;
        closeList();
        onSelect(candidate);
    };

    const onInput = async () => {
        const requestId = ++searchRequestId;
        const results = await searchCandidates(input.value);
        if (requestId !== searchRequestId) return;

        renderedCandidates = normalizeAutocompleteCandidates(results);
        selectedIndex = -1;

        if (renderedCandidates.length === 0) {
            clearList();
            return;
        }

        list.innerHTML = renderCityAutocompleteItems(renderedCandidates, list.id || 'city-autocomplete');
        openList();
        updateHighlight();
    };

    const onKeydown = event => {
        const items = Array.from(list.querySelectorAll(AUTOCOMPLETE_ITEM_SELECTOR));

        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            if (items.length === 0) return;
            event.preventDefault();
            selectedIndex = nextAutocompleteIndex(selectedIndex, items.length, event.key);
            updateHighlight();
            return;
        }

        if (event.key === 'Enter' && selectedIndex >= 0 && items[selectedIndex]) {
            event.preventDefault();
            selectItem(items[selectedIndex]);
            return;
        }

        if (event.key === 'Escape') {
            closeList();
        }
    };

    const onListClick = event => {
        const item = event.target?.closest?.(AUTOCOMPLETE_ITEM_SELECTOR);
        if (!item || !list.contains(item)) return;
        selectItem(item);
    };

    const onDocumentClick = event => {
        const target = event.target;
        if (!containsTarget(input, target) && !containsTarget(list, target)) {
            closeList();
        }
    };

    input.addEventListener('input', onInput);
    input.addEventListener('keydown', onKeydown);
    list.addEventListener('click', onListClick);
    documentRef?.addEventListener?.('click', onDocumentClick);

    return () => {
        input.removeEventListener?.('input', onInput);
        input.removeEventListener?.('keydown', onKeydown);
        list.removeEventListener?.('click', onListClick);
        documentRef?.removeEventListener?.('click', onDocumentClick);
    };
}

function setupAutocompleteAccessibility(input, list) {
    input.setAttribute?.('aria-autocomplete', 'list');
    input.setAttribute?.('aria-expanded', 'false');
    if (list.id) input.setAttribute?.('aria-controls', list.id);
    list.setAttribute?.('role', 'listbox');
}

function containsTarget(container, target) {
    return Boolean(target && container?.contains?.(target));
}
