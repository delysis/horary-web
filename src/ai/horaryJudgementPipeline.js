import { createRequire } from 'node:module';
import horaryJudgementPipeline from './horary-judgement-pipeline.json' with { type: 'json' };

const require = createRequire(import.meta.url);
const core = require('../generated/horary_ai_core_node/horary_ai_core.js');

export const HORARY_JUDGEMENT_PIPELINE = horaryJudgementPipeline;
export const JUDGEMENT_PIPELINE_VERSION = horaryJudgementPipeline.pipelineVersion;

export function buildJudgementPlan() {
    return JSON.parse(core.judgement_plan_json());
}

export function judgementMicroTaskIds() {
    return buildJudgementPlan().microTasks.map(task => task.id);
}
