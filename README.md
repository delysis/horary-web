# Whorary

Whorary is a browser-based astrology calculator for natal-style charts, horary chart casting, aspect tables, house cusps, and essential dignity reference tables.

The app is currently a Vite-powered vanilla JavaScript PWA with an early Tauri desktop shell. It performs chart calculations locally in the browser using the project modules under `src/astro/`. It does not call an online geocoder, timezone API, or remote AI service.

## Current Status

This is still a prototype, not a production astrology authority. The chart engine uses compact Meeus-style algorithms and simplified planet models. That is useful for development and UI testing. Offline golden tests now cover modeled-body longitude, one-hour aspect phase, sign-boundary checks, and station-adjacent Mercury motion against NASA/JPL Horizons plus Regiomontanus angles/house cusps against Swiss Ephemeris across DST, southern-hemisphere, high-latitude, Tokyo, and historical cases. Serious horary work still needs broader reference coverage across more planets, longer timing paths, and external ephemeris-grade certification.

Recent hardening work added:

- Centralized chart input validation for date, time, UTC offset, latitude, and longitude.
- Explicit browser-location action for horary charts.
- No fallback to implicit 0,0 or New York coordinates.
- Applying/separating aspects based on recalculated future positions.
- Centralized horary defaults and settings controls for aspect definitions/orbs, house system, zodiac, and planet set.
- Settings import/export for house system, planet set, and aspect policy JSON.
- Accessible tab markup/state for tab buttons, tab panels, keyboard navigation, and the settings shortcut.
- No remote font imports.
- Lint guard against hidden browser runtime network calls, external geocoder/timezone providers, and beacon-style telemetry.
- Tauri desktop CSP and least-privilege capability checks in the normal lint gate.
- Conditional Vite base/PWA behavior: web builds scope the manifest and service worker to `VITE_BASE_PATH`, while Tauri builds skip PWA registration.
- Package-backed app version display in the About panel so users can identify the loaded build.
- CI coverage for web checks, coverage, audit, Rust/Tauri checks, Tauri asset-mode build, and cross-platform debug no-bundle desktop builds.
- Minimal Tauri 2 desktop scaffold with a `get_app_info` smoke command.
- Tauri chart storage commands wired into SQLite-backed desktop horary history save/load.
- Horary history actions for loading, exporting JSON, and deleting saved chart records.
- Tauri settings storage commands for desktop house-system, planet-set, and aspect-policy persistence.
- Tauri offline geocoding commands for bundled city autocomplete and nearest-city lookup.
- Tauri model registry, native inference boundary, and `llama-server` sidecar lifecycle scaffold for local GGUF models.
- Tauri command and desktop UI path for installing a native local `llama-server` binary into app data.
- Sidecar status command and resolver that checks bundled Tauri resource binaries before app-data fallback binaries.
- Sidecar fallback startup waits for loopback `/health` or `/v1/models` readiness before marking a local model as running.
- Native llama.cpp runtime profile with optional `llama-cpp-2` Rust binding features for release builds.
- Bundled model manifest format plus checksum-verified model download command.
- AI prompt/schema boundary that keeps local models as interpreters of deterministic chart facts.
- Versioned traditional horary judgement pipeline with microtasks, domain modules, parallel-pass structure, cache policy, and required `judgementTrace` output.
- Tauri `generate_interpretation` command through the native local inference boundary.
- Tauri streamed interpretation command with token events and generation cancellation.
- Tauri-only Horary panel for local model import, catalog download, status, start/stop, streamed interpretation, and cancellation.
- Deterministic horary factors include sect, Ascendant ruler, planetary day/hour, essential and accidental dignities, solar condition, receptions, void-of-course Moon, antiscia/contra-antiscia, applying/separating aspects, translation/collection/prohibition/frustration candidates, and Part of Fortune.
- Offline JPL Horizons longitude/aspect-phase/motion-edge fixtures and Swiss Ephemeris Regiomontanus house fixtures for golden regression tests.
- Unit tests for validation and aspect phase behavior.

## Local Development

```bash
npm ci
npm run dev
```

Open the local URL printed by Vite.

## Verification

```bash
npm run lint
npm test
npm run test:coverage
npm run build
npm audit
npm run check:tauri
npm run check:release-assets
npm run preview
npm run tauri dev
npm run tauri:build:release
cargo test --manifest-path src-tauri/Cargo.toml
```

`npm run lint` checks JavaScript syntax, blocks remote font references, enforces the browser runtime network policy, and verifies the desktop CSP/capability baseline. `npm run check` runs lint, Node tests, and the web build. `npm run check:tauri` runs Rust formatting, tests, and clippy. `npm run check:all` runs the local web, coverage, audit, and Rust/Tauri verification gates. `npm run tauri:build:release` builds with `src-tauri/tauri.release.conf.json`. `npm run check:release-assets` validates the native llama.cpp runtime profile, optional `llama-cpp-2` Cargo feature wiring, any present fallback sidecar checksums, and any enabled verified model manifest entries.

CI runs web checks on Ubuntu and Tauri Rust checks plus a debug no-bundle desktop build on macOS, Windows, and Linux. The cross-platform CI build deliberately avoids release bundling so signing, notarization, updater configuration, optional fallback sidecar binaries, and verified catalog model artifacts remain explicit release choices instead of hidden CI assumptions.

Set `VITE_BASE_PATH=/some-subpath/` when building for a subpath deployment. The Vite base, manifest `start_url`, manifest `scope`, and service-worker registration scope are kept aligned. The About panel renders the package version injected at build time. Tauri builds use `TAURI_ENV_PLATFORM` and omit the PWA plugin output.

## Privacy And Network Behavior

See [docs/PRIVACY.md](docs/PRIVACY.md). In short: chart calculation is local, city autocomplete uses a bundled city list in the browser or Tauri backend, and browser geolocation is requested only after the user clicks `Use Browser Location`.

## Calculation Assumptions

- Tropical zodiac is the implemented calculation mode; sidereal mode is not exposed until it has a tested calculation path.
- House systems are implemented in `src/astro/houses.js`; Regiomontanus is the horary default.
- Manual UTC offset is the explicit timezone authority for chart form input.
- Chart metadata and AI chart facts preserve that manual offset as `UTC±HH:MM`; they do not infer timezone from the browser locale.
- Aspect phase is computed by recalculating positions one hour after the chart moment and comparing orb change.
- Void-of-course Moon is computed by scanning hourly future positions until the Moon leaves its current sign and checking for a sampled classical major aspect within 0.5° of perfection.
- Solar condition is classified by geocentric elongation from the Sun: cazimi within 17 arcminutes, combust within 8.5°, and under beams within 17°.
- Accidental dignity currently combines house angularity, retrograde status, solar condition, and planetary joy. It is a deterministic condition score, not a full traditional judgment.
- Planetary day/hour uses NOAA-style sunrise/sunset and a local mean solar date derived from longitude; it does not use a timezone database.
- Translation, collection, and prohibition/frustration are candidate patterns ordered by a bounded future aspect-perfection search with sign-boundary and station flags. They are still candidates, not full horary judgment.

## Desktop And AI Roadmap

Do not wrap the current app blindly and call it production-ready. The intended desktop route is direct Tauri integration with a local-first architecture. The frontend talks to native code through Tauri IPC, not a browser-visible loopback API. Local AI interpretation is optional and receives deterministic chart facts rather than calculating chart positions itself.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the current architecture and planned Tauri/llama.cpp direction. The current Tauri scaffold can discover a bundled `llama-server` sidecar when release binaries are supplied, or install a local native sidecar into app data for development/user-managed setups; that sidecar's loopback endpoint is isolated behind the Rust inference boundary. The release target is in-process llama.cpp on a native worker thread with the sidecar retained as fallback. Release readiness is now gated by `src-tauri/native-llama-runtime.json` and the optional `native-llama` Cargo feature rather than by mandatory fallback sidecar artifacts. See [docs/AI_POLICY.md](docs/AI_POLICY.md) for the AI interpretation boundary.
