import horaryInterpretationSchema from './horary-interpretation.schema.json' with { type: 'json' };

export const INTERPRETATION_SCHEMA_VERSION = '2026-07-03';
export const INTERPRETATION_PROMPT_VERSION = 'horary-interpretation-v4';
export const INTERPRETATION_TRADITION_PROFILE = 'traditional-horary-textbook-v1';

export const HORARY_INTERPRETATION_SCHEMA = horaryInterpretationSchema;

export function validateInterpretationShape(value) {
    const errors = [];

    if (typeof value !== 'object' || value == null || Array.isArray(value)) {
        return { ok: false, errors: ['Output must be an object.'] };
    }

    if (typeof value.summary !== 'string' || value.summary.trim() === '') {
        errors.push('summary must be a non-empty string.');
    }

    if (value.directAnswer != null && typeof value.directAnswer !== 'string') {
        errors.push('directAnswer must be a string when present.');
    }

    if (!['low', 'medium', 'high'].includes(value.confidence)) {
        errors.push('confidence must be low, medium, or high.');
    }

    if (!Array.isArray(value.judgementTrace) || value.judgementTrace.length === 0) {
        errors.push('judgementTrace must be a non-empty array.');
    } else {
        value.judgementTrace.forEach((step, index) => {
            if (typeof step !== 'object' || step == null || Array.isArray(step)) {
                errors.push(`judgementTrace[${index}] must be an object.`);
                return;
            }
            for (const field of ['stepId', 'finding', 'chartEvidence']) {
                if (typeof step[field] !== 'string' || step[field].trim() === '') {
                    errors.push(`judgementTrace[${index}].${field} must be a non-empty string.`);
                }
            }
            if (!['low', 'medium', 'high'].includes(step.confidence)) {
                errors.push(`judgementTrace[${index}].confidence must be low, medium, or high.`);
            }
        });
    }

    if (!Array.isArray(value.keyFactors) || value.keyFactors.length === 0) {
        errors.push('keyFactors must be a non-empty array.');
    } else {
        value.keyFactors.forEach((factor, index) => {
            if (typeof factor !== 'object' || factor == null || Array.isArray(factor)) {
                errors.push(`keyFactors[${index}] must be an object.`);
                return;
            }
            for (const field of ['factor', 'chartEvidence', 'interpretation']) {
                if (typeof factor[field] !== 'string' || factor[field].trim() === '') {
                    errors.push(`keyFactors[${index}].${field} must be a non-empty string.`);
                }
            }
        });
    }

    for (const field of ['cautions', 'followUpQuestions']) {
        if (!Array.isArray(value[field]) || value[field].some(item => typeof item !== 'string')) {
            errors.push(`${field} must be an array of strings.`);
        }
    }

    return { ok: errors.length === 0, errors };
}
