# Architecture

The current prototype is one unfolding document, rendered by React inside Tauri. `src/App.tsx` renders conversational passages and inserts charts and testimony at the moment they enter the reading. It has no branding, settings, buttons, or chart forms. Light/dark appearance follows the system. Editable earlier user passages propose explicit corrections rather than silently rewriting the record.

## Native authority

`src-tauri/src/conversation.rs` owns conversation persistence, model preparation and a bounded eight-action tool loop. The model speaks or chooses schema-constrained tools for offline place lookup, chart calculation, writing/replacing a reading section, inspecting evidence and restoring an earlier reading. No arbitrary command, path, network, or model-supplied ephemeris tool exists. Places must be resolved to IDs by the native geocoder. IANA/DST conversion and the chart calculation execute in safe Rust.

`crates/horary-ai-core/src/astronomy.rs` contains the inherited approximate ephemeris for the seven classical planets and Regiomontanus houses. Committed parity fixtures compare its positions/cusps with the existing calculator at three dates and locations. The corrected `book_method.rs` owns dignity, reception, solar conditions and cusp treatment. `events.rs` derives future event brackets from hourly samples. These approximations are not certified professional ephemerides or proof of every traditional event doctrine.

The same calculated chart feeds the document and model evidence. Chart/question changes clear the current testimony and archive its previous version. Tool calls and outcomes are retained. Reading sections point to current evidence IDs. Restoring an earlier reading preserves the version being left. Atomic session snapshots have monotonic IDs so late UI responses cannot replace newer work.

`conversation_prompt.txt` and `conversation_method.txt` are inspectable policy. The latter paraphrases the book and retains specific exceptions and limits. The private OCR is not distributed. Evidence pointers prove that a source was available, not that the model used it correctly.

## Audio and inference

`microphone_capture.rs`, adapted from pinned native-platform code, owns CPAL microphone capture on a joined thread with bounded queues/memory. `voice.rs` first tries installed, strictly on-device macOS dictation through the safe Rust Speech binding. If unavailable, the WAV goes directly to Gemma for understanding and a first tool decision, with no intermediate transcription call. Gemma transcription remains an explicit developer comparison route. Native Rust retains the stopped audio behind a one-use receipt; audio never crosses UI IPC or enters the saved conversation. Holding the document's speech invitation or Space starts capture; release finishes. Escape cancels. macOS spoken replies use the installed system speech utility without a shell or remote voice. Other platforms currently use direct audio and retain text replies.

`native_llama_worker.rs` adapts pinned native-kit host/engine/types. The host keeps one model resident, verifies the model and projector, and selects Gemma's explicit non-thinking format from loaded architecture metadata. Text tools are constrained at sampling time. Direct audio uses ordinary multimodal generation and a bounded parsed action, because controlled generation has no media support; a guard prevents silently dropping media. Direct understanding is labeled as a summary, never a verbatim transcript.

Single controlled text requests reuse the engine's exact resident token prefix. Fixed policy and book rules precede changing state. Within a turn, Rust appends new verified facts and receipts; it does not refeed model draft prose. A chart revision retires the old chart context. Cache authority is bound to the resident worker and token sequence; cancellation, failure and shutdown clear it. Only newly decoded tokens save physical work: logical context and admission limits still charge the entire request. There is no disk prompt cache or speculative decoding. Local receipts record reused/prefilled tokens and first-token delay separately from generation duration.

## Acquisition and storage

`hf_cache.rs` implements pinned immutable Hugging Face artifact acquisition with reqwest, SHA-256, OS blob locks, resumable partial files and atomic publication. It honors shared Hub cache variables and leaves refs/main untouched. Unix snapshots use relative symlinks, Windows uses hardlinks. Only a tiny cache registration is stored in app data. Model files are not copied into a second app cache.

First conversation prepares the local reader automatically; its technical progress remains off the document. Stop can cancel acquisition or inference. The session is atomically saved as `conversation.json` in native app data, including messages, current chart, previous revisions and tool receipts. Existing legacy charts and notes are preserved.

The browser serves a visual preview. Legacy calculator/AI components remain in the repository for regression coverage, but are not the new entry point. The app has no hosted inference fallback, analytics or automatic sharing. All supported development and runtime paths require no Python.

## Packaging

macOS builds target 13.0 or newer for the safe Rust Speech bridge. Microphone and Speech purposes are declared in Info.plist. The final macOS target links against the system Swift runtime at `/usr/lib/swift`. Linux microphone builds require ALSA headers. CI builds the native backend on macOS, Windows and Linux; unsigned reviewer artifacts are separate from signing/notarization and product acceptance. See [verification](VERIFICATION.md).
