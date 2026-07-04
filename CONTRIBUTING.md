# Contributing

## Setup

```bash
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

`npm run check:release-assets` is a release gate, not a development gate. It validates the native llama.cpp runtime profile, `llama-cpp-2` feature wiring, any present fallback sidecar binaries/checksums, and any enabled verified model manifest entries.

`npm run check:native-llama` is the hardware-backed native inference gate. It requires a local cached Gemma GGUF or `HORARY_NATIVE_LLAMA_TEST_MODEL=/path/to/model.gguf`. Use `HORARY_NATIVE_LLAMA_REQUIRE_MTP=1` when validating that an MTP draft GGUF is available for the release target.

For desktop packaging changes on macOS, also run:

```bash
TAURI_ENV_PLATFORM=darwin npm run build
npm run tauri -- build --debug --no-bundle
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
