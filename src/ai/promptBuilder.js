import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const core = require('../generated/horary_ai_core_node/horary_ai_core.js');

export function classifyQuestionRisk(question) {
    return core.classify_question_risk(String(question || ''));
}

export function buildHoraryInterpretationPrompt(input) {
    return readJson(core.build_interpretation_prompt_json(JSON.stringify(input || {})));
}

export function buildQuestionContext(question) {
    return readJson(core.question_context_json(String(question || '')));
}

export function buildDeterministicAssignments(questionContext, chartEvidenceIndex) {
    return readJson(core.deterministic_assignments_json(
        JSON.stringify(questionContext || {}),
        JSON.stringify(chartEvidenceIndex || {}),
    ));
}

function readJson(json) {
    return JSON.parse(json);
}
