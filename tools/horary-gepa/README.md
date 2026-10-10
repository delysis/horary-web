# Horary GEPA

An iterative Rust prompt optimizer connected to the application's actual neural functions. It uses the [`dsrust-gepa` engine](https://github.com/getlatentic/dsrust/tree/f24adde08c1d8850e4d7079d019643bb40f905cb/gepa), pinned at `f24adde08c1d8850e4d7079d019643bb40f905cb`. This is an alpha Rust port with numerical conformance tests, not a production-qualified DSPy implementation. Only its LLM-independent GEPA engine and reflection formatter are used. Python is not executed or required; the application's model runtime is unchanged.

## Executable scope

The first adapter optimizes `intake / classify_question`: method and answer facet. It stops after the real executor accepts that classifier's `Turn`; it does not reset a specialist checkpoint or pretend a later stage is an initial question. Native validation, rejected attempts and same-step repair remain active. Its fitness compares the returned frame against authored labels and allowed alternatives, not JSON validity.

The second adapter measures **input journeys** and optimizes one method's `intake / complete_selected_program` extraction teaching. Every evaluation starts with the authored original question and runs the actual `run_elicitation` pipeline, including a supplying turn only if the intended need or single agreed reframing was actually elicited. Every classifier/place/moment/guru call is fresh; source captures only pin a focused guide for inspection. No gold method, accepted specialist checkpoint or old downstream response is transplanted to make the extractor reachable. A wrong upstream decision remains an input failure.

New input searches require `--objective extractor-reliability`. The independently cited `extractor` dimension assesses the **initially accepted** consultation against the original fixture: sourced facts, actors, question/frame, retained known facts and honest unknowns. It must cite that initial record and the fixture. Rejected raw drafts are separate reliability observations; a later supplying turn cannot rescue an initially wrong record. The initial state's native classification/extraction gates and actual target invocation remain required. The aggregate grade also includes fixed elicitation, and top-level gates describe the final supplying state; neither can erase the initial extractor's signal.

Fitness is `(2s + 1/n) / 5`, where `s` is independent accepted-state correctness (0–2) and `n` is the actual focused extractor call count. Incorrect state scores zero. Complete state scores above .8; partial state scores at most .6. Fewer repairs differentiate otherwise correct observations but never outweigh more correct facts. This objective explicitly optimizes repair reliability, not demonstrated new horary understanding or wall-clock latency. `qualified` remains false for component merit.

Every review still separately records whole-input classification, elicitation, extraction, and actor/honesty/continuity/naturalness in actual replies. Those whole-journey guards remain unchanged acceptance requirements before promotion. At `ReadyReading`, input-only execution records a typed native handoff and stops; production instead generates a reading before the guru speaks. Native grading revalidates that permit against current inputs, the actual chart, and exact message position. The reviewer receives hash-bound pointed handoff and boundary receipts, checked against the native binding. The intentionally omitted final reply is unobserved, not a completed conversation. Missing-input turns still make real inquiries and only properly bound supplying turns execute. Reading, voice, the eventual local model and packaged acceptance remain unqualified.

`horary-gepa calibrate` exercises the real metric with offline counterfactual probes: correct repairs have a signal, fixed dialogue changes do not, faster wrong facts lose, uninvoked targets earn nothing, and interruptions are errors. Preparation saves this calibration before any paid calls and rejects whole-conversation input objectives whose only editable component is the initial extractor. Historical immutable plans, raw reviews and zero scores are retained; they are not retroactively regraded.

The independent reviewer returns judgments and citations only. Rust supplies the case identity and the recorded native grades. In the shared legacy `CaseReview` format, `native_journey_pass` means the supplying-turn result (`follow_up_pass`), while `input_journey_pass` is the whole input journey. An absent supplying turn remains null. Requiring a model to echo these similar metadata fields confused a completed review; native facts now remain outside its output contract. The student still uses ordinary unconstrained text.

## Search and evidence

GEPA selects candidate parents, samples reflection minibatches, proposes a mutation to one ordered teaching component, tests it on that minibatch, and retains improving candidates with their development scores. Merge is disabled for this initial integration. Components come from the actual system guide's editable teaching regions. Book blocks, tool schemas, native facts, rejected-answer feedback and other messages remain unchanged. Candidates use the existing typed `Program` and are checked against the actual classifier signature and guide hash.

Codex reflection uses the user's saved login through the existing tool-free, ephemeral process, without a model override or API-key environment. It receives only reflection-training cases, their actual inputs/contract, returned state, native failures, authored expectations and any validated independent reviews. The input reviewer is a separate invocation with an independently reserved budget; it receives one actual trace, the synthetic fixture and exact bounded OCR excerpts, without a proposed fitness or promotion conclusion. Each citation expands from an allowed reference ID to an existing JSON pointer and exact source hash. Scored conversation dimensions must cite the observed reply; pipeline scores must cite actual native state or results. Method selection and elicitation also cite the pinned book. Gold-only scores are rejected. The reviewer must preserve native pass flags, distinguish withheld words from observed replies and leave reading unobserved. Development examples and reviews select candidates and never enter reflection. Both search pools must be disjoint subsets of the existing frozen **training** partition. Membership and materialized expectations are rechecked against their pinned split/fixture sources on run. Original reserved cases remain excluded from this optimization round.

The student uses hosted `gemma-4-26b-a4b-it` through the native unconstrained-text client. No Google response schema, response MIME restriction or local inference is used. A Codex-only output schema validates the writer artifact; it does not constrain the student.

Baseline controls can reuse a sealed completed capture without new generation. The executor must recreate the exact original prompt, input and schema at every captured classifier/repair call, consume that prefix exactly, then grade its returned frame. Changed prompts always need fresh hosted generation. Raw later specialist outputs cannot be transplanted into a changed program.

Preparation inspects the current native function without constructing a model or reading credentials. The inspected prompt/schema/input and executable are pinned. Generated contract summaries can change the actual guide even when static prompt files do not change. The default `archived-capture` mode rejects such a mismatch before search; choose `--control-mode fresh-hosted` explicitly to purchase current-code baseline controls. There is no automatic live fallback. Reflection uses the current inspected input/contract and teaching, not an outdated archived guide.

Input-journey rounds always require `fresh-hosted`, an explicit method and a positive reviewer budget. Preparation rejects cases with no sealed capture of that method's focused guide; it never repairs the upstream classification from gold. The whole journey nevertheless executes fresh, so this guide-availability selection does not establish whole-bank robustness. Book context is 1–24 explicitly selected OCR pages (80KB maximum) from the named local source, pinned by whole-file SHA and reproduced byte-for-byte from unique page markers. OCR IDs and printed page labels are kept distinct. Review packets are bounded at 400KB; oversized packets stop before paid submission rather than silently dropping evidence.

## Commands

Compile the ignored native test entry `elicitation_eval::real_model_classification_function` from the isolated application checkout first. Use a separate Cargo target from any running campaign. Then prepare an immutable plan:

```sh
cargo build --manifest-path tools/horary-gepa/Cargo.toml --locked
tools/horary-gepa/target/debug/horary-gepa prepare \
  --campaign /absolute/path/to/sealed-catalogue \
  --split /absolute/path/to/frozen-split.json \
  --native-executable /absolute/path/to/isolated-native-test-executable \
  --codex /absolute/path/to/codex \
  --state /absolute/path/to/new-gepa-round \
  --training contact-explicit,choice-missing,new_job-explicit \
  --development parcel-explicit,visit-missing,hiring-missing \
  --control-mode fresh-hosted \
  --max-metric-calls 24 --max-teacher-calls 3 \
  --max-physical-generation-attempts 216
```

Case availability and training eligibility are checked against the named campaign and split. These IDs are illustrative, not a qualification bank. Optionally use `--wait-owner-pid PID` to withhold fresh student calls while an existing owner remains alive. Prompt inspection still runs; compatible archival controls and any already-supported Codex reflection can complete. Fresh-control mode needs hosted baseline measurements before reflection.

For a method-scoped input round, compile `elicitation_eval::real_model_input_journey_function` in the isolated application target and select disjoint available original-training cases. For example, a small mechanics pilot can use `movable_deal-missing` for reflection and `movable_deal-implicit` for development; these two cases alone are not a qualification bank:

```sh
tools/horary-gepa/target/debug/horary-gepa prepare \
  --function input-journey --objective extractor-reliability --target-method movable_deal \
  --campaign /absolute/path/to/sealed-catalogue \
  --split /absolute/path/to/frozen-split.json \
  --native-executable /absolute/path/to/isolated-native-test-executable \
  --codex /absolute/path/to/codex --state /absolute/path/to/new-input-round \
  --training movable_deal-missing --development movable_deal-implicit \
  --control-mode fresh-hosted --logical-calls-per-function 24 \
  --function-seconds 900 --max-metric-calls 5 --max-teacher-calls 2 \
  --max-review-calls 8 --max-physical-generation-attempts 1152 \
  --book-source /absolute/path/to/Frawley-OCR.md \
  --book-ocr-pages 16,17,33,34,35,165,166,167,168,169,170,181,182
```

The source pages must cover the selected method and chart/context obligations; the reviewer marks anything unsupported or unobserved accordingly. Neither extracted book data nor gold is supplied to Gemma. Larger method banks and complete-reading/reserved checks remain necessary before applying an overlay.

The five metric evaluations in this two-case example permit at most two reflections; the separate eight-review/1152-attempt hard bounds allow the engine to finish an iteration beyond its soft metric limit. Saved identical measurements and reviews are reused. An observed alternate upstream route receives zero fitness for an uninvoked extractor even if it is an allowed classification; a selected target that never dispatches its required extractor remains a native orchestration error.

```sh
HORARY_GOOGLE_KEY_FILE=/absolute/path/to/private-key-file \
  tools/horary-gepa/target/debug/horary-gepa run --state /absolute/path/to/new-gepa-round
tools/horary-gepa/target/debug/horary-gepa status --state /absolute/path/to/new-gepa-round
```

Only the private file's locator is inherited by the hosted native child. Its value is not written to plans or supplied to Codex. The baseline replay path removes the locator and creates no hosted client.

## Recovery and bounds

The plan pins the engine, controller, native executable, Codex executable, current inspected guide, split, campaign manifest, fixtures, initial states, call captures, prompt inspections and case-origin seals. Save/copy the controller and native executable before preparing a long-lived round; rebuilding those pinned paths makes a resume fail closed. New preparation also copies Codex into the round's `executables/` directory, verifies source/copy/source hashes and seals the original path, hash and byte length in `codex-provenance.json`. A signed macOS `CodexCLI.app` retains its bundle layout and an explicit allowlist of public signing metadata; its executable is never resigned or modified. The plan also pins the provenance manifest, and runtime checks verify every support hash or exact relative resource alias. Login, configuration and credentials remain outside the round. App updates cannot replace the sealed snapshot. Partial copies remain unsealed and are never overwritten; existing plans are never migrated. Legacy plans retain their prior executable checks. A process lock covers search and final receipt publication.

Ordered immutable operation receipts reconstruct the engine's deterministic RNG, sampler and component-selection state on resume. Completed student/reviewer/teacher operations and cache hits are hash-checked and reused without resubmission; cache availability does not change their logical operation identity. Review caches bind the actual trace, fixture, book pages, reviewer instruction/schema and Codex executable. Any prepared/submitted operation without a completed receipt blocks automatic replay. Keep its partial artifacts and reconcile it explicitly. Infrastructure failures, missing measurements and budget stops abort search; they never become a fictitious zero score.

An explicit input recovery can create a new immutable plan after an independent source-hashed audit proves that the native operation is sealed and the paid judge exited successfully. It imports the exact native response and the existing semantic judgments; it never resubmits them. The narrowly supported legacy repair binds an absent supplying-turn grade to null when the old judge instead echoed the recorded whole-journey grade. Identity, semantic grades, scores, reasons, citations and findings must remain intact and pass their original source checks. The raw answer, failed journal, owner exit and guard stay untouched. An interrupted provider, unknown result, changed packet or substantive reviewer defect is ineligible.

The new plan retains the original examples, guide, native executable, seed and selection pools. Its hard budgets subtract every predecessor reservation, including the completed paid review; imported cache operations spend no new calls. Ancestral source hashes are checked on preparation, execution and status observation. The new controller and public Codex snapshot are separately pinned. This is infrastructure recovery, not new prompt teaching or model qualification.

GEPA's metric-call setting is an iteration-boundary budget and may overshoot by one iteration. Separate hard pre-submission reservations bound teacher calls, independent review calls and physical student generation attempts. A classifier group reserves three completed-service-error attempts; an input group conservatively reserves two input branches times three service attempts (current input execution dispatches individual calls). Unknown batches invalidate the measurement. Uncertain operations retain their reservations. Report metric evaluations, logical call groups, actual provider calls, physical attempts and cache reuse separately.

Fresh native children use a fixed 14,000-input-token/minute client budget. The controller leaves 65 seconds before each fresh function, including the first one after a prior owner exits; per-function client restarts cannot bypass token pacing. Completed-operation replay, cache hits, inspection and Codex reflection do not consume this wait. This conservative pilot pacing is separate from model inference latency.

`operations/` retains exact teacher prompts, schemas, JSONL events, answers, actual native calls, checkpoints, rejection feedback, final frames and individual gates. `interruptions/` preserves each stop. `search-result.json` retains candidates, parents and development scores; `selected-program.json` is a reviewable overlay, never an automatic application change. A selected overlay still needs fresh complete-journey regression, independent source review and the fixed reserved comparison. Previously paid judge/writer jobs in other rounds stay intact and are not resubmitted by this tool.

`status` is a read-only snapshot of those receipts. It verifies completed operation artifacts and reports unsettled or unverified operations, hard reservations and remaining bounds, unique settled native physical attempts, separate native/reviewer cache reuse, native hurdle status, and independent review scores. Unknown reservations or missing native witnesses remain unknown; cache hits never count old paid attempts twice. Reservations and unfinished operations do not prove submission, failure or permission to retry. Final scores require the immutable plan's digest. Results from older copied controllers lack that binding and remain explicitly unverified in this view; their original files are preserved. The view includes evidence paths and case IDs, without copying questions, gold, prompts or reviewer reasons. It neither acquires the run lock nor opens credentials or invokes a model. The owner can still be publishing files while the snapshot is read.

## Offline checks

```sh
cargo fmt --manifest-path tools/horary-gepa/Cargo.toml -- --check
cargo test --manifest-path tools/horary-gepa/Cargo.toml --locked
cargo clippy --manifest-path tools/horary-gepa/Cargo.toml --locked --all-targets -- -D warnings
```

Ordinary tests use no credentials or models. They check real pinned-engine selection, source-preserving candidate assembly, train/development/reserved separation, immutable recovery, hard reservations, incomplete-journey boundaries and noncompensating semantic gates. Native classifier regressions separately check real executor acceptance, wrong/missing gold labels, capture tampering and logical-versus-physical call limits.
