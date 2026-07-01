import { validateCoordinates } from '../astro/chartInput.js';

export function resolveHoraryCoordinates({ geoLocation, manualLat, manualLng }) {
    const raw = geoLocation
        ? { lat: String(geoLocation.lat), lng: String(geoLocation.lng) }
        : { lat: manualLat, lng: manualLng };

    return validateCoordinates(raw);
}
