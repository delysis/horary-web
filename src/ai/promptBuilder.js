import {
    HORARY_INTERPRETATION_SCHEMA,
    INTERPRETATION_PROMPT_VERSION,
    INTERPRETATION_SCHEMA_VERSION,
    INTERPRETATION_TRADITION_PROFILE,
} from './interpretationSchema.js';
import { buildJudgementPlan } from './horaryJudgementPipeline.js';

const HIGH_STAKES_PATTERNS = [
    /\b(cancer|diagnosis|illness|disease|surgery|pregnan\w*|miscarriage|death|die|dying|suicide)\b/i,
    /\b(lawsuit|court|custody|criminal|legal|arrest|visa|deport\w*)\b/i,
    /\b(invest|investment|stock|crypto|mortgage|bankrupt|loan|debt|tax)\b/i,
    /\b(emergency|urgent danger|unsafe|abuse|violence)\b/i,
];

export function classifyQuestionRisk(question) {
    const text = String(question || '');
    return HIGH_STAKES_PATTERNS.some(pattern => pattern.test(text)) ? 'high_stakes' : 'ordinary';
}

export function buildHoraryInterpretationPrompt(input) {
    const risk = classifyQuestionRisk(input.question);
    const chartFacts = buildChartFacts(input);
    const judgementPlan = buildJudgementPlan();
    const chartEvidenceIndex = buildChartEvidenceIndex(chartFacts);
    const questionContext = buildQuestionContext(input.question);
    const deterministicAssignments = buildDeterministicAssignments(questionContext, chartEvidenceIndex);

    return {
        promptVersion: INTERPRETATION_PROMPT_VERSION,
        schemaVersion: INTERPRETATION_SCHEMA_VERSION,
        traditionProfile: INTERPRETATION_TRADITION_PROFILE,
        risk,
        responseFormat: {
            type: 'json_schema',
            json_schema: {
                name: 'horary_interpretation',
                strict: true,
                schema: HORARY_INTERPRETATION_SCHEMA,
            },
        },
        messages: [
            {
                role: 'system',
                content: [
                    'You interpret horary astrology charts from supplied deterministic chart facts.',
                    'Do not calculate or recalculate planetary positions, house cusps, aspects, dignity, or timing.',
                    'Use only the chart facts provided in the user message as evidence.',
                    'Use supplied questionContext house hints to scaffold house assignment; if you depart from a hint, explain the supplied chart fact or wording that forced the departure.',
                    'Use deterministicAssignments as the authoritative mapping from question actors to houses, house rulers, and ruler placements; do not infer a different significator unless the supplied question wording or chart facts explicitly require it.',
                    'For chartEvidence fields, copy or semicolon-combine short phrases from chartEvidenceIndex.canonicalFacts or deterministicAssignments.canonicalEvidence. Never cite raw JSON, a label alone, or an invented body placement.',
                    traditionalHoraryDoctrine(),
                    'Follow the supplied judgementPlan step by step and include a compact judgementTrace showing the decisive steps.',
                    'The judgementTrace is an audit trail: each item must name a stepId, a finding, concrete chartEvidence, and confidence.',
                    'Tie every key factor to a concrete chart fact.',
                    'Keep confidence humble and evidence-based.',
                    interpretationOutputGuardrails(),
                    `Prompt version: ${INTERPRETATION_PROMPT_VERSION}.`,
                    `Output schema version: ${INTERPRETATION_SCHEMA_VERSION}.`,
                    `Tradition profile: ${INTERPRETATION_TRADITION_PROFILE}.`,
                    highStakesPolicy(risk),
                    'Return only JSON matching the supplied schema.',
                ].join('\n'),
            },
            {
                role: 'user',
                content: JSON.stringify({
                    promptVersion: INTERPRETATION_PROMPT_VERSION,
                    schemaVersion: INTERPRETATION_SCHEMA_VERSION,
                    traditionProfile: INTERPRETATION_TRADITION_PROFILE,
                    question: input.question,
                    risk,
                    questionContext,
                    chartEvidenceIndex,
                    deterministicAssignments,
                    chart: chartFacts,
                    settings: input.settings || {},
                    judgementPlan,
                }, null, 2),
            },
        ],
    };
}

function traditionalHoraryDoctrine() {
    return [
        'Write in a traditional horary judgement order, not as generic natal astrology.',
        'Identify the querent and quesited from the question context and supplied house/significator facts.',
        'Prioritize classical significators, the Moon, house rulership, applying Ptolemaic aspects, reception, essential dignity, accidental strength, and perfection or its blockage.',
        'Treat void Moon, prohibition, frustration, translation, collection, combustion, under-beams, cazimi, and planetary hour/day as judgement conditions when supplied.',
        'Use outer planets only as secondary descriptive testimony unless the supplied chart facts make them unavoidable.',
        'Give a concise direct judgement first when the evidence supports one, then explain the chart testimony and uncertainty.',
    ].join(' ');
}

function highStakesPolicy(risk) {
    if (risk !== 'high_stakes') {
        return 'For ordinary questions, provide symbolic interpretation without deterministic claims.';
    }

    return [
        'This question may involve medical, legal, financial, emergency, pregnancy, death, or safety concerns.',
        'Do not present deterministic claims or professional advice.',
        'Give symbolic chart interpretation only and include a caution directing the user to qualified professionals or emergency support where appropriate.',
    ].join(' ');
}

export function buildQuestionContext(question) {
    const text = String(question || '').toLowerCase();
    if (/\b(job|career|offer|employer|boss|promotion|application)\b/.test(text)) {
        return context('job_career', [
            assignment('querent', 1, 'the asker and their capacity to act'),
            assignment('job_or_offer', 10, 'career, employer, boss, public success, and job offers'),
            assignment('wages_or_benefit', 11, 'money or benefit from the job when relevant'),
        ]);
    }
    if (/\b(ex|spouse|partner|relationship|reconcile|reconciliation|marry|marriage|dating|lover)\b/.test(text)) {
        return context('relationship', [
            assignment('querent', 1, 'the asker'),
            assignment('partner_or_ex', 7, 'partner, spouse, sweetheart, ex, or desired other person'),
        ]);
    }
    if (/\b(lost|missing|misplaced|find|recover|ring|wallet|phone|keys)\b/.test(text)) {
        return context('lost_object', [
            assignment('querent', 1, 'the asker'),
            assignment('lost_movable_possession', 2, "the querent's movable possession"),
        ]);
    }
    if (/\b(invest|investment|stock|crypto|savings|portfolio|loan|debt|mortgage|tax)\b/.test(text)) {
        return context('financial_high_stakes', [
            assignment('querent', 1, 'the asker'),
            assignment('savings_or_own_money', 2, "the querent's money and movable assets"),
            assignment('risk_or_other_party_money', 8, "other people's money, debt, fear, or loss exposure when relevant"),
        ]);
    }
    return context('ordinary', [
        assignment('querent', 1, 'the asker'),
    ]);
}

function context(domainModule, likelyAssignments) {
    return {
        domainModule,
        likelyAssignments,
        caution: 'Question-context hints are scaffolding for house assignment only; final judgement must still cite supplied chart facts.',
    };
}

function assignment(actor, house, rationale) {
    return { actor, house, rationale };
}

function interpretationOutputGuardrails() {
    return [
        'Return one top-level JSON object with summary, directAnswer when the question is ordinary, confidence, judgementTrace, keyFactors, cautions, and followUpQuestions.',
        'keyFactors must be a JSON array of objects with factor, chartEvidence, and interpretation; never return keyFactors as an object keyed by planet, topic, or testimony.',
        'judgementTrace must include question_scope, house_assignment, significator_selection, and synthesis_pass; synthesis_pass chartEvidence must cite concrete supplied chart facts, not only missing evidence.',
        'Use deterministicAssignments for house_assignment and significator_selection before weighing interpretation; if significatorPlacement is null, write placement unavailable rather than inventing one.',
        'Do not confuse house rulership with body placement: a planet rules a house when the house ruler names it, and a planet is in a house only when that body fact lists that house.',
        'Each chartEvidence string must be one concise line under 30 words, must not repeat a phrase, and must not contain unescaped raw JSON quotes.',
        'If perfection or blockage evidence is absent, cite canonical absence facts such as no applying major aspects supplied or no timing perfection supplied, plus any void Moon fact; never cite empty-array labels such as antisciaContacts: [].',
        'Keep summary and directAnswer under 90 words each; keep findings and interpretations under 35 words each.',
        'Use at most three followUpQuestions.',
    ].join(' ');
}

function buildChartFacts(input) {
    const chart = input.chart || {};
    return {
        castLocalTime: chart.castLocalTime,
        castUtcTime: chart.castUtcTime,
        timezone: chart.timezone,
        location: chart.location,
        ascendant: chart.ascendant,
        midheaven: chart.midheaven,
        houses: chart.houses || [],
        bodies: chart.bodies || [],
        aspects: chart.aspects || [],
        derived: chart.derived || {},
    };
}

function buildChartEvidenceIndex(chart) {
    const houseRulers = (chart.houses || []).map(house => ({
        house: house.number,
        sign: house.sign,
        ruler: house.ruler,
    }));
    const bodyPlacements = (chart.bodies || []).map(body => ({
        name: body.name,
        sign: body.sign,
        house: body.house,
    }));
    const applyingMajorAspects = (chart.aspects || [])
        .filter(aspect => aspect.applying === true && aspect.major !== false)
        .map(aspect => ({
            planet1: aspect.planet1,
            aspectName: aspect.aspectName,
            planet2: aspect.planet2,
            orb: aspect.orb,
        }));
    const timingPatterns = chart.derived?.timingPatterns || [];
    const voidOfCourseMoon = chart.derived?.voidOfCourseMoon || null;

    return {
        houseRulers,
        bodyPlacements,
        applyingMajorAspects,
        timingPatterns,
        voidOfCourseMoon,
        canonicalFacts: canonicalChartFacts({
            houseRulers,
            bodyPlacements,
            applyingMajorAspects,
            timingPatterns,
            voidOfCourseMoon,
        }),
    };
}

export function buildDeterministicAssignments(questionContext, chartEvidenceIndex) {
    const houseByNumber = new Map((chartEvidenceIndex.houseRulers || []).map(house => [house.house, house]));
    const bodyByName = new Map((chartEvidenceIndex.bodyPlacements || []).map(body => [body.name, body]));
    return (questionContext.likelyAssignments || []).map(assignment => {
        const house = houseByNumber.get(assignment.house) || {};
        const significator = house.ruler || null;
        const placement = significator ? bodyByName.get(significator) || null : null;
        const canonicalEvidence = [
            house.house ? `house ${house.house} ${house.sign || 'unknown-sign'} ruler ${significator || 'unknown-ruler'}` : null,
            placement ? `${placement.name} ${placement.sign || 'unknown-sign'} house ${placement.house ?? 'unknown-house'}` : null,
        ].filter(Boolean).join('; ');
        return {
            actor: assignment.actor,
            house: assignment.house,
            houseSign: house.sign || null,
            significator,
            significatorPlacement: placement,
            canonicalEvidence,
            rationale: assignment.rationale,
        };
    });
}

function canonicalChartFacts({
    houseRulers,
    bodyPlacements,
    applyingMajorAspects,
    timingPatterns,
    voidOfCourseMoon,
}) {
    return [
        ...houseRulers.map(house => `house ${house.house} ${house.sign || 'unknown-sign'} ruler ${house.ruler || 'unknown-ruler'}`),
        ...bodyPlacements.map(body => `${body.name} ${body.sign || 'unknown-sign'} house ${body.house ?? 'unknown-house'}`),
        ...applyingMajorAspects.map(aspect => `${aspect.planet1} ${aspect.aspectName} ${aspect.planet2} applying orb ${aspect.orb ?? 'unknown'}`),
        applyingMajorAspects.length === 0 ? 'no applying major aspects supplied' : null,
        ...timingPatterns.map(pattern => canonicalTimingPattern(pattern)),
        timingPatterns.length === 0 ? 'no timing perfection supplied' : null,
        voidOfCourseMoon ? `voidOfCourseMoon isVoid ${Boolean(voidOfCourseMoon.isVoid)}` : null,
    ].filter(Boolean);
}

function canonicalTimingPattern(pattern) {
    const type = pattern?.type || 'timing';
    const planet1 = pattern?.planet1 || 'unknown';
    const aspect = pattern?.aspectName || 'unknown-aspect';
    const planet2 = pattern?.planet2 || 'unknown';
    const hours = pattern?.estimatedPerfectsWithinHours ?? pattern?.perfectsWithinHours ?? 'unknown';
    return `timing ${type} ${planet1} ${aspect} ${planet2} ${hours}h`;
}
