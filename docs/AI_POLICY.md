# AI Interpretation Policy

Whorary's local AI interpretation boundary treats the AI as a symbolic interpreter only. The AI must never be the chart calculator.

## Required Boundary

The deterministic chart engine computes:

- Cast time and UTC time
- Location
- Ascendant and midheaven
- House cusps
- Body positions
- Aspects and applying/separating phase
- Essential and accidental dignities plus derived horary factors such as sect, Ascendant ruler, planetary day/hour, solar condition, receptions, void-of-course Moon, antiscia/contra-antiscia, translation/collection/prohibition/frustration candidates, and Part of Fortune

The AI receives those facts and returns a symbolic interpretation. It must not calculate or recalculate positions, houses, aspects, dignities, or timing.

## Output Shape

AI output must be schema-constrained JSON with:

- `summary`
- optional `directAnswer`
- `confidence`
- `judgementTrace`
- `keyFactors`
- `cautions`
- `followUpQuestions`

`judgementTrace` is a compact audit trail. Each step must name the judgement pass, the finding, concrete chart evidence, and confidence. It is intended to make small-model outputs checkable rather than to expose hidden chain-of-thought.

Every key factor must include chart evidence.

The Tauri backend validates this shape before returning a completed interpretation to the frontend. Streamed output may appear token-by-token while generation is running, but the completed response must still pass schema validation. High-stakes interpretations must include at least one caution.

## Prompt And Schema Versions

Interpretation prompts carry a prompt version, schema version, and tradition profile in both prompt metadata and the user payload sent to the local model. The prompt version identifies the instructions and evidence boundary; the schema version identifies the expected JSON shape; the tradition profile identifies the traditional horary judgement order used for prose.

The current tradition profile asks the model to write as a concise traditional horary judgement rather than generic astrology: identify querent and quesited, prioritize classical significators and the Moon, weigh applying Ptolemaic aspects, reception, dignity, accidental strength, perfection and blockage, and use outer planets only as secondary testimony unless the supplied facts require otherwise. Frontend and Tauri prompt builders are regression-tested to keep prompt/schema/profile metadata synchronized.

`src/ai/horary-judgement-pipeline.json` contains the versioned small-model judgement scaffold. It summarizes the local OCR of John Frawley's *The Horary Textbook* as procedural rules with page anchors, microtask prompts, parallel groups, cache policy, and a verifier checklist. The source PDF text is not bundled into the app. The compact runtime `judgementPlan` is included in the prompt payload; native inference can run the same microtasks as separate parallel calls with stable hot/cold KV-cached prompt prefixes.

## High-Stakes Topics

For medical, legal, financial, pregnancy, death, emergency, or safety questions, the app should provide symbolic interpretation only. It must not make deterministic claims or professional recommendations. It should include a caution directing the user to qualified professionals or emergency support where appropriate.
