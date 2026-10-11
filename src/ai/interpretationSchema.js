import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const core = require('../generated/horary_ai_core_node/horary_ai_core.js');
const metadataPrompt = JSON.parse(core.build_interpretation_prompt_json(JSON.stringify({
    question: 'Will the matter proceed?',
    chart: {},
    settings: {},
})));

export const INTERPRETATION_SCHEMA_VERSION = metadataPrompt.schemaVersion;
export const INTERPRETATION_PROMPT_VERSION = metadataPrompt.promptVersion;
export const INTERPRETATION_TRADITION_PROFILE = metadataPrompt.traditionProfile;
export const HORARY_INTERPRETATION_SCHEMA = metadataPrompt.responseFormat.json_schema.schema;

export function validateInterpretationShape(value) {
    try {
        core.parse_interpretation_content_json(JSON.stringify(value));
        return { ok: true, errors: [] };
    } catch (error) {
        return { ok: false, errors: [errorMessage(error)] };
    }
}

function errorMessage(error) {
    return error instanceof Error ? error.message : String(error);
}
