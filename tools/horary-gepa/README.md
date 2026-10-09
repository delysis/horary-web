# Horary GEPA

An iterative Rust prompt optimizer connected to the application's actual neural functions. It uses the [`dsrust-gepa` engine](https://github.com/getlatentic/dsrust/tree/f24adde08c1d8850e4d7079d019643bb40f905cb/gepa), pinned at `f24adde08c1d8850e4d7079d019643bb40f905cb`. This is an alpha Rust port with numerical conformance tests, not a production-qualified DSPy implementation. Only its LLM-independent GEPA engine and reflection formatter are used. Python is not executed or required; the application's model runtime is unchanged.

## Initial executable scope

The first adapter optimizes `intake / classify_question`: method and answer facet. It stops after the real executor accepts that classifier's `Turn`; it does not reset a specialist checkpoint or pretend a later stage is an initial question. Native validation, rejected attempts and same-step repair remain active. Its fitness compares the returned frame against authored labels and allowed alternatives, not JSON validity.

The generic metric module also represents elicitation, extraction, reading and complete-journey requirements. Those need their own execution adapters and validated independent source reviews before use in search. They are not claimed as completed integrations. A classifier improvement cannot qualify interpretation, voice, the local model or the packaged application.

## Search and evidence

GEPA selects candidate parents, samples reflection minibatches, proposes a mutation to one ordered teaching component, tests it on that minibatch, and retains improving candidates with their development scores. Merge is disabled for this initial integration. Components come from the actual system guide's editable teaching regions. Book blocks, tool schemas, native facts, rejected-answer feedback and other messages remain unchanged. Candidates use the existing typed `Program` and are checked against the actual classifier signature and guide hash.

Codex reflection uses the user's saved login through the existing tool-free, ephemeral process, without a model override or API-key environment. It receives only reflection-training cases, their actual inputs/contract, returned state, native classification failures and authored classification expectations. Development examples select candidates and never enter reflection. Both search pools must be disjoint subsets of the existing frozen **training** partition. Original reserved cases remain excluded from this optimization round.

The student uses hosted `gemma-4-26b-a4b-it` through the native unconstrained-text client. No Google response schema, response MIME restriction or local inference is used. A Codex-only output schema validates the writer artifact; it does not constrain the student.

Baseline controls can reuse a sealed completed capture without new generation. The executor must recreate the exact original prompt, input and schema at every captured classifier/repair call, consume that prefix exactly, then grade its returned frame. Changed prompts always need fresh hosted generation. Raw later specialist outputs cannot be transplanted into a changed program.

Preparation inspects the current native function without constructing a model or reading credentials. The inspected prompt/schema/input and executable are pinned. Generated contract summaries can change the actual guide even when static prompt files do not change. The default `archived-capture` mode rejects such a mismatch before search; choose `--control-mode fresh-hosted` explicitly to purchase current-code baseline controls. There is no automatic live fallback. Reflection uses the current inspected input/contract and teaching, not an outdated archived guide.

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

```sh
HORARY_GOOGLE_KEY_FILE=/absolute/path/to/private-key-file \
  tools/horary-gepa/target/debug/horary-gepa run --state /absolute/path/to/new-gepa-round
tools/horary-gepa/target/debug/horary-gepa status --state /absolute/path/to/new-gepa-round
```

Only the private file's locator is inherited by the hosted native child. Its value is not written to plans or supplied to Codex. The baseline replay path removes the locator and creates no hosted client.

## Recovery and bounds

The plan pins the engine, controller, native executable, Codex executable, current inspected guide, split, campaign manifest, fixtures, initial states, call captures, prompt inspections and case-origin seals. Save/copy the controller and native executable before preparing a long-lived round; rebuilding those pinned paths makes a resume fail closed. A process lock covers search and final receipt publication.

Ordered immutable operation receipts reconstruct the engine's deterministic RNG, sampler and component-selection state on resume. Completed operations and cache hits are hash-checked and reused without resubmission; cache availability does not change their logical operation identity. Any prepared/submitted operation without a completed receipt blocks automatic replay. Keep its partial artifacts and reconcile it explicitly. Infrastructure failures, missing measurements and budget stops abort search; they never become a fictitious zero score.

GEPA's metric-call setting is an iteration-boundary budget and may overshoot by one iteration. Separate hard pre-submission reservations bound teacher calls and physical student generation attempts, including the native client's maximum three completed-service-error attempts per logical call. Uncertain operations retain their reservations. Report metric evaluations, logical calls, physical attempts and cache reuse separately.

`operations/` retains exact teacher prompts, schemas, JSONL events, answers, actual native calls, checkpoints, rejection feedback, final frames and individual gates. `interruptions/` preserves each stop. `search-result.json` retains candidates, parents and development scores; `selected-program.json` is a reviewable overlay, never an automatic application change. A selected overlay still needs fresh complete-journey regression, independent source review and the fixed reserved comparison. Previously paid judge/writer jobs in other rounds stay intact and are not resubmitted by this tool.

## Offline checks

```sh
cargo fmt --manifest-path tools/horary-gepa/Cargo.toml -- --check
cargo test --manifest-path tools/horary-gepa/Cargo.toml --locked
cargo clippy --manifest-path tools/horary-gepa/Cargo.toml --locked --all-targets -- -D warnings
```

Ordinary tests use no credentials or models. They check real pinned-engine selection, source-preserving candidate assembly, train/development/reserved separation, immutable recovery, hard reservations, incomplete-journey boundaries and noncompensating semantic gates. Native classifier regressions separately check real executor acceptance, wrong/missing gold labels, capture tampering and logical-versus-physical call limits.
