# Horary review loop

This Rust sidecar reads completed synthetic evaluation cases, obtains structured reviews through the existing Codex login, and proposes executable teaching edits. It never edits fixtures, native guards, application source, or campaign receipts. Native first-turn and journey results remain separate from the five conversational scores. Full-reading campaigns also receive independent classification, extraction, elicitation and reading scores, using each case's source rubric, accepted specialist worksheets and actual final answer.

There are three entry points: `horary-loop` streams and reviews receipts, `horary-experiment` tests a fixed candidate, and `horary-optimize` coordinates a complete bounded round. Their artifacts remain separate from private readings and the end-user interface.

## Build

```sh
CARGO_INCREMENTAL=0 cargo build --offline --bins
CARGO_INCREMENTAL=0 cargo test --offline
CARGO_INCREMENTAL=0 cargo clippy --offline --all-targets -- -D warnings
```

The shared `../../crates/horary-prompt-program` crate supplies candidate validation and comparison. Both crates have checked-in lockfiles. Build the native campaign executable separately, then keep that executable and its sources fixed throughout trials.

## Use

One automatic round:

```sh
horary-optimize --discovery CAMPAIGN --fixtures FIXTURE_DIRECTORY \
  --native-executable NATIVE_TEST_EXECUTABLE --model GEMMA_GGUF \
  --state FRESH_ROUND_DIRECTORY --review-jobs 1 --max-native-cases 9
```

For a Google-hosted discovery, omit `--model` and provide `HORARY_GOOGLE_KEY_FILE` only in the invocation environment. Both drivers derive provider, model identity, decoding, full versus elicitation scope and case parallelism from the immutable discovery manifest. Native discoveries still require matching GGUF bytes and SHA256. Hosted trials never acquire a local model as fallback; credential locations are absent from plans, traces and reviewer environments.

Reviews and the prompt writer may stream while a hosted discovery runs. A fixed candidate is saved, but paired hosted inference waits for the discovery's completed marker and closed report, so independent processes do not compete for the same project quota. Resume the same round after discovery closes; an interrupted discovery is preserved.

The round reviews completed training receipts, reuses those reviews to propose one candidate, and runs paired control/candidate native campaigns and independent Codex reviews. Add `--watch` to consume a still-running discovery campaign. The default permits one discovery review job, one writer job and at most nine distinct affected native cases; each paired case runs once with the control and once with the candidate. An abstention, native-code repair request, uncertain paid job or over-budget scope stops further spending. A failed training comparison withholds reserved cases. The discovery-review budget survives restarts, including a crash between a paid review and its phase marker.

Resume with the same arguments and state. Completed reviews and native receipts are reused; interrupted native campaigns stay untouched. No output is installed automatically. Read `decision.json` before source review and integration. New rounds get fresh directories and explicitly frozen baselines.

Individual operations:

```sh
horary-loop judge --campaign CAMPAIGN --fixtures FIXTURE_DIRECTORY --state LOOP_STATE
horary-loop propose --campaign CAMPAIGN --fixtures FIXTURE_DIRECTORY --state LOOP_STATE
horary-loop watch --campaign CAMPAIGN --fixtures FIXTURE_DIRECTORY --state LOOP_STATE --max-jobs 4
horary-loop status --campaign CAMPAIGN --fixtures FIXTURE_DIRECTORY --state LOOP_STATE
horary-loop revalidate --campaign CAMPAIGN --fixtures FIXTURE_DIRECTORY --state LOOP_STATE
```

All paths refer to separate directories. State must be outside the campaign and fixture directories. `judge` reviews completed training cases; `watch` waits for new completed cases. `propose` reuses paid training reviews. The default budget is one job, with at most six cases, a 20-minute deadline, a 60KB prompt target and 120KB hard cap including output schema. Larger batches shrink before submission. `--max-jobs 0` explicitly removes the job-count bound; `--once` always limits work to one job.

Codex receives the prompt on stdin with `exec --sandbox read-only --ephemeral --json --output-schema ... -o ... -`. No model override or API key is supplied. API-key environment variables are removed without reading their values. Unused shell, app, plugin, browser, image and hook tools are disabled; skill discovery and web search are suppressed. A tool execution appearing in the event stream rejects the result. The configured model and saved ChatGPT login remain in use.

Short evidence references reduce output tokens. The host expands each reference to its exact case, immutable file, SHA256 and JSON pointer before accepting the review or candidate. Full original requests, prompts, schemas, inputs, raw outputs and checkpoints remain in content-addressed blobs. Grader packets contain actual words, replies, accepted facts, native errors and outcomes; full lessons are not sent to the grader. Full-reading judges receive deduplicated exact book-extract blocks and the per-case source rubric. Writer packets include the selected edit stage's separate teaching regions before and after exact protected book blocks. Each region lists ordered chunk indices, SHA and byte length; an edit must lie wholly within one region. Quoted blocks remain immutable, hashed and withheld from the writer. Native analysis batches retain distinct branch pointers; hosted provider wire receipts exclude credentials and preserve successful attempts even when the complete batch failed. Hosted inference does not qualify the eventual on-device model.

Full-reading review views also share exact repeated values, object keys and text spans in a readable `context_interning.values` table. Its instructions define recursive value aliases, ordered text concatenation and key/value pairing. Expanding that table reproduces every original view value, including malformed raw answers and quoted source bytes; citation IDs remain separate and directly available. The hard prompt-plus-schema cap remains 120KB, with review batches shrinking before submission.

New follow-up summaries carry `execution_provenance`, including a source-file hash and separate submitted-turn/observed-reply flags. The validator checks these against the immutable native outcome; old compact packets use that same original receipt. A script or arbitrary grade object is not evidence that a turn happened. Withheld turns and stale replies permit only null or entirely unobserved follow-up rubrics; every fresh observed reply must be reviewed. Conversation quality remains separate from native journey success.

## Fixed split and validation

The split freezes before the first Codex job. SHA256(method) first-byte parity reserves one of explicit/implicit per question type; missing cases stay in training. Edge IDs use SHA parity. The original bank reserves 55 of 145 cases (44 ordinary and 11 edges); parity is deterministic rather than numerically balanced. The current 21-case supplement brings the bank to 166. Review old campaigns with their frozen fixture copy; a changed bank requires fresh campaign and loop state. Previously examined examples make this **reserved validation**, not a blind benchmark. Reserved words, gold and traces never enter writer packets.

```sh
horary-loop judge --validation --candidate FIXED_CANDIDATE \
  --campaign RESERVED_CAMPAIGN --fixtures FIXTURE_DIRECTORY --state VALIDATION_STATE
```

An independent full-reading baseline may use `judge`, `watch` or `revalidate --validation` without `--candidate`; its manifest must identify `horary_pipeline::run` or `full_reading=true`. Reserved cases never go to `propose`. Paired prompt trials still require their fixed candidate.

Full-reading paired eligibility requires matching reading-rubric hashes, the same full-reading scope, candidate completion of all four native gates, and independent candidate scores of 2 (elicitation may be N/A when no inquiry is needed). A completed control may retain input failures and finish with `NeedsInformation` or `Limited`; its missing reading must remain explicitly unobserved. A complete reviewed candidate then demonstrates a native journey improvement, without inventing an old reading score. Interrupted controls, missing native witnesses, candidate missing/unobserved/partial gates and regressions block eligibility. A capability boundary without a failed input gate remains unsupported. Training must improve a measured gate or existing native/conversation result; reserved validation requires no regressions. Elicitation-only comparisons retain their previous policy and cannot certify later interpretation.

For a discovery campaign, a fixed candidate must name that exact manifest. Paired experiment campaigns instead require `campaign_phase=paired_prompt_trial` and `prompt_experiment` with the exact discovery manifest SHA, fixed candidate SHA, and role. Control requires `prompt_program=null`; candidate requires the matching `prompt_program.sha256`. Default discovery checks remain strict. Validation findings cannot feed the same optimization round. `horary-experiment` runs paired native journeys and concurrent independent reviews in separate ledgers. Native regression, interruption, unused overrides, evidence mismatch, absent reviews, misleading candidate replies or conversational regression block eligibility. Elicitation eligibility can retain unchanged failures; full-reading eligibility additionally requires all four gates. Both are results for source review, not whole-catalogue, astrology-SME or packaged voice qualification.

## Resume and evidence

`split.json` and the baseline manifest are immutable. `ledger.json` is atomically replaced under an exclusive process lock. Each content-hashed job preserves its packet, reviewer prompt, output schema, evidence map, invocation, JSONL events, stderr, last answer, canonical expanded result and completion receipt. A successful job is reused after verifying those hashes; it is never resent. Costs from `turn.completed` remain in the ledger.

A crash without a completion receipt produces an uncertain interrupted job. The tool does not silently resend it. An explicit `--retry-failed` creates another numbered attempt while retaining all prior attempts. Completed semantic failures are successful review jobs, not infrastructure errors. Partial evidence writes are retained and reported. Source/gold drift stops work.

`revalidate` locally checks saved exit-zero answers using their original packet, prompt, output schema, submitted evidence-reference table and hashed source evidence. Paired reviews still require their fixed `--candidate`. It never starts Codex or adds a model attempt. Known skill-discovery and disabled-code-mode capability warnings are recorded only before `turn.started`; unknown errors, tools, malformed events or incomplete turns reject the answer. New invocations disable code mode and suppress unstable-feature warnings in per-invocation configuration only.

Successful local recovery appends `attempts/0001/reassessments/0001/` with an exact copy of the paid answer, canonical expanded output, warning records and completion receipt. The original failed attempt, error, events and exit receipt stay unchanged. Failed reassessments also remain visible. `completed_judgments(state)` loads and verifies either accepted artifact for the experiment driver, so recovery does not require a paid retry or manual editing of failed receipts.

`completed_proposals(state)` uses the same accepted-artifact loader for complete training writer jobs. Abstentions and `repair_required` findings are returned intact; reading the saved outputs never starts Codex or reevaluates a reading.

Token usage, cached tokens, wall time and rejected attempts stay in the ledger. Packet byte limits reduce accidental cost; they are not token-count guarantees. The first expensive development review remains preserved in external evidence. Later packets omit grader lessons, protect book blocks, use short edits and prune repeated context. [The optimization guide](../../docs/PROMPT_OPTIMIZATION.md) describes the architecture and measured boundaries.
