# Verification record — 2026-09-14

This is a development review handoff, not Eileen's approval of the astrology or a signed public release.

## Local checks

The consolidated gates and subsequent focused regressions passed: ESLint, 67 React tests, 199 JavaScript tests, browser build, JavaScript coverage run, npm audit, Rust formatting, 27 shared-core tests, 84 native tests and Clippy with warnings denied. Hardware-dependent tests return without inference when their explicit model environment variable is absent; they are tracked separately below.

`npm run check:release-assets` passed against the native-kit revision and model manifest. The macOS debug app bundled successfully with `native-llama-metal`; its runtime uses native libraries, not Python, Node, a CLI, or a separate inference server. Native bindings are pinned in Cargo.lock; the obsolete vendored binding source was removed.

The acquisition tests exercise an empty isolated cache, published blob/snapshot reuse without network, interrupted downloads, exact resume ranges, servers ignoring Range, wrong ranges, corrupt data, existing regular snapshots, shared locks, and cancellation of a stalled network request. No real shared model cache was deleted for these tests.

## Visible checks

Browser review exercised offline London search and inspected the chart/reading layout. The chart's missing SVG viewBox caused clipping at narrower widths; the fix and regression are included. Native launch displayed the corrected chart and accepted text through accessibility. Stale automation element references initially prevented button activation; refreshing the references resolved it. The in-app model setup completed and displayed “Local model installed.” Both model and projector blob/snapshot paths were verified to refer to the same filesystem objects; no second copy was created.

The native `f822e88` bundle (bundle ID `app.horary.desktop`, PID 57019) completed offline London selection, a lost-ring reading, cancellation after generation began, a fresh completed reading, method inspection, Save-dialog cancellation and a successful review export. The exported build, question, chart and interpretation matched the visible reading. Quit released the resident model and the process exited. Later city-label changes have an accessibility regression; later prompt changes add the review case described below.

Component regressions verify setup on an empty machine, setup failures, reading lifecycle and cancellation, early native completion events, evidence invalidation, preserved note snapshots, the settings toggle/Escape behavior, recovery from manual coordinates to city search, and clearing a stale missing-location error after selection.

## Native reading checks

A real Gemma 4 12B IT QAT run of prompt v6 using the matching projector, native-kit chat template and constrained sampler passed all five fixtures: full London chart, job offer, reconciliation, lost ring and investment/high-stakes. The evaluator uses the same normalized facts as the prompt, including near-cusp house adjustments. Findings must explain a step rather than repeat its identifier. The complete run took about 14 minutes on this machine; this is not a claim of interactive speed on other hardware.

The subsequent native London recovery example revealed an unexplained switch of the Moon's role and an unsupported forecast in hours. Frawley explicitly permits the Moon to signify the lost object applying to Lord 1 (pp. 147–148), so that testimony must remain available. Prompt v7 requests the role explicitly, compares Lords 2 and 4 for an inanimate object, avoids assuming debility means damage, and requires stated symbolic timing reasoning. The new `london-recovery` fixture joins the five earlier hardware cases. A repeated stage-name finding in the job fixture also led to a 32-character minimum in the constrained output schema, alongside the parser's explicit rejection of findings that merely repeat identifiers. See the pull request's validation receipt for the final six-case run; passing structural checks does not certify all of its astrology.

The separate hardware gate passed with the same model/projector: resident model reuse, schema-constrained output, streamed tokens, pre-cancellation, cancellation after a real token arrives, joined shutdown, idempotent stop and rejection of generation after stop. The hardware gate took about 40 seconds.

Native review export now uses an OS Save dialog and atomic Rust file writing. Its regression checks that invalid payloads preserve an existing file and valid exports replace only the selected destination. Browser export remains a local download.

## Remaining boundaries

- Eileen has not signed off the method, house suggestions, or reading quality.
- The ephemeris is approximate. Hourly event bracketing and bounded search do not certify fine timing, stations, or contextual prevention of an event.
- The pinned native-kit API does not expose speculative decoding for controlled readings. Image/audio attachment UI is not implemented, although the projector is acquired and loaded.
- Constrained readings keep model weights resident but do not promise prompt-prefix reuse. Prompt caches are memory-only.
- CPU-only, Windows and Linux user interaction, lower-memory behavior, and distribution signing/notarization are not established by local macOS checks. Remote build results are tracked on the pull request.

See [method audit](METHOD_AUDIT.md), [model provenance](MODEL_PROVENANCE.md), and [review guide](REVIEW_GUIDE.md).

Remote CI exposed Windows dependency path lengths, an unused macOS-only helper on Linux, and a missing LLVM dependency in the latest macOS WebAssembly linker. CI enables long paths, scopes the helper to its platform, and installs Rust 1.95.0 with explicit rustfmt and Clippy components. Windows download tests also caught an append-only handle being unable to truncate a full HTTP response: the downloader now uses read/write access and explicit seek offsets for resumptions and restarts. Consult the exact head checks on [PR #1](https://github.com/delysis/horary-web/pull/1) before claiming cross-platform build success.
