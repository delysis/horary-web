# Tool formats and native batching · October 5, 2026

The format is an engineering choice to test against the actual task. We evaluated the installed Gemma 4 12B IT QAT model; these results do not qualify a 2B model, a full horary judgment, or a universal format winner. Every actual prompt and original response is retained outside Git in local evidence. The Rust comparison harnesses are `tool_formats.rs` and `worksheet_xml.rs`.

## Why these comparisons

[Johnson et al., Natural Language Tools](https://arxiv.org/abs/2510.14453) separates tool selection from execution and response writing. [Somma et al.'s replication](https://arxiv.org/abs/2607.03953) studies the same parameterless selection setting. These findings motivate trying natural-language selectors. They do not establish that unvalidated natural-language coordinates, dates or multi-step horary arguments are reliable. Native execution and argument validation remain separate from the output representation.

## Place/moment selector

Twelve authored inputs cover usable/missing device coordinates, explicit chart-place overrides, ambiguous places, context-only event locations/dates, existing-chart preservation and historical/corrected question times. Four formats received the same selection task twice in rotated order: 96 recorded outputs, with no discarded failures or repairs. Exactly one place route and one moment route are required. Expected decisions were authored, not independently SME-certified.

| Format | Exact decisions / 24 | Important observation |
|---|---:|---|
| Constrained JSON | 14 | Some outputs failed route consistency, despite valid JSON. |
| Ordinary JSON, exact whole-fence removal | 18 | The original strict selector rejected all fenced responses; wrapper recovery changes this to 18. |
| XML YES/NO tags | 20 | Best result on this small selector set; still four errors. |
| Natural-language named YES/NO lines | 18 | Short output, but still six errors. |

The ordinary JSON recovery accepts only an exact whole JSON fence. It does not repair decisions, coerce fields or discard surrounding prose. The app's worksheet decoder now has this same bounded fence handling. Duplicate fields and trailing junk fail.

The run used the earlier 1 GiB prefix bank. Observed median total latencies were approximately 15.3 seconds constrained JSON, 4.1 seconds ordinary JSON, 22.9 seconds XML and 20.7 seconds natural language. Output lengths and cache eviction differed, so these are observations rather than a controlled speed ranking. Later four-case cache measurements use the revised memory budget.

## Typed arguments and judgment worksheets

Five inputs test retention of a marriage question through a city reply, literal candidate IDs, a historical civil time, both reception directions, and a clean seven-day-absence versus one-year boundary. JSON constrained, JSON ordinary and XML use the same typed native worksheet contract.

The first attempt exposed useful failures: intake emitted null for a required focus; a generic XML date example contaminated a reception worksheet with unrelated date fields and attributes. A model also emitted an image protocol marker in prose. Original failures remain in `worksheets-attempt-01`; they are not counted as successes. The first year-boundary fixture also contained contrary reception, confounding its expected verdict; the retest isolates the boundary with no supplied motive.

The intake lesson now specifies a non-null default focus. XML examples are generated from that particular task's contract rather than an unrelated example. Protocol markers fail native prose validation. On the repaired attempt, each format passed all five defined checks. Total output tokens were 997 constrained JSON, 1,071 ordinary JSON and 1,058 XML. Ordinary JSON/XML pairs shared a native batch: their shared wall times cannot be treated as separate sequential latencies.

These checks cover selected fields and explicit limits, not every semantic implication of the answer. Later contact/sign-change and reception teaching refinements have separate reading evidence. The format experiment records exact prompt hashes so those versions are distinguishable.

The larger synthetic reading subsequently rejected two ordinary JSON batch proposals: an unsupported `mixed` check state and a duplicate `unknowns` field. Each original was retained and repaired by one constrained call. The completed answer was assessable, but manual review still found overstatement of mutual regard and an incorrect debility name in an upstream reception finding. Passing typed checks does not establish semantic correctness; these are further review targets, not silently accepted fixes.

## Decision for this draft

Keep single text tasks constrained, and use ordinary cached batching plus native validation for independent worksheets. Both XML and natural-language selector implementations remain available for comparison in the Rust test harness, without adding user-facing settings. There is not yet enough typed-argument evidence to justify changing every production worksheet to XML or natural language. A useful next evaluation would broaden ambiguous intake and argument cases with Eileen's annotated examples; syntax acceptance alone is not the target.

## Actual native batch and cache evidence

The pinned native-kit API supports four independent generation cases and an authenticated prefix per ordinary case. Horary now connects that API instead of issuing one generation per independent worksheet. Condition, reception and contact mechanics run together; lost matters add location as the fourth task. Each has its own teaching and evidence. Civil-time parsing correctly waits for the chosen place's zone; place and moment sufficiency checks run independently.

A four-case hardware probe retained the same native owner across cold and warm runs. All four distinct one-word answers were correct. Each warm case reused its own 502-token lesson, with 13–14 new prompt tokens. Batch wall time was 15,985 ms cold and 4,576 ms warm. This qualifies that cache/batch boundary on this machine, not the complete reading's latency or astrology.

A later probe with explicit lesson-bank instrumentation passed again: 11,676 ms cold and 2,414 ms warm. All four warm cases had `lessonBankHit=true`, with 0–1 ms lesson preparation. This distinguishes a bank hit from physical reuse immediately after a newly prepared prefix. The full same-owner reading measurement predates these additive metrics: 709,448 ms cold and 466,638 ms warm, including both retained validation repairs. Fixed-prefix reuse was reported throughout, but this reading remains far too slow. Timings are local observations under uncontrolled machine load, not a causal performance guarantee.

A diagnosed native-kit restore bug initially reported zero physical reuse after successful native-state restoration. The narrow fix republishes resident prefix authority only for authenticated owned native imports into sequence zero, never token replay or failed imports. The regression changed from 0/508 reused tokens to 507/508, and the exact native-kit source passed its macOS component gate. The constrained batch API still lacks supplied per-case prefixes; Horary does not conceal that boundary or claim cross-request continuous batching.

## Local evidence and reproduction

The private evidence root is `/Users/george/.codex/evidence/horary-staged-method-20261005/`. It contains selector trials, both worksheet attempts, prefix failure and repair logs, four-case cold/warm receipts, and full synthetic reading receipts. None is Eileen's private reading. Full prompts and raw outputs are inspectable there; all trials remain available.

Ignored Rust hardware tests require `HORARY_NATIVE_LLAMA_TEST_MODEL` and a **new** evidence directory. The harness refuses to overwrite a previous attempt. The ordinary test suite validates the parsers, duplicate rejection, typed shapes, source quotations and scheduler mechanics without loading weights. Hardware experiments and expert judgment are distinct gates.
