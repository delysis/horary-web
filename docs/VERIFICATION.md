# Verification record — 2026-09-14

This is a development review handoff, not Eileen's approval of the astrology or a signed public release.

## Local checks

The consolidated `npm run check:all` passed: ESLint, 67 React tests, 199 JavaScript tests, browser build, JavaScript coverage run, npm audit, Rust formatting, 26 shared-core tests, 84 native tests and Clippy with warnings denied. Hardware-dependent tests return without inference when their explicit model environment variable is absent; they are tracked separately below.

`npm run check:release-assets` passed against the native-kit revision and model manifest. The macOS debug app bundled successfully with `native-llama-metal`; its runtime uses native libraries, not Python, Node, a CLI, or a separate inference server. Native bindings are pinned in Cargo.lock; the obsolete vendored binding source was removed.

The acquisition tests exercise an empty isolated cache, published blob/snapshot reuse without network, interrupted downloads, exact resume ranges, servers ignoring Range, wrong ranges, corrupt data, existing regular snapshots, shared locks, and cancellation of a stalled network request. No real shared model cache was deleted for these tests.

## Visible checks

Browser review exercised offline London search and inspected the chart/reading layout. The chart's missing SVG viewBox caused clipping at narrower widths; the fix and regression are included. Native launch displayed the corrected chart and accepted text through accessibility. Stale automation element references initially prevented button activation; refreshing the references resolved it. The in-app model setup completed and displayed “Local model installed.” Both model and projector blob/snapshot paths were verified to refer to the same filesystem objects; no second copy was created. Native reading, export and quit interaction acceptance is being completed on the final bundle.

Component regressions verify setup on an empty machine, setup failures, reading lifecycle and cancellation, early native completion events, evidence invalidation, preserved note snapshots, the settings toggle/Escape behavior, recovery from manual coordinates to city search, and clearing a stale missing-location error after selection.

## Native reading checks

A fresh real Gemma 4 12B IT QAT run using the matching projector, native-kit chat template and constrained sampler passed all five fixtures: full London chart, job offer, reconciliation, lost ring and investment/high-stakes. The evaluator uses the same normalized facts as the prompt, including near-cusp house adjustments. Findings must explain a step rather than repeat its identifier. The complete run took about 14 minutes on this machine; this is not a claim of interactive speed on other hardware.

The separate hardware gate passed with the same model/projector: resident model reuse, schema-constrained output, streamed tokens, pre-cancellation, cancellation after a real token arrives, joined shutdown, idempotent stop and rejection of generation after stop. The hardware gate took about 40 seconds.

Native review export now uses an OS Save dialog and atomic Rust file writing. Its regression checks that invalid payloads preserve an existing file and valid exports replace only the selected destination. Browser export remains a local download.

## Remaining boundaries

- Eileen has not signed off the method, house suggestions, or reading quality.
- The ephemeris is approximate. Hourly event bracketing and bounded search do not certify fine timing, stations, or contextual prevention of an event.
- The pinned native-kit API does not expose speculative decoding for controlled readings. Image/audio attachment UI is not implemented, although the projector is acquired and loaded.
- Constrained readings keep model weights resident but do not promise prompt-prefix reuse. Prompt caches are memory-only.
- CPU-only, Windows and Linux user interaction, lower-memory behavior, distribution signing/notarization, and final remote CI are not established by local macOS checks.

See [method audit](METHOD_AUDIT.md), [model provenance](MODEL_PROVENANCE.md), and [review guide](REVIEW_GUIDE.md).

The first remote web job passed. Windows exposed Git’s legacy path-length limit while checking out the pinned llama.cpp submodule; CI now enables long paths, following native-kit’s own Windows workflow. The revised Windows run must pass before claiming portability.
