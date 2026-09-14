> The active interface is now the single-document conversation prototype. See [conversation architecture](CONVERSATION.md). Earlier form-based interaction/validation descriptions below are historical and do not establish acceptance of the new conversation or audio path.

# Reading process

The working authority is John Frawley, *The Horary Textbook* (2005). The private OCR was checked against the source; neither the OCR nor the book is distributed. References in the app are printed pages. In the reviewed scan, PDF page = printed page + 9.

## From question to reading

1. Understand the question. Use the astrologer's place and the moment of understanding; clarify an ambiguous question before choosing actors. The app makes this moment explicit and allows a historical chart.
2. Cast once. Resolve the entered civil time with its IANA timezone, reject nonexistent times, and distinguish repeated times. Use tropical positions, Regiomontanus houses, and traditional rulers. Display and interpretation share these facts.
3. Identify the relevant houses and significators. Keyword suggestions are provisional. The Moon normally co-signifies the querent unless already assigned to the quesited. Turning houses requires ownership/context, not mechanical application to every question.
4. Consider essential condition, accidental ability, and reception separately. The Rust method module uses the book's dignity table, qualitative house capacity, directed positive and negative receptions, the same-sign combustion rule, and the approximate five-degree next-cusp rule.
5. Inspect relevant occasions and impediments. An applying aspect is not sufficient by itself. Upcoming major contacts are searched independently of display orbs; the event table identifies sign changes and its bounded coverage. Context determines whether an intervening contact is an impediment. No numeric strength total or automatic radicality veto decides the answer.
6. Answer the actual question. Locating an object differs from promising its recovery. Interpret symbolic timing only against a plausible timeframe. Missing data should produce uncertainty, not invented evidence.
7. Review. The reading's reported steps and cited facts are inspectable alongside the instructions and printed page references. These are reports from one model call, not independent agents or verification passes.

## What runs

`crates/horary-ai-core` owns the method rules, event bracketing, question hints, prompt, output validation, and review records. The same Rust runs natively and as WASM. Existing JavaScript supplies the astronomical positions and cusps; its accuracy limits remain material near boundaries.

The model receives supplied facts, a canonical evidence index, provisional house suggestions, and the checked-in procedure. Native-kit applies the model's own chat template and a JSON-schema constraint. Users see ordinary prose; the structured representation connects statements to their evidence. Schema validity does not establish astrological correctness.

The current application uses one constrained model call. The catalog retains detailed steps so Eileen can inspect the intended method, but does not imply parallel execution. Model weights stay resident between readings. No persisted prompt KV cache, speculative decoder, independent checker, or automatic expert sign-off is claimed.

## Bounds of the calculation

Motion is sampled hourly for seven days. Major aspect and ingress crossings are interpolated within each interval. A reversal wholly inside an interval can remain unresolved. The Moon is declared void only when its sign exit is covered and no major contact is found before that exit; otherwise the result remains unknown. Event hours are astronomical intervals, not dates for the real-world outcome.

Translation, collection, prohibition, frustration, and refranation require contextual evaluation. The event candidates do not certify any of those judgments. Antiscia, fixed stars, lots, and legacy astronomical features need separate source/precision review before treating them as decisive.

See [method audit](METHOD_AUDIT.md) for implemented rules and remaining review boundaries.
