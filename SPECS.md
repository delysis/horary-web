> The active interface is now the single-document conversation prototype. See [conversation architecture](docs/CONVERSATION.md). Earlier form-based interaction/validation descriptions below are historical and do not establish acceptance of the new conversation or audio path.

# Horary review build specification

Horary is a native Rust/Tauri app with a React chart interface; the same interface can run as a browser calculator. Eileen's interpretation method remains provisional and is explicitly available for correction in the The method & evidence.

## Main flow

1. Choose a location from offline search, request device location, or enter coordinates. Confirm its editable IANA timezone.
2. Use the current moment or enable past-question entry. Repeated civil times require an occurrence choice; nonexistent times are rejected.
3. Cast a chart. A question is optional for calculation. Choose display sections, day/night theme and 12/24-hour time in Settings.
4. Inspect the wheel, angles, houses, planets, aspects and method. Displayed chart values and interpretation facts come from the same calculation.
5. Optionally enable local AI. Select **Set up local model** to acquire the recommended Gemma 4 12B QAT target and multimodal projector. Fresh-machine setup needs no CLI, account, Python or existing weights. Downloads resume and share the Hugging Face cache. Once installed, readings run offline.
6. Read the chart, review reported evidence and inspect the actual process instructions. Changing the question or chart invalidates an earlier reading.
7. Record a general review note or suggest a change to a particular method step. Export notes and their frozen chart/reading context for sharing.

## Calculation

The shared Rust core resolves calendar and timezone input. `src/chartCalc.ts` adapts `src/astro/chart.js` into the wheel and table format. The approximate ephemeris uses tropical positions and Regiomontanus houses. Applying/separating reflects sampled planetary motion. Five displayed major aspects use a five-degree orb: conjunction, sextile, square, trine and opposition.

The golden ephemeris fixtures define tested dates and tolerances, not blanket certification of astronomical precision. Geographic poles are rejected. Subject-matter acceptance, exact timing and boundary cases need Eileen's review.

## Runtime and storage

- Native: Tauri IPC to an in-process llama.cpp worker through Rust. CPU enabled by default; macOS builds can use Metal. No inference sidecar required.
- Browser: optional local `.litertlm` inference with WebGPU and bundled same-origin LiteRT runtime. It does not share native GGUF setup.
- Shared core: safe Rust for prompts, schema parsing, risk checks, timezone/calendar operations and review-record validation; native and WASM builds.
- Models: immutable artifact pins, digest checks, resumable downloads and shared Hub cache; app data contains only a small registration for downloaded weights.
- Review notes and display preferences: local webview/browser storage. No automatic upload, account or synchronization.
- Packaged review apps: unsigned macOS app and Windows/Linux executable artifacts from CI. Signed distribution and platform-wide acceptance are separate release work.

See [architecture](docs/ARCHITECTURE.md), [model provenance](docs/MODEL_PROVENANCE.md), [review guide](docs/REVIEW_GUIDE.md) and [verification](docs/VERIFICATION.md) for detail and limits.
