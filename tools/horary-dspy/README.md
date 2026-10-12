# Horary DSPy experiment

This crate uses the full [getlatentic/dsrust](https://github.com/getlatentic/dsrust/tree/f24adde08c1d8850e4d7079d019643bb40f905cb) API directly, pinned to `f24adde08c1d8850e4d7079d019643bb40f905cb` in [Cargo.toml](Cargo.toml) and [Cargo.lock](Cargo.lock). Rust derives the neural signatures; the port owns prompt rendering, parsing, demonstrations, evaluation and optimization. The local code supplies the Horary module, source context, provider transport and grading. It does not implement a replacement GEPA engine.

This is an experimental optimizer path. It has not been promoted into the production application. The default teacher policy is now `--teacher pool`: use the available listed Gemini text generation models in a deterministic sequence, preferring Gemini 3.8 Flash, and use the saved Codex login only after those eligible routes have known quota or availability failures. Each model has its own cache and pacing state; all teacher submissions share one physical-call cap. An uncertain response, authentication failure or completed bad semantic answer never triggers model substitution. Explicit `--teacher gemini` and `--teacher codex` remain available for controlled experiments. There is no qualified new application winner from this path.

## The complete conventional queue

`queue-plan` seals 130 queue tasks: 88 DSPy optimization runs (one mixed-family classifier, three auxiliary functions, 42 extractors and 42 readers), and 42 end-to-end checks. Each initial reading family has twenty cases, split 12/4/4. The independent judge is fixed. The queue calls the pinned port's GEPA compiler directly; it does not implement a second optimizer. This first queue optimizes instructions with zero bootstrap demonstrations, an explicit policy that avoids repeating the earlier bulky-bootstrap pilot. The optional older `optimize` command still supports the port's standard bootstrap path and preserves full teacher demonstrations.

```mermaid
flowchart TB
    sealed[Sealed source, book, examples and available models] --> c[Mixed-family classification GEPA]
    c --> aux[Place, moment and update GEPA]
    aux --> x[Selected family's extraction component GEPA]
    x --> r[Same family's reading component GEPA]
    r --> g[Fresh original question: classifier → extraction → native chart → reader]
    g --> next[Next question family]
    next --> x
    g --> review[Independent book-grounded dimensions and held-out results]
    review --> proposed[Reviewable candidate; no automatic application installation]
```

The reading component receives an independently authored accepted consultation and a real native chart. It is labelled as a component, not successful elicitation. The journey gate instead acquires its own facts and calculates its own chart from the actual classifier and extractor outputs. Compiled components must match the queue's executable, teaching sources, exact fixture digest and canonical family bank. The queue rechecks source and fixture identity at each job boundary. Expected method, facts, chart seed and grading rubric never become journey inputs.

The standalone reading component is now the version-2 [ReadingProgram](src/reading.rs), with two optimizable port predictors: `reading_branch.<family>` and `reading.<family>`. When the accepted consultation already contains a valid branch, it calls only the reader. Otherwise its typed, ordinary-text selector receives the original question, accepted consultation and that family's source taxonomy. It selects an actual branch; it cannot borrow a grading label or page-coverage witness. The reader receives the exact existing `catalogue::branch_source` allocation for that branch, including common foundations and cross-chapter passages. Chart evidence and native unknowns pass through unchanged. A received unsupported branch is an observed native rejection, not a transport retry or a guessed route. The independent judge and GEPA feedback retain the complete family source context.

Both reading predictors are saved and source-validated together. Older one-predictor reading artifacts fail the versioned loader. A fresh Journey already owns its classifier's branch, so it validates the complete artifact and explicitly projects the reader, recording the bypassed selector's identity. This projection is source-qualified, not semantic qualification. The native exporter still accepts only a classified Journey's extractor/reader artifact; it rejects a standalone reading Module rather than silently discarding its selector. No candidate is installed automatically.

```mermaid
flowchart LR
    input[Original question and accepted consultation] --> branch{Observed valid branch?}
    branch -- yes --> source[Exact existing branch source]
    branch -- absent --> select[Optimizable reading_branch predictor]
    select --> valid{Valid family branch?}
    valid -- yes --> source
    valid -- no --> failure[Observed native rejection]
    source --> reader[Optimizable reading predictor plus unchanged native evidence]
    reader --> review[Fixed full-family independent review]
    failure --> review
```

Function-local dimensions and native checks drive optimization. Inapplicable stages cannot veto a component. Raw independent-summary disagreements remain visible and withhold qualification; they are not rewritten as a win. A completed invalid fact array, unsupported route or invented native citation is an observed rejection with feedback. Missing/interrupted provider evidence remains unknown and stops further submissions. The port's fallback zero for an execution error cannot qualify a search result.

Classification validates method, method-specific facet and book branch before scoring. The allowed facets come from the same generated native contracts supplied to the classifier. A branch name in the facet field is an observed failure even when the independent judge returns a positive score; both raw outputs remain unchanged in the trace. Facet membership establishes format validity only, not whether a valid facet answers the actual question.

Google's listed input context limit is checked separately from input-token pacing. The configured rate is an initial hint, not a per-request length limit. The limiter probes higher throughput after successes up to a recorded ceiling, adopts actual provider quota hints on 429, and retains RetryInfo/Retry-After waits. One student client and limiter span the entire queue. Physical retries count against the same caps. Never restart a queue to bypass its durable guard or reset counters.

Quota dimensions normally match the exact requested model. The fixed `gemma-4-26b-a4b-it` student also accepts Google's observed `gemma-4-26b` quota group, with an optional `models/` prefix; a settled response reported 16,000 input tokens per minute under that identity. Other Gemma sizes and arbitrary prefixes do not match. Raw violations remain unchanged in the receipts. This transport correction does not retroactively change an immutable running worker or its recorded decisions.

From a built, copied executable, using absolute paths:

```sh
horary-dspy models --output /absolute/path/to/model-listing.json
horary-dspy preflight --examples /absolute/path/to/examples \
  --models /absolute/path/to/model-listing.json --output /absolute/path/to/preflight
horary-dspy queue-plan --examples /absolute/path/to/examples \
  --models /absolute/path/to/model-listing.json --output /absolute/path/to/queue
horary-dspy queue-execute --examples /absolute/path/to/examples \
  --plan /absolute/path/to/queue/queue-plan.json
horary-dspy queue-status --output /absolute/path/to/queue
```

Preflight formats every baseline exchange locally and counts the largest rendered exchange per function by default; `--all-counts` counts every exchange. For a standalone reading whose branch is unobserved, it enumerates the selector and all declared branch-reader alternatives without calling a model or pretending any branch was selected. The inventory distinguishes these alternatives from cases. Sampling and byte sizes do not prove every request's token admission. The actual transport counts every exact request before generation. These checks establish source and execution mechanics, not guaranteed semantic improvement. The plan records nominal function evaluations, up to two student functions per standalone reading, and hard physical caps. Until observed tokens, waits and latencies arrive, elapsed time is a conditional forecast rather than a promised completion time.

A closed queue with no completed jobs can carry its exact transport caches into a separately reviewed, fresh queue. `queue-continuation-inspect` performs no provider calls: it checks the parent closure receipt, exact owner/child PID absence, complete physical request/body/result identities, the full sealed carrier file sets and actual normalized response counts. Its manifest records reviewed operational source changes, unchanged original cases, splits, book, signatures, schemas and caps, and any changed reading rendering that needs a fresh baseline. The new queue imports carrier files only; it does not import grades, development traces, a partially compiled program or the old start guard. The ordinary pinned compiler runs again and exact settled calls replay through its adapters without repurchasing them. A witnessed unusable HTTP 200/MAX_TOKENS teacher output remains preserved and may use the separately bounded same-route format repair; uncertainty or an existing semantic judgment is not permission to retry.

An alternate closure may explicitly authorize one completed student transient HTTP failure. The receipt must name its exact logical key and request/failure hashes; every old attempt must have a complete 429/500/502/503/504 response, and there may be no raw, normalized or parsed model answer. At most three new physical attempts use a separate key bound to that closure and the unchanged request. The old request, failure and attempts remain byte-identical. The new ordinal span and all other calls count against the original cumulative cap; no unrelated fresh student call may consume this slot before activation. A new failed or uncertain slot cannot repeat. The teacher resumes from its last settled completed receipt and retains the complete prior route cache set. Zero/daily quota and semantic/format failures are not student recovery permissions.

Reviewed reading-v2 and export source deltas require exact before/after hashes in the closure, not prose approval. Static signatures, extraction/classification inputs, case banks, splits, book bytes, schemas and native calculations remain fixed. Reading input changes receive fresh baselines. Recursive predecessor receipts and their closed owners are checked before carriage.

Prepare the manifest with the final executable, then bind it to both new planning and execution:

```sh
horary-dspy queue-continuation-inspect --parent /absolute/path/to/closed-campaign \
  --closure-receipt /absolute/path/to/verified-closure.json \
  --examples /absolute/path/to/examples --review-note "Reviewed carrier and rendering changes" \
  > /absolute/path/to/continuation-manifest.json
horary-dspy queue-plan --examples /absolute/path/to/examples \
  --models /absolute/path/to/original-model-listing.json --output /absolute/path/to/new-queue \
  --continuation-manifest /absolute/path/to/continuation-manifest.json
horary-dspy queue-execute --examples /absolute/path/to/examples \
  --plan /absolute/path/to/new-queue/queue-plan.json \
  --continuation-manifest /absolute/path/to/continuation-manifest.json
```

Inherited student and teacher physical attempts are charged against the original caps, including completed HTTP failures and the unusable teacher output. Their fresh global ordinals continue from the parent; cache hits do not consume new calls. A second execution of the new queue remains forbidden by its own durable start guard. This carrier continuation is intentionally limited to the verified zero-complete-job boundary and cannot grandfather a learned component or establish model-quality improvement.

The pure `export-native` command can create an explicit native-format teaching seed with a source-bound authored mapping. It withholds foreign demonstrations rather than inventing native typed records. The application's separate `real_model_first_message_application_journey` gate starts a default native session with only the original words and fixture device context, optionally using a pinned native candidate. Its full traces still need an independent source review. Neither exporting a seed nor passing the DSPy journey installs or qualifies the production application.

Fresh native outcomes record `native_classification` separately from `native_structure`. Its method/facet comparison uses the unique accepted `classify_question` record and the earliest matching saved native completion checkpoint, with original message and record/job hashes. Focused extraction may subsequently refine the frame; a later reading failure cannot change this classifier grade. Missing, ambiguous or damaged acceptance remains unknown (`passed: null`), while an observed route mismatch is false. This is a mechanical comparison with private fixture labels; principal, book branch, semantic preservation of the extracted question and source-correct interpretation still require independent review. It neither stops the journey nor installs a prompt.

The separate `export-native-classification` command handles the mixed-family queue's exact single `classification` predictor. Its strict [plan](src/classification_export.rs) binds the original compiled bytes and version-1 `component-source.json` separately from the target native manifest, classification contract and captured guide. Producer optimizer/teaching hashes remain provenance; they need not equal the new exporter's implementation. Classification declaration, taxonomy and book mismatches fail. The explicit target is `intake` / `classify_question` / `method: null`; omitted or foreign selectors fail. An author must bind the learned instruction hash and write teaching for the actual native Turn contract. The tool preserves the baseline guide and every quoted book block, withholds all foreign demonstrations, and converts no branch, principal or clarification output into native records. The resulting seed needs fresh native and independent semantic qualification; it is neither installed nor admitted as a component of a different DSPy queue.

```sh
horary-dspy export-classification-requirements --output /absolute/path/to/requirements.json
horary-dspy export-native-classification \
  --compiled /absolute/path/to/original-classifier/compiled.json \
  --producer-source /absolute/path/to/original-classifier/component-source.json \
  --baseline-manifest /absolute/path/to/current-native/source-manifest.json \
  --plan /absolute/path/to/authored-classification-plan.json \
  --output /absolute/path/to/new-uninstalled-seed
```

Validation precedes creating the output directory. The export retains exact copies of the compiled source, producer receipt, target manifest and authored plan alongside `program.json` and its provenance receipt. Existing proposals are never overwritten.

The first actual student reading received a completed Codex review with score 0.75: classification and extraction passed, while reading and answer quality failed. The feedback identified unsupported absence claims, translation/collection omissions and overclaiming from `None` or unknown evidence. Its raw answer remains intact. A validator initially mistook an exact disabled-Code-Mode startup notice for tool execution; the narrow notice check was repaired so that successful paid answer can be reused without resubmission. The repair changes neither the judgment nor the failed reading's qualification.

The complete first-message module in [journey.rs](src/journey.rs) starts with only the original question and device context. Expected method, facts, rubric and chart seeds are evaluation labels; they are not student inputs. The shared classifier selects the actual method, facet, principal and book subbranch. A wrong route remains the observed route: the runner does not substitute the expected family to reach a reader.

```mermaid
flowchart TD
    Q[Original question and actual device context] --> C[Shared classifier: method, facet, principal, branch]
    C --> X[Extractor for the actual selected family]
    X --> V{Native source and input checks}
    V -->|unresolved or invalid| U[Observed incomplete journey]
    V -->|valid inputs| N[Native place, calendar and chart tools]
    N --> R[Reader for the actual selected family]
    R --> J[Separate book-grounded judge on the recorded teacher route]
    U --> J
    B[Hash-checked Frawley primary text] --> X
    B --> R
    B --> J
    E[Authored rubric: judge only] --> J
    J --> T[Recorded dimensions, feedback and qualification limits]
```

The extractor produces canonical facts with exact source quotes or explicitly explained contextual inferences. Native tools resolve the reader's place and question instant, retaining event place and event date as separate context. They compute the chart from those actual inputs rather than borrowing the fixture's expected chart. The reader receives native calculated evidence and primary text for the classifier's actual branch. The separate judge assesses classification, extraction, reading and answer quality against the authored rubric and family source context. A well-formed answer alone cannot pass.

The catalogue has these distinct scopes:

| Scope | Count | Role |
| --- | ---: | --- |
| Substantive reading families | 42 | Twenty complete first-message examples each: 840 case fixtures. |
| Native method labels / typed signature families | 44 | The 42 reading families plus `wish` and `unclassified` routing sentinels. Each label has an extraction and reading signature. |
| Auxiliary example banks | 60 | Twenty each for place, moment and consultation update; separate from the initial 840 cases. |
| Later and boundary banks | Separate | Missing-information, supplying, correction and genuine method-limit cases; preserved under `examples/later/`. |

Every initial family bank has 12 training, four development and four test cases, with at least eight explicit and eight implicit cases. Initial histories and missing-input labels are empty because the first message is intended to contain the necessary information. This describes fixture sufficiency, not measured reading success. `wish` with an undisclosed matter and `unclassified` cannot count as completed readings. A disclosed wish should route to its real subject. The native catalogue's implementation or expert-review status is also distinct from whether the book supplies a procedure.

[coverage.json](coverage.json) records the full taxonomy, subbranches, primary page witnesses and genuine limits. It uses [ELICITATION_DESIGN.md](../../docs/ELICITATION_DESIGN.md) for question distinctions and [READING_CONTRACTS.md](../../docs/READING_CONTRACTS.md) plus the full Frawley OCR for procedural obligations. The old report's BERT controller and canned sentence-bank design are not the executable design here. [book.rs](src/book.rs) checks the private OCR's exact SHA256 before rendering source context; [catalogue.rs](src/catalogue.rs) selects student text from the actual classified branch. The partial native passage index is not treated as the complete book. Missing native calculations, approximate positions and finite event-search coverage remain actual limits; the model must not invent later positions, stars, Parts or antiscia.

The family compiler exposes only its extractor and reader through `Module::named_predictors()`. The global classifier and independent judge stay fixed during family optimization, preventing a family run from learning an always-this-family classifier. The signatures are declared with `#[derive(dsrust::Signature)]` in [signatures.rs](src/signatures.rs); classifier, place, moment, update and judge signatures are also declared there. `optimize-classification` is a separate function experiment over all 840 cases, with 504 training, 168 development and 168 test cases. Its gate assesses classification alone; it does not claim that extraction or a reading executed. Its compiled classifier is not automatically installed into a family run or the app.

The actual optimization calls in [runner.rs](src/runner.rs) and [optimize.rs](src/optimize.rs) use the port's `Predict`, `Module`, `Evaluate`, `BootstrapFewShot` and `GEPA`:

```rust
let mut bootstrap = BootstrapFewShot::new(Graded::metric);
bootstrap.max_bootstrapped_demos = 1;
bootstrap.max_labeled_demos = 0;
bootstrap.metric_threshold = Some(1.0);
bootstrap.max_errors = 1;
bootstrap
    .compile_with_teacher(&mut program, &mut strong, &train)
    .await?;
program.save(&output.join("bootstrapped-full.json"))?;
program.compact_demos()?;

let search = GEPA::new(Graded::feedback, teacher)
    .with_max_metric_calls(max_metric_calls)
    .with_seed(23)
    .compile(&mut program, &train, &development)
    .await?;
```

BootstrapFewShot retains teacher-generated training demonstrations only when the metric reaches 1.0; no labeled gold demonstrations are added. Raw teacher responses remain in the transport artifacts, and family optimization saves complete demonstrations in `bootstrapped-full.json` before [demo.rs](src/demo.rs) projects them for teaching. The projection removes repeated primary/contract context and private grading fields. Demonstration chart evidence keeps every cited native ID and its exact detail, all planet positions, cusps, Moon-related evidence, native boundaries, anchors and tool limits. Unknown citations fail projection. This does not alter live oracle chart inputs, saved raw teacher outputs or rubric facts; omitted demonstration rows do not imply that the original chart lacked them.

Salient primary book text accompanies the actual reading and extraction inputs and the independent reviews. The family's reviewer context includes the primary chapter and every cross-chapter page declared by its question branches; a source-only invariant checks all 44 labels. GEPA also receives that primary context in feedback for its captured training minibatch, so the prompt writer has the applicable source alongside actual outputs and observed failures. Development scores select candidates; development and test words, gold and traces do not enter reflection or demonstrations. Test cases are evaluated after compilation. Do not tune another round using test findings and still call that same test bank held out.

The pinned GEPA engine checks `max_metric_calls` at iteration boundaries, so an in-flight iteration can exceed that soft limit. It is not a physical API-call cap and excludes the separate bootstrap and before/after/test evaluations. [google.rs](src/google.rs) has separate generation limits and rolling input-token pacing. The upstream GEPA adapter can assign its fallback zero when a module errors; that is not a successful independent judgment. Transport, parsing or reviewer errors still withhold a qualified comparison, and their original requests and partial traces must be retained.

The CLI accepts one family or `--method all` for both `run` and `optimize`. The all-family path processes the 42 substantive banks with shared student and teacher clients, exact-response caches, physical-call counters and one rolling student token window. Quota is not reset between families. The metric budget for family optimization is per family; global-classifier optimization has its own mixed-bank budget. From the repository root, these commands validate or inspect source without making model calls:

```sh
cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  check --examples tools/horary-dspy/examples

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  inspect --method movable_deal --examples tools/horary-dspy/examples

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  signatures --output /absolute/path/to/signatures.json
```

`inspect` reports component-input sizes and fixture chart evidence; it does not execute or qualify a first-message journey. The separate component builder in `catalogue::example(task="reading")` deliberately uses an authored consultation for component diagnostics. `run` and `optimize` instead use `Journey::example`, whose declared student inputs are only question and device.

These commands make hosted model calls and require a new output directory:

```sh
cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  --teacher codex run --method movable_deal \
  --examples tools/horary-dspy/examples --training-limit 1 \
  --output /absolute/path/to/fresh-transport-check --student-tokens-per-minute 14000

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  --teacher codex run --method all --examples tools/horary-dspy/examples \
  --output /absolute/path/to/fresh-all-family-control --student-tokens-per-minute 14000

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  --teacher codex optimize --method movable_deal --examples tools/horary-dspy/examples \
  --output /absolute/path/to/fresh-search --max-metric-calls 24 \
  --student-tokens-per-minute 14000

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  --teacher codex optimize --method all --examples tools/horary-dspy/examples \
  --output /absolute/path/to/fresh-all-family-search --max-metric-calls 24 \
  --student-tokens-per-minute 14000

cargo run --manifest-path tools/horary-dspy/Cargo.toml -- \
  --teacher codex optimize-classification --examples tools/horary-dspy/examples \
  --output /absolute/path/to/fresh-global-classifier --max-metric-calls 2048 \
  --student-tokens-per-minute 14000
```

`run --training-limit N` is a training-only transport check, not the full family evaluation; with `--method all`, it selects up to N training cases per family. `models` checks the requested Google identities even when the temporary teacher is Codex. The student always remains hosted `gemma-4-26b-a4b-it`. Select `--teacher gemini` before the subcommand to use `gemini-3.8-flash` for teaching, reflection and judging. The Google credential is read from a private file locator and is never an example, prompt or evidence artifact.

The port's `ChatAdapter::without_json_fallback()` renders and parses ordinary text. Both transports reject response schemas and tools, so typed Rust signatures do not imply constrained provider decoding. [codex.rs](src/codex.rs) invokes the existing saved login without a model override, disables tool surfaces, and checks the saved event stream for a completed tool-free turn. Its prompt treats synthetic questions, observed outputs, rubrics and book passages as data. The queue's listed Gemini pool and Gemma student use `generateContent`; the legacy explicit preferred-Gemini transport also retains stateless Interactions with `store=false`. The grading criterion is fixed, while quota-driven reviewer model changes are recorded and remain a comparison limitation requiring source review.

Completed responses can be reused by exact request identity. The Google transport permits at most three physical submissions for a logical request when it has fully received HTTP 429, 500, 502, 503 or 504, with bounded backoff. Each submission is paced, charged against the physical cap and recorded separately. Send failures, incomplete responses, parsing failures and nontransient HTTP errors are not retried; automatic protocol retries and redirects are disabled. A failed or unresolved request already present on disk is never automatically resumed. `import-codex` can copy an already successful, tool-free raw answer into a new cache after a local reporting/parser repair, preserving its original request, prompt, answer, events and exit plus their hashes; it makes no model call and does not revise the judgment. `run --cached-student-only --cached-teacher-only` disables fresh generation for both transports and fails if an exact needed response is absent. This is bounded saved-answer reuse, not a new owner/retry engine or authority to replay uncertain work.

The listed Gemini teacher pool has one narrow format repair for a fully received HTTP 200 response whose preserved raw output ends with `MAX_TOKENS`. It verifies the original request, attempt and response hashes, then seals a distinct request on the same route with an output-only reminder and a larger output bound of at most 8,192 tokens, capped by that model's listed limit. The original input messages, book context, raw response and failed key remain intact; only the cloned system instruction and output bound change. Both calls consume the same shared physical budget. A durable binding links the original key and raw hash to the new key and observed response. Original successful caches take precedence, then bound repaired caches, before any fresh submission. A pending, failed or again-truncated repair stops without another attempt or a substitute teacher. Completed semantic/schema errors without this truncation witness remain actual rejections. Student requests and the legacy explicit teacher transport have no such repair.

Each run preserves a source/configuration manifest and `dataset.json`. Shared Google artifacts include `*.tokens.json`, `*.request.json`, successful `*.raw.json`, `*.response.json`, `*.receipt.json` and HTTP `*.failure.json` where those stages occurred. Physical submission artifacts use `*.attempt-N.request.json`, `*.attempt-N.body` and `*.attempt-N.result.json`, including complete-response and retry decisions. Each Codex operation preserves `request.json`, `prompt.txt`, `answer.txt`, `events.jsonl`, `stderr.log`, `exit.json` and a completed `response.json` when validated. Partial native stage observations are written before independent review; completed reviewed journeys append JSONL containing original inputs, actual stages, native failures, review and score. An all-family run writes separate family results beneath the shared output directory.

Family optimization also saves `baseline-program.json`, `baseline.json`, baseline/teacher/search observations, full and projected bootstrapped programs, `compiled.json` and the final result when execution reaches those steps. The global-classifier command saves a separate component result. Inspect these files directly while a run is active; rerunning a command is not a status query. A missing final artifact is not an empty successful result. There are no Python or frontend steps in this tool's execution path.

Source/bank review, successful execution, an independent semantic grade, a development improvement, held-out performance and production acceptance are separate claims. A compiled program is still experimental: promotion requires fresh actual application journeys, unchanged source and native guards, source-correct full readings and the application's own acceptance checks. This crate does not install a learned prompt into the app or qualify local Gemma, packaged behavior, voice delivery or unsupported native methods.
