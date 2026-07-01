import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { buildHoraryInterpretationPrompt, classifyQuestionRisk } from '../promptBuilder.js';
import {
    buildJudgementPlan,
    HORARY_JUDGEMENT_PIPELINE,
    judgementMicroTaskIds,
} from '../horaryJudgementPipeline.js';
import {
    HORARY_INTERPRETATION_SCHEMA,
    INTERPRETATION_PROMPT_VERSION,
    INTERPRETATION_SCHEMA_VERSION,
    INTERPRETATION_TRADITION_PROFILE,
    validateInterpretationShape,
} from '../interpretationSchema.js';

const sampleInput = {
    question: 'Will I get the contract?',
    chart: {
        castLocalTime: '2026-06-30 12:00',
        castUtcTime: '2026-06-30T16:00:00.000Z',
        timezone: 'UTC-04:00',
        location: { label: 'New York, US', latitude: 40.7128, longitude: -74.006 },
        ascendant: { sign: 'Libra', degree: 12.3 },
        midheaven: { sign: 'Cancer', degree: 15.1 },
        houses: [{ number: 1, sign: 'Libra', degree: 12.3 }],
        bodies: [{ name: 'Moon', sign: 'Capricorn', degree: 14.2, house: 4 }],
        aspects: [{ planet1: 'Moon', aspectName: 'Trine', planet2: 'Venus', orb: 2.1, applying: true }],
        derived: { ascendantRuler: 'Venus' },
    },
    settings: { tradition: 'traditional', tone: 'technical' },
};

test('buildHoraryInterpretationPrompt uses deterministic chart facts and forbids recalculation', () => {
    const prompt = buildHoraryInterpretationPrompt(sampleInput);
    const system = prompt.messages[0].content;
    const user = JSON.parse(prompt.messages[1].content);

    assert.equal(prompt.promptVersion, INTERPRETATION_PROMPT_VERSION);
    assert.equal(prompt.schemaVersion, INTERPRETATION_SCHEMA_VERSION);
    assert.equal(prompt.traditionProfile, INTERPRETATION_TRADITION_PROFILE);
    assert.equal(prompt.responseFormat.type, 'json_schema');
    assert.deepEqual(prompt.responseFormat.json_schema.schema, HORARY_INTERPRETATION_SCHEMA);
    assert.equal(user.promptVersion, INTERPRETATION_PROMPT_VERSION);
    assert.equal(user.schemaVersion, INTERPRETATION_SCHEMA_VERSION);
    assert.equal(user.traditionProfile, INTERPRETATION_TRADITION_PROFILE);
    assert.equal(user.judgementPlan.profile, INTERPRETATION_TRADITION_PROFILE);
    assert.equal(user.judgementPlan.executionMode, 'single_call_with_required_trace');
    assert.ok(user.judgementPlan.microTasks.some(task => task.id === 'house_assignment'));
    assert.ok(user.judgementPlan.domainModules.some(module => module.id === 'relationship'));
    assert.ok(user.judgementPlan.futureMultiCallPipeline.some(step => step.id === 'reception_checker'));
    assert.ok(user.judgementPlan.parallelGroups.some(group => group.id === 'condition'));
    assert.match(system, /Do not calculate or recalculate planetary positions/);
    assert.match(system, /traditional horary judgement order/);
    assert.match(system, /querent and quesited/);
    assert.match(system, /judgementTrace/);
    assert.match(system, new RegExp(`Prompt version: ${INTERPRETATION_PROMPT_VERSION.replaceAll('-', '\\-')}`));
    assert.match(system, new RegExp(`Tradition profile: ${INTERPRETATION_TRADITION_PROFILE.replaceAll('-', '\\-')}`));
    assert.equal(user.chart.bodies[0].name, 'Moon');
    assert.equal(user.chart.aspects[0].applying, true);
});

test('frontend and Tauri prompt metadata stays synchronized', () => {
    const rustAi = readFileSync(new URL('../../../src-tauri/src/ai.rs', import.meta.url), 'utf8');
    const promptVersionMatch = /const INTERPRETATION_PROMPT_VERSION: &str = "([^"]+)";/.exec(rustAi);
    const schemaVersionMatch = /const INTERPRETATION_SCHEMA_VERSION: &str = "([^"]+)";/.exec(rustAi);
    const traditionProfileMatch = /const INTERPRETATION_TRADITION_PROFILE: &str = "([^"]+)";/.exec(rustAi);

    assert.ok(promptVersionMatch, 'Rust prompt version constant should exist');
    assert.ok(schemaVersionMatch, 'Rust schema version constant should exist');
    assert.ok(traditionProfileMatch, 'Rust tradition profile constant should exist');
    assert.equal(promptVersionMatch[1], INTERPRETATION_PROMPT_VERSION);
    assert.equal(schemaVersionMatch[1], INTERPRETATION_SCHEMA_VERSION);
    assert.equal(traditionProfileMatch[1], INTERPRETATION_TRADITION_PROFILE);
});

test('horary judgement pipeline exposes small-model microtasks and cache policy', () => {
    const plan = buildJudgementPlan();
    const taskIds = judgementMicroTaskIds();

    assert.equal(HORARY_JUDGEMENT_PIPELINE.profile, INTERPRETATION_TRADITION_PROFILE);
    assert.equal(plan.pipelineVersion, HORARY_JUDGEMENT_PIPELINE.pipelineVersion);
    for (const id of [
        'question_scope',
        'house_assignment',
        'significator_selection',
        'essential_dignity_pass',
        'accidental_strength_pass',
        'reception_pass',
        'perfection_pass',
        'blockage_pass',
        'timing_pass',
        'adversarial_checker_pass',
    ]) {
        assert.ok(taskIds.includes(id), `missing microtask ${id}`);
    }
    assert.equal(plan.cachePolicy.separateStablePromptFromVariablePayload, true);
    assert.equal(plan.checkerOutputContract.shape.strength, 'decisive|strong|moderate|minor');
    assert.ok(plan.domainModules.some(module => module.id === 'job_career'));
    assert.ok(plan.traceRequirement.minimumSteps.includes('synthesis_pass'));
});

test('classifyQuestionRisk flags high-stakes questions', () => {
    assert.equal(classifyQuestionRisk('Should I invest my savings in this stock?'), 'high_stakes');
    assert.equal(classifyQuestionRisk('Will I get the contract?'), 'ordinary');
});

test('high-stakes prompt includes professional-advice caution policy', () => {
    const prompt = buildHoraryInterpretationPrompt({
        ...sampleInput,
        question: 'Do I have cancer?',
    });

    assert.equal(prompt.risk, 'high_stakes');
    assert.match(prompt.messages[0].content, /qualified professionals/);
    assert.match(prompt.messages[0].content, /Do not present deterministic claims/);
});

test('validateInterpretationShape accepts valid output and rejects missing evidence', () => {
    const valid = validateInterpretationShape({
        summary: 'The chart emphasizes the contract significator.',
        confidence: 'medium',
        judgementTrace: [
            {
                stepId: 'perfection_pass',
                finding: 'Moon applies to Venus.',
                chartEvidence: 'Moon trine Venus, 2.1 degree applying orb',
                confidence: 'medium',
            },
        ],
        keyFactors: [
            {
                factor: 'Moon applying to Venus',
                chartEvidence: 'Moon trine Venus, 2.1 degree applying orb',
                interpretation: 'A constructive contact is forming.',
            },
        ],
        cautions: [],
        followUpQuestions: [],
    });
    const invalid = validateInterpretationShape({
        summary: 'Looks good.',
        confidence: 'certain',
        keyFactors: [{ factor: 'Moon' }],
        cautions: [],
        followUpQuestions: [],
    });

    assert.equal(valid.ok, true);
    assert.equal(invalid.ok, false);
    assert.ok(invalid.errors.some(error => error.includes('confidence')));
    assert.ok(invalid.errors.some(error => error.includes('judgementTrace')));
    assert.ok(invalid.errors.some(error => error.includes('chartEvidence')));
});
