# Architecture

## Current Web App

Whorary is currently a Vite app with a vanilla JavaScript UI controller.

```text
index.html
  |
  v
src/main.js
  |-- chart form and horary UI state
  |-- validation and rendering orchestration
  |-- city autocomplete via browser or Tauri local geocoder
  |
  v
src/astro/
  |-- chart.js: chart orchestration and tropical chart result assembly
  |-- time.js: Julian date and sidereal time helpers
  |-- planets.js: compact planetary position algorithms
  |-- houses.js: house-system calculations
  |-- antiscia.js: antiscion and contra-antiscion contact detection
  |-- aspectPerfections.js: bounded future aspect-perfection search
  |-- aspects.js: aspect detection and phase calculation
  |-- horarySettings.js: default house, zodiac, planet-set, aspect, and orb policy
  |-- dignities.js: essential dignity helpers
  |-- accidentalDignity.js: house angularity, motion, solar-condition, and joy-based accidental dignity helpers
  |-- solarCondition.js: cazimi, combust, and under-beams detection
  |-- lots.js: deterministic traditional lot calculations such as Part of Fortune
  |-- planetaryHours.js: planetary day/hour from sunrise and sunset
  |-- receptions.js: deterministic reception detection from dignity rulers
  |-- timingPatterns.js: translation, collection, and prohibition/frustration candidates
  |-- voidOfCourse.js: deterministic Moon void-of-course scan
  |-- chartInput.js: date/time/coordinate validation

src/ui/
  |-- html.js: shared HTML escaping helper for string-rendered view fragments
  |-- renderers.js: chart wheel, result tables, and aspect grid/list rendering
  |-- horaryRenderers.js: Horary-specific positions, factors, reception, antiscia, timing, and applying-aspect snippets
  |-- horaryDashboard.js: Horary tab dashboard composition
  |-- tabsController.js: accessible tab activation, keyboard traversal, and settings shortcut wiring

src/location/
  |-- coordinates.js: horary coordinate resolution with no implicit fallback
  |-- geocoder.js: offline bundled-city geocoder provider and in-memory query cache
  |-- locationController.js: browser geolocation and city autocomplete wiring

src/data/
  |-- cities.json: shared offline city catalog for browser and Tauri geocoding

src/app/
  |-- appDom.js: browser DOM boot helpers, Horary error surface updates, and JSON downloads
  |-- appState.js: initial shared app state and persisted settings payload boundary

src/desktop/
  |-- desktopRuntimeController.js: Tauri runtime startup, desktop settings sync, and native-only UI reveal

src/history/
  |-- horaryHistory.js: saved-chart serialization and web/Tauri history normalization

src/horary/
  |-- horaryController.js: Horary casting, saved-chart actions, history rendering, and clock wiring
  |-- horaryHistoryView.js: saved-chart history list markup and export filename helpers

src/ai/
  |-- aiPanelController.js: Local AI panel event wiring, Tauri command workflow, and stream state
  |-- aiPanelView.js: Local AI panel option/status/output rendering helpers
  |-- promptBuilder.js: deterministic-facts-only AI prompt construction
  |-- interpretationSchema.js: frontend interpretation constants and shape validator
  |-- horary-interpretation.schema.json: shared JSON schema used by frontend and Tauri prompts
  |-- horary-judgement-pipeline.json: summarized traditional horary rule scaffold, microtasks, domain modules, cache policy, and verifier checklist
  |-- horaryJudgementPipeline.js: compact runtime judgement-plan builder

src-tauri/src/
  |-- geocode.rs: offline bundled-city geocoding commands and in-memory query cache
  |-- inference.rs: fallback sidecar inference boundary used when the in-process native worker is not running
  |-- ai.rs: local model prompt, inference request, stream handling, judgement trace parsing, and JSON response validation
  |-- native_llama.rs: optional llama-cpp-2 runtime metadata and release-target capability surface
  |-- model_manifest.rs: bundled model catalog parsing and checksum-verified downloads
```

`src-tauri/native-llama-runtime.json` is the release profile for the preferred native llama.cpp backend.

The web app stores user settings and horary history in `localStorage`. In Tauri, house-system, planet-set, and aspect-policy settings are synchronized to `settings.json`, and cast horary chart summaries/payloads are saved to `charts.sqlite3` under the operating system app data directory. Existing `charts.json` history is migrated into SQLite on first chart storage access.

Web builds use `VITE_BASE_PATH` to keep the Vite base, PWA manifest `start_url`, manifest `scope`, and service-worker registration scope aligned for root or subpath hosting. The Vite config also injects the package version for the About panel so the loaded build is visible at runtime. Tauri builds set `TAURI_ENV_PLATFORM`, use a relative asset base, and disable PWA plugin output so the desktop app does not register a service worker.

## Calculation Flow

1. The user enters local date, local time, manual UTC offset, latitude, and longitude.
2. `validateChartInput()` rejects malformed or out-of-range values.
3. The valid local date/time is converted to a UTC `Date` using the manual UTC offset, and the local date/time/offset metadata is retained on the chart result.
4. `calculateChart()` in `src/astro/chart.js` converts the UTC date to Julian Date.
5. Planet positions, house cusps, aspects, essential and accidental dignities, planetary day/hour, receptions, void-of-course Moon, antiscia contacts, timing-pattern candidates, and Part of Fortune are computed locally.
6. Aspect phase is determined by calculating positions one hour after the chart moment and comparing future orb to current orb.

The manual UTC offset is currently the timezone authority. There is no timezone lookup service. AI chart facts use explicit chart metadata such as `UTC-04:00` or fall back to `UTC`; they do not infer timezone from the browser locale.

## Known Limits

- The chart engine is backed by offline JPL Horizons longitude fixtures for the modeled bodies and Swiss Ephemeris fixtures for Regiomontanus angles/house cusps.
- Representative external golden fixtures now cover sign-boundary and station-adjacent cases, but broader external coverage is still needed across more planets, longer timing paths, and ephemeris-grade horary timing scenarios.
- The UI controller is still larger than ideal; initial app state, browser DOM boot/download helpers, generic and Horary-specific renderers, tab navigation, desktop runtime/settings startup, Horary dashboard composition, Horary casting/history orchestration/history list helpers, coordinate resolution, bundled-city geocoder providers, autocomplete/location behavior, chart field-error rendering, settings payload/markup/controller helpers, chart calculation input wiring, local AI panel view/controller helpers, PWA install prompt behavior, and horary history persistence have been split out. Future work should continue trimming boot sequencing and shared state wiring as stable controller seams emerge.
- Navigation tabs now maintain `role=tab`/`tabpanel`, `aria-selected`, panel hiding, keyboard navigation, and settings button expansion state; broader component-level accessibility review is still needed.
- Aspect definitions, default orbs, and core horary defaults are centralized in `src/astro/horarySettings.js`; the Settings tab exposes aspect enable/orb controls backed by web and desktop settings storage.
- The Settings tab can export/import house-system, planet-set, and aspect-policy JSON. Imports are normalized through the same settings validation path used by persisted settings.
- The horary factor model includes sect, Ascendant ruler, planetary day/hour, essential and accidental dignities, solar condition, receptions, void-of-course Moon, antiscia/contra-antiscia, applying/separating aspects, translation/collection/prohibition/frustration candidates, and Part of Fortune.
- Accidental dignity currently scores classical planets from house angularity, retrograde state, solar condition, and planetary joy. It is a deterministic condition model, not a complete accidental dignity doctrine.
- The void-of-course Moon check scans hourly future positions until the Moon leaves its current sign and looks for a sampled classical major aspect within 0.5° of perfection. This is a deterministic guardrail, not a replacement for a full ephemeris-grade timing solver.
- Planetary day/hour uses NOAA-style sunrise/sunset, local mean solar date from longitude, and Chaldean hour rulers. It is deterministic and offline, but it does not resolve civil timezones or polar-day/polar-night edge cases.
- Translation, collection, and prohibition/frustration are labeled as candidates because the detector now orders current applying major aspects with a bounded future perfection search and sign-boundary/station flags, but it does not replace full horary judgment around reception, prohibition type, house context, or external ephemeris certification.
- Sidereal zodiac is not exposed until a tested calculation path is implemented.

## Golden Ephemeris Fixtures

`src/astro/__fixtures__/jpl-horizons-longitudes.json` stores offline reference values retrieved from the NASA/JPL Horizons API on 2026-06-30. The current regression test compares calculated ecliptic longitude for the modeled bodies against those fixtures across DST transition instants, southern-hemisphere geometry, Tokyo, Reykjavik high latitude, and a pre-1900 historical chart. Sun and Moon tolerances are tight; planetary tolerances are wider because the current engine uses compact Meeus/orbital-elements approximations.

`src/astro/__fixtures__/jpl-aspect-phases.json` stores JPL current and one-hour-future longitude pairs for representative applying and separating aspects. The golden test checks fixture self-consistency, phase direction, and rough orb agreement with the app's one-hour aspect phase calculation.

`src/astro/__fixtures__/jpl-motion-edge-cases.json` stores JPL current and 24-hour-future Mercury longitudes around 2024 station-adjacent moments. The golden test checks longitude, daily motion direction, and the app retrograde flag. Selected JPL longitude fixtures also assert calculated sign index near solar ingress boundaries.

`src/astro/__fixtures__/swiss-regiomontanus-houses.json` stores Swiss Ephemeris references for ASC, MC, Vertex, and all 12 Regiomontanus house cusps across DST transition instants, southern-hemisphere geometry, Tokyo, Reykjavik high latitude, and a pre-1900 historical chart.

This is a stronger guardrail, not a complete astrology validation suite. Unit tests cover the local sign-boundary and station flags in the timing search, while the JPL edge fixtures pin representative external cases. The next reference layer should broaden these cases across more bodies and timing patterns.

## Current Tauri Backend

`src-tauri/` contains the Tauri 2 desktop backend:

- `tauri.conf.json` points dev mode at Vite and production at `../dist`.
- `tauri.release.conf.json` is a release-only config overlay that can still declare fallback `llama-server` sidecar packaging. Ordinary debug/dev builds do not require sidecar binaries.
- `src/lib.rs` registers the desktop IPC commands, routes model startup to the native worker when compiled with `native-llama`, and falls back to the managed sidecar path otherwise.
- `src/storage.rs` stores saved chart history in SQLite and desktop settings JSON in the app data directory.
- Registered chart storage commands: `save_chart`, `list_charts`, `get_chart`, and `delete_chart`.
- Registered settings storage commands: `get_settings` and `save_settings`.
- `src/geocode.rs` provides offline `geocode_location` and `reverse_geocode_location` commands over the shared bundled city catalog, with a bounded in-memory query cache for desktop autocomplete.
- The Horary tab can load saved chart payloads back into the chart view, export saved chart JSON through a local browser download, and delete history records. Desktop builds load/delete via native storage commands; browser builds use `localStorage`.
- `src/native_llama.rs` exposes runtime metadata for the preferred native llama.cpp backend. Cargo feature `native-llama` wires optional `llama-cpp-2` Rust bindings, with `native-llama-metal`, `native-llama-cuda`, `native-llama-vulkan`, and `native-llama-dynamic-backends` variants for release builds.
- `src/llama.rs` manages imported `.gguf` model files, app-data native sidecar installation, and the `llama-server` sidecar lifecycle scaffold used as a fallback.
- `src/native_llama_worker.rs` owns the feature-gated in-process llama.cpp backend. In native builds it loads the GGUF model on a single worker thread, accepts generation jobs over channels, batches prompt/decode work across sequence IDs, keeps a hot resident sequence-state cache, persists deterministic prefix state to disk, and exposes health/cache counters over Tauri IPC.
- Registered model/sidecar commands: `list_models`, `import_model`, `import_llama_sidecar`, `get_model_info`, `get_model_status`, `get_llama_sidecar_info`, `start_llama`, and `stop_llama`.
- `src/model_manifest.rs` parses `src-tauri/model-manifest.json` and installs manifest models only after HTTPS/loopback download and SHA-256 verification.
- Registered model acquisition commands: `get_model_manifest` and `download_model`.
- `src/inference.rs` is the local inference boundary for the fallback sidecar. Native builds route interpretation generation directly through `src/native_llama_worker.rs` when that worker is running, without browser-visible loopback HTTP.
- `src/ai.rs` sends deterministic chart facts and the compact horary judgement plan through the local inference boundary, streams token chunks through Tauri events, supports cancellation, and validates final schema-shaped JSON output including `judgementTrace`.
- Registered interpretation/runtime commands: `generate_interpretation`, `start_interpretation_stream`, `cancel_interpretation_stream`, and `get_native_llama_runtime_info`.
- `src/tauriBridge.js` calls native commands only when the app is running in Tauri.
- The Horary tab exposes a Tauri-only local AI panel for installing a native local `llama-server` binary, BYO `.gguf` import, verified catalog download, model refresh, start/stop, streamed interpretation generation, and cancellation.
- The capability file grants only `core:default`. The Tauri config also sets an explicit production CSP that allows local app assets, inline styles currently required by existing markup, Tauri IPC (`ipc:` and `http://ipc.localhost`), local/data images, and blocks object embeds, base URI changes, and framing. `npm run lint` verifies the CSP baseline and capability permissions.

The scaffold still does not include online geocoding provider configuration, signed production model manifests, verified remote model entries, signing, or updater configuration. The native decode worker exists and is gated by `npm run check:native-llama` against a real cached Gemma GGUF on local hardware. `npm run check:release-assets` validates `src-tauri/native-llama-runtime.json`, optional `llama-cpp-2` Cargo feature wiring, any present fallback sidecar checksums, and any enabled verified model entries. Empty remote catalog manifests are allowed because user-imported GGUF files are the supported model acquisition path until a model artifact is actually verified.

## Tauri Direction

Use direct Tauri, not Pake, for a real desktop product. Pake is acceptable only for a disposable window wrapper demo.

Recommended desktop shape:

```text
Vite frontend
  |
  | invoke()
  v
Tauri Rust backend
  |-- settings and chart storage
  |-- local geocoder provider/cache and future online provider boundary
  |-- model registry
  |-- model manifest and checksum verifier
  |-- local inference boundary
  |-- in-process llama.cpp worker backend (target)
  |-- llama.cpp sidecar manager (current fallback)
  |-- logs and import/export
  |
  v
local GGUF inference backend
  |
  v
local GGUF model files in app data
```

The Vite config uses `TAURI_ENV_PLATFORM` to switch the base path to `./`, disable PWA generation during Tauri builds, and inject the package version for the runtime About panel.

## Native Inference Direction

The frontend should never need an HTTP loopback API. The shipped app boundary is Tauri IPC: the UI calls commands such as `start_llama`, `start_interpretation_stream`, and `cancel_interpretation_stream`, and receives stream chunks through Tauri events. Any HTTP involved in the current build is strictly a Rust-backend implementation detail for the managed `llama-server` sidecar.

The release target is an in-process llama.cpp backend described by `src-tauri/native-llama-runtime.json` and compiled through the optional `native-llama` Cargo feature. The implemented worker thread owns the llama.cpp backend, model, context, samplers, token history, hot resident sequence state, and cold disk sequence state. Tauri state holds a handle/channel to that worker, not a self-referential model/context struct. The current sidecar backend remains useful as a fallback and development bridge.

The remaining native inference path is:

1. Keep the existing Tauri commands and frontend state machine stable.
2. Add JSON/grammar constrained native sampling where the binding exposes a stable interface, while keeping the final schema validation in `ai.rs`.
3. Keep KV cache limited to deterministic prompt prefixes keyed by model hash, llama.cpp binding/build, context size, prompt/schema/tradition/pipeline versions, and generation settings.
4. Use continuous batching by default for parallel microtask requests, with a default of four slots and a bounded maximum of sixteen.
5. Implement the draft-MTP bridge only through a stable raw/sys or vendored binding surface. Native startup currently rejects draft/MTP model requests rather than silently ignoring them.
6. Remove the sidecar install UI and `bundle.externalBin` only after the native worker fully replaces the fallback path.

## Horary Judgement Pipeline

The AI should interpret deterministic chart facts, not calculate the chart. The prompt input includes question text, chart time, explicit timezone/offset metadata, location, angles, house cusps, body positions, aspects, and derived horary factors. The model output is schema-validated and must cite chart factors for each interpretive claim.

The prompt carries explicit prompt, schema, tradition-profile, and judgement-pipeline versions so instruction changes, JSON shape changes, and horary-prose doctrine changes can be tracked separately. Frontend and Tauri prompt builders use the shared `src/ai/horary-interpretation.schema.json` response schema and are tested to keep prompt/schema/profile metadata synchronized. The current tradition profile instructs local models to write in a traditional horary judgement order: querent/quesited, classical significators, Moon, applying Ptolemaic aspects, reception, dignity, accidental strength, perfection or blockage, and concise evidence-based judgement before uncertainty.

`src/ai/horary-judgement-pipeline.json` summarizes the local Frawley OCR into a machine-readable scaffold: setup passes, condition passes, event/perfection passes, timing, domain modules, a claim-level checker output contract, parallel groups, and cache policy. In the current single-call path, this is included as a compact `judgementPlan` and the model must return a bounded `judgementTrace`. In the intended multi-call path, deterministic chart code supplies feature facts and small local LLM calls run narrow passes such as house assignment, reception, accidental strength, prevention, domain-specific checks, and synthesis. The stable microtask prompts are designed for KV-cached prefix reuse by an in-process backend.

The current sidecar scaffold imports `.gguf` files into the app data model directory, can copy a local native `llama-server` binary into the app data `binaries/` directory under the current target-triple name, validates model filenames, hashes model files, starts `llama-server` on a random loopback port when a sidecar binary is installed, waits for `/health` or `/v1/models` on that loopback endpoint before marking the model as running, and writes logs under the app data log directory. It launches with continuous batching, four default parallel slots, prompt cache, bounded RAM cache, idle-slot cache, and a disk slot-save path under app data for cold KV reuse. If a draft model is explicitly selected, or if an imported `<model-id>-assistant.gguf` model exists, it adds draft-MTP speculative decoding flags with bounded draft-token count and keeps the draft model on CPU by default to protect VRAM. If startup times out or the child exits early, the process is cleaned up and the error points to the local log file. Later status/connection checks also clear exited child processes so a crashed sidecar is not reported as still running. `get_llama_sidecar_info` reports whether a runnable sidecar was found. Resolution checks the Tauri resource directory first, looking for `binaries/llama-server-<target-triple>`, `binaries/llama-server`, `llama-server-<target-triple>`, and `llama-server`; it then falls back to the same target-specific and plain names under the app data `binaries/` directory. This supports release sidecar fallback while allowing local native sidecar installation for development and user-managed setups.

The backend validates launch arguments before spawning the sidecar. Context size, parallelism, GPU layer controls, RAM cache size, and speculative draft-token count are bounded and normalized; model paths must resolve inside the app data model directory. The bundled model manifest is intentionally empty until release-time artifact verification is complete; the downloader requires a manifest `source.url` and SHA-256 before it will install a model. Enabled catalog entries still require `license`, `minRamGb`, `recommendedRamGb`, `defaultContextTokens`, and `defaultParams.temperature`, `defaultParams.top_p`, and `defaultParams.max_tokens` so a catalog entry is a tested runtime profile rather than just a URL. `generate_interpretation` provides a blocking JSON path, while `start_interpretation_stream` uses the local inference boundary, emits token chunks through Tauri events, validates the final JSON, and rejects outputs that omit required chart evidence or judgement trace. It deliberately does not spawn arbitrary commands.

See `docs/AI_POLICY.md` for the interpretation boundary and high-stakes topic policy.
