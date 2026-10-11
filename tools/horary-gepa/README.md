# Horary GEPA

An iterative Rust prompt optimizer connected to the application's actual neural functions. It uses the [`dsrust-gepa` engine](https://github.com/getlatentic/dsrust/tree/f24adde08c1d8850e4d7079d019643bb40f905cb/gepa), pinned at `f24adde08c1d8850e4d7079d019643bb40f905cb`. This is an alpha Rust port with numerical conformance tests, not a production-qualified DSPy implementation. Only its LLM-independent GEPA engine and reflection formatter are used. Python is not executed or required; the application's model runtime is unchanged.

## Executable scope

The first adapter optimizes `intake / classify_question`: method and answer facet. It stops after the real executor accepts that classifier's `Turn`; it does not reset a specialist checkpoint or pretend a later stage is an initial question. Native validation, rejected attempts and same-step repair remain active. Its fitness compares the returned frame against authored labels and allowed alternatives, not JSON validity.

The second adapter optimizes one method's `intake / complete_selected_program` extraction teaching. New `extractor-reliability` plans observe the **initial extractor only**: start with the original question, run the real classifier and native extraction/repair, then stop immediately after the extractor's atomic patch is accepted. The test-only observer records that accepted consultation before place, moment, chart, conversation or a supplying turn can change the result. No gold method, accepted checkpoint or old downstream response is transplanted to reach the extractor. Source captures pin its guide for inspection; every measured model call is fresh.

New input searches require `--objective extractor-reliability`, which sets the immutable `initial_extractor_observation` plan flag. An independently cited assessment compares the accepted consultation with the original fixture: sourced facts, actors, question/frame, retained known facts and honest unknowns. Native component grading checks those inputs, excluding chart anchors, readiness, conversation and supplying turns. The accepted native record, revision, input hash and actual initial target attempts bind the observation. A later turn cannot rescue an initially wrong record or add to its repair count.

If the target exhausts its call budget on native rejection, its actual rejected outputs and errors provide negative feedback only after every provider attempt and request/result pair is proven settled. There is no invented accepted consultation. An uninvoked target, deadline, uncertain submission or infrastructure failure remains unobserved and stops measurement. Historical plans omit the new flag and retain their original execution, review protocol and paid identities; their old scores are not rewritten.

Fitness is `(2s + 1/n) / 5`, where `s` is independent accepted-state correctness (0–2) and `n` is the actual focused extractor call count. Incorrect state scores zero. Complete state scores above .8; partial state scores at most .6. Fewer repairs differentiate otherwise correct observations but never outweigh more correct facts. This objective explicitly optimizes repair reliability, not demonstrated new horary understanding or wall-clock latency. `qualified` remains false for component merit.

Every review still separately records whole-input classification, elicitation, extraction, and actor/honesty/continuity/naturalness in actual replies. Those whole-journey guards remain unchanged acceptance requirements before promotion. At `ReadyReading`, input-only execution records a typed native handoff and stops; production instead generates a reading before the guru speaks. Native grading revalidates that permit against current inputs, the actual chart, and exact message position. The reviewer receives hash-bound pointed handoff and boundary receipts, checked against the native binding. The intentionally omitted final reply is unobserved, not a completed conversation. Missing-input turns still make real inquiries and only properly bound supplying turns execute. Reading, voice, the eventual local model and packaged acceptance remain unqualified.

`horary-gepa calibrate` exercises the real metric with offline counterfactual probes: correct repairs have a signal, fixed dialogue changes do not, faster wrong facts lose, uninvoked targets earn nothing, and interruptions are errors. Preparation saves this calibration before any paid calls and rejects whole-conversation input objectives whose only editable component is the initial extractor. Historical immutable plans, raw reviews and zero scores are retained; they are not retroactively regraded.

The independent reviewer returns judgments and citations only. Rust supplies the case identity and the recorded native grades. In the shared legacy `CaseReview` format, `native_journey_pass` means the supplying-turn result (`follow_up_pass`), while `input_journey_pass` is the whole input journey. An absent supplying turn remains null. Requiring a model to echo these similar metadata fields confused a completed review; native facts now remain outside its output contract. The student still uses ordinary unconstrained text.

The third adapter executes **complete reading journeys** and optimizes one actual specialist: significators, condition, reception, contacts, location, or judgment. Use `--function reading-journey --objective selected-stage-reliability --target-method METHOD --target-stage STAGE`. It begins with the original question and device facts, obtains a current native `ReadyReading` permit through the real conversation and extraction process, then runs `horary_pipeline::run`. No accepted record, chart, gold method or specialist checkpoint is injected. The authored rubric is supplied only to the independent reviewer.

Reading merit is the independent 0–2 score for the observed, editable stage, divided by two. The review must cite its actual generation output, native context and pinned book. Separate acceptance gates check classification, elicitation, extraction, conversational actor/honesty/continuity, the actual final interpretation, and **every** authored decisive test, inference boundary and answer obligation. A good component cannot hide an incorrect complete answer. A stage never reached is unobserved; repair the upstream function rather than grading an invented specialist output. A bounded native rejection after authentic invocation can score zero only when every hosted response and request/result pair has settled and the exact rejected native job is preserved. Interruptions and uncertain provider requests remain errors.

The deal contracts distinguish transaction action (`buy`, `sell`, `rent`), contracting actor, literal title owner and, for financial questions, the separately sourced beneficiary. Ordinary completion uses the actual parties; an unused asset role does not remove native intervening contact evidence. Native contact graphs retain possible prohibition, translation and collection witnesses, with their supplied house, condition and reception facts. These representation and routing guards are code responsibilities. GEPA changes teaching, not those facts or guards.

## Search and evidence

GEPA selects candidate parents, samples reflection minibatches, proposes a mutation to one ordered teaching component, tests it on that minibatch, and retains improving candidates with their development scores. Merge is disabled for this initial integration. Components come from the actual system guide's editable teaching regions. Book blocks, tool schemas, native facts, rejected-answer feedback and other messages remain unchanged. Candidates use the existing typed `Program` and are checked against the actual classifier signature and guide hash.

Codex reflection uses the user's saved login through the existing tool-free, ephemeral process, without a model override or API-key environment. It receives only reflection-training cases, their actual inputs/contract, returned state, native failures, authored expectations and validated independent reviews. The initial-extractor reviewer is a separate invocation with its own reserved budget. It compares the actual accepted consultation, or settled native rejection, with the synthetic fixture and bounded OCR witnesses. It does not grade conversation that never ran. Citations bind to exact native records, results, fixture and book hashes; expected answers alone cannot support a score. Historical whole-input reviewers retain their separate conversational and continuity gates. All input protocols leave reading unobserved. Development examples and reviews select candidates and never enter reflection. Both search pools must be disjoint subsets of the existing frozen **training** partition. Membership and materialized expectations are rechecked against their pinned split/fixture sources on run. Original reserved cases remain excluded from this optimization round.

Reflection uses GEPA's default template; a scope reminder is appended separately. The third formatter argument replaces the entire template, so using a reminder there silently discarded the current instruction and all examples. That adapter defect invalidated claims that those mutations used supplied failure feedback. Historical observed grades remain intact. Before reserving a paid reflection, the adapter now checks the actual final stdin for the verbatim current instruction and every training sample's serialized input/schema, observed output and feedback. A sealed `payload-coverage.json` binds those witnesses to the submitted prompt hash. The offline preview and paid submission share this assembly function.

The student uses hosted `gemma-4-26b-a4b-it` through the native unconstrained-text client. No Google response schema, response MIME restriction or local inference is used. A Codex-only output schema validates the writer artifact; it does not constrain the student.

Baseline controls can reuse a sealed completed capture without new generation. The executor must recreate the exact original prompt, input and schema at every captured classifier/repair call, consume that prefix exactly, then grade its returned frame. Changed prompts always need fresh hosted generation. Raw later specialist outputs cannot be transplanted into a changed program.

Preparation inspects the current native function without constructing a model or reading credentials. The inspected prompt/schema/input and executable are pinned. Generated contract summaries can change the actual guide even when static prompt files do not change. The default `archived-capture` mode rejects such a mismatch before search; choose `--control-mode fresh-hosted` explicitly to purchase current-code baseline controls. There is no automatic live fallback. Reflection uses the current inspected input/contract and teaching, not an outdated archived guide.

Input-journey rounds always require `fresh-hosted`, an explicit method and a positive reviewer budget. Preparation rejects cases with no sealed capture of that method's focused guide; it never repairs upstream classification from gold. Classification and extraction execute afresh from the original question. Selecting available guides does not establish whole-bank robustness. A provenance-only case closed by proven pre-submission quota cancellation may supply a guide for fresh controls; it cannot supply an archived answer, measurement or replay. Book context is 1–24 explicitly selected OCR pages (80KB maximum) from the named local source, pinned by whole-file SHA and reproduced byte-for-byte from unique page markers. OCR IDs and printed page labels are kept distinct. Review packets are bounded at 400KB; oversized packets stop before paid submission rather than silently dropping evidence.

## Commands

`prepare-feedback-trial --source /absolute/closed-input-pilot --state /absolute/new-trial` creates a separate one-reflection experiment after a closed input pilot. It reuses exactly the two sealed seed controls and their existing source-validated reviews, retains the original pool/guide/native/seed, and previews the actual training-only writer stdin with no new paid calls. Source closure, every completion seal and recorded PID absence are checked. Changed programs and uncertain work are ineligible. The new hard budget allows one materially different feedback-inclusive writer invocation and at most two changed student functions with independent reviews; predecessor ledgers are retained, not reset or counted as new trials. Imports are revalidated against their original request, answer and source ancestry on journal replay and status. Cached controls are not replications. This narrowly scoped preparation does not install an overlay or qualify a reading.

Compile the ignored native test entry `elicitation_eval::real_model_classification_function` from the isolated application checkout first. Use a separate Cargo target from any running campaign. Then prepare an immutable plan:

For a new source baseline, `campaign` runs the actual hosted full-reading entry on an explicit case list. It seals ownership before releasing the native child, retains unsuccessful outcomes, and never retries an existing directory. `seal-corpus` subsequently verifies the closed native observations and supplies ordinary case origins for search; it must run after the launcher has exited. Neither command fabricates a checkpoint, gold input, semantic score or recovery ancestry.

```sh
HORARY_GOOGLE_KEY_FILE=/absolute/path/to/private-key-file \
  /absolute/copied/horary-gepa campaign \
  --evidence /absolute/fresh-campaign \
  --native-executable /absolute/copied/native-test-executable \
  --cases deal10-movable-explicit-sale-profit,deal10-movable-missing-owner \
  --case-seconds 3600 --max-calls 28
/absolute/copied/horary-gepa seal-corpus \
  --campaign /absolute/fresh-campaign \
  --closure /absolute/fresh-campaign/corpus-closure.json
```

Native exit 101 can mean completed negative semantic observations. An uncertain provider submission, missing response or interrupted corpus remains ineligible for sealing. `campaign` writes its pre-start receipt in a sibling `.owner` directory because native execution creates the campaign directory itself; it mirrors those exact bytes after child exit. Provider quota waiting is recorded separately from generation latency. Use an isolated Cargo target to build new helpers, leaving all historical owned binaries intact.

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

The five metric evaluations in this two-case example permit at most two reflections; the separate eight-review/1152-attempt hard bounds allow the engine to finish an iteration beyond its soft metric limit. Saved identical measurements and reviews are reused. In new component rounds an uninvoked extractor is unobserved and stops measurement, including an alternate upstream route. Historical plans retain their original scoring boundary; their recorded zeros are not rewritten.

```sh
HORARY_GOOGLE_KEY_FILE=/absolute/path/to/private-key-file \
  tools/horary-gepa/target/debug/horary-gepa run --state /absolute/path/to/new-gepa-round
tools/horary-gepa/target/debug/horary-gepa status --state /absolute/path/to/new-gepa-round
```

Only the private file's locator is inherited by the hosted native child. Its value is not written to plans or supplied to Codex. The baseline replay path removes the locator and creates no hosted client.

## Recovery and bounds

The plan pins the engine, controller, native executable, Codex executable, current inspected guide, split, campaign manifest, fixtures, initial states, call captures, prompt inspections and case-origin seals. Save/copy the controller and native executable before preparing a long-lived round; rebuilding those pinned paths makes a resume fail closed. New preparation also copies Codex into the round's `executables/` directory, verifies source/copy/source hashes and seals the original path, hash and byte length in `codex-provenance.json`. A signed macOS `CodexCLI.app` retains its bundle layout and an explicit allowlist of public signing metadata; its executable is never resigned or modified. The plan also pins the provenance manifest, and runtime checks verify every support hash or exact relative resource alias. Login, configuration and credentials remain outside the round. App updates cannot replace the sealed snapshot. Partial copies remain unsealed and are never overwritten; existing plans are never migrated. Legacy plans retain their prior executable checks. A process lock covers search and final receipt publication.

Ordered immutable operation receipts reconstruct the engine's deterministic RNG, sampler and component-selection state on resume. Completed student/reviewer/teacher operations and cache hits are hash-checked and reused without resubmission; cache availability does not change their logical operation identity. Review caches bind the actual trace, fixture, book pages, reviewer instruction/schema and Codex executable. Any prepared/submitted operation without a completed receipt blocks automatic replay. Keep its partial artifacts and reconcile it explicitly. Infrastructure failures, missing measurements and hard reservation exhaustion abort search; they never become a fictitious zero score. A component's settled native rejection at its logical call cap is a different observation: its real failure can provide negative feedback under the exact source-bound proof.

An explicit input recovery can create a new immutable plan after an independent source-hashed audit proves that the native operation is sealed and the paid judge exited successfully. It imports the exact native response and the existing semantic judgments; it never resubmits them. The narrowly supported legacy repair binds an absent supplying-turn grade to null when the old judge instead echoed the recorded whole-journey grade. Identity, semantic grades, scores, reasons, citations and findings must remain intact and pass their original source checks. The raw answer, failed journal, owner exit and guard stay untouched. An interrupted provider, unknown result, changed packet or substantive reviewer defect is ineligible.

For a settled native call-budget failure, the paused job owns the original input seal, while each repair has its own input seal. Corpus sealing binds every single or parallel hosted result to its actual native record, finds the unique unrepaired input and paused job, and verifies each rejected repair against the preceding worksheet and validation error. Parallel sibling stages remain separate lineages. Comparing the last repair seal directly with the job seal would incorrectly turn a completed negative observation into an uncertain request. The sealer preserves the original failure and requires all provider responses to be settled; it never supplies a score or authorizes resubmission.

The current judged-only protocol also permits validation-only recovery after an independent audit identifies a validator defect. Preparation revalidates the original paid answer against its exact source packet before creating the new state. No judgment changes are allowed. When input execution stops at a reading permit without a conversational reply, reply quality remains unobserved; inquiry can separately be inapplicable if neither authored nor actual inputs require one. A necessary inquiry cannot be marked inapplicable. Recovery verifies every recorded owner, controller, native and judge PID is absent, including both historic and current owner-receipt formats. Imported judgments retain their raw answer hash and explicit citation/metadata binding receipt.

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
