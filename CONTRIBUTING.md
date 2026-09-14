# Contributing

## Setup

Install Rust 1.95.0 (the CI toolchain), Node.js 22+, CMake and the platform-specific Tauri prerequisites. No Python environment is required.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.13.1 --locked
npm ci
npm run dev
```

## Required Checks

Run these before handing off changes:

```bash
npm run check
npm run test:coverage
npm audit
npm run check:tauri
```

Use `npm run check:all` for the same required local verification sequence in one command.

`npm run check:release-assets` is a release gate, not a development gate. It validates the native llama.cpp runtime profile, native-kit provenance and feature wiring, any present fallback sidecar binaries/checksums, and any enabled verified model manifest entries.

`npm run check:native-llama` is the hardware-backed native inference gate. It requires a local cached Gemma GGUF or `HORARY_NATIVE_LLAMA_TEST_MODEL=/path/to/model.gguf`. Speculative decoding is not exposed by this pinned runtime; requesting an MTP gate fails explicitly.

For desktop packaging changes on macOS, also run:

```bash
TAURI_ENV_PLATFORM=darwin npm run build
npm run tauri -- build --debug --features native-llama-metal --bundles app
```

## Testing Expectations

Add node:test coverage for changes under `src/astro/`. Validation and chart calculation changes should include explicit boundary cases, not only "returns a value" tests.

Useful test categories:

- Date/time parsing and invalid calendar dates.
- UTC offset conversion.
- Latitude/longitude boundaries.
- Aspect phase changes around exact aspects.
- House-system edge cases at high latitude.
- Golden chart fixtures checked against an external ephemeris source.

## Release Notes

Before any public desktop or PWA release, update:

- `README.md`
- `docs/PRIVACY.md`
- `docs/ARCHITECTURE.md`
- The version in `package.json`

Run `npm ci`, `npm run check:all`, `npm run check:release-assets`, and the macOS desktop packaging checks from a clean checkout. Public desktop releases additionally need signing, notarization, and updater configuration.
