# Architecture

The current prototype is one unfolding document, rendered by React inside Tauri. `src/App.tsx` renders conversational passages and inserts charts and testimony at the moment they enter the reading. It has no branding, settings, buttons, or chart forms. Light/dark appearance follows the system. Editable earlier user passages propose explicit corrections rather than silently rewriting the record.

## Native authority

`src-tauri/src/conversation.rs` owns conversation persistence, model preparation and a bounded eight-action tool loop. The model speaks or chooses schema-constrained tools for offline place lookup, chart calculation, writing/replacing a reading section, inspecting evidence and restoring an earlier reading. No arbitrary command, path, network, or model-supplied ephemeris tool exists. Places must be resolved to IDs by the native geocoder. IANA/DST conversion and the chart calculation execute in safe Rust.

`crates/horary-ai-core/src/astronomy.rs` contains the inherited approximate ephemeris for the seven classical planets and Regiomontanus houses. Committed parity fixtures compare its positions/cusps with the existing calculator at three dates and locations. The corrected `book_method.rs` owns dignity, reception, solar conditions and cusp treatment. `events.rs` derives future event brackets from hourly samples. These approximations are not certified professional ephemerides or proof of every traditional event doctrine.

The same calculated chart feeds the document and model evidence. Chart/question changes clear the current testimony and archive its previous version. Tool calls and outcomes are retained. Reading sections point to current evidence IDs. Restoring an earlier reading preserves the version being left. Atomic session snapshots have monotonic IDs so late UI responses cannot replace newer work.

`conversation_prompt.txt` and `conversation_method.txt` are inspectable policy. The latter paraphrases the book and retains specific exceptions and limits. The private OCR is not distributed. Evidence pointers prove that a source was available, not that the model used it correctly.

## Audio and inference

`microphone_capture.rs`, adapted from pinned native-platform code, owns CPAL microphone capture on a joined thread with bounded queues/memory. `voice.rs` passes a stopped WAV to Gemma through native-kit's media API, then returns the transcript as a visible conversation turn. Raw audio is held only in memory. Holding the document's speech invitation or Space starts capture; release finishes. Escape cancels. macOS spoken replies use the installed system speech utility without a shell or remote voice. Other platforms currently retain text replies.

`native_llama_worker.rs` adapts pinned native-kit host/engine/types. The host keeps one model resident, verifies the model and projector, selects the model's real chat template, and constrains tool actions at sampling time. Speech transcription uses native multimodal generation. Cancellation and shutdown join owned work. Prompt caching is memory-only; controlled generation does not promise prefix reuse. The pinned API does not expose speculative decoding.

## Acquisition and storage

`hf_cache.rs` implements pinned immutable Hugging Face artifact acquisition with reqwest, SHA-256, OS blob locks, resumable partial files and atomic publication. It honors shared Hub cache variables and leaves refs/main untouched. Unix snapshots use relative symlinks, Windows uses hardlinks. Only a tiny cache registration is stored in app data. Model files are not copied into a second app cache.

First conversation prepares the local reader automatically; its technical progress remains off the document. Stop can cancel acquisition or inference. The session is atomically saved as `conversation.json` in native app data, including messages, current chart, previous revisions and tool receipts. Existing legacy charts and notes are preserved.

The browser serves a visual preview. Legacy calculator/AI components remain in the repository for regression coverage, but are not the new entry point. The app has no hosted inference fallback, analytics or automatic sharing. All supported development and runtime paths require no Python.

## Packaging

macOS builds target 11.0 or newer. The microphone purpose is declared in Info.plist. Linux microphone builds require ALSA headers. CI builds the native backend on macOS, Windows and Linux; unsigned reviewer artifacts are separate from signing/notarization and product acceptance. See [verification](VERIFICATION.md).
