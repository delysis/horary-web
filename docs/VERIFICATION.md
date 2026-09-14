# Verification record — 2026-09-14

This is a development review handoff, not Eileen's approval of the astrology or a signed public release.

## Local checks

The consolidated `npm run check:all` passed: ESLint, 67 React tests, 199 JavaScript tests, browser build, JavaScript coverage run, npm audit, Rust formatting, 25 shared-core tests, native tests and Clippy with warnings denied. Hardware-dependent tests return without inference when their explicit model environment variable is absent; they are tracked separately below.

`npm run check:release-assets` passed against the native-kit revision and model manifest. The macOS debug app bundled successfully with `native-llama-metal`; its runtime uses native libraries, not Python, Node, a CLI, or a separate inference server. Native bindings are pinned in Cargo.lock; the obsolete vendored binding source was removed.

The acquisition tests exercise an empty isolated cache, published blob/snapshot reuse without network, interrupted downloads, exact resume ranges, servers ignoring Range, wrong ranges, corrupt data, existing regular snapshots, shared locks, and cancellation of a stalled network request. No real shared model cache was deleted for these tests.

## Visible checks

Browser review exercised offline London search and inspected the chart/reading layout. The chart's missing SVG viewBox caused clipping at narrower widths; the fix and regression are included. Native launch displayed the corrected chart and accepted text through accessibility. The current native automation session did not reliably activate webview buttons; native setup/read/cancel/quit interaction acceptance is still outstanding.

Component regressions verify setup on an empty machine, setup failures, reading lifecycle and cancellation, early native completion events, evidence invalidation, preserved note snapshots, the settings toggle/Escape behavior, recovery from manual coordinates to city search, and clearing a stale missing-location error after selection.

## Native reading checks

A real Gemma 4 12B IT QAT run with the native-kit chat template and constrained sampler completed five sample readings. Four passed the then-current checks; the fifth correctly used a near-cusp house adjustment that the old evaluator failed to apply. The evaluator now checks the normalized facts actually sent to the model.

Inspection also found that some reported findings merely repeated step identifiers. The prompt now asks for explanatory sentences and validation rejects identifier-only findings. A fresh run using the model plus its matching projector has passed the full London chart case with explanatory findings and supplied evidence. The remaining cases and the separate native streaming/cancellation/shutdown gate are being rerun; completion must be recorded before calling native review acceptance complete.

## Remaining boundaries

- Eileen has not signed off the method, house suggestions, or reading quality.
- The ephemeris is approximate. Hourly event bracketing and bounded search do not certify fine timing, stations, or contextual prevention of an event.
- The pinned native-kit API does not expose speculative decoding for controlled readings. Image/audio attachment UI is not implemented, although the projector is acquired and loaded.
- Constrained readings keep model weights resident but do not promise prompt-prefix reuse. Prompt caches are memory-only.
- CPU-only, Windows and Linux user interaction, lower-memory behavior, distribution signing/notarization, and the remote CI result are not established by local macOS checks.

See [method audit](METHOD_AUDIT.md), [model provenance](MODEL_PROVENANCE.md), and [review guide](REVIEW_GUIDE.md).
