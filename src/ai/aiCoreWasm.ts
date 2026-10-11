import initAiCore, {
  build_interpretation_prompt_json,
  classify_question_risk,
  resolve_chart_time,
  validate_dms,
  parse_interpretation_content_json,
  validate_interpretation_json,
  apply_book_method_json,
} from '../generated/horary_ai_core_web/horary_ai_core.js'
import type { HoraryInterpretation, InterpretationRequest } from '../tauriBridge.ts'

export type InterpretationPrompt = {
  promptVersion: string
  schemaVersion: string
  traditionProfile: string
  risk: 'ordinary' | 'high_stakes' | string
  responseFormat: unknown
  messages: Array<{
    role: string
    content: string
  }>
}

let initPromise: Promise<void> | null = null

export async function ensureAiCoreLoaded() {
  if (!initPromise) {
    initPromise = initAiCore().then(() => undefined).catch(error => { initPromise = null; throw error })
  }
  await initPromise
}

export async function buildInterpretationPrompt(request: InterpretationRequest) {
  await ensureAiCoreLoaded()
  return JSON.parse(build_interpretation_prompt_json(JSON.stringify(request))) as InterpretationPrompt
}

export async function parseInterpretationContent(content: string) {
  await ensureAiCoreLoaded()
  return JSON.parse(parse_interpretation_content_json(content)) as HoraryInterpretation
}

export async function validateInterpretationContent(content: string, risk: string) {
  await ensureAiCoreLoaded()
  return JSON.parse(validate_interpretation_json(content, risk)) as HoraryInterpretation
}

export async function classifyQuestionRisk(question: string) {
  await ensureAiCoreLoaded()
  return classify_question_risk(question)
}

// Synchronous after app bootstrap; both desktop and browser use the Rust rules.
export function resolveChartTime(local: string, timezone: string, occurrence = '') {
  return new Date(resolve_chart_time(local, timezone, occurrence))
}

export function applyBookMethod(chart: unknown) {
  return JSON.parse(apply_book_method_json(JSON.stringify(chart)))
}

export function validateDms(degrees: string, minutes: string, seconds: string, sign: string) {
  return validate_dms(degrees.trim() ? Number(degrees) : NaN, minutes.trim() ? Number(minutes) : 0, seconds.trim() ? Number(seconds) : 0, sign)
}

export { read_review_notes_json, append_review_note_json, nudge_chart_time } from '../generated/horary_ai_core_web/horary_ai_core.js'
