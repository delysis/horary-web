# Architecture

React renders an unfolding document inside Tauri. Safe Rust owns the reading, device context, audio, inference, geocoding, chart calculation, validation and storage. The browser is a visual preview without hosted inference. Supported development and runtime paths require no Python.

## Reading authority

`conversation.rs` adapts native preparation and persistence to `horary_pipeline.rs`. The explicit pipeline replaces the old general tool loop. `horary_lessons.rs` builds one complete teaching prompt per task from numbered procedures, contrasting examples and selected Frawley quotations. `horary_prompts/` is the source of that teaching. Generated [living documentation](LLM_PROCESS.md) includes exact prompts, narrow contracts, dependencies and a fixture captured from the scheduler.

Intake retains question, scope and context. Independent native place/moment sufficiency checks use device defaults where appropriate. Civil-time interpretation depends on the selected place's IANA zone; conversion and ambiguity validation are native. Model place choices must resolve to offline/native candidate IDs. The model cannot supply arbitrary coordinates or astronomy.

The native chart establishes positions and traditional rulers. Role selection precedes the independent condition, reception, contact-mechanics and optional lost-location batch. The final interpretation depends on their worksheets. Native typed event data protects copied sign-change status. Reference and shape checks are explicit boundaries, not certification of model semantics.

`horary_step.rs` owns an enforced transition catalog and the completion permit. Its durable journal distinguishes prepared, running, repairing, awaiting-user, paused, complete and superseded work. `horary_executor.rs` routes every single/batch proposal through the same acceptance path; all sibling receipts are kept before any repair. No attempt cap converts rejection into abandonment. The original input remains unchanged through repairs, and cancellation/backend interruption retains unfinished work. Native validation, lesson, contract, input and revision fingerprints scope reuse; saved data is checked again.

`horary_contract.rs` defines narrow schemas. Intake now extracts source-backed people and the subject separately from per-turn intent. `horary_role_options.rs` binds named choices to those people and objects, computes turned houses, and requires every relevant role slot. Missing relationship/ownership permits an information request instead of a guessed assignment. The actor selects literal option IDs and explains relevance; it does not compute a husband's possessions and accidentally give that house to the husband. Quotation and English relationship-word checks improve extraction provenance but do not prove arbitrary language semantics.

Explanation dispatch requires the actual follow-up words and selected passage or native chart context. Chart time/place can be explained without a completed interpretation. Missing internal artifacts never become requests for user-supplied chart data. Explicit continue commands resume the current matter and can reuse validated completed steps.

`reading_store.rs` archives the complete active leaf before starting or reopening another one. Chart corrections archive the version being left. Native operations are serialized against leaf replacement and check the expected reading ID. Snapshot IDs remain monotonic, preventing late responses from replacing newer work. First native process open starts fresh; renderer reloads resume the active leaf.

## Calculations

`crates/horary-ai-core/src/astronomy.rs` implements the inherited approximate ephemeris and Regiomontanus houses. `book_method.rs` owns source-corrected dignity, directed reception, solar conditions and cusp treatment. `events.rs` searches hourly contact brackets over seven days. The same chart supplies UI and inference facts. Exact event chains, intermediate stations, fixed stars, antiscia and symbolic timing are not certified. The timing lesson is reviewable but bypassed until native moving-target travel is available.

## Native inference and acquisition

`native_llama_worker.rs` uses pinned native-kit host/engine/types. One resident model admits native batches of one to four distinct sequences. Each ordinary batch case can restore its own authenticated saved fixed lesson. An owner mutex protects restoration and generation transactions; it does not serialize cases inside a batch. Cross-request continuous batching is not enabled.

The in-memory prefix bank uses exact fixed-system token prefixes bound to the live model owner, with one eighth of RAM and a 4 GiB cap. Authenticated native restores republish physical resident-prefix authority; token replay does not. Full prompts still count for logical admission. Cache, new prefill, output tokens, first-token delay and parent batch wall time are recorded separately. Single constrained text requests restore their lesson; batched tasks use ordinary decoding plus native worksheet validation. Audio has no reuse qualification. No disk prompt cache or speculative decoding is claimed.

`hf_cache.rs` uses immutable Hugging Face revisions, verified blobs, OS locks, resumable partial downloads and atomic publication. It honors shared Hub cache paths and leaves refs/main untouched. Snapshot links share blobs; app data contains only a small registration. The native owner verifies pinned model/projector digests at initial load and checks file identity on reuse rather than hashing the payload on every turn.

## Audio and local records

`microphone_capture.rs` owns bounded CPAL capture on a joined thread. `voice.rs` uses installed on-device macOS recognition when available, direct Gemma audio otherwise, and keeps Gemma transcription as an explicit comparison. Audio remains behind one-use native receipts and never enters UI IPC or persistent conversation data. macOS speech output uses the installed system utility without a shell.

The main document shows proposed interpretation; margins expose evidence and quotations. Processing diagnostics are nested further and retain original responses and validation failures. A rotating progress journal omits private words, location and audio. Exact method receipts are local private files. Legacy calculator components remain for regression coverage, but the general dense conversation prompt and old tool loop are removed.

## Packaging and acceptance

macOS targets 13.0 or newer, declares microphone/Speech purpose strings, and links the system Swift runtime. Linux microphone builds require ALSA headers. Native bundles, real generation, CI, source/domain review and signing/notarization remain distinct gates. See [verification](VERIFICATION.md); rendering or a valid worksheet alone does not establish product acceptance.
