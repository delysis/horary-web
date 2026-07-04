# Privacy And Network Behavior

## Summary

Horary is local-first in its current form. Chart calculation runs locally in the browser/webview. City search uses the bundled city catalog in the browser build and the Tauri backend in desktop builds. Desktop AI interpretation uses local GGUF models behind Tauri IPC. Browser builds keep settings in browser storage; desktop builds save settings and local model data under the operating system app data directory.

## Browser Geolocation

The current upstream React UI requests browser geolocation when the app opens so it can prefill the chart location.

When location is granted:

- The browser provides latitude and longitude to the app.
- The coordinates are copied into the latitude/longitude inputs.
- The coordinates are matched locally against the bundled city catalog for a nearby city/timezone label.

If the user denies location, they can enter coordinates manually or select a bundled city.

## City Search

City autocomplete uses the shared bundled city catalog in `src/data/cities.json`. Browser builds query it through `src/location/geocoder.js`; desktop builds query it through the Tauri `geocode_location` command in `src-tauri/src/geocode.rs`. Repeated local queries are cached in memory. It does not call an online search provider.

## External Services Not Used

The current app does not use:

- Nominatim
- OpenStreetMap API calls
- timeapi.io
- Remote AI APIs
- Remote font APIs

The CSS intentionally avoids Google Fonts imports, so loading the app does not make hidden font requests.

`npm run lint` enforces this browser runtime boundary by failing on direct browser network primitives, external geocoding/timezone provider references, remote HTTP(S) URLs, and beacon-style telemetry in runtime source files. Tests and offline reference fixtures may contain example URLs, but the browser app code may not.

## PWA Service Worker

For web builds, the checked-in service worker handles same-origin app files for GitHub Pages deployment. For Tauri builds, the Vite base path switches to relative assets through `TAURI_ENV_PLATFORM`.

## Local Storage

The web app stores:

- House system setting
- Planet set setting
- Display and location input preferences

These values are stored in the browser's `localStorage` for the current origin.

## Desktop App Data

The Tauri desktop shell saves settings and model data through native commands. The native storage layer also supports saved horary chart history in `charts.sqlite3` for the fuller desktop workflow. These files stay local unless the user backs up, syncs, or shares that directory outside the app.

The Horary history export action creates a local JSON download in the browser/webview. That file contains the saved question, chart factors, coordinates, and calculation settings for that saved chart.

The settings export action creates a local JSON download containing house-system, planet-set, and aspect-policy settings. The settings import action reads a user-selected local JSON file in the browser/webview and applies only recognized settings fields.

Imported local AI models are copied into the app data model directory as `.gguf` files. A user-selected native `llama-server` binary can also be copied into the app data `binaries/` directory for desktop local inference. The desktop shell can install a model from a bundled manifest entry, but only when that entry includes a download URL, SHA-256 checksum, license, RAM/context metadata, and bounded default generation parameters. The current bundled manifest has no enabled remote model entries.

Starting the current sidecar fallback uses the bundled or app-data `llama-server` on `127.0.0.1` with a random local port, waits for a local readiness response, and writes local logs under the app data log directory. The frontend does not call that loopback endpoint directly; it uses Tauri IPC. The native release path is an in-process llama.cpp backend behind the same Tauri command boundary, with the sidecar backend retained only as a fallback.

When the fallback Tauri interpretation commands use `llama-server`, they send the question, chart facts, and interpretation settings to the loopback endpoint on the same machine. Streamed interpretation sends token chunks back to the frontend through local Tauri events. It does not send this data to a remote AI provider.

## Future Online Geocoding

If online geocoding is added later, it must be behind an explicit user action and a provider abstraction. It should include rate limiting, caching, visible attribution, and a clear privacy note before precise coordinates or search text are sent to a third party.
