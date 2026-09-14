# Architecture

Horary uses a React view inside a Tauri 2 desktop application. The shared Rust crate is `crates/horary-ai-core`; its WebAssembly exports are built for browser/webview use and Node-based regression tests. There is no Python build or runtime dependency in the supported CPU/Metal application paths. The pinned CPU/Metal binding builds the native libraries without Python tooling.

## Chart authority

`src/App.tsx` resolves the entered civil time and IANA timezone through Rust (`chart_input.rs`) into a single UTC instant. The same module validates DMS components and performs calendar nudges without the host machine’s timezone affecting them.

`src/chartCalc.ts` adapts `src/astro/chart.js` to Eileen’s existing chart wheel and tables. That result also feeds `src/ai/chartFacts.js`. The previous separate display calculation and fallback AI chart are no longer used by the app. The active display aspect policy is five major aspects with a five-degree orb. Rust applies the source-specific method rules and independently brackets upcoming contacts from seven days of hourly ephemeris samples. The method inspector identifies sign changes and estimated timing. Other legacy standalone UI modules remain outside the React entrypoint.

The astronomical engine is approximate. The committed golden fixtures cover positions, house cusps, aspect phase, and motion edges at their recorded tolerances. The largest planetary tolerances are unsuitable for treating sign-boundary or exact-timing results as professionally certified; Eileen should compare such cases with her trusted ephemeris.

## Interpretation

`judgementEngine.ts` owns one active reading, cancellation, model setup, and cleanup. The desktop adapter uses Tauri IPC and filters events by generation ID, including events received before the start reply. The browser adapter uses a user-selected local LiteRT model and same-origin bundled runtime assets. It never selects a hosted inference fallback.

The Rust core owns prompt construction, provisional question/house hints, schema parsing, and risk checks. Tauri calls the same native crate; browser execution uses its WASM exports. Versioned prompt metadata and output limits are included in review evidence.

Native inference is enabled by default through the pinned native-kit host and engine. The kit selects CPU/Metal by platform. Horary keeps one model resident and uses its own chat template with native schema-constrained sampling. Prompt caches remain memory-only; constrained readings do not promise prefix reuse. The API does not expose speculative decoding. Model/projector artifacts are distinct, and only full models appear in the picker. The old sidecar remains an explicit compatibility path for builds without native inference.

## Model acquisition

The native Rust module `hf_cache.rs` implements the narrow pinned-artifact subset of the [Hub cache protocol](https://huggingface.co/docs/hub/cache). It uses reqwest, SHA-256, OS file locks and atomic publication. No CLI or Python is invoked. Cache hits require no network. Interrupted downloads retain bytes, validate HTTP range responses, and verify the complete digest before publication. Cancellation also interrupts pending network reads. On Unix snapshots are relative symlinks; on Windows they are hardlinks, so no symlink privileges or duplicate weights are needed. Filesystems without hardlink support produce an explicit error instead of silently copying weights.

The app registers the cache location only after both model and projector pass verification; it loads blobs directly and verifies them again on model startup. Source revisions and hashes are in `src-tauri/model-manifest.json`. See [the model provenance record](MODEL_PROVENANCE.md).

## Review interface

`MethodReview.tsx` displays the checked-in process instructions, provisional house suggestions, and any corresponding model-reported steps. The current runtime sends this scaffold in one model call; the future multi-call plan is not presented as implemented.

`ReviewNotes.tsx` captures a frozen question/chart/reading/method-step context. Rust validates stored review records before appending to them. Failed writes keep the draft visible and leave existing records intact. Export is a local JSON download, never an automatic upload.

## Packaging

macOS builds target 11.0 or newer. Tauri’s packaged CSP permits the shared WASM module, same-origin assets, and IPC. The only added frontend capability is opening the native model chooser. Browser LiteRT binaries are excluded from native build output.

Desktop CI builds the actual native backend through the pinned native-kit dependency and attaches reviewer artifacts. Signed/notarized distribution and model redistribution are separate release work, not represented as complete by a review artifact.
