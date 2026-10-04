# October 4 review pass

The immediate failure was a place-search loop. The offline US parser left “United States” attached to “Woodbridge,” returned nothing, and the reader repeated the search instead of reaching a chart. Country suffixes now resolve correctly, with a regression for Woodbridge, Virginia, United States. Search attempts are bounded.

## The ordinary path

The first question requests the device's location through the native location service, alongside its clock time zone and language. The operating system may ask for location permission. Accurate coordinates are retained: the nearest bundled city supplies a label and a cross-check on the time zone, not substitute coordinates. A stated place takes precedence. Unavailable, very coarse or conflicting location information leads to a short clarification. A time zone alone is never treated as a geographic position.

Space starts speech from anywhere outside editable words, without clicking the invitation first. While writing, ordinary spaces remain ordinary spaces; Option–Space starts speech there. Release finishes; Escape or switching away cancels capture. The system microphone message now describes the actual interaction. The keyboard regression exercises these behaviors; real microphone permission and audio capture still need direct native review.

The question's candidate moment is fixed before model acquisition or inference. A clarification can establish a later moment; a correction to an existing question keeps its chart moment unless the person changes it. Frawley, printed pp. 7–8, remains the governing distinction. The app's recorded moment is reviewable, not a claim that software perfectly identifies understanding.

## The unfolding method

A clear question can proceed through three short passages in one turn: who the chart represents, relevant testimony, and what remains open. It should ask another question only when context is needed. Each successful stage is unavailable for repetition within that turn, with a separate limit on writing attempts. This removes permission questions that merely ask whether to continue an already requested reading.

Rust calculates each house's traditional ruler. The reader chooses a contextual role and its house, then explains why. Natural Moon, Sun and Venus roles are explicit and cannot displace a planet already claimed as a house ruler. The relationship chapter distinguishes a particular partner from a prospective one and preserves the Moon's relevant testimony. Printed pp. 191–198.

Each passage's “How this follows” margin separates:

- calculated chart facts;
- the reason for choosing the roles;
- an editorial paraphrase of a relevant Frawley rule, with printed pages;
- the reader's inference connecting them.

A valid reference is not proof of the inference. The runtime rejects invented evidence or rules, stage order errors, reception explanations citing only own dignity, and judgments that omit the calculation boundary. Revising role assignments clears dependent testimony and judgment; revising testimony clears its judgment. Previous tool calls remain in the audit.

Directed receptions now say explicitly who regards whom, and by which major or minor dignity. Own debility is separate. Contact facts retain the actors, their current angular distance from the aspect, the estimated astronomical hours, and whether sign changes intervene. Astronomical hours are not a promised calendar date. The Moon's strict void status is distinguished from unknown coverage. These checks address real errors observed in the local model runs.

## Appearance and useful exploration

The document uses warm paper or dark ink according to system appearance. New passages unfold; a moving gold thread accompanies work and speech has a quiet breathing treatment. Reduced-motion preferences are respected. There is no logo, settings control, theme switch or chart form.

Touch or focus a planet to reveal its calculated testimony immediately. The chart's Moon phase comes from the calculated Sun–Moon separation. Source margins, earlier leaves and the reading's progress notes are disclosures within the document. Earlier words remain editable as explicit conversational corrections.

## Speed and logging

Model prompts now carry evidence appropriate to the current stage and actors, instead of the whole chart, hourly ephemeris and repeated tool prose. Fetching evidence already present in state is no longer another model action. Reviewer builds optimize native sampling while preserving the model, constraints and artifact verification. The pinned controlled runtime clears its prompt cache between requests; no unverified claim of cross-turn prefix reuse is made.

A measured roles passage fell from 61.0 seconds to 24.5 seconds after prompt compaction on this M4 Max. The final synthetic device-location reading reached all three method passages in 81.09 seconds. These are local observations, including prompt work, not a portable benchmark or a packaged voice journey. Residual model errors remain part of review.

The native app writes a bounded `reading-progress.jsonl` journal in its app-data directory, with build identity, stages, durations and inference token metrics. The page's “Notes from this reading” shows plain-language progress. Raw questions, coordinates and audio are absent from this separate journal. The existing `conversation.json` retains the conversation, places, chart revisions, tool calls, errors and inference receipts. Nothing is automatically sent to a developer. Eileen can keep independent notes or speak observations into the conversation.

## What this pass does not establish

The ephemeris is inherited and approximate. Hourly contact samples span seven days; stations between samples, fine event order, stars and antiscia are not certified. Missing contacts cannot settle a one-year question. The model can still misinterpret correct facts, especially reception and the Moon's role. Local model output is preserved for inspection; valid tool syntax does not qualify its astrology. Eileen's assessment remains essential. See the current [verification record](VERIFICATION.md).

The final design makes an important limit visible: current local model runs can still contradict receptions or invent placements. Native method prose and calculated facts are the main reading. The reader’s original proposed interpretation and rationale remain in the margin for Eileen to examine; the resulting answer stays open. This preserves failures and makes progress inspectable without publishing a confident forecast from an unqualified reader. The planner selects roles, evidence and rules before prose; JSON field order is preserved and tested.
