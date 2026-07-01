import horaryJudgementPipeline from './horary-judgement-pipeline.json' with { type: 'json' };

export const HORARY_JUDGEMENT_PIPELINE = horaryJudgementPipeline;
export const JUDGEMENT_PIPELINE_VERSION = horaryJudgementPipeline.pipelineVersion;

export function buildJudgementPlan() {
    return {
        profile: horaryJudgementPipeline.profile,
        pipelineVersion: horaryJudgementPipeline.pipelineVersion,
        executionMode: 'single_call_with_required_trace',
        sourceBasis: horaryJudgementPipeline.sourceBasis.summary,
        cachePolicy: horaryJudgementPipeline.cachePolicy,
        checkerOutputContract: horaryJudgementPipeline.checkerOutputContract,
        globalRules: horaryJudgementPipeline.globalRules,
        houseAssignmentRules: horaryJudgementPipeline.houseAssignmentRules,
        commonHouseMap: horaryJudgementPipeline.commonHouseMap,
        microTasks: horaryJudgementPipeline.microTasks.map(task => ({
            id: task.id,
            title: task.title,
            phase: task.phase,
            parallelGroup: task.parallelGroup,
            objective: task.objective,
            instruction: task.compactPrompt,
            outputKeys: task.outputKeys,
        })),
        domainModules: horaryJudgementPipeline.domainModules,
        futureMultiCallPipeline: horaryJudgementPipeline.futureMultiCallPipeline,
        parallelGroups: horaryJudgementPipeline.parallelGroups,
        traceRequirement: horaryJudgementPipeline.singleCallTraceRequirement,
        verifierChecklist: horaryJudgementPipeline.verifierChecklist,
    };
}

export function judgementMicroTaskIds() {
    return horaryJudgementPipeline.microTasks.map(task => task.id);
}
