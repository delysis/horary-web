# Conversation and the unfolding reading

Speak or type into one unbranded document. Questions, the chart and short interpretations unfold inline. Appearance follows the system. Earlier words can be corrected in place; their original text remains in the record. There is no chart form or settings screen.

Each app launch opens a fresh reading and archives the previous leaf. Renderer reloads retain the current leaf. “Begin a fresh reading,” Command–N, and the native File menu start another reading. “Our earlier readings” reopens saved leaves. Native commands carry the reading identity, so a delayed input cannot enter a different leaf. The full conversation, chart, revisions, worksheets and diagnostics are archived before replacement.

## The method

[The living process map](LLM_PROCESS.md) is the primary review document. It contains Mermaid diagrams, exact stage lessons, output contracts, source quotations with printed-page references, and authored examples captured from the actual Rust scheduler. A freshness test detects source drift. The complete private OCR is not bundled.

`horary_pipeline.rs` owns the sequence. Intake preserves the actual question and horizon before casting. Native place and moment checks run independently: usable device coordinates and the receipt instant are defaults; a stated chart place or historical understood moment overrides them. Event context is not automatically a chart override. Civil-time parsing waits for the chosen place's time zone and native clock. Rust handles DST gaps and overlaps.

Rust calculates the chart, binds people/objects to named role options, counts turned houses and derives their rulers. The role lesson selects the relevant supplied option IDs and explains the choice. Required context and role slots must be filled before testimony can run. Condition, directed reception and contact mechanics then run in one native batch; lost matters add location as the fourth task. Each receives its own lesson and facts. The final judgment combines accepted data and answers the original question. Ask why to inspect one step; place/moment explanations receive the actual chart even before an interpretation exists.

Model output is a proposal. All single and batch results use one acceptance path for bounded fields, source-backed extraction, required role slots, current evidence IDs, directed reception references, contact sign-change status and calculation boundaries. Rejection retains the output and retries that same stage with its unchanged original input. A necessary user question suspends it; replies return to that stage. Completion requires accepted data, not an existing chart or merely valid JSON. Cancellation/backend failure pauses unfinished work. These checks do not certify all prose or astrology; Eileen should review the evidence, source method and interpretation together.

The inherited approximate seven-planet ephemeris, Regiomontanus houses, corrected dignity/reception rules and hourly seven-day contact search remain calculation boundaries. Exact symbolic timing is bypassed because native travel to moving-target perfection is not available. A seven-day absence does not settle a one-year question.

## Voice, acquisition and caching

Hold Space outside editable text to speak; release to finish. Option–Space works while writing. Holding the invitation also works. Escape and window blur cancel capture. Native capture produces bounded mono 16 kHz WAV in memory. Audio never crosses UI IPC or enters the saved conversation.

The default route tries installed, strictly on-device macOS dictation. If unavailable, Gemma hears the audio directly and supplies intake's meaning summary and worksheet in one call. Gemma transcription remains a developer comparison route. `HORARY_VOICE_MODE=auto|native|direct|gemma-transcription` selects the route without adding a settings screen. Explicit native mode does not silently fall back. macOS speaks clarifications or the final interpretation with its installed system voice.

First use acquires pinned verified weights and projector automatically in the shared Hugging Face cache. No Python, Hugging Face CLI, duplicate weight store or hosted inference is required. Escape pauses acquisition or inference; saved words and completed passages remain.

One native-kit owner holds one weight copy. It admits up to four independent generation cases. Fixed lessons have separate authenticated saved prefixes, held in memory within one eighth of physical RAM, capped at 4 GiB. Single text tasks use constrained JSON; independent tasks use ordinary cached batching and native validation because the current constrained API lacks per-case supplied prefixes. Audio does not claim prefix reuse. Eviction, shutdown or changed teaching can require prefill again. See [format experiments](FORMAT_EXPERIMENTS.md) and [verification](VERIFICATION.md).

## Review records

“How this follows” reveals each passage's checks, calculated evidence and book extracts. Detailed input, original model output, validated worksheet, token counts, first-token delay and tool receipts are further inside **In the margins → Processing details**. Technical processing is kept out of the ordinary document.

`conversation.json` and reading archives use bounded atomic writes. A separate rotating `reading-progress.jsonl` journal logs build identity and metrics without question text, coordinates or audio. Private method receipts retain exact task input and output for diagnosis. They remain local; no automatic sharing occurs. Microphone interaction, ambiguity, interruption and expert assessment still require native review.
