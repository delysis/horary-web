import {
    cloneDefaultAspectSettings,
    normalizeAspectSettings,
} from './aspectSettings.js';
import {
    exportableSettingsPayload,
    normalizeImportedSettingsPayload,
} from './settingsPayload.js';
import {
    renderAspectSettingsMarkup,
    renderHouseSystemOptions,
    renderHouseSystemPills,
} from './settingsMarkup.js';

const RADIO_PILL_SELECTOR = '.radio-pill';

export function setupSettingsController({
    documentRef = globalThis.document,
    houseSystems = [],
    getSettings = () => ({}),
    setHouseSystem = () => {},
    setPlanetSet = () => {},
    setAspectSettings = () => {},
    saveSettings = () => {},
    recalculate = () => {},
    hasChart = () => false,
    downloadJson = () => {},
} = {}) {
    if (!documentRef?.getElementById) {
        return emptySettingsController();
    }

    const houseSelect = documentRef.getElementById('houseSystem');
    const houseSystemGroup = documentRef.getElementById('settingsHouseSystem');
    const planetGroup = documentRef.getElementById('settingsPlanets');
    const aspectsContainer = documentRef.getElementById('settingsAspects');
    const resetAspectSettingsButton = documentRef.getElementById('resetAspectSettingsBtn');
    const exportSettingsButton = documentRef.getElementById('exportSettingsBtn');
    const importSettingsButton = documentRef.getElementById('importSettingsBtn');
    const importSettingsFile = documentRef.getElementById('importSettingsFile');
    const importStatus = documentRef.getElementById('settingsImportStatus');

    const renderHouseSystemControls = () => {
        const selectedHouseSystem = getSettings()?.houseSystem;
        if (houseSelect) {
            houseSelect.innerHTML = renderHouseSystemOptions(houseSystems, selectedHouseSystem);
            houseSelect.value = selectedHouseSystem || '';
        }
        if (houseSystemGroup) {
            houseSystemGroup.innerHTML = renderHouseSystemPills(houseSystems, selectedHouseSystem);
        }
    };

    const renderAspectSettingsControls = () => {
        if (!aspectsContainer) return;
        aspectsContainer.innerHTML = renderAspectSettingsMarkup(getSettings()?.aspectSettings);
    };

    const updateSettingsControls = () => {
        const settings = getSettings() || {};
        if (houseSelect) houseSelect.value = settings.houseSystem || '';
        updatePillGroup(houseSystemGroup, settings.houseSystem);
        updatePillGroup(planetGroup, settings.planetSet);
        renderAspectSettingsControls();
    };

    const setImportStatus = (message, isError = false) => {
        if (!importStatus) return;
        importStatus.textContent = message || '';
        importStatus.classList?.toggle?.('status-error', Boolean(isError));
    };

    const persistAndRecalculate = () => {
        saveSettings();
        recalculate();
    };

    const onHouseSelectChange = event => {
        const value = event.target?.value;
        if (!value) return;
        setHouseSystem(value);
        updatePillGroup(houseSystemGroup, value);
        persistAndRecalculate();
    };

    const onHouseSystemGroupClick = event => {
        const pill = closestWithin(event.target, RADIO_PILL_SELECTOR, houseSystemGroup);
        if (!pill?.dataset?.value) return;
        setHouseSystem(pill.dataset.value);
        if (houseSelect) houseSelect.value = pill.dataset.value;
        updatePillGroup(houseSystemGroup, pill.dataset.value);
        persistAndRecalculate();
    };

    const onPlanetGroupClick = event => {
        const pill = closestWithin(event.target, RADIO_PILL_SELECTOR, planetGroup);
        if (!pill?.dataset?.value) return;
        setPlanetSet(pill.dataset.value);
        updatePillGroup(planetGroup, pill.dataset.value);
        persistAndRecalculate();
    };

    const onAspectsChange = event => {
        const enabledInput = closestWithin(event.target, '[data-aspect-enabled]', aspectsContainer);
        const orbInput = closestWithin(event.target, '[data-aspect-orb]', aspectsContainer);
        const aspectSettings = getSettings()?.aspectSettings || {};

        if (enabledInput) {
            const key = enabledInput.dataset.aspectEnabled;
            setAspectSettings(normalizeAspectSettings({
                ...aspectSettings,
                [key]: {
                    ...aspectSettings[key],
                    enabled: enabledInput.checked,
                },
            }));
        } else if (orbInput) {
            const key = orbInput.dataset.aspectOrb;
            setAspectSettings(normalizeAspectSettings({
                ...aspectSettings,
                [key]: {
                    ...aspectSettings[key],
                    orb: orbInput.value,
                },
            }));
            renderAspectSettingsControls();
        } else {
            return;
        }

        persistAndRecalculate();
    };

    const onResetAspectSettingsClick = () => {
        setAspectSettings(cloneDefaultAspectSettings());
        renderAspectSettingsControls();
        persistAndRecalculate();
        setImportStatus('Aspect policy reset.');
    };

    const onExportSettingsClick = () => {
        downloadJson('whorary-settings.json', exportableSettingsPayload(getSettings()));
        setImportStatus('Settings exported.');
    };

    const onImportSettingsClick = () => {
        importSettingsFile?.click?.();
    };

    const onImportSettingsFileChange = async event => {
        const input = event.target;
        const file = input?.files?.[0];
        if (!file) return;

        try {
            const payload = JSON.parse(await file.text());
            const imported = normalizeImportedSettingsPayload(payload, { houseSystems });
            setHouseSystem(imported.houseSystem);
            setPlanetSet(imported.planetSet);
            setAspectSettings(imported.aspectSettings);
            saveSettings();
            updateSettingsControls();
            if (hasChart()) recalculate();
            setImportStatus('Settings imported.');
        } catch (error) {
            setImportStatus(error?.message || String(error), true);
        } finally {
            input.value = '';
        }
    };

    renderHouseSystemControls();
    renderAspectSettingsControls();

    houseSelect?.addEventListener?.('change', onHouseSelectChange);
    houseSystemGroup?.addEventListener?.('click', onHouseSystemGroupClick);
    planetGroup?.addEventListener?.('click', onPlanetGroupClick);
    aspectsContainer?.addEventListener?.('change', onAspectsChange);
    resetAspectSettingsButton?.addEventListener?.('click', onResetAspectSettingsClick);
    exportSettingsButton?.addEventListener?.('click', onExportSettingsClick);
    importSettingsButton?.addEventListener?.('click', onImportSettingsClick);
    importSettingsFile?.addEventListener?.('change', onImportSettingsFileChange);

    return {
        updateSettingsControls,
        renderAspectSettingsControls,
        cleanup() {
            houseSelect?.removeEventListener?.('change', onHouseSelectChange);
            houseSystemGroup?.removeEventListener?.('click', onHouseSystemGroupClick);
            planetGroup?.removeEventListener?.('click', onPlanetGroupClick);
            aspectsContainer?.removeEventListener?.('change', onAspectsChange);
            resetAspectSettingsButton?.removeEventListener?.('click', onResetAspectSettingsClick);
            exportSettingsButton?.removeEventListener?.('click', onExportSettingsClick);
            importSettingsButton?.removeEventListener?.('click', onImportSettingsClick);
            importSettingsFile?.removeEventListener?.('change', onImportSettingsFileChange);
        },
    };
}

function updatePillGroup(group, value) {
    group?.querySelectorAll?.(RADIO_PILL_SELECTOR).forEach(pill => {
        pill.classList?.toggle?.('active', pill.dataset?.value === value);
    });
}

function closestWithin(target, selector, container) {
    const match = target?.closest?.(selector);
    if (!match) return null;
    if (container?.contains && !container.contains(match)) return null;
    return match;
}

function emptySettingsController() {
    return {
        updateSettingsControls() {},
        renderAspectSettingsControls() {},
        cleanup() {},
    };
}
