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
                    traditionalHoraryDoctrine(),
                    'Follow the supplied judgementPlan step by step and include a compact judgementTrace showing the decisive steps.',
                    'The judgementTrace is an audit trail: each item must name a stepId, a finding, concrete chartEvidence, and confidence.',
                    'Tie every key factor to a concrete chart fact.',
                    'Keep confidence humble and evidence-based.',
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
