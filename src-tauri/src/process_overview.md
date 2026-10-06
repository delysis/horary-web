# How the horary reading is made

This is the review map for Eileen. The app now uses separate teaching tasks rather than a general prompt asking the model to supply the whole horary method. Each lesson defines its terms, gives numbered checks, contrasts worked examples with mistakes, and includes selected passages from **John Frawley, The Horary Textbook (2005)**. The exact teaching prompts and output contracts appear below.

The method is a working implementation for assessment. Source quotes are checked against the locally supplied OCR; editorial procedures and examples are identified separately. Correctly quoting a rule or producing a valid worksheet does not establish a correct judgment. The deployed model is Gemma 4 12B IT QAT; a 2B model has **not** been qualified.

## Reading the diagrams

**C** means classification or careful extraction. **J** means contextual horary judgment. **W** means explaining the answer. **N** means native calculation, lookup, storage or validation. Independent native checks run in parallel. Independent analysis tasks are submitted in one native batch, with up to four distinct inference sequences sharing one copy of the weights.

The primary flow is derived from the stage dependency catalog alongside the explicit Rust pipeline. The second and third diagrams explain native branches and caching; their source is fingerprinted. An executable fixture captures the requests actually submitted by the pipeline, so prompt examples are not separately invented instructions.

## What Eileen should examine

1. Does intake preserve the original question, ownership, horizon, and negation through intermediate replies? Is a place or time in the story being mistaken for the chart's place or moment?
2. Are house and natural roles justified by the matter? For the querent's lost object, are Lords 2 and 4 compared? For another owner, is the owner's second house turned correctly? Is the Moon's role explicit?
3. Are quality, ability, and motive distinguished? Does each reception run from the planet in the dignity to that dignity's ruler? Are mixed or negative receptions retained?
4. Is an applying contact relevant to the selected actors? What changes or intervenes before it? Does the calculation actually establish a claimed translation, collection, or prevention?
5. Does the final passage answer the original question in context? What supports it, what opposes it, and what remains unknown? An uncertain answer should still explain what the testimony means for the person.

The main document carries the proposed interpretation. Chart facts, source extracts and structured checks remain inspectable in its margins. Detailed input, original output, validation and timing receipts are behind **In the margins → Processing details**. These are local records.

## Present calculation boundary

Planetary positions are approximate. The event search uses hourly brackets over seven days; fine event order, intermediate stations, fixed stars and antiscia are not certified. It does not supply the applying planet's travel to exact moving-target perfection. The timing lesson is ready for review but its model call is bypassed with an explicit native unestablished receipt. The model is not asked to invent a numeric duration from the angular gap or astronomical hours. No missing seven-day candidate can by itself answer a one-year question negatively.

Location interpretation runs only for a missing object or animal. It uses the chosen object's **occupied house**, not simply the house it rules. Room suggestions are conditional on context; neither a debilitated significator nor a house assignment establishes damage, theft, or recovery.

## Output formats under comparison

Single production tasks currently use constrained JSON; independent analysis tasks use ordinary JSON batches with native checks. The selector experiment compared constrained JSON, unconstrained JSON, XML and Natural Language Tools on the same question/place/moment decisions, with two recorded repetitions and rotated order. XML matched 20 of 24 routes, natural language 18, fenced JSON after exact wrapper removal 18, and constrained JSON 14. A separate repaired five-case typed worksheet test passed its defined checks for all three JSON/XML variants. Every failure, raw response, prompt digest, expected selection and latency is retained. Parseability is not correctness. See [the experiment report](FORMAT_EXPERIMENTS.md) for limits and the first failed attempt.

[Johnson et al. (2025)](https://arxiv.org/abs/2510.14453) separate parameterless YES/NO selection from execution and response writing. [Somma et al. (2026)](https://arxiv.org/abs/2607.03953) replicate that setting and exclude parameterized calls and multi-turn interactions. These studies motivate a Horary comparison; they do not establish that natural-language coordinates, civil times, or judgments are reliable. Format choice and argument validation remain separate questions.

## Review and refresh

Rust generates this document from the live lesson builder, schemas, dependency catalog and executable authored fixture. The fixture is labeled; it is not a model result. A normal test fails when code or teaching material changes without refreshing this reference.

Run `cargo test --manifest-path src-tauri/Cargo.toml --locked process_reference::tests::regenerate -- --ignored --nocapture` to refresh. The companion source manifest fingerprints the exact files. Private readings, audio and the complete OCR are not included.
