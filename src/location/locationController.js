import { setupCityAutocomplete } from './autocomplete.js';

const GEOLOCATION_OPTIONS = Object.freeze({
    enableHighAccuracy: false,
    timeout: 10000,
});

export function setupLocationController({
    documentRef = globalThis.document,
    navigatorRef = globalThis.navigator,
    input = documentRef?.getElementById?.('inputCity'),
    list = documentRef?.getElementById?.('cityList'),
    latitudeInput = documentRef?.getElementById?.('inputLat'),
    longitudeInput = documentRef?.getElementById?.('inputLng'),
    locationDisplay = documentRef?.getElementById?.('horaryLocation'),
    useLocationButton = documentRef?.getElementById?.('useLocationBtn'),
    getTauriRuntime = () => false,
    geocodeLocation = async () => [],
    localGeocoder = { search: () => [] },
    setGeoLocation = () => {},
    setLocationStatus = () => {},
    showHoraryError = () => {},
    consoleRef = globalThis.console,
    setupAutocomplete = setupCityAutocomplete,
} = {}) {
    const searchLocationCandidates = async query => {
        if (getTauriRuntime()) {
            try {
                return await geocodeLocation({ query, limit: 8 });
            } catch (error) {
                consoleRef?.warn?.('Tauri geocode failed; falling back to bundled city list:', error);
            }
        }
        return localGeocoder?.search?.(query) || [];
    };

    const selectCityCandidate = candidate => {
        setGeoLocation(null);
        setLocationStatus('manualValid');
        if (latitudeInput) latitudeInput.value = candidate.latitude;
        if (longitudeInput) longitudeInput.value = candidate.longitude;
    };

    const requestGeolocation = () => {
        showHoraryError('');
        if (!navigatorRef?.geolocation) {
            setLocationDisplay('Geolocation not supported; enter coordinates manually.', 'location-display location-error');
            setLocationStatus('manualRequired');
            return;
        }

        setLocationStatus('detecting');
        setLocationDisplay('Acquiring location...', 'location-display location-acquiring');

        navigatorRef.geolocation.getCurrentPosition(
            position => {
                const latitude = position.coords.latitude;
                const longitude = position.coords.longitude;
                setGeoLocation({ lat: latitude, lng: longitude });
                setLocationStatus('detected');
                if (locationDisplay) {
                    locationDisplay.innerHTML = `<strong>Location acquired:</strong> ${latitude.toFixed(4)}°, ${longitude.toFixed(4)}°`;
                    locationDisplay.className = 'location-display location-acquired';
                }
                if (latitudeInput) latitudeInput.value = latitude.toFixed(4);
                if (longitudeInput) longitudeInput.value = longitude.toFixed(4);
            },
            error => {
                setLocationStatus('manualRequired');
                setLocationDisplay('Location unavailable; enter coordinates manually.', 'location-display location-error');
                showHoraryError(error?.message ? `Location unavailable: ${error.message}` : 'Location unavailable; enter coordinates manually.');
            },
            GEOLOCATION_OPTIONS
        );
    };

    const cleanupAutocomplete = setupAutocomplete({
        input,
        list,
        searchCandidates: searchLocationCandidates,
        onSelect: selectCityCandidate,
        documentRef,
    });

    const onUseLocationClick = () => {
        requestGeolocation();
    };

    useLocationButton?.addEventListener?.('click', onUseLocationClick);

    return {
        requestGeolocation,
        searchLocationCandidates,
        selectCityCandidate,
        cleanup() {
            useLocationButton?.removeEventListener?.('click', onUseLocationClick);
            cleanupAutocomplete?.();
        },
    };

    function setLocationDisplay(message, className) {
        if (!locationDisplay) return;
        locationDisplay.textContent = message;
        locationDisplay.className = className;
    }
}
