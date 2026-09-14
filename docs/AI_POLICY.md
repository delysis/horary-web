> The active interface is now the single-document conversation prototype. See [conversation architecture](CONVERSATION.md). Earlier form-based interaction/validation descriptions below are historical and do not establish acceptance of the new conversation or audio path.

# Interpretation and review policy

The model interprets a supplied chart. It must not calculate planetary positions, houses, aspects, dignities, or event timing. The visible chart and the interpretation facts now originate from the same calculation.

The shared safe Rust core builds the prompt and validates the response structure for native and browser execution. Responses contain a summary, optional direct answer, confidence, key factors, cited chart evidence, cautions, clarification questions, and a compact report of method steps. Failed parsing is surfaced as an error; partial JSON is not shown as a completed reading.

Question-to-house assignments are provisional keyword hints, not authoritative SME decisions. Whole-word matching prevents substrings such as “ex” in “next” from assigning the wrong domain. The model is told to check the suggestions against the real question and clarify ambiguous actors or topics.

The process scaffold is inherited from an earlier traditional-horary source summary. Its page anchors and doctrinal choices require Eileen’s review; they are not treated as a completed source audit or as her endorsement. The Method workshop shows the actual compact instructions plus the draft review checklist, and allows feedback to be attached to a step.

The runtime currently uses one model call. It reports four required setup/synthesis steps and at most two other decisive steps, leaving room for a complete bounded response. A model-reported “adversarial check” is not an independent checker. The future multi-call/checker plan remains a proposal.

Structured-output validation checks required content, confidence labels, and response bounds. Medical, legal, financial, pregnancy, death, emergency, and safety questions must remain symbolic and include appropriate cautions. These checks do not certify the accuracy of the prose, exhaustively classify every high-stakes question, or replace expert judgment.

The hardware reading gate checks synthetic job, relationship, lost-object, and financial scenarios for parseability, required trace steps, supplied evidence, obvious unsupported placement/rulership claims, and cautions. Passing these examples is an engineering regression result, not subject-matter acceptance. Eileen’s known charts and corrections should become additional regression fixtures with her approval.
