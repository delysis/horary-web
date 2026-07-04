# Architecture

## Active Shape

Horary is a Vite React app wrapped by a Tauri 2 desktop shell. The visible design and calculator workflow are owned by the upstream React files:

```text
index.html
  |
  v
src/main.tsx
  |
  v
src/App.tsx
  |-- question, time, location, settings, chart display
  |-- upstream ChartWheel and AspectGrid rendering
  |-- native-only Local AI panel
  |
  +--> src/chartCalc.ts
  |      upstream display chart calculation
  |
  +--> src/astro/*.js + src/ai/chartFacts.js
         richer deterministic facts for AI prompts only
```

The React UI keeps Eileen's current upstream layout and controls. The added AI path is an adapter under the existing calculator, not a replacement frontend.

## Native Boundary

The frontend talks to the desktop backend through Tauri IPC commands and Tauri events:

```text
React UI
  |
  | invoke(), listen()
  v
src/tauriBridge.ts
  |
  v
src-tauri/src/lib.rs
  |-- chart/settings storage commands
  |-- bundled city geocoder commands
  |-- model import/status/start/stop commands
  |-- interpretation stream commands
  |
  v
src-tauri/src/native_llama_worker.rs
```

The browser frontend never calls the local llama endpoint directly. The native build routes interpretation through in-process llama.cpp when compiled with `native-llama`; the sidecar path remains a fallback behind the same IPC surface.

## Local AI Flow

```text
Question + chart display state
  |
  v
AI fact adapter
  |-- full deterministic horary facts from src/astro/*.js when available
  |-- display-summary fallback from src/chartCalc.ts
  |
  v
Tauri start_interpretation_stream
  |
  v
ai.rs prompt builder
  |-- risk classifier
  |-- question context and house hints
  |-- chart evidence index
  |-- deterministic assignments
  |-- horary judgement plan
  |
  v
native_llama_worker.rs
  |-- continuous batching
  |-- hot prompt cache
  |-- cold disk KV cache
  |-- MTP speculative decoding when a draft model is available
  |
  v
schema validation and streamed React output
```

## Test Gates

The main gates are:

- `npm run check:all`: React/TypeScript build, JS/Rust tests, lint, audit, Rust fmt/clippy.
- `HORARY_NATIVE_LLAMA_REQUIRE_MTP=1 npm run check:native-llama`: real local Gemma GGUF native llama.cpp gate.
- `HORARY_NATIVE_LLAMA_REQUIRE_MTP=1 npm run check:horary-readings`: real local reading-eval gate over multiple horary scenarios.
- `npm run tauri -- build --debug --features native-llama-metal`: native desktop bundle build on Apple Silicon/Metal.

## Release Notes

The app identity is `Horary`, bundle identifier `app.horary.desktop`, and Rust crate `horary`. Any old-name storage keys are treated as pre-release artifacts, not product identity.

Remote model catalog entries remain disabled until a release artifact has a verified URL, license, size, checksum, RAM profile, context profile, and default generation parameters.
