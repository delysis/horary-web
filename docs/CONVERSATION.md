# Conversation and the unfolding reading

The September 14 direction replaces the chart-entry workspace with a conversation. Speak or type; the native agent resolves the place, casts the chart, and develops a provisional reading on a scroll. There is no branding, button, settings control or chart form. The system controls light/dark appearance. Questions, charts and testimony appear inline in one document. The browser is a visual preview of this native experience, not a hosted AI service.

The microphone is explicit: hold the speech invitation or Space to begin, and release to finish. Native CPAL capture produces mono 16 kHz WAV in memory. Gemma 4 12B IT QAT receives that audio through the matching projector and native-kit media API, transcribes it, and the transcript becomes the visible user turn. Corrections are ordinary conversation. The UI stops recording after two minutes; the recorder also enforces its own bounded storage. Audio is not written into the conversation file. macOS can speak the reply using its installed system voice through `/usr/bin/say`; there is no shell or remote speech service. Other platforms currently retain text replies.

The first message prepares the local reader. Model acquisition still uses pinned, verified Hugging Face blobs and shared snapshot links. Downloading may require about 7.15 GB once. Stop pauses acquisition or cancels inference. A failed operation leaves the question and completed scroll sections saved.

## Authority

`src-tauri/src/conversation.rs` owns the conversation, tool loop, local file and chart revisions. The model can select only schema-constrained actions: speak, find a place, cast a chart, write a scroll section, or read evidence. It cannot provide a filename, execute a command, supply arbitrary coordinates, or substitute model-produced planetary positions. Place IDs must come from the offline geocoder. Civil times are resolved against that place's IANA timezone, including DST gaps and repeated times.

`crates/horary-ai-core/src/astronomy.rs` ports the inherited approximate seven-planet ephemeris and Regiomontanus houses into safe Rust. The port has parity fixtures at three dates/locations; parity is not a new certification of astronomical accuracy. It feeds the previously corrected dignity, reception, solar-condition and hourly event rules. The UI only renders the returned chart.

Every chart revision clears the current interpretation and retains its predecessor. Every executed tool has a receipt. Scroll sections cite IDs in the current calculated evidence. Ask “why?” to explore evidence, or ask to return to a previous reading. The agent has a restore tool that preserves the version being left. Evidence-ID validation prevents dangling references, but does not prove that the model's prose accurately interprets its reference.

The conversation is atomically persisted in the native app-data `conversation.json`, with a 16 MB bound. No old chart records or review notes are deleted. There is currently one active conversation; a separate conversation library and export-by-conversation are still to be completed. The older review export backend remains available to legacy callers.

## Book fidelity

The agent receives editorial method notes in `conversation_method.txt`, with printed-page references. These preserve the important lost-object exception: the Moon can signify the lost object applying to Lord 1 for recovery. Inanimate lost possessions require comparing Lords 2 and 4. The agent must establish roles, avoid automatic damage/theft assumptions, and distinguish astronomical aspect times from symbolic timing of the earthly matter.

The book OCR remains private and is not bundled. The method notes are inspectable software policy, not Eileen's final endorsement. Model testimony remains provisional. In particular, approximate ephemerides and hourly seven-day event brackets do not certify fine event order or every traditional doctrine.

## Acceptance still required

Exercise real voice input, conversational ambiguity, interrupted replies, revised ownership, a historical DST time, a completed reading, and reopening saved work. Native hardware output and visible interaction are separate from unit tests. Do not call the experience accepted merely because the interface renders or a structured tool call parses.
