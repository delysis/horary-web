//! Executable synthetic elicitation cases and opt-in real-model campaigns.
//! Expected answers stay in the evaluator, never in a model's input.
#![forbid(unsafe_code)]

use crate::{
    conversation::{Message, Session},
    geocode::{GeocodeState, LocationCandidate},
    horary_lessons::{self, Matter, Stage},
    horary_pipeline::Runtime,
    native_llama_worker::{
        generate_native, generate_native_batch, NativeGenerateOptions, NativeGenerationResult,
        NativeLlamaState,
    },
    reading_contracts::{self, Anchor, Facet, Field, Method, RequirementKey},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

const EVALUATOR_VERSION: &str = "horary-four-hurdle-evaluator-2026-10-10.3";
#[path = "reading_neural_eval.rs"]
mod reading;
const CORE: &str = include_str!("../test-fixtures/elicitation/core.json");
const SPECIALIST: &str = include_str!("../test-fixtures/elicitation/specialist.json");

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Explicit,
    Implicit,
    Missing,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    method: Method,
    mode: Mode,
    words: String,
    #[serde(default = "device_available")]
    device_available: bool,
    #[serde(default)]
    follow_up: Option<String>,
    #[serde(default)]
    follow_up_expected: Option<Expected>,
    #[serde(default)]
    follow_up_accepts_proposal: bool,
    expected: Expected,
    source_pages: String,
    rationale: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    #[serde(default)]
    facet: Option<Facet>,
    #[serde(default)]
    allowed_facets: Vec<Facet>,
    #[serde(default)]
    allowed_methods: Option<Vec<Method>>,
    #[serde(default)]
    facts: Vec<ExpectedFact>,
    #[serde(default)]
    needs: Vec<RequirementKey>,
    #[serde(default)]
    needs_alternatives: Vec<Vec<RequirementKey>>,
    ready: bool,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    subject_kind: Option<String>,
    #[serde(default)]
    subject_name_contains: Option<String>,
    #[serde(default)]
    subject_name_any_contains: Vec<String>,
    #[serde(default)]
    canonical_mentions: Vec<String>,
    #[serde(default)]
    relationships: Option<BTreeMap<String, String>>,
    #[serde(default)]
    chart_place_contains: Option<String>,
    #[serde(default)]
    chart_local_time: Option<String>,
    #[serde(default)]
    chart_moment_ms: Option<f64>,
    #[serde(default)]
    forbidden_facts: Vec<Field>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExpectedFact {
    field: Field,
    contains: String,
    #[serde(default)]
    place_qualifier_any: Vec<String>,
}

fn device_available() -> bool {
    true
}

/// Optional banks share the strict Case parser. Keep provenance metadata outside
/// this directory: every JSON file here is an executable case-array bank.
fn optional_fixture_banks(directory: &Path) -> Result<Vec<PathBuf>, String> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut banks = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json")
            || matches!(
                entry.file_name().to_str(),
                Some("core.json" | "specialist.json")
            )
        {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err(format!(
                "Fixture bank {} must be an ordinary file",
                path.display()
            ));
        }
        banks.push(path);
    }
    banks.sort();
    Ok(banks)
}

fn catalogue_in(directory: &Path) -> Result<Vec<Case>, String> {
    let mut cases: Vec<Case> = serde_json::from_str(CORE).map_err(|e| e.to_string())?;
    cases.extend(serde_json::from_str::<Vec<Case>>(SPECIALIST).map_err(|e| e.to_string())?);
    for bank in optional_fixture_banks(directory)? {
        cases.extend(
            serde_json::from_slice::<Vec<Case>>(
                &fs::read(&bank).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("Fixture bank {}: {error}", bank.display()))?,
        );
    }
    Ok(cases)
}

fn catalogue() -> Result<Vec<Case>, String> {
    catalogue_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/elicitation"))
}

fn case_selected(case: &Case, filters: &[&str]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| case.id == *filter || case.method.name() == *filter)
}

fn validate_rubric_coverage(
    cases: &[Case],
    rubrics: &BTreeMap<String, crate::reading_eval::Rubric>,
) -> Result<(), String> {
    if cases.len() != rubrics.len() {
        return Err(format!(
            "Reading rubric coverage differs: {} cases, {} rubrics",
            cases.len(),
            rubrics.len()
        ));
    }
    for case in cases {
        let rubric = rubrics
            .get(&case.id)
            .ok_or_else(|| format!("Case {} has no reading rubric", case.id))?;
        let mode = match case.mode {
            Mode::Explicit => "explicit",
            Mode::Implicit => "implicit",
            Mode::Missing => "missing",
        };
        if rubric.declared_method != case.method || rubric.mode != mode {
            return Err(format!(
                "Reading rubric method/mode disagrees with immutable fixture {}",
                case.id
            ));
        }
    }
    Ok(())
}

pub(crate) fn catalogue_size(root: &Path) -> Result<usize, String> {
    catalogue_in(&root.join("src-tauri/test-fixtures/elicitation")).map(|cases| cases.len())
}
fn frozen_moment() -> f64 {
    horary_ai_core::chart_input::resolve_chart_time("2026-10-07T12:00", "America/New_York", "")
        .expect("The synthetic Wednesday noon is a unique civil instant")
}

fn device() -> LocationCandidate {
    LocationCandidate {
        id: "device-location".into(),
        label: "Near Woodbridge, VA, United States".into(),
        name: "Woodbridge".into(),
        country: "US".into(),
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
        provider: "device".into(),
    }
}

fn current_anchor(session: &Session) -> Option<Anchor> {
    let place = session.place.as_ref().or_else(|| {
        session
            .candidates
            .iter()
            .find(|candidate| candidate.provider == "device")
    })?;
    Some(Anchor {
        timestamp_ms: session
            .chart
            .as_ref()
            .and_then(|chart| chart["timestampMs"].as_f64())
            .or(session.candidate_moment_ms)
            .unwrap_or_else(frozen_moment),
        latitude: place.latitude,
        longitude: place.longitude,
        timezone: place.timezone.clone(),
    })
}

fn input_handoff_binding(session: &Session) -> Option<Value> {
    let case = session.method.consultation.as_ref()?;
    session.chart.as_ref()?;
    session.place.as_ref()?;
    let anchor = current_anchor(session)?;
    if session.chart.as_ref()?["timestampMs"].as_f64() != Some(anchor.timestamp_ms) {
        return None;
    }
    let ready = reading_contracts::ReadyReading::prepare(case, anchor).ok()?;
    let binding = serde_json::to_value(ready.binding()).expect("Reading bindings serialize");
    let (index, boundary) = session
        .audit
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| e["event"] == "input_evaluation_boundary")?;
    if boundary["boundary"] != "ready_reading"
        || boundary["binding"] != binding
        || boundary["after_message"].as_u64() != Some(session.messages.len() as u64)
        || boundary["reading_executed"] != false
        || boundary["conversation_executed"] != false
    {
        return None;
    }
    session.audit[..index]
        .iter()
        .any(|e| {
            e["event"] == "contract_handoff"
                && e["binding"] == binding
                && e["request"]["binding"] == binding
        })
        .then_some(binding)
}

fn input_handoff_boundary(session: &Session) -> bool {
    input_handoff_binding(session).is_some()
}

#[derive(Debug, Serialize)]
struct Grade {
    semantic_pass: bool,
    mismatches: Vec<String>,
    actual: Value,
    fluidity_review_flags: Vec<String>,
    human_fluidity_review: &'static str,
    hurdles: crate::reading_eval::Hurdles,
}

fn same_actor(
    case: Option<&reading_contracts::Consultation>,
    actual: &str,
    expected: &str,
) -> bool {
    if actual == expected {
        return true;
    }
    if matches!(expected, "" | "querent") || matches!(actual, "" | "querent") {
        return false;
    }
    let Some(case) = case else {
        return false;
    };
    let actual_person = case.people.get(actual);
    let expected_person = case.people.get(expected).or_else(|| {
        let mut matches = case
            .people
            .values()
            .filter(|person| person.label.eq_ignore_ascii_case(expected));
        let first = matches.next();
        if matches.next().is_none() {
            first
        } else {
            None
        }
    });
    actual_person
        .zip(expected_person)
        .is_some_and(|(actual, expected)| actual.id == expected.id)
}

fn same_need(
    case: Option<&reading_contracts::Consultation>,
    actual: &RequirementKey,
    expected: &RequirementKey,
) -> bool {
    match (actual, expected) {
        (
            RequirementKey::PersonRelationship(actual),
            RequirementKey::PersonRelationship(expected),
        ) => same_actor(case, actual, expected),
        _ => actual == expected,
    }
}

fn grade(case: &Case, session: &Session, error: Option<&str>) -> Grade {
    grade_at_moment(case, session, error, frozen_moment())
}

fn scripted_need_is_eligible(case: &Case, session: &Session) -> bool {
    let consultation = session.method.consultation.as_ref();
    consultation
        .and_then(|case| case.requested.as_ref())
        .is_some_and(|requested| {
            std::iter::once(&case.expected.needs)
                .chain(case.expected.needs_alternatives.iter())
                .flatten()
                .any(|expected| same_need(consultation, requested, expected))
        })
}

fn grade_at_moment(
    case: &Case,
    session: &Session,
    error: Option<&str>,
    default_moment: f64,
) -> Grade {
    use crate::reading_eval::Gate;
    let mut mismatches = Vec::new();
    if let Some(error) = error {
        mismatches.push(format!("Execution did not complete: {error}"));
    }
    let evidence_start = mismatches.len();
    let consultation = session.method.consultation.as_ref();
    let anchor = current_anchor(session);
    let plan = consultation.map(|c| c.plan(anchor.as_ref()));
    let clipboard = crate::horary_conversation::clipboard(session);
    let needs: Vec<RequirementKey> = clipboard["reminders"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|need| serde_json::from_value(need["key"].clone()).ok())
        .collect();
    let expected_needs = std::iter::once(&case.expected.needs)
        .chain(case.expected.needs_alternatives.iter())
        .find(|expected| {
            expected.iter().all(|need| {
                needs
                    .iter()
                    .any(|actual| same_need(consultation, actual, need))
            })
        })
        .unwrap_or(&case.expected.needs);
    for mention in &case.expected.canonical_mentions {
        let retained = consultation.is_some_and(|c| {
            c.subject
                .resolved()
                .is_some_and(|s| s.name.to_lowercase().contains(&mention.to_lowercase()))
                || c.people
                    .values()
                    .any(|p| p.label.to_lowercase().contains(&mention.to_lowercase()))
                || c.facts.values().any(|s| {
                    s.resolved()
                        .is_some_and(|v| v.to_lowercase().contains(&mention.to_lowercase()))
                })
        });
        if !retained {
            mismatches.push(format!("Canonical facts must retain {mention:?}; the question/transcript alone is not a factual binding"));
        }
    }
    let classification_start = mismatches.len();
    let frame = consultation.and_then(|c| c.frame.resolved());
    let allowed = case
        .expected
        .allowed_methods
        .as_deref()
        .unwrap_or(std::slice::from_ref(&case.method));
    let unresolved_frame_is_expected = frame.is_none()
        && allowed.contains(&Method::Unclassified)
        && case.expected.needs.contains(&RequirementKey::Frame);
    if !unresolved_frame_is_expected && !frame.is_some_and(|frame| allowed.contains(&frame.method))
    {
        mismatches.push(format!(
            "Method must be one of {:?}; actual {:?}",
            allowed,
            frame.map(|frame| frame.method)
        ));
    }
    let mut allowed_facets = case.expected.allowed_facets.clone();
    allowed_facets.extend(case.expected.facet);
    if !allowed_facets.is_empty()
        && frame.is_none_or(|frame| !allowed_facets.contains(&frame.facet))
    {
        mismatches.push(format!(
            "Facet must be one of {allowed_facets:?}; actual {:?}",
            frame.map(|frame| frame.facet)
        ));
    }
    let facts_start = mismatches.len();
    for fact in &case.expected.facts {
        let actual = consultation.and_then(|c| c.text(fact.field));
        let actor_field = matches!(
            fact.field,
            Field::PrincipalId | Field::Seller | Field::DealParty
        ) || fact.field == Field::Sender
            && consultation.and_then(|c| c.method()) == Some(Method::Money);
        if actual.is_none_or(|value| {
            if actor_field {
                !same_actor(consultation, value, &fact.contains)
            } else {
                !value.to_lowercase().contains(&fact.contains.to_lowercase())
            }
        }) {
            mismatches.push(format!(
                "Resolved {} must contain {:?}; actual {:?}",
                fact.field.name(),
                fact.contains,
                actual
            ));
        }
        if !fact.place_qualifier_any.is_empty()
            && actual.is_none_or(|value| {
                !value.split(|c: char| !c.is_alphanumeric()).any(|word| {
                    fact.place_qualifier_any
                        .iter()
                        .any(|qualifier| word.eq_ignore_ascii_case(qualifier))
                })
            })
        {
            mismatches.push(format!(
                "Resolved {} must qualify the place with one of {:?}; actual {:?}",
                fact.field.name(),
                fact.place_qualifier_any,
                actual
            ));
        }
    }
    for field in &case.expected.forbidden_facts {
        if consultation.is_some_and(|c| {
            c.facts
                .get(field)
                .is_some_and(|slot| !matches!(slot, reading_contracts::Slot::Missing))
        }) {
            mismatches.push(format!(
                "{} must not be invented or reinterpreted as an anchor",
                field.name()
            ));
        }
    }
    // A required inquiry is also an authored statement that this fact cannot
    // yet be resolved. A blocked handoff must not hide an invented input behind
    // a limitation and receive an extraction pass just because it stayed blocked.
    for need in expected_needs {
        let RequirementKey::Field(field) = need else {
            continue;
        };
        if let Some(value) = consultation.and_then(|c| c.text(*field)) {
            mismatches.push(format!(
                "Genuinely missing {} must remain unresolved; actual resolved {value:?}",
                field.name()
            ));
        }
    }
    if let Some(owner) = &case.expected.owner {
        let actual = consultation
            .and_then(|c| c.subject.resolved())
            .map(|subject| subject.owner_id.as_str());
        if actual.is_none_or(|actual| !same_actor(consultation, actual, owner)) {
            mismatches.push(format!("Owner must be {owner:?}; actual {actual:?}"));
        }
    }
    let subject = consultation.and_then(|case| case.subject.resolved());
    if let Some(kind) = &case.expected.subject_kind {
        if subject.is_none_or(|subject| subject.kind != *kind) {
            mismatches.push(format!(
                "Subject kind must be {kind:?}; actual {:?}",
                subject.map(|subject| &subject.kind)
            ));
        }
    }
    if let Some(name) = &case.expected.subject_name_contains {
        if subject.is_none_or(|subject| !subject.name.to_lowercase().contains(&name.to_lowercase()))
        {
            mismatches.push(format!(
                "Subject name must retain {name:?}; actual {:?}",
                subject.map(|subject| &subject.name)
            ));
        }
    }
    if !case.expected.subject_name_any_contains.is_empty()
        && subject.is_none_or(|subject| {
            !case
                .expected
                .subject_name_any_contains
                .iter()
                .any(|name| subject.name.to_lowercase().contains(&name.to_lowercase()))
        })
    {
        mismatches.push(format!(
            "Subject name must retain one of {:?}; actual {:?}",
            case.expected.subject_name_any_contains,
            subject.map(|s| &s.name)
        ));
    }
    for (person, relation) in case.expected.relationships.iter().flatten() {
        let actual = consultation.and_then(|c| {
            c.people
                .get(person)
                .or_else(|| {
                    c.people
                        .values()
                        .find(|p| p.label.eq_ignore_ascii_case(person))
                })
                .map(|p| p.relationship.as_str())
        });
        if actual != Some(relation.as_str()) {
            mismatches.push(format!(
                "{person}'s relationship must be {relation:?}; actual {actual:?}"
            ));
        }
    }
    let elicitation_start = mismatches.len();
    for expected in expected_needs {
        if !needs
            .iter()
            .any(|actual| same_need(consultation, actual, expected))
        {
            mismatches.push(format!(
                "Necessary information {expected:?} was not tracked"
            ));
        }
    }
    if expected_needs.is_empty() && !needs.is_empty() {
        mismatches.push(format!(
            "Complete inputs acquired unnecessary needs: {needs:?}"
        ));
    }
    let requested = consultation.and_then(|c| c.requested.as_ref());
    if !expected_needs.is_empty()
        && requested.is_none_or(|key| {
            !expected_needs
                .iter()
                .any(|expected| same_need(consultation, key, expected))
        })
    {
        mismatches.push(format!(
            "The reader must elicit one expected missing fact; selected {requested:?}"
        ));
    }
    let handoff_start = mismatches.len();
    let ready = session.chart.is_some()
        && plan
            .as_ref()
            .is_some_and(|p| p.needs.is_empty() && p.limitation.is_none());
    if ready != case.expected.ready {
        mismatches.push(format!(
            "Native readiness must be {}; actual {ready}",
            case.expected.ready
        ));
    }
    if case.expected.chart_local_time.is_none()
        && case.expected.chart_moment_ms.is_none()
        && session
            .chart
            .as_ref()
            .is_some_and(|chart| chart["timestampMs"].as_f64() != Some(default_moment))
    {
        mismatches.push("An ordinary question's chart did not retain the frozen question-receipt instant; event dates cannot replace it".into());
    }
    if let Some(place) = &case.expected.chart_place_contains {
        let actual = session.place.as_ref().map(|p| p.label.as_str());
        if actual.is_none_or(|label| !label.to_lowercase().contains(&place.to_lowercase())) {
            mismatches.push(format!(
                "Chart place must contain {place:?}; actual {actual:?}"
            ));
        }
    }
    if let Some(moment) = &case.expected.chart_local_time {
        let actual = session
            .chart
            .as_ref()
            .and_then(|chart| chart["timestampMs"].as_f64())
            .zip(session.place.as_ref())
            .and_then(|(time, place)| {
                horary_ai_core::chart_input::local_clock(time, &place.timezone).ok()
            });
        if actual
            .as_ref()
            .is_none_or(|actual| !actual.starts_with(moment))
        {
            mismatches.push(format!(
                "Chart civil time must start with {moment:?}; actual {actual:?}"
            ));
        }
    }
    if let Some(expected) = case.expected.chart_moment_ms {
        let actual = session
            .chart
            .as_ref()
            .and_then(|chart| chart["timestampMs"].as_f64());
        if actual != Some(expected) {
            mismatches.push(format!(
                "Chart UTC instant must be {expected}; actual {actual:?}"
            ));
        }
    }
    let reply_start = mismatches.len();
    if expected_needs.is_empty() && requested.is_some() {
        mismatches.push(format!(
            "The reader requested unnecessary information {requested:?}"
        ));
    }
    let reply = latest_reply_after_user(session).unwrap_or_default();
    let input_binding = input_handoff_binding(session);
    let input_handoff = input_binding.is_some();
    if reply.trim().is_empty() && !input_handoff {
        mismatches.push("No conversational reply was delivered".into());
    }
    let flags = fluidity_flags(session, reply, &needs);
    let classification = Gate::checked(
        mismatches[classification_start..facts_start].to_vec(),
        "Authored allowed methods and answer facets; dialogue meaning is independently reviewed",
    );
    let mut extraction_failures = mismatches[evidence_start..classification_start].to_vec();
    extraction_failures.extend_from_slice(&mismatches[facts_start..elicitation_start]);
    extraction_failures.extend_from_slice(&mismatches[handoff_start..reply_start]);
    let extraction = Gate::checked(extraction_failures, "Authored canonical facts, actor/owner bindings, normalization, chart anchor and native handoff; original quotes are retained by production acceptance");
    let mut elicitation_failures = mismatches[elicitation_start..handoff_start].to_vec();
    elicitation_failures.extend_from_slice(&mismatches[reply_start..]);
    let elicitation = Gate::checked(elicitation_failures, "Authored genuine gaps or no gaps, selected reminder and a fresh response; the meaning and fluency of the actual inquiry need independent review");
    Grade {
        semantic_pass: mismatches.is_empty(),
        mismatches,
        actual: json!({"frame":frame,"question":session.question,"needs":needs,
            "requested":requested,"ready":ready,"plan":plan,"chart":session.chart,
            "reader_place":session.place,"anchor":anchor,"consultation":consultation,
            "reply":reply,"conversation_reply_observed":!reply.trim().is_empty(),
            "input_handoff_boundary":input_handoff,"input_handoff_binding":input_binding,
            "native_result":session.method.result}),
        fluidity_review_flags: flags,
        human_fluidity_review:
            "Not reviewed; flags are heuristics, not certification of conversational quality",
        hurdles: crate::reading_eval::Hurdles {
            classification,
            elicitation,
            extraction,
            reading: crate::reading_eval::reading_gate(session, false, true, None),
        },
    }
}

fn with_reading_grade(
    mut grade: Grade,
    session: &Session,
    full: bool,
    error: Option<&str>,
) -> Grade {
    use crate::reading_eval::Status;
    let inputs_pass = [
        &grade.hurdles.classification,
        &grade.hurdles.elicitation,
        &grade.hurdles.extraction,
    ]
    .iter()
    .all(|gate| gate.status == Status::Pass);
    grade.hurdles.reading = crate::reading_eval::reading_gate(session, full, inputs_pass, error);
    grade
}

fn fluidity_flags(session: &Session, reply: &str, needs: &[RequirementKey]) -> Vec<String> {
    let mut flags = Vec::new();
    let lower = reply.to_lowercase();
    if reply.chars().count() > 520 {
        flags.push("Long elicitation reply; review whether every sentence helps".into());
    }
    if [
        "the user",
        "the querent has",
        "you have provided the initial",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
    {
        flags.push("Possible third-person case summary instead of conversation".into());
    }
    if ["worksheet", "pipeline", "json", "schema", "model output"]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        flags.push("Processing vocabulary appears in the conversation".into());
    }
    if session.chart.is_none()
        && [
            "this chart shows",
            "the chart indicates",
            "this chart suggests",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        flags.push("Possible findings claimed before a chart exists".into());
    }
    if lower.matches('?').count() > 1 {
        flags.push("Multiple questions in one reply; review conversational load".into());
    }
    if reply.contains('?')
        && !needs.is_empty()
        && session
            .method
            .consultation
            .as_ref()
            .is_none_or(|case| case.requested.is_none())
    {
        flags.push("Question was not bound to a tracked reminder; may be a valid reframing".into());
    }
    if session
        .messages
        .iter()
        .rev()
        .filter(|message| message.role == "assistant")
        .skip(1)
        .any(|old| old.text.trim().eq_ignore_ascii_case(reply.trim()))
    {
        flags.push("Reader repeated an earlier reply verbatim".into());
    }
    if session.method.brief.intent != "explain"
        && !session
            .sections
            .iter()
            .any(|section| section.method_stage == Some(Stage::Judgment))
        && [
            "i have the chart cast",
            "i've cast the chart",
            "i'm looking into the chart",
            "i am looking into the chart",
            "let's look at what the chart shows",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        flags.push("Progress announcement before interpretation; review conversational usefulness within this elicitation-only run".into());
    }
    let source = session
        .messages
        .iter()
        .filter(|message| message.role == "user")
        .map(|message| message.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let has_checked_interpretation = session.method.records.iter().any(|record| {
        record.revision == session.revision
            && record.validation_error.is_none()
            && matches!(
                record.stage,
                Stage::Condition
                    | Stage::Reception
                    | Stage::Contacts
                    | Stage::Location
                    | Stage::Judgment
            )
    });
    if !has_checked_interpretation
        && [
            "the chart shows",
            "the chart suggests",
            "the chart indicates",
            "the chart reveals",
            "the stars show",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        flags.push("Possible unsupported chart interpretation: no current checked astrological finding or judgment exists; inspect the reply against anchor-only context".into());
    }
    let source_tokens: BTreeSet<_> = source.split(|c: char| !c.is_alphabetic()).collect();
    let reply_tokens: BTreeSet<_> = lower.split(|c: char| !c.is_alphabetic()).collect();
    for (gender, pronouns, sources) in [
        (
            "male",
            &["he", "him", "his"][..],
            &[
                "he",
                "him",
                "his",
                "husband",
                "boyfriend",
                "father",
                "dad",
                "brother",
                "son",
                "uncle",
                "grandfather",
                "grandpa",
                "mr",
                "sir",
                "male",
                "man",
                "boy",
            ][..],
        ),
        (
            "female",
            &["she", "her", "hers"][..],
            &[
                "she",
                "her",
                "hers",
                "wife",
                "girlfriend",
                "mother",
                "mum",
                "mom",
                "sister",
                "daughter",
                "aunt",
                "grandmother",
                "grandma",
                "mrs",
                "ms",
                "female",
                "woman",
                "girl",
            ][..],
        ),
    ] {
        if pronouns.iter().any(|word| reply_tokens.contains(word))
            && !sources.iter().any(|word| source_tokens.contains(word))
        {
            flags.push(format!("Possible invented {gender} pronoun; no matching gender evidence occurs in the authored user dialogue"));
        }
    }
    flags
}

fn latest_reply_after_user(session: &Session) -> Option<&str> {
    let user = session
        .messages
        .iter()
        .rposition(|message| message.role == "user")?;
    session.messages[user + 1..]
        .iter()
        .rev()
        .find(|message| message.role == "assistant" && !message.text.trim().is_empty())
        .map(|message| message.text.as_str())
}

fn proposal_is_eligible(case: &Case, first: &Grade, session: &Session) -> bool {
    case.follow_up_accepts_proposal
        && first.semantic_pass
        && matches!(session.method.result.as_ref(), Some(reading_contracts::ReadingResult::Limited { limitation }) if limitation.code == "unsupported_facet")
        && session
            .method
            .consultation
            .as_ref()
            .is_some_and(|consultation| {
                consultation.requested.is_none()
                    && consultation
                        .plan(current_anchor(session).as_ref())
                        .limitation
                        .is_some_and(|limit| limit.code == "unsupported_facet")
            })
        && latest_reply_after_user(session).is_some_and(|reply| reply.matches('?').count() == 1)
        && session
            .audit
            .iter()
            .rev()
            .find(|event| event["event"] == "conversation_reminder_selected")
            .is_some_and(|event| event["ask"] == "")
}

fn follow_up_grade(
    case: &Case,
    words: &str,
    previous: &Session,
    session: &Session,
    result: &Result<(), String>,
) -> Value {
    let clipboard = crate::horary_conversation::clipboard(session);
    let needs: Vec<RequirementKey> = clipboard["reminders"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|need| serde_json::from_value(need["key"].clone()).ok())
        .collect();
    let mut failures = Vec::new();
    if let Err(error) = result {
        failures.push(format!("Follow-up execution failed: {error}"));
    }
    let unresolved: Vec<_> = case
        .expected
        .needs
        .iter()
        .filter(|need| {
            if needs
                .iter()
                .any(|actual| same_need(session.method.consultation.as_ref(), actual, need))
            {
                return true;
            }
            let Some(consultation) = session.method.consultation.as_ref() else {
                return true;
            };
            match need {
                RequirementKey::Question => consultation.question.resolved().is_none(),
                RequirementKey::Frame => consultation
                    .method()
                    .is_none_or(|method| matches!(method, Method::Wish | Method::Unclassified)),
                RequirementKey::Subject => consultation.subject.resolved().is_none(),
                RequirementKey::Owner => consultation
                    .subject
                    .resolved()
                    .is_none_or(|subject| subject.owner_id.is_empty()),
                RequirementKey::PersonRelationship(id) => consultation
                    .people
                    .get(id)
                    .or_else(|| {
                        consultation
                            .people
                            .values()
                            .find(|person| person.label.eq_ignore_ascii_case(id))
                    })
                    .is_none_or(|person| person.relationship == "unknown"),
                RequirementKey::Field(field) => consultation.text(*field).is_none(),
                RequirementKey::ChartPlace => session.place.is_none(),
                RequirementKey::ChartMoment => session.chart.is_none(),
            }
        })
        .cloned()
        .collect();
    for need in &unresolved {
        failures.push(format!("The supplied follow-up did not resolve {need:?}"));
    }
    // A clarification is not permission to replace the matter or its sky.
    // Only an explicitly authored new reading changes both; a consented
    // proposal may change the question's facet while keeping its moment.
    let lower = words.to_lowercase();
    let new_matter = [
        "new question",
        "different question",
        "start over",
        "fresh reading",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase));
    let accepts_proposal = proposal_is_eligible(case, &grade(case, previous, None), previous);
    let prior_understood = previous
        .method
        .consultation
        .as_ref()
        .is_some_and(reading_contracts::Consultation::understood);
    if prior_understood && !new_matter && !accepts_proposal && previous.question != session.question
    {
        failures.push("An information-only follow-up replaced the established question without authored authorization".into());
    }
    let correcting_clock = case.expected.needs.iter().any(|need| {
            matches!(
                need,
                RequirementKey::ChartMoment
                    | RequirementKey::Field(Field::QuestionTime | Field::TimeOccurrence)
            )
        }) && session.method.consultation.as_ref().is_some_and(|consultation| {
            [Field::QuestionTime, Field::TimeOccurrence].iter().any(|field| {
                let prior = previous.method.consultation.as_ref().and_then(|c| c.text(*field));
                consultation.facts.get(field).is_some_and(|slot| {
                    matches!(slot, reading_contracts::Slot::Resolved { observation }
                        if Some(observation.value.as_str()) != prior
                            && matches!(&observation.evidence, reading_contracts::Evidence::User { quote, .. }
                                if !quote.trim().is_empty() && lower.contains(&quote.to_lowercase())))
                })
            })
        });
    if previous.candidate_moment_ms.is_some()
        && !correcting_clock
        && !new_matter
        && previous.candidate_moment_ms != session.candidate_moment_ms
    {
        failures.push(
            "The original candidate question moment was lost during a factual follow-up".into(),
        );
    }
    let fresh_reply = latest_reply_after_user(session);
    let input_handoff = input_handoff_boundary(session);
    if fresh_reply.is_none() && !input_handoff {
        failures.push("No new conversational reply followed the supplied follow-up; an earlier reply cannot qualify this turn".into());
    }
    let reply = fresh_reply.unwrap_or_default();
    let anchor = current_anchor(session);
    let consultation = session.method.consultation.as_ref();
    let plan = consultation.map(|consultation| consultation.plan(anchor.as_ref()));
    let native_needs: Vec<_> = plan
        .as_ref()
        .into_iter()
        .flat_map(|p| p.needs.iter())
        .map(|need| &need.key)
        .collect();
    if !needs.is_empty() {
        failures.push(format!(
            "The supplied follow-up left clipboard input needs: {needs:?}"
        ));
    }
    if !native_needs.is_empty() {
        failures.push(format!(
            "The supplied follow-up left native contract input needs: {native_needs:?}"
        ));
    }
    if let Some(reading_contracts::ReadingResult::NeedsInformation { need }) =
        session.method.result.as_ref()
    {
        failures.push(format!("A native information request still remains after a sufficiently supplying follow-up: {:?}", need.key));
    }
    let default_moment = previous
        .candidate_moment_ms
        .unwrap_or_else(|| frozen_moment() + 60_000.);
    let after_grade = case.follow_up_expected.as_ref().map(|expected| {
        let mut after_case = case.clone();
        after_case.expected = expected.clone();
        grade_at_moment(
            &after_case,
            session,
            result.as_ref().err().map(String::as_str),
            default_moment,
        )
    });
    if let Some(after) = &after_grade {
        failures.extend(
            after
                .mismatches
                .iter()
                .map(|mismatch| format!("After-state: {mismatch}")),
        );
    } else {
        failures.push("The scripted follow-up has no authored after-state expectation; its journey is unqualified".into());
    }
    // The latest fact patch must reach the actual native permit boundary.
    // Specialist limitations are an honest outcome once input gaps are gone.
    let mut ready_binding = None;
    let mut native_handoff_recorded = false;
    match plan.as_ref().and_then(|plan| plan.limitation.as_ref()) {
        Some(limit) => {
            if !matches!(session.method.result.as_ref(), Some(reading_contracts::ReadingResult::Limited { limitation }) if limitation.code == limit.code)
            {
                failures.push(format!(
                    "The native method limitation {:?} was not retained as the actual outcome",
                    limit.code
                ));
            }
        }
        None => {
            let chart_anchor =
                session
                    .chart
                    .as_ref()
                    .zip(session.place.as_ref())
                    .and_then(|(chart, place)| {
                        Some(Anchor {
                            timestamp_ms: chart["timestampMs"].as_f64()?,
                            latitude: place.latitude,
                            longitude: place.longitude,
                            timezone: place.timezone.clone(),
                        })
                    });
            match consultation.zip(chart_anchor).map(|(case, anchor)| reading_contracts::ReadyReading::prepare(case, anchor)) {
                Some(Ok(ready)) => {
                    let binding = serde_json::to_value(ready.binding()).expect("Native reading bindings are serializable");
                    native_handoff_recorded = session.audit.iter().any(|event| event["event"] == "contract_handoff" && event["binding"] == binding);
                    if !native_handoff_recorded {
                        failures.push("Complete implemented inputs did not reach a recorded native ReadyReading handoff".into());
                    }
                    ready_binding = Some(binding);
                }
                Some(Err(rejected)) => failures.push(format!("The actual chart did not authorize ReadyReading: {:?}", rejected.needs)),
                None => failures.push("Complete implemented inputs did not produce an actual chart and native ReadyReading permit".into()),
            }
        }
    }
    let input_complete = consultation.is_some() && needs.is_empty() && native_needs.is_empty();
    json!({"pass":failures.is_empty(),"failures":failures,"unresolved_original_needs":unresolved,
        "current_needs":needs,"native_needs":native_needs,"input_complete":input_complete,
        "after_state_grade":after_grade,"fresh_assistant_reply":fresh_reply.is_some(),
        "input_handoff_boundary":input_handoff,"conversation_complete":fresh_reply.is_some(),
        "ready_binding":ready_binding,"native_handoff_recorded":native_handoff_recorded,
        "question_preserved":previous.question==session.question,
        "candidate_moment_preserved":previous.candidate_moment_ms==session.candidate_moment_ms,
        "explicit_new_matter":new_matter,"accepted_authored_proposal":accepts_proposal,
        "sourced_chart_clock_correction":correcting_clock,"fluidity_review_flags":fluidity_flags(session,reply,&needs),
        "human_fluidity_review":"Not reviewed; successful slot resolution does not certify a fluid conversation"})
}

fn retain_partial(file: tempfile::NamedTempFile) -> String {
    match file.keep() {
        Ok((_, path)) => format!("Partial evidence retained at {}", path.display()),
        Err(error) => format!(
            "Could not retain the partial evidence file: {}",
            error.error
        ),
    }
}

fn atomic_evidence_file(
    path: &Path,
    immutable: bool,
    write: impl FnOnce(&mut fs::File) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Evidence path has no parent directory")?;
    let mut file = tempfile::Builder::new()
        .prefix("evidence-partial-")
        .tempfile_in(parent)
        .map_err(|error| format!("Evidence I/O failure at {}: {error}", path.display()))?;
    if let Err(error) = write(file.as_file_mut())
        .and_then(|()| file.as_file().sync_all().map_err(|error| error.to_string()))
    {
        return Err(format!(
            "Evidence I/O failure at {}: {error}. {}",
            path.display(),
            retain_partial(file)
        ));
    }
    let persisted = if immutable {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    };
    persisted.map(|_| ()).map_err(|error| {
        format!(
            "Evidence I/O failure at {}: {}. {}",
            path.display(),
            error.error,
            retain_partial(error.file)
        )
    })
}

fn write_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
    atomic_evidence_file(path, true, |file| {
        serde_json::to_writer_pretty(file, value).map_err(|error| error.to_string())
    })
}

fn write_index_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    atomic_evidence_file(path, false, |file| {
        serde_json::to_writer_pretty(file, value).map_err(|error| error.to_string())
    })
}

fn write_index_text(path: &Path, value: &str) -> Result<(), String> {
    atomic_evidence_file(path, false, |file| {
        file.write_all(value.as_bytes())
            .map_err(|error| error.to_string())
    })
}

struct Reader<'a> {
    state: &'a NativeLlamaState,
    hosted: Option<Arc<crate::hosted_gemma_eval::Client>>,
    dir: PathBuf,
    cancelled: Arc<AtomicBool>,
    calls: Mutex<Vec<Value>>,
    sequence: AtomicU64,
    checkpoints: AtomicU64,
    max_calls: u64,
    device_available: bool,
    device_calls: AtomicU64,
    dispatcher: Option<mpsc::Sender<BatchCall>>,
    group: Option<usize>,
    program: Option<Arc<horary_prompt_program::Program>>,
}

/// Experimental teaching is applied to the actual native request, not to an
/// evaluator facsimile. Facts, output contracts and repair messages are intact.
fn trial_prompt(
    stage: Stage,
    matter: Matter,
    input: &Value,
    schema: &Value,
    program: Option<&horary_prompt_program::Program>,
) -> Result<(String, Option<horary_prompt_program::Applied>), String> {
    let prompt = crate::horary_contract::prompt(stage, matter, input, schema)?;
    let Some(program) = program else {
        return Ok((prompt, None));
    };
    let mut messages: Value = serde_json::from_str(&prompt).map_err(|e| e.to_string())?;
    let applied = horary_prompt_program::apply_messages(
        program,
        horary_prompt_program::signature(stage.name(), input),
        &mut messages,
    )?;
    Ok((messages.to_string(), applied))
}

fn experiment_origin(program: &Value) -> Result<Value, String> {
    let origin = std::env::var("HORARY_EVAL_ORIGIN_SHA256").ok();
    let fixed = std::env::var("HORARY_EVAL_FIXED_CANDIDATE_SHA256").ok();
    let role = std::env::var("HORARY_EVAL_EXPERIMENT_ROLE").ok();
    if origin.is_none() && fixed.is_none() && role.is_none() {
        return Ok(Value::Null);
    }
    let valid_digest = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let origin = origin
        .filter(|s| valid_digest(s))
        .ok_or("Experiment needs the discovery manifest SHA256")?;
    let fixed = fixed
        .filter(|s| valid_digest(s))
        .ok_or("Experiment needs the fixed candidate SHA256")?;
    let role = role
        .filter(|s| s == "control" || s == "candidate")
        .ok_or("Experiment role must be control or candidate")?;
    if role == "control" && !program.is_null() {
        return Err("An experiment control must use the original teaching".into());
    }
    if role == "candidate"
        && (program["sha256"] != fixed || program["baseline_manifest_sha256"] != origin)
    {
        return Err(
            "Experiment candidate changed or belongs to a different discovery origin".into(),
        );
    }
    Ok(json!({"discovery_manifest_sha256":origin,"fixed_candidate_sha256":fixed,"role":role}))
}

#[test]
fn experimental_teaching_changes_actual_requests_but_not_inputs_or_repairs() {
    use horary_prompt_program::{digest, Evidence, Override, Program};
    let original = json!({"consultation":{"frame":{"state":"resolved","observation":{"value":{"method":"lost_animal"}}}}});
    let schema = json!({"type":"object","properties":{"reply":{"type":"string"}}});
    let guide =
        crate::horary_contract::guide_for(Stage::Conversation, Matter::LostAnimal, &original)
            .unwrap();
    let program = Program {
        version:1,id:"actual-native-request-test".into(),baseline_manifest_sha256:digest("manifest"),
        overrides:vec![Override {stage:"conversation".into(),recognition_phase:None,method:Some("lost_animal".into()),expected_guide_sha256:digest(&guide),replacement_text:format!("A test teaching addition to the actual request. Preserve every native fact and contract.\n{guide}"),edits:vec![]}],
        rationale:"Regression for the experiment hook".into(),
        evidence:vec![Evidence {case_id:"training".into(),file:"trace.json".into(),json_pointer:"".into(),sha256:digest("trace")}],
        training_case_ids:vec!["training".into()],holdout_case_ids:vec!["reserved".into()],
    };
    program.validate().unwrap();
    let repair = json!({"original_input":original,"previous_worksheet":{"reply":"rejected"},"native_validation_error":"Wrong evidence"});
    for input in [&original, &repair] {
        let baseline: Value = serde_json::from_str(
            &crate::horary_contract::prompt(
                Stage::Conversation,
                Matter::LostAnimal,
                input,
                &schema,
            )
            .unwrap(),
        )
        .unwrap();
        let (prompt, applied) = trial_prompt(
            Stage::Conversation,
            Matter::LostAnimal,
            input,
            &schema,
            Some(&program),
        )
        .unwrap();
        let trial: Value = serde_json::from_str(&prompt).unwrap();
        assert!(applied.is_some());
        assert_ne!(baseline[0], trial[0]);
        assert_eq!(
            &baseline.as_array().unwrap()[1..],
            &trial.as_array().unwrap()[1..]
        );
    }
    let unrelated = json!({"consultation":{"frame":{"state":"resolved","observation":{"value":{"method":"new_job"}}}}});
    let (prompt, receipt) = trial_prompt(
        Stage::Conversation,
        Matter::Work,
        &unrelated,
        &schema,
        Some(&program),
    )
    .unwrap();
    assert!(receipt.is_none());
    assert_eq!(
        prompt,
        crate::horary_contract::prompt(Stage::Conversation, Matter::Work, &unrelated, &schema)
            .unwrap()
    );
    let mut stale = program;
    stale.overrides[0].expected_guide_sha256 = digest("stale");
    assert!(trial_prompt(
        Stage::Conversation,
        Matter::LostAnimal,
        &original,
        &schema,
        Some(&stale)
    )
    .is_err());
}

struct BatchCall {
    prompt: String,
    max_tokens: u32,
    response: mpsc::Sender<Result<NativeGenerationResult, String>>,
}

/// Four independent consultations may share native decoding. This changes the
/// decoder to unconstrained JSON and is explicitly an exploration experiment.
fn dispatch_batches(
    state: &NativeLlamaState,
    requests: mpsc::Receiver<BatchCall>,
    cancelled: Arc<AtomicBool>,
) {
    while let Ok(first) = requests.recv() {
        let mut group = vec![first];
        let coalesce_until = Instant::now() + Duration::from_millis(25);
        while group.len() < 4 {
            let Some(wait) = coalesce_until.checked_duration_since(Instant::now()) else {
                break;
            };
            match requests.recv_timeout(wait) {
                Ok(request) => group.push(request),
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    break
                }
            }
        }
        let prompts = group
            .iter()
            .map(|request| (request.prompt.clone(), request.max_tokens))
            .collect();
        let result=generate_native_batch(state,prompts,NativeGenerateOptions {
            temperature:0.,seed:0,cache_lesson:true,cancel:Some(cancelled.clone()),..Default::default()
        }).map_err(|e|format!("Exploration batch backend interrupted (shared group cancellation affects every peer): {}",e.message));
        match result {
            Ok(outputs) if outputs.len() == group.len() => {
                for (request, output) in group.into_iter().zip(outputs) {
                    let _ = request.response.send(Ok(output));
                }
            }
            Ok(_) => {
                for request in group {
                    let _=request.response.send(Err("Exploration batch backend returned the wrong branch count; no peer can qualify".into()));
                }
            }
            Err(error) => {
                for request in group {
                    let _ = request.response.send(Err(error.clone()));
                }
            }
        }
    }
}

struct Deadline {
    stop: mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Deadline {
    fn start(seconds: u64, cancelled: Arc<AtomicBool>) -> Self {
        let (stop, done) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            if matches!(
                done.recv_timeout(Duration::from_secs(seconds)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                cancelled.store(true, Ordering::Release);
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone)]
struct CaseRun {
    full_reading: bool,
    seconds: u64,
    max_calls: u64,
    dispatcher: Option<mpsc::Sender<BatchCall>>,
    shared_cancelled: Option<Arc<AtomicBool>>,
    group: Option<usize>,
    program: Option<Arc<horary_prompt_program::Program>>,
    rubrics: Option<Arc<BTreeMap<String, crate::reading_eval::Rubric>>>,
    hosted: Option<Arc<crate::hosted_gemma_eval::Client>>,
}
impl Reader<'_> {
    fn case_id(&self) -> &str {
        self.dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    }
    fn keep_call(&self, sequence: u64, call: Value) -> Result<(), String> {
        // A recoverable final trace can retain the raw result even if this
        // particular persistence attempt fails. An I/O error is never success.
        self.calls
            .lock()
            .map_err(|e| e.to_string())?
            .push(call.clone());
        write_new(
            &self
                .dir
                .join("calls")
                .join(format!("{sequence:04}-result.json")),
            &call,
        )?;
        println!(
            "{}",
            json!({"event":"model_result","case_id":self.case_id(),"sequence":sequence,"stage":call["request"]["stage"],"batch_tasks":call["request"]["tasks"].as_array().map(|tasks|tasks.iter().map(|task|task[0].clone()).collect::<Vec<_>>()),"wall_ms":call["wall_ms"],"error":call["result"]["Err"]})
        );
        Ok(())
    }
}
impl Runtime for Reader<'_> {
    fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
        let sequence = self.device_calls.fetch_add(1, Ordering::AcqRel) + 1;
        let result = self.device_available.then(device);
        write_new(
            &self
                .dir
                .join(format!("synthetic-device-acquisition-{sequence:04}.json")),
            &result,
        )?;
        Ok(result)
    }
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        self.check()?;
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let (prompt, applied) =
            trial_prompt(stage, matter, input, schema, self.program.as_deref())?;
        let messages: Value = serde_json::from_str(&prompt).map_err(|e| e.to_string())?;
        let request = json!({"sequence":sequence,"stage":stage,"matter":matter,
            "provider":self.hosted.as_ref().map(|client|client.metadata()),
            "prompt":messages,
            "schema":schema,"input":input,
            "guide_sha256":horary_lessons::digest(messages[0]["content"].as_str().ok_or("Missing actual system teaching")?),
            "baseline_guide_sha256":horary_lessons::digest(&crate::horary_contract::guide_for(stage,matter,input)?),
            "prompt_program":applied,
            "prompt_sha256":horary_lessons::digest(&prompt),
            "schema_sha256":horary_lessons::digest(&schema.to_string()),
            "decoder":if self.hosted.is_some(){"hosted unconstrained text; prompt-specified contract, native acceptance and repair; not on-device qualification"}else if self.dispatcher.is_some(){"EXPLORATION unconstrained independent batch; not production-decoder qualification"}else{"production constrained single text generation"},
            "exploration_group":self.group,
            "cancellation_scope":if self.group.is_some(){"shared group; a deadline can interrupt every peer"}else{"individual case"}});
        write_new(
            &self
                .dir
                .join("calls")
                .join(format!("{sequence:04}-request.json")),
            &request,
        )?;
        let start = Instant::now();
        println!(
            "{}",
            json!({"event":"model_started","case_id":self.case_id(),"sequence":sequence,"stage":stage,"hosted":self.hosted.is_some()})
        );
        let mut provider_receipt = Value::Null;
        let result = if let Some(hosted) = &self.hosted {
            if audio.is_some() {
                return Err("Hosted evaluation accepts synthetic text cases only".into());
            }
            let attempt = hosted.generate(
                &prompt,
                schema,
                if stage == Stage::Judgment { 1400 } else { 1000 },
                self.cancelled.clone(),
            );
            provider_receipt = attempt.receipt;
            attempt.result
        } else if let Some(dispatcher) = &self.dispatcher {
            if audio.is_some() {
                return Err("Exploratory batch supports text elicitation only".into());
            }
            let (response, ready) = mpsc::channel();
            dispatcher
                .send(BatchCall {
                    prompt,
                    max_tokens: if stage == Stage::Judgment { 1400 } else { 1000 },
                    response,
                })
                .map_err(|_| "Exploration dispatcher disconnected before submission")?;
            ready.recv().map_err(|_| {
                "Exploration dispatcher disconnected before delivering a native result"
            })?
        } else {
            generate_native(
                self.state,
                prompt,
                NativeGenerateOptions {
                    max_tokens: if stage == Stage::Judgment { 1400 } else { 1000 },
                    temperature: 0.,
                    seed: 0,
                    response_schema: audio.is_none().then(|| schema.to_string()),
                    audio: audio.map(<[u8]>::to_vec),
                    cache_lesson: audio.is_none(),
                    cancel: Some(self.cancelled.clone()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.message)
        };
        self.keep_call(
            sequence,
            json!({"request":request,"wall_ms":start.elapsed().as_millis(),
            "provider_receipt":provider_receipt,
            "result":result.as_ref().map_err(String::as_str)}),
        )?;
        result
    }
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String> {
        if self.dispatcher.is_some() {
            return Err("Exploration batch case flows stop at elicitation; nested full-reading analysis batches are unsupported".into());
        }
        self.check()?;
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let prepared = tasks
            .iter()
            .map(|(stage, matter, input, schema)| {
                trial_prompt(*stage, *matter, input, schema, self.program.as_deref())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let prompts: Vec<(String, u32)> = prepared
            .iter()
            .map(|(prompt, _)| (prompt.clone(), 1000))
            .collect();
        let request = json!({"sequence":sequence,"tasks":tasks,"prompts":prompts.iter()
            .map(|(p,_)|serde_json::from_str::<Value>(p)).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?,
            "prompt_program":prepared.iter().map(|(_,applied)|applied).collect::<Vec<_>>(),
            "provider":self.hosted.as_ref().map(|client|client.metadata()),
            "decoder":if self.hosted.is_some(){"hosted parallel unconstrained text branches; prompt contracts and native acceptance unchanged"}else{"production unconstrained independent analysis batch"}});
        write_new(
            &self
                .dir
                .join("calls")
                .join(format!("{sequence:04}-request.json")),
            &request,
        )?;
        let start = Instant::now();
        println!(
            "{}",
            json!({"event":"model_started","case_id":self.case_id(),"sequence":sequence,"stages":tasks.iter().map(|task|task.0).collect::<Vec<_>>(),"hosted":self.hosted.is_some()})
        );
        let mut provider_receipts = Value::Null;
        let result = if let Some(hosted) = &self.hosted {
            let attempts = std::thread::scope(|scope| {
                let workers = prompts
                    .iter()
                    .zip(tasks)
                    .map(|((prompt, max_tokens), (_, _, _, schema))| {
                        let cancel = self.cancelled.clone();
                        scope.spawn(move || hosted.generate(prompt, schema, *max_tokens, cancel))
                    })
                    .collect::<Vec<_>>();
                workers
                    .into_iter()
                    .map(|worker| {
                        worker
                            .join()
                            .map_err(|_| "Hosted analysis worker panicked".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()
            });
            match attempts {
                Ok(attempts) => {
                    provider_receipts = json!(attempts
                        .iter()
                        .map(|attempt| &attempt.receipt)
                        .collect::<Vec<_>>());
                    attempts.into_iter().map(|attempt| attempt.result).collect()
                }
                Err(error) => Err(error),
            }
        } else {
            generate_native_batch(
                self.state,
                prompts,
                NativeGenerateOptions {
                    max_tokens: 1000,
                    temperature: 0.,
                    seed: 0,
                    cache_lesson: true,
                    cancel: Some(self.cancelled.clone()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.message)
        };
        self.keep_call(
            sequence,
            json!({"request":request,"wall_ms":start.elapsed().as_millis(),
            "provider_receipts":provider_receipts,
            "result":result.as_ref().map_err(String::as_str)}),
        )?;
        result
    }
    fn publish(&self, session: &mut Session) -> Result<(), String> {
        let sequence = self.checkpoints.fetch_add(1, Ordering::AcqRel) + 1;
        write_new(
            &self
                .dir
                .join("checkpoints")
                .join(format!("{sequence:04}.json")),
            &json!({"session":session,"candidates":session.candidates}),
        )?;
        let path = self.dir.join("reading.json");
        crate::reading_store::write(&path, session, false)
            .map_err(|error| format!("Evidence I/O failure at {}: {error}", path.display()))?;
        write_index_json(
            &self.dir.join("live.json"),
            &json!({"case_id":self.case_id(),"checkpoint":sequence,"current_stage":session.status,"accepted_records":session.method.records.len(),"completed_stages":session.sections.iter().filter_map(|section|section.method_stage).collect::<Vec<_>>(),"pending":session.method.flow.pending,"native_result":session.method.result.as_ref().map(|result| match result {reading_contracts::ReadingResult::Judgment {..}=>"judgment",reading_contracts::ReadingResult::NeedsInformation {..}=>"needs_information",reading_contracts::ReadingResult::Limited {..}=>"limited"})}),
        )
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            if self.group.is_some() {
                return Err("Exploration group deadline reached; every peer shares this cancellation, not a semantic prompt failure".into());
            }
            return Err(
                "Synthetic case deadline reached; actual generation cancellation requested".into(),
            );
        }
        if self.sequence.load(Ordering::Acquire) >= self.max_calls {
            return Err(
                "Synthetic case call budget exhausted; no successful completion implied".into(),
            );
        }
        Ok(())
    }
    fn directory(&self) -> &Path {
        &self.dir
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const STYLE: &str = "body{font:17px/1.65 system-ui;background:#172128;color:#eae1cd;max-width:1000px;margin:60px auto;padding:0 24px}a{color:#c7b67e}h1,h2{font-family:Georgia,serif}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:13px/1.6 ui-monospace;background:#11191e;padding:20px;border-radius:8px}details{margin:18px 0;border-top:1px solid #435059;padding-top:16px}summary{cursor:pointer}table{border-collapse:collapse;width:100%}td,th{text-align:left;padding:10px;border-bottom:1px solid #435059}.pass{color:#abc59b}.fail{color:#e7a18b}.note{color:#b4bdbb}blockquote{border-left:2px solid #a29167;margin:18px 0;padding-left:20px}";

fn trace_html(
    case: &Case,
    first: &Grade,
    session: &Session,
    calls: &[Value],
    follow_up: &Value,
) -> String {
    let mut html = format!("<!doctype html><meta charset=utf-8><title>{}</title><style>{STYLE}</style><h1>{}</h1><p class=note>Synthetic authored words and device context. Actual inference outputs and native decisions; each call identifies its provider and decoding mode. Input grades do not qualify astrological judgment or microphone recognition.</p><h2>Conversation</h2>",escape(&case.id),escape(&case.id));
    for message in &session.messages {
        html.push_str(&format!(
            "<blockquote><small>{}</small><br>{}</blockquote>",
            escape(&message.role),
            escape(&message.text)
        ));
    }
    for (title, value) in [
        ("Expected inputs and author rationale", json!(case)),
        ("First-turn semantic grade", json!(first)),
        (
            "Four separate hurdles (native checks; interpretation review is separate)",
            json!(first.hurdles),
        ),
        ("Follow-up observation", follow_up.clone()),
        (
            "Final native record",
            json!({"session":session,"candidates":session.candidates}),
        ),
    ] {
        html.push_str(&format!(
            "<details><summary>{}</summary><pre>{}</pre></details>",
            escape(title),
            escape(&serde_json::to_string_pretty(&value).unwrap_or_default())
        ));
    }
    if !session.sections.is_empty() {
        html.push_str("<h2>The actual reading</h2>");
        for section in &session.sections {
            html.push_str(&format!(
                "<h3>{}</h3><p>{}</p>",
                escape(&section.title),
                escape(&section.body)
            ));
        }
    }
    html.push_str("<h2>Full model calls, including rejected attempts</h2>");
    for (i, call) in calls.iter().enumerate() {
        html.push_str(&format!(
            "<details><summary>Call {} · {}</summary><pre>{}</pre></details>",
            i + 1,
            escape(
                call["request"]["stage"]
                    .as_str()
                    .unwrap_or("independent batch")
            ),
            escape(&serde_json::to_string_pretty(call).unwrap_or_default())
        ));
    }
    html
}

fn save_report(dir: &Path, manifest: &Value, results: &[Value]) -> Result<(), String> {
    save_report_state(dir, manifest, results, &json!({"status":"running"}))
}

fn outcome_qualified(result: &Value) -> bool {
    result["first_turn_execution_completed"] == true
        && result["grade"]["semantic_pass"] == true
        && if result["follow_up_scripted"] == true {
            result["follow_up_pass"] == true
        } else {
            result["follow_up_pass"] != false
        }
        && (result["full_reading"] != true
            || result["hurdles"]["reading"]["status"] == "structure_pass_review_pending")
}

fn save_report_state(
    dir: &Path,
    manifest: &Value,
    results: &[Value],
    campaign_state: &Value,
) -> Result<(), String> {
    let failures = results
        .iter()
        .filter(|result| {
            result["first_turn_execution_completed"] == true
                && result["grade"]["semantic_pass"] != true
        })
        .count();
    let passes = results
        .iter()
        .filter(|result| {
            result["first_turn_execution_completed"] == true
                && result["grade"]["semantic_pass"] == true
        })
        .count();
    let interruptions = results
        .iter()
        .filter(|result| result["first_turn_execution_completed"] != true)
        .count();
    let journey_passes = results
        .iter()
        .filter(|result| result["follow_up_pass"] == true)
        .count();
    let journey_failures = results
        .iter()
        .filter(|result| {
            result["follow_up_execution_completed"] == true && result["follow_up_pass"] == false
        })
        .count();
    let campaign_failures = results
        .iter()
        .filter(|result| !outcome_qualified(result))
        .count();
    let withheld = results
        .iter()
        .filter(|result| result["follow_up_scripted"] == true && result["follow_up_pass"].is_null())
        .count();
    let follow_up_interruptions = results
        .iter()
        .filter(|result| result["follow_up_execution_completed"] == false)
        .count();
    let applied_calls: u64 = results
        .iter()
        .filter_map(|r| r["prompt_program_applied_calls"].as_u64())
        .sum();
    let hurdle_counts = ["classification", "elicitation", "extraction", "reading"]
        .iter()
        .map(|stage| {
            let mut counts = BTreeMap::<String, usize>::new();
            for result in results {
                let status = result["hurdles"][*stage]["status"]
                    .as_str()
                    .unwrap_or("not_recorded");
                *counts.entry(status.into()).or_default() += 1;
            }
            ((*stage).to_string(), counts)
        })
        .collect::<BTreeMap<_, _>>();
    let report = json!({"manifest":manifest,"campaign_state":campaign_state,"completed":results.len(),"semantic_passes":passes,
        "prompt_program_applied_calls":applied_calls,
        "semantic_failures":failures,"human_fluidity_review":"pending",
        "first_turn_execution_interruptions":interruptions,"follow_up_execution_interruptions":follow_up_interruptions,
        "follow_up_passes":journey_passes,"follow_up_failures":journey_failures,
        "scripted_followups_withheld":withheld,
        "follow_up_not_executed":results.len()-journey_passes-journey_failures-follow_up_interruptions,"campaign_failures":campaign_failures,
        "hurdle_counts":hurdle_counts,
        "reading_semantic_review":"separate immutable Codex source-rubric reviews; pending is not passing",
        "coverage_boundary":"Actual text pipeline. Each input hurdle is separate; native structure completion still needs source-rubric interpretation review. No speech or SME acceptance implied.","cases":results});
    write_index_json(&dir.join("report.json"), &report)?;
    let status = campaign_state["status"].as_str().unwrap_or("unqualified");
    let reason = campaign_state["reason"].as_str().unwrap_or("");
    let mut html = format!("<!doctype html><meta charset=utf-8><title>Horary scenario review</title><style>{STYLE}</style><h1>Horary scenario review</h1><p class=note>{} {}</p><p>{} completed · {passes} first-turn semantic passes · {failures} first-turn semantic failures · {interruptions} execution interruptions.</p><p>{journey_passes} follow-ups passed · {journey_failures} follow-ups failed · {follow_up_interruptions} follow-ups interrupted.</p><p class=note>Authored synthetic questions; actual model outputs. Conversational quality still needs human review. Every failed attempt is retained. These tests assess elicitation; no astrological answer is certified by a passing grade. First-turn passes are separate from follow-up passes; an unexecuted follow-up is not a successful journey. Batch4 uses unconstrained native generation and shared group cancellation; it is exploration, not production-decoder qualification. Interrupted cases remain unqualified and are counted separately from completed semantic failures.</p><table><thead><tr><th>Case</th><th>Method</th><th>Inputs</th><th>Result</th><th>Review flags</th></tr></thead><tbody>",escape(status),escape(reason),results.len());
    if manifest["full_reading"] == true {
        html.push_str("<tr><td colspan=5>The complete application reading procedure is selected. Open each trace for four separate hurdles, its provider and source rubric. A completed native procedure remains pending interpretation review.</td></tr>");
    }
    for result in results {
        let id = result["id"].as_str().unwrap_or("");
        let passed = outcome_qualified(result);
        let label = if result["first_turn_execution_completed"] != true
            || result["follow_up_execution_completed"] == false
        {
            "interrupted"
        } else if result["follow_up_scripted"] == true && result["follow_up_pass"].is_null() {
            "journey withheld"
        } else if passed {
            "pass"
        } else {
            "fail"
        };
        html.push_str(&format!("<tr><td><a href=\"cases/{}/trace.html\">{}</a></td><td>{}</td><td>{}</td><td class={}>{}</td><td>{}</td></tr>",escape(id),escape(id),escape(result["method"].as_str().unwrap_or("")),escape(result["mode"].as_str().unwrap_or("")),if passed {"pass"}else{"fail"},label,result["grade"]["fluidity_review_flags"].as_array().map_or(0,Vec::len)));
        if result["full_reading"] == true {
            html.push_str(&format!(
                "<tr><td colspan=5>Classify: {} · Elicit: {} · Extract: {} · Read: {}</td></tr>",
                escape(
                    result["hurdles"]["classification"]["status"]
                        .as_str()
                        .unwrap_or("not recorded")
                ),
                escape(
                    result["hurdles"]["elicitation"]["status"]
                        .as_str()
                        .unwrap_or("not recorded")
                ),
                escape(
                    result["hurdles"]["extraction"]["status"]
                        .as_str()
                        .unwrap_or("not recorded")
                ),
                escape(
                    result["hurdles"]["reading"]["status"]
                        .as_str()
                        .unwrap_or("not recorded")
                )
            ));
        }
    }
    html.push_str(&format!("</tbody></table><details><summary>Frozen campaign provenance</summary><pre>{}</pre></details>",escape(&serde_json::to_string_pretty(manifest).unwrap_or_default())));
    write_index_text(&dir.join("review.html"), &html)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn source_name(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|name| name.replace('\\', "/"))
        .ok_or_else(|| "A source fingerprint needs a UTF-8 repository path".into())
}

fn source_hashes(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut names: BTreeSet<String> = git(
        root,
        &[
            "ls-files",
            "src-tauri/src",
            "src-tauri/Cargo.toml",
            "src-tauri/Cargo.lock",
            "src-tauri/test-fixtures/elicitation",
            "src-tauri/test-fixtures/readings",
            "crates/horary-ai-core/src",
            "crates/horary-prompt-program/src",
            "crates/horary-prompt-program/Cargo.toml",
            "crates/horary-prompt-program/Cargo.lock",
            "src/data/cities.json",
            "src/data/us_locations.json",
            "docs/ELICITATION_EVALUATION.md",
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    // This opt-in entry is compiled even when a local ignore rule hides it.
    names.insert("src-tauri/src/reading_neural_eval.rs".into());
    // Include this untracked runner during development, as well as any new
    // production source. Ignored build artifacts are excluded by Git.
    names.extend(
        git(
            root,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "src-tauri/src",
                "src-tauri/test-fixtures/elicitation",
                "src-tauri/test-fixtures/readings",
                "crates/horary-ai-core/src",
                "crates/horary-prompt-program/src",
                "crates/horary-prompt-program/Cargo.toml",
                "crates/horary-prompt-program/Cargo.lock",
                "docs/ELICITATION_EVALUATION.md",
            ],
        )?
        .lines()
        .map(str::to_owned),
    );
    // A loaded optional bank is authoritative even if a local ignore rule
    // excludes it from Git's development-file listing.
    for bank in optional_fixture_banks(&root.join("src-tauri/test-fixtures/elicitation"))? {
        let relative = bank.strip_prefix(root).map_err(|error| error.to_string())?;
        names.insert(source_name(relative)?);
    }
    let rubric_directory = root.join("src-tauri/test-fixtures/readings");
    if rubric_directory.exists() {
        for bank in crate::reading_eval::banks(&rubric_directory)? {
            names.insert(source_name(
                bank.strip_prefix(root).map_err(|e| e.to_string())?,
            )?);
        }
    }
    names
        .into_iter()
        .map(|name| {
            let bytes = fs::read(root.join(&name)).map_err(|e| e.to_string())?;
            use sha2::{Digest, Sha256};
            Ok((name, format!("{:x}", Sha256::digest(bytes))))
        })
        .collect()
}

fn model_digest(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        sha.update(&buffer[..read]);
    }
    Ok(format!("{:x}", sha.finalize()))
}

#[test]
fn fixtures_cover_every_method_with_explicit_implicit_and_missing_inputs() {
    let cases = catalogue().expect("Reading fixtures must deserialize strictly");
    let mut ids = BTreeSet::new();
    for case in &cases {
        assert!(ids.insert(&case.id), "Duplicate case {}", case.id);
        assert!(
            !case.id.is_empty()
                && case.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'_')
        );
        assert!(!case.words.trim().is_empty() && case.words.len() <= 8000);
        assert!(!case.source_pages.trim().is_empty() && !case.rationale.trim().is_empty());
        assert!(
            case.expected.needs_alternatives.is_empty() || !case.expected.needs.is_empty(),
            "{} alternatives need a primary authored gap",
            case.id
        );
        assert!(
            case.expected
                .needs_alternatives
                .iter()
                .all(|needs| !needs.is_empty()),
            "{} has a vacuous gap alternative",
            case.id
        );
        assert!(
            case.expected
                .canonical_mentions
                .iter()
                .chain(&case.expected.subject_name_any_contains)
                .all(|s| !s.trim().is_empty()),
            "{} has a vacuous target assertion",
            case.id
        );
        for fact in &case.expected.facts {
            assert!(
                !fact.contains.is_empty(),
                "{} has a vacuous fact expectation",
                case.id
            );
        }
        if let Some(methods) = &case.expected.allowed_methods {
            assert!(!methods.is_empty());
        }
        if let Some(words) = &case.follow_up {
            assert!(
                !words.trim().is_empty(),
                "{} has an empty follow-up",
                case.id
            );
            let after = case
                .follow_up_expected
                .as_ref()
                .unwrap_or_else(|| panic!("{} needs an authored follow_up_expected", case.id));
            for fact in &after.facts {
                assert!(
                    !fact.contains.is_empty(),
                    "{} has a vacuous after-state fact",
                    case.id
                );
            }
            if let Some(methods) = &after.allowed_methods {
                assert!(
                    !methods.is_empty(),
                    "{} has no allowed after-state method",
                    case.id
                );
            }
        } else {
            assert!(
                case.follow_up_expected.is_none() && !case.follow_up_accepts_proposal,
                "{} has follow-up expectations without a scripted follow-up",
                case.id
            );
        }
    }
    for method in Method::ALL {
        for mode in [Mode::Explicit, Mode::Implicit, Mode::Missing] {
            assert!(
                cases
                    .iter()
                    .any(|case| case.method == *method && case.mode == mode),
                "{} lacks {mode:?}",
                method.name()
            );
        }
    }
}

#[test]
fn every_scenario_has_its_own_source_based_reading_rubric() {
    let rubrics = crate::reading_eval::catalogue(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/readings"),
    )
    .expect("Strict reading rubrics");
    validate_rubric_coverage(&catalogue().unwrap(), &rubrics).unwrap();
}

#[test]
fn full_reading_qualification_cannot_stop_at_a_successful_input_handoff() {
    let mut outcome = json!({"first_turn_execution_completed":true,"grade":{"semantic_pass":true},"follow_up_scripted":false,"follow_up_pass":null,"full_reading":true,"hurdles":{"reading":{"status":"blocked"}}});
    assert!(!outcome_qualified(&outcome));
    for status in ["not_run", "awaiting_information", "fail"] {
        outcome["hurdles"]["reading"]["status"] = json!(status);
        assert!(!outcome_qualified(&outcome));
    }
    outcome["hurdles"]["reading"]["status"] = json!("structure_pass_review_pending");
    assert!(
        outcome_qualified(&outcome),
        "Only native completion is established; the source-rubric judge is still mandatory"
    );
}

#[test]
fn semantic_grade_rejects_wrong_method_even_with_nonempty_reply() {
    let case: Case = serde_json::from_value(json!({"id":"semantic-regression","method":"lost_object","mode":"missing","words":"Where is my ring?",
        "expected":{"facet":"location","ready":false,"needs":[{"kind":"owner"}]},"source_pages":"146–153","rationale":"A readable reply cannot substitute for classifying the actual concern."})).unwrap();
    let mut session = Session::default();
    session.method.consultation = Some(reading_contracts::Consultation::default());
    session.messages.push(Message {
        role: "assistant".into(),
        text: "What would you like to know?".into(),
    });
    let result = grade(&case, &session, None);
    assert!(!result.semantic_pass);
    assert!(result
        .mismatches
        .iter()
        .any(|m| m.starts_with("Method must")));
    assert!(result.mismatches.iter().any(|m| m.contains("not tracked")));
}

fn missing_money_source_fixture() -> (Case, Session) {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "money-missing")
        .unwrap();
    let mut session = Session {
        question: case.words.clone(),
        place: Some(device()),
        candidate_moment_ms: Some(frozen_moment()),
        ..Default::default()
    };
    session.method.consultation = Some(reading_contracts::Consultation {
        question: resolved_fixture(case.words.clone()),
        frame: resolved_fixture(reading_contracts::Frame {
            method: Method::Money,
            facet: Facet::Event,
        }),
        subject: resolved_fixture(crate::horary_role_options::Subject {
            name: "money".into(),
            kind: "money".into(),
            owner_id: "querent".into(),
            source_quote: "the money I am waiting for".into(),
        }),
        facts: BTreeMap::from([(Field::PrincipalMode, resolved_fixture("self".into()))]),
        requested: Some(RequirementKey::Field(Field::MoneySource)),
        ..Default::default()
    });
    session.messages = vec![
        Message {
            role: "user".into(),
            text: case.words.clone(),
        },
        Message {
            role: "assistant".into(),
            text: "Where is the money coming from?".into(),
        },
    ];
    (case, session)
}

#[test]
fn a_blocked_handoff_cannot_mask_an_invented_missing_field_as_an_extraction_pass() {
    use crate::reading_eval::Status;
    let (case, mut session) = missing_money_source_fixture();
    session
        .method
        .consultation
        .as_mut()
        .unwrap()
        .facts
        .insert(Field::MoneySource, resolved_fixture("other".into()));
    let graded = grade(&case, &session, None);
    assert_eq!(graded.hurdles.classification.status, Status::Pass);
    assert_eq!(graded.hurdles.extraction.status, Status::Fail);
    assert_eq!(graded.actual["ready"], false);
    assert_eq!(
        graded.actual["plan"]["limitation"]["code"],
        "money_source_needs_review"
    );
    assert!(graded
        .hurdles
        .extraction
        .failures
        .iter()
        .any(|failure| failure.starts_with("Genuinely missing money_source")));
}

#[test]
fn unresolved_field_states_and_a_completed_follow_up_have_distinct_extraction_expectations() {
    use crate::reading_contracts::{Evidence, Observation, Slot};
    use crate::reading_eval::Status;
    let (case, session) = missing_money_source_fixture();
    for state in [
        Slot::Missing,
        Slot::Proposed {
            observation: Observation {
                value: "other".into(),
                evidence: Evidence::User {
                    turn: 1,
                    quote: "waiting for".into(),
                },
            },
        },
        Slot::Unavailable {
            reason: "I do not know the source".into(),
            evidence: Evidence::User {
                turn: 1,
                quote: "I do not know the source".into(),
            },
        },
    ] {
        let mut attempt = session.clone();
        attempt
            .method
            .consultation
            .as_mut()
            .unwrap()
            .facts
            .insert(Field::MoneySource, state);
        assert_eq!(
            grade(&case, &attempt, None).hurdles.extraction.status,
            Status::Pass,
            "An unresolved fact isn't an asserted source; elicitation is graded separately"
        );
    }
    let mut after_case = case.clone();
    after_case.expected = after_case.follow_up_expected.take().unwrap();
    let mut after = session;
    after
        .method
        .consultation
        .as_mut()
        .unwrap()
        .facts
        .insert(Field::MoneySource, resolved_fixture("customer".into()));
    after.method.consultation.as_mut().unwrap().requested = None;
    after.chart = Some(json!({"timestampMs":frozen_moment()}));
    let graded = grade(&after_case, &after, None);
    assert_eq!(
        graded.hurdles.extraction.status,
        Status::Pass,
        "Actual follow-up data must supersede the first-turn absence expectation: {:?}",
        graded.hurdles.extraction.failures
    );
}

fn resolved_fixture<T>(value: T) -> reading_contracts::Slot<T> {
    reading_contracts::Slot::Resolved {
        observation: reading_contracts::Observation {
            value,
            evidence: reading_contracts::Evidence::User {
                turn: 0,
                quote: "my knowledge".into(),
            },
        },
    }
}

fn scheduler_handoff_case(missing_place: bool) -> Case {
    serde_json::from_value(json!({"id":"scheduler-handoff-regression","method":"relationship",
        "mode":if missing_place {"missing"} else {"explicit"},
        "words":"Will I get married in the next year?","device_available":!missing_place,
        "follow_up":if missing_place {Some("Woodbridge, Virginia, United States.")} else {None},
        "expected":{"facet":"event","ready":!missing_place,
            "needs":if missing_place {json!([RequirementKey::ChartPlace])} else {json!([])},
            "chart_moment_ms":if missing_place {None} else {Some(1789387200000.)}},
        "follow_up_expected":if missing_place {Some(json!({"facet":"event","ready":true,"chart_moment_ms":1789387200000.}))} else {None},
        "source_pages":"Authored offline native-handoff integration probe",
        "rationale":"Actual scheduler and grader must agree on input completion without inventing a conversation or interpretation."})).unwrap()
}

#[test]
fn explicit_input_handoff_passes_native_grade_without_claiming_a_reply() {
    let fixture = crate::horary_pipeline::process_examples_at_boundary(true, true).unwrap();
    let session: Session = serde_json::from_value(fixture["accepted_session"].clone()).unwrap();
    let case = scheduler_handoff_case(false);
    let graded = grade_at_moment(&case, &session, None, 1789387200000.);
    assert!(graded.semantic_pass, "{:?}", graded.mismatches);
    assert_eq!(graded.actual["conversation_reply_observed"], false);
    assert_eq!(graded.actual["input_handoff_boundary"], true);
    assert_eq!(
        graded.hurdles.extraction.status,
        crate::reading_eval::Status::Pass
    );
    let mut stale = session.clone();
    stale.audit.last_mut().unwrap()["after_message"] = json!(0);
    assert!(!grade_at_moment(&case, &stale, None, 1789387200000.).semantic_pass);
    let mut mismatched = session;
    mismatched.audit.last_mut().unwrap()["binding"]["input_sha256"] = json!("0".repeat(64));
    assert!(!input_handoff_boundary(&mismatched));
}

#[test]
fn supplying_input_handoff_passes_native_grade_but_leaves_conversation_unobserved() {
    let fixture = crate::horary_pipeline::process_examples_at_boundary(false, true).unwrap();
    let previous: Session = serde_json::from_value(fixture["first_session"].clone()).unwrap();
    let after: Session = serde_json::from_value(fixture["accepted_session"].clone()).unwrap();
    let case = scheduler_handoff_case(true);
    let first_grade = grade_at_moment(&case, &previous, None, 1789387200000.);
    assert!(first_grade.semantic_pass, "{:?}", first_grade.mismatches);
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["pass"], true, "{}", result["failures"]);
    assert_eq!(result["input_complete"], true);
    assert_eq!(result["native_handoff_recorded"], true);
    assert_eq!(result["input_handoff_boundary"], true);
    assert_eq!(result["fresh_assistant_reply"], false);
    assert_eq!(result["conversation_complete"], false);
    assert_eq!(result["candidate_moment_preserved"], true);
    let mut absent = after;
    absent
        .audit
        .retain(|event| event["event"] != "input_evaluation_boundary");
    assert_eq!(
        follow_up_grade(
            &case,
            case.follow_up.as_deref().unwrap(),
            &previous,
            &absent,
            &Ok(())
        )["pass"],
        false
    );
}

fn follow_up_fixture() -> (Case, Session, Session) {
    let case: Case = serde_json::from_value(json!({"id":"follow-up-regression","method":"knowledge","mode":"missing",
        "words":"Is my knowledge sound?","follow_up":"I mean the quality of my knowledge, not earnings.",
        "expected":{"facet":"situation","needs":[{"kind":"field","id":"knowledge_task"}],"ready":false,"owner":"querent"},
        "follow_up_expected":{"allowed_methods":["knowledge"],"facet":"situation","ready":false,"owner":"querent",
            "facts":[{"field":"knowledge_task","contains":"quality"}]},
        "source_pages":"216–218","rationale":"Input completion differs from specialist judgment qualification."})).unwrap();
    let mut previous = Session::default();
    previous.candidates.push(device());
    previous.candidate_moment_ms = Some(frozen_moment());
    previous.question = case.words.clone();
    let consultation = reading_contracts::Consultation {
        question: resolved_fixture(case.words.clone()),
        frame: resolved_fixture(reading_contracts::Frame {
            method: Method::Knowledge,
            facet: Facet::Situation,
        }),
        subject: resolved_fixture(crate::horary_role_options::Subject {
            name: "my knowledge".into(),
            kind: "knowledge".into(),
            owner_id: "querent".into(),
            source_quote: "my knowledge".into(),
        }),
        requested: Some(RequirementKey::Field(Field::KnowledgeTask)),
        ..Default::default()
    };
    previous.method.consultation = Some(consultation);
    previous.messages = vec![
        Message {
            role: "user".into(),
            text: case.words.clone(),
        },
        Message {
            role: "assistant".into(),
            text: "Do you mean the knowledge itself, or earning from it?".into(),
        },
    ];
    let mut after = previous.clone();
    let consultation = after.method.consultation.as_mut().unwrap();
    consultation.requested = None;
    consultation
        .facts
        .insert(Field::KnowledgeTask, resolved_fixture("quality".into()));
    let limit = after
        .method
        .consultation
        .as_ref()
        .unwrap()
        .plan(current_anchor(&after).as_ref())
        .limitation
        .unwrap();
    after.method.result = Some(reading_contracts::ReadingResult::Limited {
        limitation: (&limit).into(),
    });
    after.messages.push(Message {
        role: "user".into(),
        text: case.follow_up.clone().unwrap(),
    });
    after.messages.push(Message {
        role: "assistant".into(),
        text: "The concern is the quality of your knowledge.".into(),
    });
    (case, previous, after)
}

#[test]
fn a_later_reading_failure_does_not_erase_successful_input_hurdles() {
    use crate::reading_eval::Status;
    let (mut case, _, session) = follow_up_fixture();
    case.expected = case.follow_up_expected.take().unwrap();
    let error = "The specialist did not complete";
    let graded = with_reading_grade(
        grade(&case, &session, Some(error)),
        &session,
        true,
        Some(error),
    );
    assert!(
        !graded.semantic_pass,
        "The legacy aggregate retains its error"
    );
    assert_eq!(graded.hurdles.classification.status, Status::Pass);
    assert_eq!(graded.hurdles.extraction.status, Status::Pass);
    assert_eq!(graded.hurdles.elicitation.status, Status::Pass);
    assert_eq!(graded.hurdles.reading.status, Status::Fail);
}

#[test]
fn follow_up_accepts_complete_specialist_inputs_without_claiming_a_judgment() {
    let (case, previous, after) = follow_up_fixture();
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["pass"], true, "{result}");
    assert_eq!(result["input_complete"], true);
    assert_eq!(result["after_state_grade"]["actual"]["ready"], false);
    assert_eq!(result["ready_binding"], Value::Null);
}

#[test]
fn follow_up_rejects_extra_needs_after_the_original_slot_was_filled() {
    let (case, previous, mut after) = follow_up_fixture();
    after.method.consultation.as_mut().unwrap().additional.push(
        reading_contracts::InformationNeed {
            key: RequirementKey::Field(Field::Context),
            reason: "Unexpected extra input request".into(),
            question: Some("And what else?".into()),
        },
    );
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["unresolved_original_needs"], json!([]));
    assert_eq!(result["pass"], false);
    assert_eq!(result["input_complete"], false);
    assert!(result["failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure
            .as_str()
            .unwrap()
            .contains("native contract input needs")));
}

#[test]
fn follow_up_requires_a_fresh_reply_and_authored_after_state() {
    let (mut case, previous, mut after) = follow_up_fixture();
    after.messages.pop();
    case.follow_up_expected = None;
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["pass"], false);
    assert_eq!(result["fresh_assistant_reply"], false);
    assert!(result["failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure
            .as_str()
            .unwrap()
            .contains("no authored after-state")));
}

#[test]
fn follow_up_rejects_wrong_after_route_even_if_it_has_no_input_gaps() {
    let (case, previous, mut after) = follow_up_fixture();
    after.method.consultation.as_mut().unwrap().frame =
        resolved_fixture(reading_contracts::Frame {
            method: Method::PersonDescription,
            facet: Facet::Description,
        });
    let limit = after
        .method
        .consultation
        .as_ref()
        .unwrap()
        .plan(current_anchor(&after).as_ref())
        .limitation
        .unwrap();
    after.method.result = Some(reading_contracts::ReadingResult::Limited {
        limitation: (&limit).into(),
    });
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["input_complete"], true);
    assert_eq!(result["pass"], false);
    assert!(result["failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure
            .as_str()
            .unwrap()
            .contains("After-state: Method must")));
}

#[test]
fn a_generic_i_mean_clarification_cannot_reset_the_question_or_candidate_moment() {
    let (case, previous, mut after) = follow_up_fixture();
    after.question = "A different undocumented concern".into();
    after.candidate_moment_ms = Some(frozen_moment() + 60_000.);
    let result = follow_up_grade(
        &case,
        case.follow_up.as_deref().unwrap(),
        &previous,
        &after,
        &Ok(()),
    );
    assert_eq!(result["pass"], false);
    assert_eq!(result["explicit_new_matter"], false);
    assert_eq!(result["sourced_chart_clock_correction"], false);
    let failures = result["failures"].as_array().unwrap();
    assert!(failures
        .iter()
        .any(|failure| failure.as_str().unwrap().contains("established question")));
    assert!(failures.iter().any(|failure| failure
        .as_str()
        .unwrap()
        .contains("candidate question moment")));
}

#[test]
fn proposal_acceptance_requires_one_question_an_empty_ask_and_a_first_turn_pass() {
    let mut case: Case = serde_json::from_value(json!({"id":"proposal-regression","method":"movable_deal","mode":"explicit",
        "words":"How many fish will I sell?","follow_up":"Yes.","follow_up_accepts_proposal":true,
        "expected":{"facet":"quantity","owner":"querent","ready":false},
        "follow_up_expected":{"facet":"event","owner":"querent","ready":true},
        "source_pages":"156–161","rationale":"A short affirmative only answers one concrete proposal."})).unwrap();
    let mut session = Session::default();
    session.candidates.push(device());
    session.question = case.words.clone();
    let mut consultation = reading_contracts::Consultation {
        question: resolved_fixture(case.words.clone()),
        frame: resolved_fixture(reading_contracts::Frame {
            method: Method::MovableDeal,
            facet: Facet::Quantity,
        }),
        subject: resolved_fixture(crate::horary_role_options::Subject {
            name: "fish".into(),
            kind: "movable".into(),
            owner_id: "querent".into(),
            source_quote: "my fish".into(),
        }),
        ..Default::default()
    };
    consultation
        .facts
        .insert(Field::DealCapacity, resolved_fixture("sell".into()));
    consultation
        .facts
        .insert(Field::Seller, resolved_fixture("querent".into()));
    session.method.consultation = Some(consultation);
    let limit = session
        .method
        .consultation
        .as_ref()
        .unwrap()
        .plan(current_anchor(&session).as_ref())
        .limitation
        .unwrap();
    session.method.result = Some(reading_contracts::ReadingResult::Limited {
        limitation: (&limit).into(),
    });
    session.messages = vec![
        Message {
            role: "user".into(),
            text: case.words.clone(),
        },
        Message {
            role: "assistant".into(),
            text: "Would you like to examine whether you will sell the fish?".into(),
        },
    ];
    session
        .audit
        .push(json!({"event":"conversation_reminder_selected","ask":""}));
    let mut first = grade(&case, &session, None);
    assert!(first.semantic_pass, "{:?}", first.mismatches);
    assert!(proposal_is_eligible(&case, &first, &session));
    session
        .messages
        .last_mut()
        .unwrap()
        .text
        .push_str(" Or its profit?");
    assert!(!proposal_is_eligible(&case, &first, &session));
    session.messages.last_mut().unwrap().text =
        "Would you like to examine whether you will sell the fish?".into();
    session.audit.last_mut().unwrap()["ask"] = json!("need_0");
    assert!(!proposal_is_eligible(&case, &first, &session));
    session.audit.last_mut().unwrap()["ask"] = json!("");
    first.semantic_pass = false;
    assert!(!proposal_is_eligible(&case, &first, &session));
    first.semantic_pass = true;
    case.follow_up_accepts_proposal = false;
    assert!(!proposal_is_eligible(&case, &first, &session));
}

#[test]
fn trace_renderer_escapes_every_authored_and_model_string() {
    assert_eq!(escape("<script>\"&'"), "&lt;script&gt;&quot;&amp;&#39;");
}

#[test]
fn actor_identity_uses_unique_person_labels_without_aliasing_the_querent() {
    let mut case = reading_contracts::Consultation::default();
    case.people.insert(
        "neighbor_pat".into(),
        crate::horary_role_options::Person {
            id: "neighbor_pat".into(),
            label: "Pat".into(),
            relationship: "neighbor".into(),
            source_quote: "my neighbor Pat".into(),
        },
    );
    assert!(same_actor(Some(&case), "neighbor_pat", "pat"));
    assert!(same_need(
        Some(&case),
        &RequirementKey::PersonRelationship("neighbor_pat".into()),
        &RequirementKey::PersonRelationship("pat".into())
    ));
    assert!(!same_actor(Some(&case), "neighbor_pat", "querent"));
    assert!(!same_actor(Some(&case), "neighbor_pat", "alex"));
    case.people.insert(
        "coworker_pat".into(),
        crate::horary_role_options::Person {
            id: "coworker_pat".into(),
            label: "Pat".into(),
            relationship: "employee".into(),
            source_quote: "my coworker Pat".into(),
        },
    );
    assert!(
        !same_actor(Some(&case), "neighbor_pat", "pat"),
        "Two named Pats require disambiguation rather than label-only identity"
    );
}

#[test]
fn fluidity_flags_unsourced_gender_but_respects_explicit_gender() {
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: "My cat Moss is missing.".into(),
    });
    assert!(fluidity_flags(
        &session,
        "I'm looking into the chart to see where he is.",
        &[]
    )
    .iter()
    .any(|flag| flag.contains("invented male")));
    session.messages.push(Message {
        role: "user".into(),
        text: "Moss is male.".into(),
    });
    assert!(!fluidity_flags(
        &session,
        "I'm looking into the chart to see where he is.",
        &[]
    )
    .iter()
    .any(|flag| flag.contains("invented male")));
}

#[test]
fn conversation_review_flags_chart_verdicts_without_checked_interpretation() {
    let session = Session::default();
    assert!(
        fluidity_flags(&session, "The chart shows that the sale will succeed.", &[])
            .iter()
            .any(|flag| flag.contains("unsupported chart interpretation"))
    );
    assert!(
        !fluidity_flags(&session, "Who is selling these books?", &[])
            .iter()
            .any(|flag| flag.contains("unsupported chart interpretation"))
    );
}

#[test]
fn semantic_grade_checks_subject_identity_and_kind_separately_from_owner() {
    let (mut case, _, mut session) = follow_up_fixture();
    case.expected = case.follow_up_expected.take().unwrap();
    case.expected.subject_kind = Some("other".into());
    case.expected.subject_name_contains = Some("knowledge".into());
    let native = session.method.consultation.as_mut().unwrap();
    if let reading_contracts::Slot::Resolved { observation } = &mut native.subject {
        observation.value.kind = "other".into();
    }
    let good = grade_at_moment(&case, &session, None, frozen_moment());
    assert!(
        !good.mismatches.iter().any(|m| m.starts_with("Subject")),
        "{good:?}"
    );
    let native = session.method.consultation.as_mut().unwrap();
    if let reading_contracts::Slot::Resolved { observation } = &mut native.subject {
        observation.value.kind = "person".into();
        observation.value.name = "someone else".into();
    }
    let wrong = grade_at_moment(&case, &session, None, frozen_moment());
    assert!(wrong
        .mismatches
        .iter()
        .any(|m| m.starts_with("Subject kind")));
    assert!(wrong
        .mismatches
        .iter()
        .any(|m| m.starts_with("Subject name")));
}

#[test]
fn report_keeps_cancelled_backend_work_separate_from_completed_semantic_failure() {
    let dir = tempfile::tempdir().unwrap();
    let results = [
        json!({"id":"cancelled","method":"relationship","mode":"explicit","first_turn_execution_completed":false,
        "grade":{"semantic_pass":false,"fluidity_review_flags":[]},"follow_up_pass":null,"follow_up_execution_completed":null}),
        json!({"id":"wrong-method","method":"relationship","mode":"explicit","first_turn_execution_completed":true,
        "grade":{"semantic_pass":false,"fluidity_review_flags":[]},"follow_up_pass":null,"follow_up_execution_completed":null}),
    ];
    save_report(
        dir.path(),
        &json!({"decoder":"authored report-aggregation test; no model invoked"}),
        &results,
    )
    .unwrap();
    let report: Value =
        serde_json::from_slice(&fs::read(dir.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(report["semantic_failures"], 1);
    assert_eq!(report["first_turn_execution_interruptions"], 1);
    assert_eq!(
        report["campaign_failures"], 2,
        "Cancelled cases remain unqualified"
    );
}

#[test]
fn withheld_scripted_journey_cannot_qualify_a_passing_first_turn() {
    let dir = tempfile::tempdir().unwrap();
    let result = json!({"id":"withheld","method":"relationship","mode":"missing",
        "first_turn_execution_completed":true,"grade":{"semantic_pass":true,"fluidity_review_flags":[]},
        "follow_up_scripted":true,"follow_up_pass":null,"follow_up_execution_completed":null});
    save_report(dir.path(), &json!({"selected_count":1}), &[result]).unwrap();
    let report: Value =
        serde_json::from_slice(&fs::read(dir.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(
        report["semantic_passes"], 1,
        "The first-turn grade must remain intact"
    );
    assert_eq!(report["semantic_failures"], 0);
    assert_eq!(report["follow_up_passes"], 0);
    assert_eq!(report["scripted_followups_withheld"], 1);
    assert_eq!(
        report["campaign_failures"], 1,
        "An unexecuted authored journey remains unqualified"
    );
}

#[test]
fn evidence_receipts_are_never_replaced_and_failed_writes_remain_visible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    write_new(&path, &json!({"attempt":1})).unwrap();
    let error = write_new(&path, &json!({"attempt":2})).unwrap_err();
    assert!(error.contains("Evidence I/O failure") && error.contains("Partial evidence retained"));
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(original["attempt"], 1);
    let partial = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("evidence-partial-")
        })
        .expect("The failed attempt must not disappear");
    let rejected: Value = serde_json::from_slice(&fs::read(partial).unwrap()).unwrap();
    assert_eq!(rejected["attempt"], 2);
    let interrupted = dir.path().join("not-complete.json");
    let error = atomic_evidence_file(&interrupted, true, |file| {
        file.write_all(b"partial raw bytes")
            .map_err(|error| error.to_string())?;
        Err("simulated storage interruption".into())
    })
    .unwrap_err();
    assert!(error.contains("simulated storage interruption"));
    assert!(
        !interrupted.exists(),
        "A partial write must not masquerade as a complete receipt"
    );
}

#[test]
fn campaign_infrastructure_receipt_closes_index_without_regrading_completed_cases() {
    let dir = tempfile::tempdir().unwrap();
    let progress = CampaignProgress {
        manifest: Some(
            json!({"selected_count":2,"decoder":"authored infrastructure regression; no model"}),
        ),
        selected_count: 2,
        results: vec![
            json!({"id":"completed","method":"relationship","mode":"explicit",
            "first_turn_execution_completed":true,"grade":{"semantic_pass":true,"fluidity_review_flags":[]},
            "follow_up_scripted":false,"follow_up_pass":null,"follow_up_execution_completed":null}),
        ],
        active_case_ids: vec!["unwritten".into()],
    };
    let error = close_interrupted_campaign(dir.path(), &progress, "No space left on device");
    assert!(error.contains("Campaign infrastructure interrupted"));
    let receipt: Value =
        serde_json::from_slice(&fs::read(dir.path().join("interrupted.json")).unwrap()).unwrap();
    assert_eq!(receipt["completed_cases"], 1);
    assert_eq!(receipt["selected_cases"], 2);
    assert_eq!(
        receipt["active_or_unfinished_case_ids"],
        json!(["unwritten"])
    );
    let report: Value =
        serde_json::from_slice(&fs::read(dir.path().join("report.json")).unwrap()).unwrap();
    assert_eq!(
        report["campaign_state"]["status"],
        "infrastructure_interrupted"
    );
    assert_eq!(report["semantic_passes"], 1);
    assert_eq!(report["semantic_failures"], 0);
    assert!(!dir.path().join("completed.json").exists());
    assert!(fs::read_to_string(dir.path().join("review.html"))
        .unwrap()
        .contains("No space left on device"));
}

#[test]
fn retained_target_grade_excludes_words_only_in_the_question() {
    let (mut case, _, mut session) = follow_up_fixture();
    case.expected.canonical_mentions = vec!["Riverton".into()];
    session.method.consultation.as_mut().unwrap().question = reading_contracts::Slot::Resolved {
        observation: reading_contracts::Observation {
            value: "What will happen in Riverton?".into(),
            evidence: reading_contracts::Evidence::Migration {
                detail: "Authored question-only regression".into(),
            },
        },
    };
    assert!(grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Canonical facts")));
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::Context,
        reading_contracts::Slot::Resolved {
            observation: reading_contracts::Observation {
                value: "Riverton".into(),
                evidence: reading_contracts::Evidence::Migration {
                    detail: "Authored retained context regression".into(),
                },
            },
        },
    );
    assert!(!grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Canonical facts")));
}

#[test]
fn repeated_civil_time_grade_requires_the_correct_utc_occurrence() {
    let (mut case, _, mut session) = follow_up_fixture();
    case.expected.chart_local_time = Some("2025-11-02T01:30".into());
    case.expected.chart_moment_ms = Some(1762065000000.);
    session.place = Some(device());
    session.chart = Some(json!({"timestampMs":1762061400000.}));
    let wrong = grade(&case, &session, None);
    assert!(
        !wrong
            .mismatches
            .iter()
            .any(|m| m.starts_with("Chart civil")),
        "Both instants share this civil time"
    );
    assert!(wrong.mismatches.iter().any(|m| m.starts_with("Chart UTC")));
    session.chart = Some(json!({"timestampMs":1762065000000.}));
    assert!(!grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Chart UTC")));
}

#[test]
fn a_narrow_clock_occurrence_inquiry_satisfies_an_authored_anchor_alternative() {
    let (mut case, _, mut session) = follow_up_fixture();
    case.expected.needs = vec![RequirementKey::ChartMoment];
    case.expected.needs_alternatives = vec![vec![RequirementKey::Field(Field::TimeOccurrence)]];
    let native = session.method.consultation.as_mut().unwrap();
    native.require_information(
        RequirementKey::Field(Field::TimeOccurrence),
        "The civil hour occurred twice".into(),
    );
    native.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
    let alternate = grade(&case, &session, None);
    assert!(
        !alternate
            .mismatches
            .iter()
            .any(|m| m.starts_with("Necessary information")
                || m.starts_with("The reader must elicit"))
    );
    case.expected.needs_alternatives.clear();
    assert!(grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Necessary information")));
}

#[test]
fn scripted_clock_answer_runs_for_an_authored_narrow_gap_but_not_unrelated_needs() {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "anchor-clock-fold")
        .expect("The authored repeated-hour journey must remain in the bank");
    assert!(case.follow_up.is_some() && case.follow_up_expected.is_some());
    let mut session = Session {
        method: crate::horary_pipeline::MethodState {
            consultation: Some(reading_contracts::Consultation::default()),
            ..Default::default()
        },
        ..Default::default()
    };
    let consultation = session.method.consultation.as_mut().unwrap();
    consultation.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
    assert!(scripted_need_is_eligible(&case, &session));
    let mut without_alternative = case.clone();
    without_alternative.expected.needs_alternatives.clear();
    assert!(!scripted_need_is_eligible(&without_alternative, &session));
    session.method.consultation.as_mut().unwrap().requested = Some(RequirementKey::ChartPlace);
    assert!(!scripted_need_is_eligible(&case, &session));
    session.method.consultation.as_mut().unwrap().requested = Some(RequirementKey::ChartMoment);
    assert!(scripted_need_is_eligible(&case, &session));
    session.method.consultation.as_mut().unwrap().requested = None;
    assert!(!scripted_need_is_eligible(&case, &session));
}

#[test]
fn actor_fact_grade_resolves_the_participant_instead_of_matching_id_spelling() {
    let (mut case, _, mut session) = follow_up_fixture();
    case.expected.facts = vec![ExpectedFact {
        field: Field::Seller,
        contains: "Bob".into(),
        place_qualifier_any: vec![],
    }];
    let native = session.method.consultation.as_mut().unwrap();
    native.people.insert(
        "p1".into(),
        crate::horary_role_options::Person {
            id: "p1".into(),
            label: "Bob".into(),
            relationship: "friend".into(),
            source_quote: "my friend Bob".into(),
        },
    );
    native.facts.insert(
        Field::Seller,
        reading_contracts::Slot::Resolved {
            observation: reading_contracts::Observation {
                value: "p1".into(),
                evidence: reading_contracts::Evidence::Migration {
                    detail: "Authored actor-ID alias regression".into(),
                },
            },
        },
    );
    assert!(!grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Resolved seller")));
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::Seller,
        reading_contracts::Slot::Resolved {
            observation: reading_contracts::Observation {
                value: "bob_someone_else".into(),
                evidence: reading_contracts::Evidence::Migration {
                    detail: "Authored false substring regression".into(),
                },
            },
        },
    );
    assert!(grade(&case, &session, None)
        .mismatches
        .iter()
        .any(|m| m.starts_with("Resolved seller")));
}

#[test]
#[ignore = "Real Gemma catalogue campaign: fresh HORARY_EVAL_EVIDENCE plus an explicitly configured native or hosted provider"]
fn real_model_catalogue_campaign() -> Result<(), String> {
    run_campaign()
}

fn evaluate_case(
    state: &NativeLlamaState,
    dir: &Path,
    case: Case,
    config: CaseRun,
) -> Result<Value, String> {
    use crate::native_llama_worker::native_llama_health;
    let full_reading = config.full_reading;
    let seconds = config.seconds;
    let max_calls = config.max_calls;
    let case_dir = dir.join("cases").join(&case.id);
    fs::create_dir(&case_dir).map_err(|e| e.to_string())?;
    fs::create_dir(case_dir.join("calls")).map_err(|e| e.to_string())?;
    fs::create_dir(case_dir.join("checkpoints")).map_err(|e| e.to_string())?;
    write_new(&case_dir.join("fixture.json"), &case)?;
    if let Some(rubrics) = &config.rubrics {
        let rubric = rubrics
            .get(&case.id)
            .ok_or_else(|| format!("Missing reading rubric {}", case.id))?;
        write_new(&case_dir.join("reading-rubric.json"), rubric)?;
    }
    let cancelled = config
        .shared_cancelled
        .clone()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let deadline = config
        .shared_cancelled
        .is_none()
        .then(|| Deadline::start(seconds, cancelled.clone()));
    let reader = Reader {
        state,
        hosted: config.hosted.clone(),
        dir: case_dir.clone(),
        cancelled,
        calls: Mutex::new(Vec::new()),
        sequence: AtomicU64::new(0),
        checkpoints: AtomicU64::new(0),
        max_calls,
        device_available: case.device_available,
        device_calls: AtomicU64::new(0),
        dispatcher: config.dispatcher.clone(),
        group: config.group,
        program: config.program.clone(),
    };
    let mut session = Session::default();
    if case.device_available {
        session.candidates.push(device());
    }
    session.device_context = Some(crate::conversation::DeviceContext {
        timezone: "America/New_York".into(),
        locale: "en-US".into(),
        latitude: case.device_available.then_some(38.657),
        longitude: case.device_available.then_some(-77.249),
        accuracy_meters: case.device_available.then_some(30.),
    });
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: case.words.clone(),
    });
    write_new(
        &case_dir.join("initial.json"),
        &json!({"session":session,"candidates":session.candidates}),
    )?;
    let geocode = GeocodeState::default();
    let started = Instant::now();
    let execute = |session: &mut Session, instant: f64, previous: &Session| {
        if full_reading {
            crate::horary_pipeline::run(session, &reader, &geocode, instant, None, previous)
        } else {
            crate::horary_pipeline::run_elicitation(
                session, &reader, &geocode, instant, None, previous,
            )
        }
    };
    let result = execute(&mut session, frozen_moment(), &previous);
    let mut execution_error = result.as_ref().err().cloned();
    if let Err(error) = &result {
        session.method.flow.pause(error.clone());
        note_case_error(&case.id, "first_turn", error);
    }
    let mut evidence_error = result
        .as_ref()
        .err()
        .filter(|error| error.contains("Evidence I/O failure"))
        .cloned();
    let first = with_reading_grade(
        grade(&case, &session, result.as_ref().err().map(String::as_str)),
        &session,
        full_reading,
        result.as_ref().err().map(String::as_str),
    );
    let mut final_hurdles = first.hurdles.clone();
    let first_turn_execution_completed = result.is_ok();
    write_new(
        &case_dir.join("first-turn.json"),
        &json!({"result":result,"grade":first,"hurdles":first.hurdles,"session":session,"candidates":session.candidates}),
    ).map_err(|error| retain_execution_error(error, result.as_ref().err().map(String::as_str)))?;
    let mut follow_up = json!({"status":"not scripted"});
    let mut follow_up_pass: Option<bool> = None;
    let mut follow_up_execution_completed: Option<bool> = None;
    if let Some(words) = &case.follow_up {
        let bound_need = scripted_need_is_eligible(&case, &session);
        let proposal = proposal_is_eligible(&case, &first, &session);
        if result.is_ok() && (bound_need || proposal) {
            let previous = session.clone();
            session.messages.push(Message {
                role: "user".into(),
                text: words.clone(),
            });
            let next = execute(&mut session, frozen_moment() + 60_000., &previous);
            if let Err(error) = &next {
                execution_error = Some(error.clone());
                session.method.flow.pause(error.clone());
                note_case_error(&case.id, "follow_up", error);
            }
            if let Some(error) = next
                .as_ref()
                .err()
                .filter(|error| error.contains("Evidence I/O failure"))
            {
                evidence_error = Some(error.clone());
            }
            follow_up_execution_completed = Some(next.is_ok());
            let next_grade = follow_up_grade(&case, words, &previous, &session, &next);
            if let Some(expected) = &case.follow_up_expected {
                let mut after_case = case.clone();
                after_case.expected = expected.clone();
                let after = with_reading_grade(
                    grade_at_moment(
                        &after_case,
                        &session,
                        next.as_ref().err().map(String::as_str),
                        previous
                            .candidate_moment_ms
                            .unwrap_or_else(|| frozen_moment() + 60_000.),
                    ),
                    &session,
                    full_reading,
                    next.as_ref().err().map(String::as_str),
                );
                final_hurdles = after.hurdles;
            }
            follow_up_pass = next_grade["pass"].as_bool();
            follow_up = json!({"status":if proposal {"executed after one eligible authored proposal"} else {"executed after matching elicitation"},"words":words,"result":next,
                "consultation":session.method.consultation,"native_result":session.method.result,
                "chart":session.chart,"first_turn_grade_retained":true,"grade":next_grade,"hurdles":final_hurdles});
        } else {
            follow_up = json!({"status":"withheld because the intended fact or single eligible proposal was not correctly elicited","words":words,
                "bound_need_matched":bound_need,"proposal_eligible":proposal});
        }
    }
    drop(deadline);
    let calls = reader
        .calls
        .into_inner()
        .map_err(|e| retain_execution_error(e.to_string(), execution_error.as_deref()))?;
    write_new(
        &case_dir.join("final.json"),
        &json!({"session":session,"candidates":session.candidates,"follow_up":follow_up,"hurdles":final_hurdles}),
    ).map_err(|error| retain_execution_error(error, execution_error.as_deref()))?;
    write_index_text(
        &case_dir.join("trace.html"),
        &trace_html(&case, &first, &session, &calls, &follow_up),
    )
    .map_err(|error| retain_execution_error(error, execution_error.as_deref()))?;
    let repairs = session
        .method
        .records
        .iter()
        .filter(|record| record.validation_error.is_some())
        .count();
    let applied_calls = calls
        .iter()
        .filter(|call| {
            let program = &call["request"]["prompt_program"];
            program.is_object()
                || program
                    .as_array()
                    .is_some_and(|items| items.iter().any(Value::is_object))
        })
        .count();
    let provider_attempts = calls
        .iter()
        .flat_map(|call| {
            if call["provider_receipt"].is_object() {
                vec![&call["provider_receipt"]]
            } else {
                call["provider_receipts"]
                    .as_array()
                    .map(|items| items.iter().collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>();
    let provider_stop = provider_attempts.iter().find_map(|attempt| {
        let status=attempt["response"]["http_status"].as_u64()?;
        [401,403,404,429].contains(&status).then(||format!("Hosted provider HTTP {status}; stop the campaign before repeating authentication, availability or quota failures"))
    });
    let outcome = json!({"id":case.id,"method":case.method,"mode":case.mode,"grade":first,
        "full_reading":full_reading,"hurdles":final_hurdles,"reading_semantic_review":"pending; native structure does not certify the textbook interpretation",
        "prompt_program_applied_calls":applied_calls,
        "elapsed_ms":started.elapsed().as_millis(),"model_calls":calls.len(),"rejected_attempts":repairs,
        "hosted_http_requests":provider_attempts.iter().map(|attempt| {
            attempt["generation_attempts"].as_array().map(|items|items.iter().filter(|item|item["submitted"]==true).count())
                .unwrap_or_else(||usize::from(attempt["submitted"]==true))
        }).sum::<usize>(),
        "provider_stop":provider_stop,
        "deadline_cancelled":reader.cancelled.load(Ordering::Acquire),"follow_up":follow_up,
        "follow_up_pass":follow_up_pass,"follow_up_scripted":case.follow_up.is_some(),
        "first_turn_execution_completed":first_turn_execution_completed,
        "follow_up_execution_completed":follow_up_execution_completed,
        "exploration_group":config.group,
        "decoder_mode":if config.hosted.is_some(){"hosted_unconstrained_text"}else if config.dispatcher.is_some(){"exploration_unconstrained_batch"}else{"production_constrained_single"},
        "group_cancelled":config.group.is_some() && reader.cancelled.load(Ordering::Acquire),
        "infrastructure_error":evidence_error,
        "execution_status":if evidence_error.is_some(){"evidence_io_interrupted"}
            else if config.group.is_some() && reader.cancelled.load(Ordering::Acquire){"exploration_group_interrupted"}
            else if reader.cancelled.load(Ordering::Acquire){"deadline_cancelled"}
            else if reader.sequence.load(Ordering::Acquire)>=max_calls && result.is_err(){"call_budget_exhausted"}
            else if result.is_err(){"execution_error"}else{"completed"},
        "health":if let Some(hosted)=&config.hosted {hosted.metadata()}else{json!(native_llama_health(state).map_err(|e|e.message))},"trace":format!("cases/{}/trace.html",case.id)});
    write_new(&case_dir.join("outcome.json"), &outcome)
        .map_err(|error| retain_execution_error(error, execution_error.as_deref()))?;
    println!(
        "{}",
        json!({"event":"case_completed","case_id":case.id,"first_turn_execution_completed":first_turn_execution_completed,"hurdles":final_hurdles,"model_calls":calls.len(),"rejected_attempts":repairs,"elapsed_ms":started.elapsed().as_millis()})
    );
    Ok(outcome)
}

fn retain_execution_error(recording_error: String, original: Option<&str>) -> String {
    match original {
        Some(original) => format!(
            "Original execution failure: {original}. Further recording failure: {recording_error}"
        ),
        None => recording_error,
    }
}

fn note_case_error(case_id: &str, phase: &str, error: &str) {
    // Logging itself may fail on the same full filesystem. Never turn that
    // secondary failure into a panic which discards the original execution error.
    let _ = writeln!(
        std::io::stderr().lock(),
        "{}",
        json!({"event":"case_execution_error","case_id":case_id,"phase":phase,"error":error})
    );
}

fn recorded_group(errors: Vec<String>, recording: Result<(), String>) -> Result<(), String> {
    let mut errors = errors;
    if let Err(error) = recording {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[test]
fn an_unsent_repair_write_and_later_index_failure_preserve_the_original_error() {
    let directory = tempfile::tempdir().unwrap();
    let response = json!({"result":{"Ok":{"content":"{\"owner_id\":null}"}}});
    let received = directory.path().join("0005-result.json");
    write_new(&received, &response).unwrap();
    let before = fs::read(&received).unwrap();
    let request = directory.path().join("0006-request.json");
    let original = atomic_evidence_file(&request, true, |file| {
        file.write_all(b"{\"sequence\":6,")
            .map_err(|e| e.to_string())?;
        Err("injected ENOSPC while storing the next repair before submission".into())
    })
    .unwrap_err();
    assert!(!request.exists());
    assert!(original.contains("Partial evidence retained"));
    let joined = retain_execution_error(
        "first-turn receipt could not be saved".into(),
        Some(&original),
    );
    let stopped =
        recorded_group(vec![joined], Err("report.json could not be saved".into())).unwrap_err();
    for detail in [
        "0006-request.json",
        "injected ENOSPC",
        "first-turn receipt",
        "report.json",
    ] {
        assert!(
            stopped.contains(detail),
            "Original and subsequent recording errors must remain: {stopped}"
        );
    }
    assert_eq!(fs::read(&received).unwrap(), before);
    assert!(
        recorded_group(Vec::new(), Ok(())).is_ok(),
        "A recorded semantic failure does not stop the next independent scenario"
    );
}

#[derive(Default)]
struct CampaignProgress {
    manifest: Option<Value>,
    selected_count: usize,
    results: Vec<Value>,
    active_case_ids: Vec<String>,
}

fn close_interrupted_campaign(dir: &Path, progress: &CampaignProgress, error: &str) -> String {
    let interrupted = json!({"status":"infrastructure_interrupted","reason":error,
        "completed_cases":progress.results.len(),"selected_cases":progress.selected_count,
        "active_or_unfinished_case_ids":progress.active_case_ids,
        "boundary":"Campaign collection/setup stopped. Completed case semantic grades are unchanged; absent outcomes do not certify model failure or success."});
    let mut recording_errors = Vec::new();
    if let Err(error) = write_new(&dir.join("interrupted.json"), &interrupted) {
        recording_errors.push(error);
    }
    let setup_manifest = json!({"version":EVALUATOR_VERSION,"setup_completed":false});
    if let Err(error) = save_report_state(
        dir,
        progress.manifest.as_ref().unwrap_or(&setup_manifest),
        &progress.results,
        &interrupted,
    ) {
        recording_errors.push(error);
    }
    let recording = if recording_errors.is_empty() {
        format!(
            "Interruption receipt and index preserved at {}",
            dir.display()
        )
    } else {
        format!(
            "Further evidence recording also failed: {}",
            recording_errors.join("; ")
        )
    };
    format!("Campaign infrastructure interrupted: {error}. {recording}")
}

fn run_campaign() -> Result<(), String> {
    let dir = PathBuf::from(
        std::env::var_os("HORARY_EVAL_EVIDENCE")
            .ok_or("Set a fresh HORARY_EVAL_EVIDENCE directory")?,
    );
    // Never reuse an earlier campaign directory, even on startup failure.
    fs::create_dir(&dir).map_err(|e| {
        format!(
            "Campaign setup blocked; keep previous evidence at {}: {e}",
            dir.display()
        )
    })?;
    let mut progress = CampaignProgress::default();
    match run_campaign_in(&dir, &mut progress) {
        Ok(0) => Ok(()),
        Ok(failed) => Err(format!(
            "{failed}/{} completed cases did not qualify first-turn or scripted-journey checks; full traces preserved at {}",
            progress.results.len(),dir.display()
        )),
        Err(error) => Err(close_interrupted_campaign(&dir, &progress, &error)),
    }
}

#[cfg(not(feature = "native-llama"))]
fn run_campaign_in(_dir: &Path, _progress: &mut CampaignProgress) -> Result<usize, String> {
    Err("A real-model catalogue campaign requires the native-llama feature. Ordinary fixture and contract tests can run without it; enable native-llama to launch the model campaign.".into())
}

#[cfg(not(feature = "native-llama"))]
#[test]
fn a_model_campaign_without_native_capability_reports_the_boundary_without_touching_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut progress = CampaignProgress::default();
    let error = run_campaign_in(dir.path(), &mut progress).unwrap_err();
    assert!(error.contains("requires the native-llama feature"));
    assert!(progress.results.is_empty());
    assert!(progress.manifest.is_none());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(feature = "native-llama")]
fn run_campaign_in(dir: &Path, progress: &mut CampaignProgress) -> Result<usize, String> {
    use crate::native_llama_worker::{start_native_llama_from_path, stop_native_llama};
    let provider = std::env::var("HORARY_EVAL_PROVIDER").unwrap_or_else(|_| "native".into());
    if !["native", "google"].contains(&provider.as_str()) {
        return Err("HORARY_EVAL_PROVIDER must be native or google".into());
    }
    let hosted = if provider == "google" {
        let keyfile = PathBuf::from(
            std::env::var_os("HORARY_GOOGLE_KEY_FILE")
                .ok_or("Set a private HORARY_GOOGLE_KEY_FILE outside the repository")?,
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("Repository root unavailable")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if keyfile
            .canonicalize()
            .map_err(|_| "Hosted credential unavailable")?
            .starts_with(root)
        {
            return Err("Keep the hosted credential outside the repository".into());
        }
        Some(Arc::new(crate::hosted_gemma_eval::Client::from_file(
            &keyfile, 4,
        )?))
    } else {
        None
    };
    let batch_size = std::env::var("HORARY_EVAL_BATCH")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    if !matches!(batch_size, 1 | 4) {
        return Err("HORARY_EVAL_BATCH must be 1 (exact production decoding) or 4 (unconstrained exploration)".into());
    }
    let full_reading = std::env::var("HORARY_EVAL_FULL").as_deref() == Ok("1");
    if batch_size == 4 && full_reading && hosted.is_none() {
        return Err("HORARY_EVAL_BATCH=4 is an elicitation exploration; HORARY_EVAL_FULL requires exact production batch=1".into());
    }
    let model = if hosted.is_none() {
        Some(PathBuf::from(
            std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL")
                .ok_or("Set HORARY_NATIVE_LLAMA_TEST_MODEL")?,
        ))
    } else {
        None
    };
    fs::create_dir(dir.join("cases")).map_err(|e| e.to_string())?;
    let all_cases = catalogue()?;
    let rubrics = if full_reading {
        let rubrics = crate::reading_eval::catalogue(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/readings"),
        )?;
        validate_rubric_coverage(&all_cases, &rubrics)?;
        Some(Arc::new(rubrics))
    } else {
        None
    };
    let filter = std::env::var("HORARY_EVAL_FILTER").unwrap_or_default();
    let filters: Vec<_> = filter
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let modes = std::env::var("HORARY_EVAL_MODES").unwrap_or_default();
    let modes: Vec<Mode> = modes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|mode| match mode {
            "explicit" => Ok(Mode::Explicit),
            "implicit" => Ok(Mode::Implicit),
            "missing" => Ok(Mode::Missing),
            _ => Err(format!("Unknown case mode {mode}")),
        })
        .collect::<Result<_, _>>()?;
    let cases: Vec<_> = all_cases
        .iter()
        .filter(|case| {
            case_selected(case, &filters) && (modes.is_empty() || modes.contains(&case.mode))
        })
        .cloned()
        .collect();
    if cases.is_empty() {
        return Err("The filter selects no cases".into());
    }
    progress.selected_count = cases.len();
    let seconds = std::env::var("HORARY_EVAL_CASE_SECONDS")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(if full_reading { 300 } else { 90 });
    if seconds == 0 {
        return Err("Case deadline must be positive".into());
    }
    let max_calls = std::env::var("HORARY_EVAL_MAX_CALLS")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(if full_reading { 28 } else { 12 });
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Repository root unavailable")?;
    let sources = source_hashes(root)?;
    // Read a candidate once, preserve its exact bytes, and never reread mutable
    // teaching between cases. This hook exists only in the opt-in test runner.
    let (program, program_manifest) = if let Some(path) = std::env::var_os("HORARY_EVAL_PROGRAM") {
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let program = horary_prompt_program::Program::parse(&bytes)?;
        atomic_evidence_file(&dir.join("prompt-program.json"), true, |file| {
            file.write_all(&bytes).map_err(|e| e.to_string())
        })?;
        let receipt = json!({"id":program.id,"sha256":horary_prompt_program::digest(&bytes),
            "file":"prompt-program.json","baseline_manifest_sha256":program.baseline_manifest_sha256,
            "overrides":program.overrides.iter().map(|o|o.selector()).collect::<Vec<_>>()});
        (Some(Arc::new(program)), receipt)
    } else {
        (None, Value::Null)
    };
    let prompt_experiment = experiment_origin(&program_manifest)?;
    let model_manifest = if let Some(hosted) = &hosted {
        hosted.metadata()
    } else {
        let model = model.as_ref().ok_or("Native model path missing")?;
        let metadata = fs::metadata(model).map_err(|e| e.to_string())?;
        json!({"provider":"native_llama","path":model,"bytes":metadata.len(),"sha256":model_digest(model)?})
    };
    let manifest = json!({"version":EVALUATOR_VERSION,"authorship":"All case words, expectations and device data are authored synthetic fixtures; actual Gemma outputs are separately traced",
        "campaign_phase":std::env::var("HORARY_EVAL_PHASE").unwrap_or_else(|_|"exploration".into()),
        "prompt_program":program_manifest,
        "prompt_experiment":prompt_experiment,
        "holdout_status":"No blind qualification implied; selected authored cases are visible during prompt development",
        "repository_sha":git(root,&["rev-parse","HEAD"] )?.trim(),
        "working_diff_sha256":horary_lessons::digest(&git(root,&["diff","--binary","HEAD"])?),
        "sources":sources,"catalogue_version":reading_contracts::VERSION,
        "book_ocr_sha256":horary_lessons::BOOK_OCR_SHA256,
        "model":model_manifest,
        "all_fixture_count":all_cases.len(),"selected_count":cases.len(),"selection_filter":filter,"selection_modes":modes,
        "fixture_sha256":horary_lessons::digest(&serde_json::to_string(&all_cases).map_err(|e|e.to_string())?),
        "entry_point":if full_reading {"horary_pipeline::run"}else{"horary_pipeline::run_elicitation"},"full_reading":full_reading,
        "reading_rubric_sha256":rubrics.as_ref().map(|rubrics|horary_lessons::digest(&serde_json::to_string(rubrics).expect("Reading rubrics serialize"))),
        "decoder":if hosted.is_some(){"hosted unconstrained text; exact application prompts, executor and independent analysis stages; not native-model qualification"}else if batch_size==1{"exact production single constrained text; production independent batching only inside full reading"}else{"EXPLORATION unconstrained native independent batches of up to4; exact production executor validates each proposal"},
        "case_flow_parallelism":batch_size,
        "cancellation_scope":if hosted.is_some() || batch_size==1{"individual case"}else{"shared four-case group; deadline cancellation affects every peer and is an infrastructure interruption"},
        "frozen_clock":{"local":"2026-10-07T12:00","timezone":"America/New_York","timestamp_ms":frozen_moment()},
        "device":device(),"temperature":0,"seed":0,"ctx_tokens":crate::native_llama_worker::READING_CONTEXT_TOKENS,
        "case_deadline_seconds":if hosted.is_some() || batch_size==1{Some(seconds)}else{None},
        "group_deadline_seconds":if batch_size==4 && hosted.is_none(){Some(seconds)}else{None},
        "case_max_calls":max_calls,"expected_answers_sent_to_model":false});
    progress.manifest = Some(manifest.clone());
    write_new(&dir.join("manifest.json"), &manifest)?;
    write_new(&dir.join("fixtures.json"), &all_cases)?;
    if let Some(rubrics) = &rubrics {
        write_new(&dir.join("reading-rubrics.json"), rubrics.as_ref())?;
    }
    let state = NativeLlamaState::default();
    struct Stop<'a>(&'a NativeLlamaState);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            let _ = stop_native_llama(self.0);
        }
    }
    let _stop = if let Some(model) = model {
        start_native_llama_from_path(&state,"horary-catalogue-eval".into(),String::new(),model,PathBuf::new(),
            serde_json::from_value(json!({"modelId":"horary-catalogue-eval","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"})).map_err(|e|e.to_string())?)
            .map_err(|e|e.message)?;
        Some(Stop(&state))
    } else {
        None
    };
    save_report(dir, &manifest, &progress.results)?;
    let config = CaseRun {
        full_reading,
        seconds,
        max_calls,
        dispatcher: None,
        shared_cancelled: None,
        group: None,
        program,
        rubrics,
        hosted,
    };
    for (group_index, group) in cases.chunks(batch_size).enumerate() {
        progress.active_case_ids = group.iter().map(|case| case.id.clone()).collect();
        if source_hashes(root)? != sources {
            return Err("Application source changed during this frozen campaign; earlier receipts remain. Start a new campaign for the changed source.".into());
        }
        let outcomes = if batch_size == 1 {
            vec![evaluate_case(&state, dir, group[0].clone(), config.clone())]
        } else if config.hosted.is_some() {
            // Independent cases retain their own deadlines. The hosted client's
            // four-request limiter also covers each case's analysis branches.
            std::thread::scope(|scope| {
                let workers = group
                    .iter()
                    .cloned()
                    .map(|case| {
                        let config = config.clone();
                        let state_ref = &state;
                        scope.spawn(move || evaluate_case(state_ref, dir, case, config))
                    })
                    .collect::<Vec<_>>();
                workers
                    .into_iter()
                    .map(|worker| {
                        worker
                            .join()
                            .unwrap_or_else(|_| Err("A hosted case worker panicked".into()))
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            let group_cancelled = Arc::new(AtomicBool::new(false));
            let group_deadline = Deadline::start(seconds, group_cancelled.clone());
            let outputs = std::thread::scope(|scope| {
                let (dispatch, requests) = mpsc::channel();
                let state_ref = &state;
                let cancel = group_cancelled.clone();
                scope.spawn(move || dispatch_batches(state_ref, requests, cancel));
                let group_config = CaseRun {
                    dispatcher: Some(dispatch.clone()),
                    shared_cancelled: Some(group_cancelled.clone()),
                    group: Some(group_index),
                    ..config.clone()
                };
                let mut workers = Vec::new();
                for case in group.iter().cloned() {
                    let config = group_config.clone();
                    let directory = dir;
                    workers.push(
                        scope.spawn(move || evaluate_case(state_ref, directory, case, config)),
                    );
                }
                drop(group_config);
                drop(dispatch);
                workers
                    .into_iter()
                    .map(|worker| {
                        worker.join().unwrap_or_else(|_| {
                            Err("An exploration case worker panicked; no completion implied".into())
                        })
                    })
                    .collect::<Vec<_>>()
            });
            drop(group_deadline);
            outputs
        };
        let mut infrastructure_errors = Vec::new();
        for outcome in outcomes {
            match outcome {
                Ok(outcome) => {
                    if let Some(error) = outcome["infrastructure_error"].as_str() {
                        infrastructure_errors.push(error.to_owned());
                    }
                    if let Some(error) = outcome["provider_stop"].as_str() {
                        infrastructure_errors.push(error.to_owned());
                    }
                    progress.results.push(outcome);
                }
                Err(error) => infrastructure_errors.push(error),
            }
        }
        recorded_group(
            infrastructure_errors,
            save_report(dir, &manifest, &progress.results),
        )?;
        if source_hashes(root)? != sources {
            return Err("Application source changed during a frozen group; case receipts remain, and no campaign completion is asserted.".into());
        }
        progress.active_case_ids.clear();
    }
    let failed = progress
        .results
        .iter()
        .filter(|result| !outcome_qualified(result))
        .count();
    save_report_state(
        dir,
        &manifest,
        &progress.results,
        &json!({"status":"completed"}),
    )?;
    write_new(
        &dir.join("completed.json"),
        &json!({"completed":progress.results.len(),"campaign_failures":failed,"human_fluidity_review":"pending"}),
    )?;
    Ok(failed)
}

#[test]
fn campaign_selection_uses_exact_ids_or_method_names_without_expanding_the_budget() {
    let cases = catalogue().unwrap();
    let ids: Vec<_> = cases
        .iter()
        .filter(|case| case_selected(case, &["investment-explicit"]))
        .map(|case| case.id.as_str())
        .collect();
    assert_eq!(ids, ["investment-explicit"]);
    assert!(
        cases
            .iter()
            .filter(|case| case_selected(case, &[Method::Investment.name()]))
            .count()
            >= 3
    );
    assert!(cases.iter().all(|case| !case_selected(case, &["explicit"])));
    assert!(cases.iter().all(|case| case_selected(case, &[])));
}

#[test]
fn supplementary_cases_deserialize_strictly_without_regrading_their_unresolved_boundaries() {
    let words = include_str!("../test-fixtures/elicitation/invariant-adversarial.json");
    let cases: Vec<Case> = serde_json::from_str(words).unwrap();
    assert_eq!(cases.len(), 21);
    let agency = cases
        .iter()
        .find(|case| case.id == "invariant-sale-agent-distinct-from-contract-party")
        .unwrap();
    assert!(!agency.expected.ready);
    assert!(agency.expected.needs.is_empty());
    assert_eq!(agency.expected.owner.as_deref(), Some("querent"));
    for case in cases
        .iter()
        .filter(|case| matches!(case.method, Method::Parcel | Method::Visit))
    {
        assert!(
            !case.expected.ready,
            "{} keeps its reviewed-program boundary",
            case.id
        );
    }
    for case in cases {
        let value = serde_json::to_value(case).unwrap();
        let recovered: Case = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(recovered).unwrap(), value);
    }
}

#[test]
fn a_new_optional_bank_name_is_loaded_without_changing_the_required_case_banks() {
    let directory = tempfile::tempdir().unwrap();
    let required = serde_json::from_str::<Vec<Case>>(CORE).unwrap().len()
        + serde_json::from_str::<Vec<Case>>(SPECIALIST).unwrap().len();
    let mut case = serde_json::from_str::<Vec<Value>>(CORE).unwrap().remove(0);
    case["id"] = json!("future-supplemental-case");
    fs::write(
        directory.path().join("future-bank.json"),
        serde_json::to_vec(&vec![case]).unwrap(),
    )
    .unwrap();
    fs::write(directory.path().join("provenance.md"), "Not a case array.").unwrap();
    let cases = catalogue_in(directory.path()).unwrap();
    assert_eq!(cases.len(), required + 1);
    assert!(cases
        .iter()
        .any(|case| case.id == "future-supplemental-case"));
    fs::write(directory.path().join("malformed-bank.json"), "{}").unwrap();
    let error = match catalogue_in(directory.path()) {
        Ok(_) => panic!("A metadata object must not be treated as a case-array bank"),
        Err(error) => error,
    };
    assert!(error.contains("malformed-bank.json"));
}

#[test]
fn fingerprint_names_use_git_separators_for_both_host_path_styles() {
    let expected = "src-tauri/test-fixtures/readings/local-rubrics.json";
    assert_eq!(source_name(Path::new(expected)).unwrap(), expected);
    assert_eq!(
        source_name(Path::new(
            r"src-tauri\test-fixtures\readings\local-rubrics.json"
        ))
        .unwrap(),
        expected
    );
}

#[test]
fn a_loaded_supplement_is_fingerprinted_even_when_git_ignores_it() {
    use sha2::{Digest, Sha256};
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--quiet"]).unwrap();
    fs::write(
        repository.path().join(".gitignore"),
        "invariant-adversarial.json\nlocal-rubrics.json\nreading_neural_eval.rs\n",
    )
    .unwrap();
    let directory = repository
        .path()
        .join("src-tauri/test-fixtures/elicitation");
    fs::create_dir_all(&directory).unwrap();
    let data = include_str!("../test-fixtures/elicitation/invariant-adversarial.json");
    fs::write(directory.join("invariant-adversarial.json"), data).unwrap();
    let rubric_directory = repository.path().join("src-tauri/test-fixtures/readings");
    fs::create_dir_all(&rubric_directory).unwrap();
    let rubric_path = rubric_directory.join("local-rubrics.json");
    let rubric_data = "[]\n";
    fs::write(&rubric_path, rubric_data).unwrap();
    let reading_key = "src-tauri/src/reading_neural_eval.rs";
    let reading_path = repository.path().join(reading_key);
    fs::create_dir_all(reading_path.parent().unwrap()).unwrap();
    let reading_source = "// Synthetic compiled reading module\n";
    fs::write(&reading_path, reading_source).unwrap();
    let before = source_hashes(repository.path()).unwrap();
    let key = "src-tauri/test-fixtures/elicitation/invariant-adversarial.json";
    let rubric_key = "src-tauri/test-fixtures/readings/local-rubrics.json";
    assert_eq!(before[key], format!("{:x}", Sha256::digest(data)));
    assert_eq!(
        before[rubric_key],
        format!("{:x}", Sha256::digest(rubric_data))
    );
    assert_eq!(
        before[reading_key],
        format!("{:x}", Sha256::digest(reading_source))
    );
    assert!(before.keys().all(|name| !name.contains('\\')));
    fs::write(
        directory.join("invariant-adversarial.json"),
        format!("{data}\n"),
    )
    .unwrap();
    assert_ne!(before[key], source_hashes(repository.path()).unwrap()[key]);
    fs::write(&rubric_path, format!("{rubric_data}\n")).unwrap();
    assert_ne!(
        before[rubric_key],
        source_hashes(repository.path()).unwrap()[rubric_key]
    );
    fs::write(
        &reading_path,
        format!("{reading_source}// Changed module\n"),
    )
    .unwrap();
    assert_ne!(
        before[reading_key],
        source_hashes(repository.path()).unwrap()[reading_key]
    );
    fs::remove_file(&reading_path).unwrap();
    assert!(source_hashes(repository.path()).is_err());
}

#[cfg(unix)]
#[test]
fn optional_fixture_banks_reject_symlinks_instead_of_loading_nonlocal_sources() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("case-data.txt");
    fs::write(&target, CORE).unwrap();
    std::os::unix::fs::symlink(&target, directory.path().join("foreign-bank.json")).unwrap();
    assert!(optional_fixture_banks(directory.path())
        .unwrap_err()
        .contains("ordinary file"));
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClassificationTask {
    case_id: String,
    source_case_directory: PathBuf,
    source_request_file: PathBuf,
    source_request_sha256: String,
    source_initial_sha256: String,
    baseline_manifest_sha256: String,
    #[serde(default)]
    program_file: Option<PathBuf>,
    #[serde(default)]
    replay_completed_capture: bool,
    #[serde(default)]
    inspect_prompt_only: bool,
}

fn classification_source_bytes(path: &Path, expected: &str) -> Result<Vec<u8>, String> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Classification source needs a SHA256 digest".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if !horary_prompt_program::digest(&bytes).eq_ignore_ascii_case(expected) {
        return Err(format!(
            "Classification source hash changed: {}",
            path.display()
        ));
    }
    Ok(bytes)
}

/// This is a fresh classifier invocation, never a reset of a specialist session.
fn classification_scope(
    case: &Case,
    request: &Value,
    initial: &Value,
) -> Result<(Session, Value), String> {
    let stage: Stage = serde_json::from_value(request["stage"].clone())
        .map_err(|error| format!("Captured classification stage: {error}"))?;
    let input = crate::horary_step::original_input(&request["input"]);
    if stage != Stage::Intake || input["recognition_phase"] != "classify_question" {
        return Err("Only Intake/classify_question captures can replay classification".into());
    }
    let mut session: Session = serde_json::from_value(initial["session"].clone())
        .map_err(|error| format!("Captured classification session: {error}"))?;
    session.candidates = serde_json::from_value(initial["candidates"].clone())
        .map_err(|error| format!("Captured classification candidates: {error}"))?;
    let default_brief = serde_json::to_value(crate::horary_pipeline::Brief::default())
        .map_err(|error| error.to_string())?;
    if session.chart.is_some()
        || !session.sections.is_empty()
        || !session.facts.is_empty()
        || !session.revisions.is_empty()
        || !session.question.is_empty()
        || session.method.consultation.is_some()
        || session.method.result.is_some()
        || !session.method.records.is_empty()
        || !session.method.flow.is_empty()
        || !session.method.replies.is_empty()
        || serde_json::to_value(&session.method.brief).map_err(|error| error.to_string())?
            != default_brief
    {
        return Err("First-classification replay rejects accepted chart, analysis or specialist state; nothing is cleared".into());
    }
    let state = &input["consultation_state"];
    if input["chart_exists"] != false
        || !state.is_object()
        || !state["chart"].is_null()
        || state["interpretation_exists"] != false
        || state["completed_steps"]
            .as_array()
            .is_none_or(|steps| !steps.is_empty())
        || !state["unfinished_step"].is_null()
        || state["pending_user_requests"]
            .as_array()
            .is_none_or(|needs| !needs.is_empty())
        || !input["pending_requirement"].is_null()
        || input["consultation"]["frame"]["state"] != "missing"
        || input.get("reading_request").is_some()
    {
        return Err(
            "Captured classification input already contains accepted or pending work".into(),
        );
    }
    if session.messages.len() != 1
        || session.messages[0].role != "user"
        || session.messages[0].text != case.words
        || input["latest_words"].as_str() != Some(case.words.as_str())
        || input["canonical_question"].as_str() != Some(session.question.as_str())
        || request["matter"]
            != serde_json::to_value(session.method.brief.matter)
                .map_err(|error| error.to_string())?
    {
        return Err("Captured request, initial session and authored case do not bind the same first question".into());
    }
    Ok((session, input.clone()))
}

fn classification_grade(
    case: &Case,
    frame: Option<&reading_contracts::Frame>,
    error: Option<&str>,
) -> crate::reading_eval::Gate {
    use crate::reading_eval::{Gate, Status};
    if let Some(error) = error {
        return Gate {
            status: Status::NotRun,
            failures: vec![error.into()],
            basis: "Classification unobserved: executor or infrastructure interrupted".into(),
        };
    }
    let mut failures = Vec::new();
    let allowed = case
        .expected
        .allowed_methods
        .as_deref()
        .unwrap_or(std::slice::from_ref(&case.method));
    if !frame.is_some_and(|frame| allowed.contains(&frame.method)) {
        failures.push(format!(
            "Method must be one of {allowed:?}; actual {:?}",
            frame.map(|frame| frame.method)
        ));
    }
    let mut facets = case.expected.allowed_facets.clone();
    facets.extend(case.expected.facet);
    if !facets.is_empty() && frame.is_none_or(|frame| !facets.contains(&frame.facet)) {
        failures.push(format!(
            "Facet must be one of {facets:?}; actual {:?}",
            frame.map(|frame| frame.facet)
        ));
    }
    Gate::checked(failures, "Authored method/facet gold for the returned native-checked Turn only; a completed result without a frame fails classification")
}

fn classification_call_limits(max_calls: u64) -> Result<Value, String> {
    if max_calls == 0 {
        return Err("Classification logical model call cap must be positive".into());
    }
    // The existing hosted client permits three completed-HTTP service attempts
    // per logical generate call. The controller reserves this entire bound
    // before launching the function; it is not a three-physical-call promise.
    let reservation = max_calls
        .checked_mul(3)
        .ok_or("Classification generation-attempt reservation overflow")?;
    Ok(json!({"logical_model_call_cap":max_calls,
        "physical_generation_attempt_reservation":reservation}))
}

fn classification_generation_attempts(calls: &[Value]) -> usize {
    calls
        .iter()
        .map(|call| {
            let receipt = &call["provider_receipt"];
            receipt["generation_attempts"]
                .as_array()
                .map(|attempts| {
                    attempts
                        .iter()
                        .filter(|attempt| attempt["submitted"] == true)
                        .count()
                })
                .unwrap_or_else(|| usize::from(receipt["submitted"] == true))
        })
        .sum()
}

struct ClassificationCapturedCall {
    request: Value,
    generation: NativeGenerationResult,
    request_sha256: String,
    result_sha256: String,
    provider_receipt: Value,
}

struct ClassificationCapture {
    calls: Vec<ClassificationCapturedCall>,
    provenance: Value,
}

fn classification_capture(
    task: &ClassificationTask,
    case: &Case,
    source_dir: &Path,
    baseline: &Value,
    initial: &Value,
) -> Result<ClassificationCapture, String> {
    if task.program_file.is_some() {
        return Err("Recorded source replay cannot apply a prompt program".into());
    }
    let campaign = source_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Replay source campaign unavailable")?;
    let origin_bytes = fs::read(
        campaign
            .join("case-origins")
            .join(format!("{}.json", case.id)),
    )
    .map_err(|error| format!("Replay needs a closed case origin: {error}"))?;
    let origin: Value = serde_json::from_slice(&origin_bytes).map_err(|error| error.to_string())?;
    if origin["case_id"] != case.id || !origin["files"].is_object() {
        return Err("Replay origin does not seal this case".into());
    }
    let sealed = |relative: &str| -> Result<Vec<u8>, String> {
        let digest = origin["files"][relative]
            .as_str()
            .ok_or_else(|| format!("Replay file is not sealed: {relative}"))?;
        classification_source_bytes(&source_dir.join(relative), digest)
    };
    if origin["files"]["initial.json"].as_str() != Some(task.source_initial_sha256.as_str()) {
        return Err("Replay origin initial session differs from the task binding".into());
    }
    sealed("initial.json")?;
    let outcome: Value =
        serde_json::from_slice(&sealed("outcome.json")?).map_err(|error| error.to_string())?;
    if outcome["id"] != case.id
        || !matches!(
            outcome["execution_status"].as_str(),
            Some(
                "completed"
                    | "execution_error"
                    | "call_budget_exhausted"
                    | "deadline_cancelled"
                    | "evidence_io_interrupted"
                    | "exploration_group_interrupted"
            )
        )
        || !outcome["first_turn_execution_completed"].is_boolean()
    {
        return Err("Replay needs a sealed closed-case outcome, not a partial live capture".into());
    }
    // Recovered whole cases can come from a different attempt manifest. Bind
    // that manifest separately while retaining the task's original baseline.
    let origin_dir = PathBuf::from(
        origin["source_directory"]
            .as_str()
            .ok_or("Replay origin has no source directory")?,
    );
    let origin_manifest_path = origin_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Replay origin manifest unavailable")?
        .join("manifest.json");
    let origin_manifest_sha = origin["source_manifest_sha256"]
        .as_str()
        .ok_or("Replay origin manifest is not sealed")?;
    let origin_manifest_bytes =
        classification_source_bytes(&origin_manifest_path, origin_manifest_sha)?;
    let source: Value =
        serde_json::from_slice(&origin_manifest_bytes).map_err(|error| error.to_string())?;
    if !source["sources"].is_object()
        || !source["repository_sha"].is_string()
        || !source["prompt_program"].is_null()
        || !baseline["prompt_program"].is_null()
        || [
            "sources",
            "repository_sha",
            "working_diff_sha256",
            "catalogue_version",
            "book_ocr_sha256",
        ]
        .iter()
        .any(|key| source[*key] != baseline[*key])
        || source["model"]["id"] != "gemma-4-26b-a4b-it"
        || source["model"]["local_inference"] != false
        || [
            "id",
            "provider",
            "temperature",
            "seed",
            "thinking_level",
            "local_inference",
        ]
        .iter()
        .any(|key| source["model"][*key] != baseline["model"][*key])
    {
        return Err(
            "Replay source native/model fingerprint differs from the pinned baseline".into(),
        );
    }
    let mut calls = Vec::new();
    // Only the contiguous initial classifier/repair prefix can be replayed.
    // A later focused extraction or reading is never substituted for a repair.
    for sequence in 1u64.. {
        let request_name = format!("calls/{sequence:04}-request.json");
        if origin["files"].get(&request_name).is_none() {
            break;
        }
        let request_bytes = sealed(&request_name)?;
        let request: Value =
            serde_json::from_slice(&request_bytes).map_err(|error| error.to_string())?;
        if request["stage"] != "intake"
            || crate::horary_step::original_input(&request["input"])["recognition_phase"]
                != "classify_question"
        {
            break;
        }
        classification_scope(case, &request, initial)?;
        if request["sequence"].as_u64() != Some(sequence) || !request["prompt_program"].is_null() {
            return Err("Replay classifier sequence or original teaching changed".into());
        }
        if sequence == 1
            && !horary_prompt_program::digest(&request_bytes)
                .eq_ignore_ascii_case(&task.source_request_sha256)
        {
            return Err("Replay must begin with the task-bound first classifier request".into());
        }
        let result_name = format!("calls/{sequence:04}-result.json");
        let result_bytes = sealed(&result_name)?;
        let result: Value =
            serde_json::from_slice(&result_bytes).map_err(|error| error.to_string())?;
        if result["request"] != request {
            return Err("Replay result belongs to a different captured request".into());
        }
        let generation: NativeGenerationResult =
            serde_json::from_value(result["result"]["Ok"].clone()).map_err(|error| {
                format!("Replay requires a completed generation result: {error}")
            })?;
        calls.push(ClassificationCapturedCall {
            request,
            generation,
            request_sha256: horary_prompt_program::digest(&request_bytes),
            result_sha256: horary_prompt_program::digest(&result_bytes),
            provider_receipt: result["provider_receipt"].clone(),
        });
    }
    if calls.is_empty() {
        return Err("Replay origin has no completed initial classifier capture".into());
    }
    Ok(ClassificationCapture {
        calls,
        provenance: json!({
        "mode":"recorded_source_replay","origin_sha256":horary_prompt_program::digest(&origin_bytes),
        "baseline_manifest_sha256":task.baseline_manifest_sha256,"source_manifest_sha256":origin_manifest_sha,
        "source_native_fingerprint":{"repository_sha":source["repository_sha"],
            "working_diff_sha256":source["working_diff_sha256"],"sources_sha256":horary_prompt_program::digest(source["sources"].to_string()),
            "catalogue_version":source["catalogue_version"],"book_ocr_sha256":source["book_ocr_sha256"]},
        "sealed_source_outcome_sha256":origin["files"]["outcome.json"],
        "source_outcome_used_for":"closed-capture provenance only; its grades are not used",
        "new_generation_attempts":0}),
    })
}

fn classification_replay_matches(
    capture: &ClassificationCapturedCall,
    stage: Stage,
    matter: Matter,
    input: &Value,
    schema: &Value,
) -> Result<(), String> {
    let (prompt, _) = trial_prompt(stage, matter, input, schema, None)?;
    let prompt_sha = horary_prompt_program::digest(&prompt);
    let schema_sha = horary_prompt_program::digest(schema.to_string());
    let request = &capture.request;
    if stage != Stage::Intake
        || crate::horary_step::original_input(input)["recognition_phase"] != "classify_question"
        || request["stage"] != "intake"
        || request["matter"] != serde_json::to_value(matter).map_err(|error| error.to_string())?
        || request["prompt_sha256"] != prompt_sha
        || horary_prompt_program::digest(request["prompt"].to_string()) != prompt_sha
        || request["schema_sha256"] != schema_sha
        || horary_prompt_program::digest(request["schema"].to_string()) != schema_sha
        || horary_prompt_program::digest(request["input"].to_string())
            != horary_prompt_program::digest(input.to_string())
    {
        return Err("Recorded classifier prompt/schema/input does not exactly match this native invocation; no fallback or repair substitution".into());
    }
    Ok(())
}

struct ClassificationReplayRuntime<'a, 'b> {
    reader: &'a Reader<'b>,
    capture: &'a ClassificationCapture,
}

impl Runtime for ClassificationReplayRuntime<'_, '_> {
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        self.reader.check()?;
        let sequence = self.reader.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let captured = usize::try_from(sequence - 1)
            .ok()
            .and_then(|index| self.capture.calls.get(index));
        let result = if audio.is_some() {
            Err("Recorded classification replay accepts text only".into())
        } else if let Some(captured) = captured {
            classification_replay_matches(captured, stage, matter, input, schema)
                .map(|()| captured.generation.clone())
        } else {
            Err(
                "Recorded classifier prefix ended before native completion; no live fallback"
                    .into(),
            )
        };
        let (prompt, _) = trial_prompt(stage, matter, input, schema, None)?;
        let request = json!({"sequence":sequence,"stage":stage,"matter":matter,"input":input,"schema":schema,
            "prompt":serde_json::from_str::<Value>(&prompt).map_err(|error| error.to_string())?,
            "prompt_sha256":horary_prompt_program::digest(&prompt),"schema_sha256":horary_prompt_program::digest(schema.to_string()),
            "input_sha256":horary_prompt_program::digest(input.to_string()),"provider":"recorded_source_replay"});
        write_new(
            &self
                .reader
                .dir
                .join("calls")
                .join(format!("{sequence:04}-request.json")),
            &request,
        )?;
        self.reader.keep_call(sequence, json!({"request":request,"result":result.as_ref().map_err(String::as_str),
            "provider_receipt":{"submitted":false,"generation_attempts":[],"provenance":self.capture.provenance,
                "captured_request_sha256":captured.map(|call| &call.request_sha256),
                "captured_result_sha256":captured.map(|call| &call.result_sha256)},
            "recorded_source_provider_receipt":captured.map(|call| &call.provider_receipt)}))?;
        result
    }
    fn generate_batch(
        &self,
        _tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String> {
        Err("Recorded classification replay cannot execute another stage or batch".into())
    }
    fn publish(&self, session: &mut Session) -> Result<(), String> {
        self.reader.publish(session)
    }
    fn check(&self) -> Result<(), String> {
        self.reader.check()
    }
    fn directory(&self) -> &Path {
        self.reader.directory()
    }
}

#[test]
#[ignore = "Hosted classification function: hash-bound HORARY_NEURAL_TASK, fresh HORARY_EVAL_EVIDENCE and Google credential locator required"]
fn real_model_classification_function() -> Result<(), String> {
    let task_bytes = fs::read(PathBuf::from(
        std::env::var_os("HORARY_NEURAL_TASK").ok_or("Set HORARY_NEURAL_TASK")?,
    ))
    .map_err(|error| error.to_string())?;
    let task: ClassificationTask =
        serde_json::from_slice(&task_bytes).map_err(|error| error.to_string())?;
    if task.replay_completed_capture && task.program_file.is_some() {
        return Err("Recorded source replay cannot apply a prompt program".into());
    }
    let case = catalogue()?
        .into_iter()
        .find(|case| case.id == task.case_id)
        .ok_or("Classification task names no authored catalogue case")?;
    let source_dir = task
        .source_case_directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if source_dir.file_name().and_then(|name| name.to_str()) != Some(case.id.as_str()) {
        return Err("Classification source directory must name its authored case".into());
    }
    let request_path = if task.source_request_file.is_absolute() {
        task.source_request_file.clone()
    } else {
        source_dir.join(&task.source_request_file)
    }
    .canonicalize()
    .map_err(|error| error.to_string())?;
    if !request_path.starts_with(&source_dir) {
        return Err("Captured classification request must belong to its source case".into());
    }
    let request_bytes = classification_source_bytes(&request_path, &task.source_request_sha256)?;
    let initial_bytes = classification_source_bytes(
        &source_dir.join("initial.json"),
        &task.source_initial_sha256,
    )?;
    let baseline_path = source_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Classification source campaign unavailable")?
        .join("manifest.json");
    let baseline_bytes =
        classification_source_bytes(&baseline_path, &task.baseline_manifest_sha256)?;
    let baseline: Value =
        serde_json::from_slice(&baseline_bytes).map_err(|error| error.to_string())?;
    let request: Value =
        serde_json::from_slice(&request_bytes).map_err(|error| error.to_string())?;
    let initial: Value =
        serde_json::from_slice(&initial_bytes).map_err(|error| error.to_string())?;
    let (mut session, input) = classification_scope(&case, &request, &initial)?;
    let max_calls = std::env::var("HORARY_EVAL_MAX_CALLS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(3);
    let mut limits = classification_call_limits(max_calls)?;
    let capture = task
        .replay_completed_capture
        .then(|| classification_capture(&task, &case, &source_dir, &baseline, &initial))
        .transpose()?;
    if capture.is_some() {
        limits["physical_generation_attempt_reservation"] = json!(0);
    }
    let seconds = std::env::var("HORARY_EVAL_CASE_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(90);
    if seconds == 0 {
        return Err("Classification case deadline must be positive".into());
    }
    let program_bytes = task
        .program_file
        .as_ref()
        .map(fs::read)
        .transpose()
        .map_err(|error| error.to_string())?;
    let program = program_bytes
        .as_deref()
        .map(horary_prompt_program::Program::parse)
        .transpose()?;
    if program.as_ref().is_some_and(|program| {
        !program
            .baseline_manifest_sha256
            .eq_ignore_ascii_case(&task.baseline_manifest_sha256)
    }) {
        return Err(
            "Classification prompt program belongs to a different baseline manifest".into(),
        );
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Repository root unavailable")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if task.inspect_prompt_only {
        if task.program_file.is_some() || task.replay_completed_capture {
            return Err("Prompt inspection cannot replay or apply a program".into());
        }
        let matter = session.method.brief.matter;
        let schema = crate::horary_step::response_schema_for(Stage::Intake, matter, &input, &[]);
        let (prompt, _) = trial_prompt(Stage::Intake, matter, &input, &schema, None)?;
        let messages: Value = serde_json::from_str(&prompt).map_err(|error| error.to_string())?;
        let dir = PathBuf::from(
            std::env::var_os("HORARY_EVAL_EVIDENCE")
                .ok_or("Set a fresh prompt inspection directory")?,
        );
        fs::create_dir(&dir).map_err(|error| error.to_string())?;
        write_new(
            &dir.join("inspection.json"),
            &json!({"scope":"classification_prompt_inspection_only",
            "case_id":case.id,"task_sha256":horary_prompt_program::digest(&task_bytes),"prompt":messages,
            "guide_sha256":horary_prompt_program::digest(messages[0]["content"].as_str().ok_or("No native classifier guide")?),
            "prompt_sha256":horary_prompt_program::digest(&prompt),"schema":schema,"input":input,
            "repository_sha":git(&root,&["rev-parse","HEAD"])?.trim(),"sources":source_hashes(&root)?,
            "model_constructed":false,"new_generation_attempts":0}),
        )?;
        return Ok(());
    }
    let hosted = if capture.is_some() {
        // Replay never reads the credential locator or constructs a client.
        None
    } else {
        let keyfile = PathBuf::from(
            std::env::var_os("HORARY_GOOGLE_KEY_FILE")
                .ok_or("Hosted Google is mandatory: set HORARY_GOOGLE_KEY_FILE")?,
        );
        if keyfile
            .canonicalize()
            .map_err(|_| "Hosted credential unavailable")?
            .starts_with(&root)
        {
            return Err("Keep the hosted credential outside the repository".into());
        }
        Some(Arc::new(crate::hosted_gemma_eval::Client::from_file(
            &keyfile, 1,
        )?))
    };
    // Live mode has a mandatory hosted client; replay uses only its opaque
    // Runtime below. Neither path can reach Reader's native generate fallback.
    let model = hosted
        .as_ref()
        .map(|client| client.metadata())
        .unwrap_or_else(
            || json!({"provider":"recorded_source_replay","new_generation_attempts":0}),
        );
    let dir = PathBuf::from(
        std::env::var_os("HORARY_EVAL_EVIDENCE")
            .ok_or("Set a fresh HORARY_EVAL_EVIDENCE directory")?,
    );
    fs::create_dir(&dir)
        .map_err(|error| format!("Classification evidence must be fresh: {error}"))?;
    fs::create_dir(dir.join("calls")).map_err(|error| error.to_string())?;
    fs::create_dir(dir.join("checkpoints")).map_err(|error| error.to_string())?;
    for (name, bytes) in [
        ("task.json", &task_bytes),
        ("source-request.json", &request_bytes),
        ("source-initial.json", &initial_bytes),
    ] {
        atomic_evidence_file(&dir.join(name), true, |file| {
            file.write_all(bytes).map_err(|error| error.to_string())
        })?;
    }
    if let Some(bytes) = &program_bytes {
        atomic_evidence_file(&dir.join("prompt-program.json"), true, |file| {
            file.write_all(bytes).map_err(|error| error.to_string())
        })?;
    }
    write_new(
        &dir.join("manifest.json"),
        &json!({"version":EVALUATOR_VERSION,
        "scope":"classification_function_only","case_id":case.id,"task":task,
        "repository_sha":git(&root, &["rev-parse", "HEAD"])?.trim(),
        "working_diff_sha256":horary_lessons::digest(&git(&root, &["diff", "--binary", "HEAD"])?),
        "sources":source_hashes(&root)?,
        "task_sha256":horary_prompt_program::digest(&task_bytes),"limits":limits,
        "logical_model_call_cap":max_calls,"physical_generation_attempt_reservation":limits["physical_generation_attempt_reservation"],
        "program_sha256":program_bytes.as_ref().map(horary_prompt_program::digest),
        "model":model,"replay_provenance":capture.as_ref().map(|capture| &capture.provenance),
        "entry_point":"horary_executor::execute(Stage::Intake)",
        "case_seconds":seconds,"full_reading":false}),
    )?;
    let state = NativeLlamaState::default();
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Deadline::start(seconds, cancelled.clone());
    let reader = Reader {
        state: &state,
        hosted,
        dir: dir.clone(),
        cancelled,
        calls: Mutex::new(Vec::new()),
        sequence: AtomicU64::new(0),
        checkpoints: AtomicU64::new(0),
        max_calls,
        device_available: case.device_available,
        device_calls: AtomicU64::new(0),
        dispatcher: None,
        group: None,
        program: program.map(Arc::new),
    };
    write_new(
        &dir.join("initial.json"),
        &json!({"session":session,"candidates":session.candidates}),
    )?;
    let started = Instant::now();
    let mut result = if let Some(capture) = &capture {
        let replay = ClassificationReplayRuntime {
            reader: &reader,
            capture,
        };
        crate::horary_executor::execute(&mut session, &replay, Stage::Intake, input, &[], None)
    } else {
        crate::horary_executor::execute(&mut session, &reader, Stage::Intake, input, &[], None)
    };
    if result.is_ok()
        && capture.as_ref().is_some_and(|capture| {
            u64::try_from(capture.calls.len()).ok() != Some(reader.sequence.load(Ordering::Acquire))
        })
    {
        result = Err("Native completion did not consume the sealed classifier/repair prefix; replay cannot substitute a changed acceptance path".into());
    }
    drop(deadline);
    let error = result.as_ref().err().cloned();
    if let Some(error) = &error {
        session.method.flow.pause(error.clone());
    }
    let turn = result
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|data| data.turn());
    let classification = classification_grade(
        &case,
        turn.and_then(|turn| turn.frame.as_ref()),
        error.as_deref(),
    );
    let not_run = || crate::reading_eval::Gate {
        status: crate::reading_eval::Status::NotRun,
        failures: Vec::new(),
        basis: "Classification function only: this hurdle was not executed".into(),
    };
    let hurdles = crate::reading_eval::Hurdles {
        classification,
        elicitation: not_run(),
        extraction: not_run(),
        reading: not_run(),
    };
    let calls = reader
        .calls
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let attempts = classification_generation_attempts(&calls);
    let execution_status = if error.is_none() {
        "completed"
    } else if reader.cancelled.load(Ordering::Acquire) {
        "deadline_cancelled"
    } else if error
        .as_deref()
        .is_some_and(|error| error.contains("call budget exhausted"))
    {
        "logical_call_budget_exhausted"
    } else if capture.is_some() {
        "recorded_source_replay_interrupted"
    } else if calls.iter().any(|call| call["result"]["Err"].is_string()) {
        "hosted_transport_or_provider_error"
    } else {
        "executor_or_evidence_error"
    };
    let observed = error.is_none();
    let pass =
        observed.then_some(hurdles.classification.status == crate::reading_eval::Status::Pass);
    write_new(&dir.join("calls.json"), &calls)
        .map_err(|recording| retain_execution_error(recording, error.as_deref()))?;
    write_new(&dir.join("final.json"), &json!({"session":session,"candidates":session.candidates,"returned_turn":turn,"hurdles":hurdles}))
        .map_err(|recording| retain_execution_error(recording, error.as_deref()))?;
    let outcome = json!({"id":case.id,"scope":"classification_function_only","full_reading":false,
        "hurdles":hurdles,"classification_observed":observed,"classification_pass":pass,
        "classification_score":pass.map(u8::from),"classification_execution_completed":observed,
        "execution_status":execution_status,"infrastructure_error":error,"returned_turn":turn,
        "model_calls":if capture.is_some(){0}else{calls.len()},"logical_calls":calls.len(),"logical_model_call_cap":max_calls,
        "physical_generation_attempt_reservation":limits["physical_generation_attempt_reservation"],
        "physical_generation_attempts":attempts,"hosted_http_requests":attempts,
        "rejected_attempts":session.method.records.iter().filter(|record| record.validation_error.is_some()).count(),
        "elapsed_ms":started.elapsed().as_millis(),"deadline_cancelled":reader.cancelled.load(Ordering::Acquire),
        "decoder_mode":if capture.is_some(){"recorded_source_replay"}else{"hosted_unconstrained_text"},
        "replay_provenance":capture.as_ref().map(|capture| &capture.provenance),
        "new_generation_attempts":attempts,"source_request_sha256":task.source_request_sha256,
        "source_initial_sha256":task.source_initial_sha256,"baseline_manifest_sha256":task.baseline_manifest_sha256});
    write_new(&dir.join("outcome.json"), &outcome)
        .map_err(|recording| retain_execution_error(recording, error.as_deref()))?;
    println!("{}", outcome);
    error.map_or(Ok(()), Err)
}

#[test]
fn classification_capture_rejects_specialist_scope_without_resetting_it() {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "contact-explicit")
        .unwrap();
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: case.words.clone(),
    });
    let initial = json!({"session":session,"candidates":[]});
    let request = json!({"stage":"intake","matter":"other","input":{
        "recognition_phase":"classify_question","latest_words":case.words,"canonical_question":"",
        "chart_exists":false,"consultation":{"frame":{"state":"missing"}},
        "pending_requirement":null,"consultation_state":{"chart":null,"interpretation_exists":false,
            "completed_steps":[],"unfinished_step":null,"pending_user_requests":[]}}});
    assert!(classification_scope(&case, &request, &initial).is_ok());
    let mut repaired = request.clone();
    repaired["input"] = json!({"original_input":request["input"],"previous_worksheet":{"frame":{"method":"parcel","facet":"timing"}}});
    assert_eq!(
        classification_scope(&case, &repaired, &initial).unwrap().1,
        request["input"]
    );
    for altered in [
        json!({"stage":"conversation"}),
        json!({"recognition_phase":"complete_selected_program"}),
        json!({"chart_exists":true}),
    ] {
        let mut wrong = request.clone();
        for (key, value) in altered.as_object().unwrap() {
            if key == "stage" {
                wrong[key] = value.clone();
            } else {
                wrong["input"][key] = value.clone();
            }
        }
        assert!(classification_scope(&case, &wrong, &initial).is_err());
    }
    let mut specialist = initial.clone();
    specialist["session"]["method"]["brief"]["matter"] = json!("money");
    assert!(classification_scope(&case, &request, &specialist)
        .err()
        .unwrap()
        .contains("nothing is cleared"));
    let mut chart = initial.clone();
    chart["session"]["chart"] = json!({"accepted":true});
    assert!(classification_scope(&case, &request, &chart).is_err());
    let mut wrong_question = request;
    wrong_question["input"]["latest_words"] = json!("A different question");
    assert!(classification_scope(&case, &wrong_question, &initial).is_err());
}

#[test]
fn classification_grade_observes_only_authored_method_and_facet() {
    use crate::reading_eval::Status;
    let mut case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "contact-explicit")
        .unwrap();
    let correct = reading_contracts::Frame {
        method: case.method,
        facet: case.expected.facet.unwrap(),
    };
    assert_eq!(
        classification_grade(&case, Some(&correct), None).status,
        Status::Pass
    );
    let wrong_facet = reading_contracts::Frame {
        facet: Facet::Timing,
        ..correct.clone()
    };
    assert_eq!(
        classification_grade(&case, Some(&wrong_facet), None).status,
        Status::Fail
    );
    let wrong_method = reading_contracts::Frame {
        method: Method::Parcel,
        ..correct.clone()
    };
    assert_eq!(
        classification_grade(&case, Some(&wrong_method), None).status,
        Status::Fail
    );
    assert_eq!(classification_grade(&case, None, None).status, Status::Fail);
    assert_eq!(
        classification_grade(&case, Some(&correct), Some("Hosted transport interrupted")).status,
        Status::NotRun
    );
    // Exercise both authored allow-list fields independently of the fallback.
    case.expected.allowed_methods = Some(vec![Method::Contact, Method::Parcel]);
    case.expected.allowed_facets = vec![Facet::Timing];
    assert_eq!(
        classification_grade(&case, Some(&wrong_method), None).status,
        Status::Pass
    );
    assert_eq!(
        classification_grade(&case, Some(&wrong_facet), None).status,
        Status::Pass
    );
    case.expected.allowed_methods = None;
    case.expected.allowed_facets.clear();
    case.expected.facet = None;
    assert_eq!(classification_grade(&case, None, None).status, Status::Fail);
    case.expected.allowed_methods = Some(vec![Method::Unclassified]);
    case.expected.needs = vec![RequirementKey::Frame];
    assert_eq!(classification_grade(&case, None, None).status, Status::Fail);
}

#[test]
fn classification_budget_reserves_service_retries_and_counts_physical_attempts() {
    let limits = classification_call_limits(3).unwrap();
    assert_eq!(limits["logical_model_call_cap"], 3);
    assert_eq!(limits["physical_generation_attempt_reservation"], 9);
    assert!(classification_call_limits(0).is_err());
    assert!(classification_call_limits(u64::MAX).is_err());
    let calls = vec![
        json!({"provider_receipt":{"submitted":true,"generation_attempts":[{"submitted":true,"http_status":503},{"submitted":true,"http_status":200}]}}),
        json!({"provider_receipt":{"submitted":false,"generation_attempts":[{"submitted":false}]}}),
        json!({"provider_receipt":{"submitted":true,"generation_attempts":[{"submitted":true,"transport_error":"uncertain"}]}}),
    ];
    assert_eq!(classification_generation_attempts(&calls), 3);
}

#[test]
fn classification_sources_reject_hash_changes_before_execution() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("request.json");
    fs::write(&file, b"{\"stage\":\"intake\"}").unwrap();
    let bytes = fs::read(&file).unwrap();
    let digest = horary_prompt_program::digest(&bytes);
    assert_eq!(classification_source_bytes(&file, &digest).unwrap(), bytes);
    fs::write(&file, b"{\"stage\":\"conversation\"}").unwrap();
    assert!(classification_source_bytes(&file, &digest)
        .unwrap_err()
        .contains("hash changed"));
}

#[test]
fn classification_replay_rejects_response_prompt_schema_and_input_tampering() {
    let input = json!({"recognition_phase":"classify_question","latest_words":"Will she call?",
        "consultation":{"frame":{"state":"missing"}}});
    let schema = crate::horary_step::response_schema_for(Stage::Intake, Matter::Other, &input, &[]);
    let (prompt, _) = trial_prompt(Stage::Intake, Matter::Other, &input, &schema, None).unwrap();
    let generation: NativeGenerationResult = serde_json::from_value(json!({
        "content":"{}","promptTokens":0,"generatedTokens":0,"elapsedMs":0,"tokensPerSecond":0.,
        "promptCacheHit":false,"cachedPromptTokens":0,"prefilledPromptTokens":0}))
    .unwrap();
    let mut captured = ClassificationCapturedCall {
        request: json!({"sequence":1,"stage":"intake","matter":"other","input":input,"schema":schema,
            "prompt":serde_json::from_str::<Value>(&prompt).unwrap(),
            "prompt_sha256":horary_prompt_program::digest(&prompt),"schema_sha256":horary_prompt_program::digest(schema.to_string())}),
        generation,
        request_sha256: "source-request".into(),
        result_sha256: "source-result".into(),
        provider_receipt: Value::Null,
    };
    assert!(classification_replay_matches(
        &captured,
        Stage::Intake,
        Matter::Other,
        &input,
        &schema
    )
    .is_ok());
    let original_request = captured.request.clone();
    captured.request["prompt"][0]["content"] = json!("Tampered teaching");
    assert!(classification_replay_matches(
        &captured,
        Stage::Intake,
        Matter::Other,
        &input,
        &schema
    )
    .is_err());
    captured.request = original_request.clone();
    captured.request["schema"]["properties"]["frame"] = json!({"type":"null"});
    assert!(classification_replay_matches(
        &captured,
        Stage::Intake,
        Matter::Other,
        &input,
        &schema
    )
    .is_err());
    captured.request = original_request;
    captured.request["input"]["latest_words"] = json!("Another question");
    assert!(classification_replay_matches(
        &captured,
        Stage::Intake,
        Matter::Other,
        &input,
        &schema
    )
    .is_err());
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("0001-result.json");
    let result = json!({"result":{"Ok":captured.generation}});
    let bytes = serde_json::to_vec(&result).unwrap();
    let seal = horary_prompt_program::digest(&bytes);
    fs::write(&file, &bytes).unwrap();
    assert!(classification_source_bytes(&file, &seal).is_ok());
    let mut altered = result;
    altered["result"]["Ok"]["content"] = json!("A substituted answer");
    fs::write(&file, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(classification_source_bytes(&file, &seal).is_err());
}

#[test]
fn classification_replay_runs_native_acceptance_and_gold_without_a_model() {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "contact-explicit")
        .unwrap();
    let input = json!({"recognition_phase":"classify_question","latest_words":case.words,
        "spoken_input":false,"consultation":reading_contracts::Consultation::default()});
    let schema = crate::horary_step::response_schema_for(Stage::Intake, Matter::Other, &input, &[]);
    let (prompt, _) = trial_prompt(Stage::Intake, Matter::Other, &input, &schema, None).unwrap();
    // The captured frame is deliberately contrary to this question's gold.
    // Native acceptance and authored grading must run again on that raw result.
    let content =
        json!({"intent":"read","question":case.words,"frame":{"method":"parcel","facet":"event"},
        "people":[],"subject":null,"updates":[],"heard":"","unavailable_quote":"",
        "focus":"judgment","restore_revision":null})
        .to_string();
    let generation = serde_json::from_value(json!({"content":content,
        "promptTokens":0,"generatedTokens":0,"elapsedMs":0,"tokensPerSecond":0.,
        "promptCacheHit":false,"cachedPromptTokens":0,"prefilledPromptTokens":0}))
    .unwrap();
    let capture = ClassificationCapture {
        calls: vec![ClassificationCapturedCall {
            request: json!({"sequence":1,"stage":"intake","matter":"other","input":input,"schema":schema,
            "prompt":serde_json::from_str::<Value>(&prompt).unwrap(),
            "prompt_sha256":horary_prompt_program::digest(&prompt),"schema_sha256":horary_prompt_program::digest(schema.to_string())}),
            generation,
            request_sha256: "source-request".into(),
            result_sha256: "source-result".into(),
            provider_receipt: json!({"submitted":true}),
        }],
        provenance: json!({"mode":"recorded_source_replay","new_generation_attempts":0}),
    };
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("calls")).unwrap();
    fs::create_dir(directory.path().join("checkpoints")).unwrap();
    let state = NativeLlamaState::default();
    let reader = Reader {
        state: &state,
        hosted: None,
        dir: directory.path().into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        calls: Mutex::new(Vec::new()),
        sequence: AtomicU64::new(0),
        checkpoints: AtomicU64::new(0),
        max_calls: 1,
        device_available: false,
        device_calls: AtomicU64::new(0),
        dispatcher: None,
        group: None,
        program: None,
    };
    let runtime = ClassificationReplayRuntime {
        reader: &reader,
        capture: &capture,
    };
    let mut session = Session::default();
    let result =
        crate::horary_executor::execute(&mut session, &runtime, Stage::Intake, input, &[], None)
            .unwrap()
            .unwrap();
    let frame = result.turn().unwrap().frame.as_ref();
    assert_eq!(
        classification_grade(&case, frame, None).status,
        crate::reading_eval::Status::Fail
    );
    assert_eq!(session.method.records.len(), 1);
    assert!(session.method.records[0].validation_error.is_none());
    let calls = reader.calls.lock().unwrap();
    assert_eq!(classification_generation_attempts(&calls), 0);
    assert_eq!(
        calls[0]["provider_receipt"]["captured_result_sha256"],
        "source-result"
    );
    assert!(runtime
        .check()
        .unwrap_err()
        .contains("call budget exhausted"));
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InputJourneyTask {
    case_id: String,
    source_case_directory: PathBuf,
    source_request_file: PathBuf,
    source_request_sha256: String,
    source_initial_sha256: String,
    source_fixture_sha256: String,
    source_origin_sha256: String,
    baseline_manifest_sha256: String,
    target_method: Method,
    #[serde(default)]
    program_file: Option<PathBuf>,
    #[serde(default)]
    inspect_prompt_only: bool,
}

/// Predict only the authored starting state for a pre-network identity check.
/// evaluate_case still creates its own fresh Session; this value is never
/// transplanted into the pipeline and contains no captured specialist state.
fn input_journey_initial(case: &Case) -> Value {
    let mut session = Session::default();
    if case.device_available {
        session.candidates.push(device());
    }
    session.device_context = Some(crate::conversation::DeviceContext {
        timezone: "America/New_York".into(),
        locale: "en-US".into(),
        latitude: case.device_available.then_some(38.657),
        longitude: case.device_available.then_some(-77.249),
        accuracy_meters: case.device_available.then_some(30.),
    });
    session.messages.push(Message {
        role: "user".into(),
        text: case.words.clone(),
    });
    json!({"session":session,"candidates":session.candidates})
}

fn input_journey_source_scope(
    task: &InputJourneyTask,
    case: &Case,
    request: &Value,
    initial: &Value,
    fixture: &Value,
    origin: &Value,
    request_name: &str,
) -> Result<(Matter, Value), String> {
    if case.id != task.case_id
        || *fixture != serde_json::to_value(case).map_err(|error| error.to_string())?
        || *initial != input_journey_initial(case)
    {
        return Err("Input journey source differs from the immutable authored fixture or fresh initial state; nothing is reset".into());
    }
    let sequence = request["sequence"]
        .as_u64()
        .filter(|sequence| *sequence > 0)
        .ok_or("Focused source request has no logical call sequence")?;
    if origin["case_id"] != case.id
        || !origin["files"].is_object()
        || request_name != format!("calls/{sequence:04}-request.json")
        || [
            ("initial.json", &task.source_initial_sha256),
            ("fixture.json", &task.source_fixture_sha256),
            (request_name, &task.source_request_sha256),
        ]
        .iter()
        .any(|(name, expected)| {
            origin["files"][*name]
                .as_str()
                .is_none_or(|actual| !actual.eq_ignore_ascii_case(expected))
        })
    {
        return Err(
            "Input journey origin does not bind its fixture, initial session and focused request"
                .into(),
        );
    }
    let signature = horary_prompt_program::signature("intake", &request["input"]);
    let input = crate::horary_step::original_input(&request["input"]);
    let state = &input["consultation_state"];
    if request["stage"] != "intake"
        || signature.recognition_phase != Some("complete_selected_program")
        || signature.method != Some(task.target_method.name())
        || !request["prompt_program"].is_null()
        || input["latest_words"].as_str() != Some(case.words.as_str())
        || input["legacy_user_fact_sources"] != json!([case.words])
        || input["chart_exists"] != false
        || !state.is_object()
        || !state["chart"].is_null()
        || state["interpretation_exists"] != false
        || state["completed_steps"]
            .as_array()
            .is_none_or(|steps| !steps.is_empty())
        || !state["unfinished_step"].is_null()
        || state["pending_user_requests"]
            .as_array()
            .is_none_or(|needs| !needs.is_empty())
        || !input["pending_requirement"].is_null()
        || !input["consultation"]["requested"].is_null()
        || input.get("reading_request").is_some()
        || input.get("stage_user_replies").is_some()
    {
        return Err("Input journey inspection needs one first-turn Intake/complete_selected_program capture for the selected target method; downstream or supplying state is forbidden".into());
    }
    if !request["schema"].is_object()
        || request["schema_sha256"] != horary_prompt_program::digest(request["schema"].to_string())
        || !request["prompt"].is_array()
        || request["prompt_sha256"] != horary_prompt_program::digest(request["prompt"].to_string())
    {
        return Err("Focused source request has inconsistent prompt or schema hashes".into());
    }
    let matter = serde_json::from_value(request["matter"].clone())
        .map_err(|error| format!("Focused source matter: {error}"))?;
    // Inspection retains the actual repair wrapper, if present. Live execution
    // uses neither this input nor its tentative frame, only the authored Case.
    Ok((matter, request["input"].clone()))
}

fn input_journey_source_fingerprint(origin: &Value, baseline: &Value) -> Result<Value, String> {
    let source_dir = PathBuf::from(
        origin["source_directory"]
            .as_str()
            .ok_or("Input journey origin has no source directory")?,
    );
    let manifest_path = source_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Input journey origin manifest unavailable")?
        .join("manifest.json");
    let manifest_sha = origin["source_manifest_sha256"]
        .as_str()
        .ok_or("Input journey origin has no sealed manifest")?;
    let bytes = classification_source_bytes(&manifest_path, manifest_sha)?;
    let source: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !source["sources"].is_object()
        || !source["repository_sha"].is_string()
        || !source["prompt_program"].is_null()
        || !baseline["prompt_program"].is_null()
        || [
            "sources",
            "repository_sha",
            "working_diff_sha256",
            "catalogue_version",
            "book_ocr_sha256",
        ]
        .iter()
        .any(|key| source[*key] != baseline[*key])
        || source["model"]["provider"] != "google_gemini_api"
        || source["model"]["id"] != "gemma-4-26b-a4b-it"
        || source["model"]["local_inference"] != false
        || [
            "id",
            "provider",
            "temperature",
            "seed",
            "thinking_level",
            "local_inference",
        ]
        .iter()
        .any(|key| source["model"][*key] != baseline["model"][*key])
    {
        return Err(
            "Input journey source native/model fingerprint differs from its pinned campaign".into(),
        );
    }
    Ok(json!({"source_manifest_sha256":manifest_sha,
        "source_native_fingerprint":{"repository_sha":source["repository_sha"],
            "working_diff_sha256":source["working_diff_sha256"],
            "sources_sha256":horary_prompt_program::digest(source["sources"].to_string()),
            "catalogue_version":source["catalogue_version"],"book_ocr_sha256":source["book_ocr_sha256"]},
        "source_state_used_for":"Hash-bound identity and focused prompt inspection only; no checkpoint or response reuse"}))
}

fn input_journey_program(
    task: &InputJourneyTask,
    bytes: Option<&[u8]>,
) -> Result<Option<horary_prompt_program::Program>, String> {
    let program = bytes
        .map(horary_prompt_program::Program::parse)
        .transpose()?;
    if program.as_ref().is_some_and(|program| {
        !program
            .baseline_manifest_sha256
            .eq_ignore_ascii_case(&task.baseline_manifest_sha256)
            || program.overrides.len() != 1
            || program.overrides[0].stage != "intake"
            || program.overrides[0].recognition_phase.as_deref()
                != Some("complete_selected_program")
            || program.overrides[0].method.as_deref() != Some(task.target_method.name())
    }) {
        return Err("Input journey program must have exactly one Intake/complete_selected_program override for its target method and pinned baseline".into());
    }
    Ok(program)
}

fn input_journey_calls(case_dir: &Path) -> Result<(Vec<Value>, bool), String> {
    let mut requests = BTreeSet::new();
    let mut results = BTreeMap::new();
    if !case_dir.join("calls").exists() {
        return Ok((Vec::new(), false));
    }
    for entry in fs::read_dir(case_dir.join("calls")).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("Non-UTF8 native call receipt")?;
        if let Some(sequence) = name.strip_suffix("-request.json") {
            requests.insert(sequence.to_string());
        } else if let Some(sequence) = name.strip_suffix("-result.json") {
            let value =
                serde_json::from_slice(&fs::read(entry.path()).map_err(|error| error.to_string())?)
                    .map_err(|error| format!("Native call receipt {name}: {error}"))?;
            results.insert(sequence.to_string(), value);
        }
    }
    let complete = requests == results.keys().cloned().collect();
    Ok((results.into_values().collect(), complete))
}

fn input_journey_generation_attempts(calls: &[Value]) -> u64 {
    calls
        .iter()
        .flat_map(|call| {
            if call["provider_receipt"].is_object() {
                vec![&call["provider_receipt"]]
            } else {
                call["provider_receipts"]
                    .as_array()
                    .map(|items| items.iter().collect())
                    .unwrap_or_default()
            }
        })
        .map(|receipt| {
            receipt["generation_attempts"]
                .as_array()
                .map(|attempts| {
                    attempts
                        .iter()
                        .filter(|attempt| attempt["submitted"] == true)
                        .count() as u64
                })
                .unwrap_or_else(|| u64::from(receipt["submitted"] == true))
        })
        .sum()
}

fn input_journey_call_limits(max_groups: u64) -> Result<Value, String> {
    if max_groups == 0 {
        return Err("Input journey logical call group cap must be positive".into());
    }
    // Reader's sequence cap counts generate_batch once. Reserve two possible
    // independent Place/Moment providers, each with the client's three service
    // attempts, even though current prepare_reading dispatches them separately.
    let provider_cap = max_groups
        .checked_mul(2)
        .ok_or("Input journey provider call cap overflow")?;
    let reservation = provider_cap
        .checked_mul(3)
        .ok_or("Input journey generation-attempt reservation overflow")?;
    Ok(
        json!({"logical_model_call_cap":max_groups,"logical_model_call_cap_units":"native_generation_groups",
        "logical_call_group_cap":max_groups,"logical_provider_call_cap":provider_cap,
        "max_provider_calls_per_group":2,"physical_generation_attempt_reservation":reservation}),
    )
}

fn input_journey_call_counts(calls: &[Value]) -> Result<(u64, u64), String> {
    let mut providers = 0u64;
    for call in calls {
        let request = &call["request"];
        let count = if let Some(tasks) = request["tasks"].as_array() {
            let mut stages = BTreeSet::new();
            if tasks.is_empty()
                || tasks.len() > 2
                || tasks.iter().any(|task| {
                    task.as_array().is_none_or(|tuple| tuple.len() != 4)
                        || !matches!(task[0].as_str(), Some("place" | "moment"))
                        || !stages.insert(task[0].as_str().unwrap_or_default())
                })
            {
                return Err("Input journey encountered an unsupported batch; only at most two independent Place/Moment branches fit its reserved bound".into());
            }
            tasks.len() as u64
        } else if matches!(
            request["stage"].as_str(),
            Some("intake" | "place" | "moment" | "conversation")
        ) {
            1
        } else {
            return Err("Input journey encountered an unsupported call stage".into());
        };
        providers = providers
            .checked_add(count)
            .ok_or("Input journey provider call count overflow")?;
    }
    Ok((calls.len() as u64, providers))
}

fn input_journey_after_grade(
    case: &Case,
    first: &Value,
    final_state: &Value,
) -> Result<Option<Grade>, String> {
    let Some(expected) = &case.follow_up_expected else {
        return Ok(None);
    };
    if !final_state["follow_up"]["result"].is_object() {
        return Ok(None);
    }
    let mut session: Session = serde_json::from_value(final_state["session"].clone())
        .map_err(|error| error.to_string())?;
    session.candidates = serde_json::from_value(final_state["candidates"].clone())
        .map_err(|error| error.to_string())?;
    let previous: Session =
        serde_json::from_value(first["session"].clone()).map_err(|error| error.to_string())?;
    let mut after_case = case.clone();
    after_case.expected = expected.clone();
    let error = final_state["follow_up"]["result"]["Err"].as_str();
    Ok(Some(with_reading_grade(
        grade_at_moment(
            &after_case,
            &session,
            error,
            previous
                .candidate_moment_ms
                .unwrap_or_else(|| frozen_moment() + 60_000.),
        ),
        &session,
        false,
        error,
    )))
}

fn input_journey_measurement(
    case: &Case,
    raw: &Value,
    first: &Value,
    final_state: &Value,
    calls: &[Value],
    calls_complete: bool,
    run_error: Option<&str>,
) -> Result<Value, String> {
    if raw["full_reading"] != false
        || [
            raw.get("hurdles"),
            first.get("hurdles"),
            final_state.get("hurdles"),
        ]
        .into_iter()
        .flatten()
        .any(|hurdles| hurdles["reading"]["status"] != "not_run")
    {
        return Err(
            "Input journey cannot qualify or reuse a reading stage; full_reading must remain false"
                .into(),
        );
    }
    let after = input_journey_after_grade(case, first, final_state)?;
    let mut errors = Vec::new();
    let counts = input_journey_call_counts(calls);
    let unsupported = counts.is_err();
    let (logical_groups, logical_providers) = counts.unwrap_or_else(|error| {
        errors.push(error);
        (
            calls.len() as u64,
            calls
                .iter()
                .map(|call| {
                    call["request"]["tasks"].as_array().map_or_else(
                        || u64::from(call["request"]["stage"].is_string()),
                        |tasks| tasks.len() as u64,
                    )
                })
                .sum(),
        )
    });
    for error in [
        run_error,
        first["result"]["Err"].as_str(),
        final_state["follow_up"]["result"]["Err"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        if !errors.iter().any(|previous: &String| previous == error) {
            errors.push(error.to_string());
        }
    }
    let transport = calls.iter().any(|call| call["result"]["Err"].is_string());
    let observed = errors.is_empty()
        && calls_complete
        && raw["first_turn_execution_completed"] == true
        && raw["follow_up_execution_completed"] != false;
    let first_pass = raw["grade"]["semantic_pass"].as_bool().unwrap_or(false);
    // A withheld script is a measured elicitation failure, not a supplied turn.
    // Neither a later good grade nor a cached source answer can erase it.
    let supply_pass = case.follow_up.is_none()
        || raw["follow_up_execution_completed"] == true
            && raw["follow_up_pass"] == true
            && after.as_ref().is_none_or(|grade| grade.semantic_pass);
    let pass = observed.then_some(first_pass && supply_pass);
    let status = if observed {
        "completed"
    } else if unsupported {
        "unsupported_input_call_or_batch"
    } else if raw["deadline_cancelled"] == true {
        "deadline_cancelled"
    } else if raw["infrastructure_error"].is_string() || !calls_complete {
        "evidence_io_interrupted"
    } else if errors
        .iter()
        .any(|error| error.contains("call budget exhausted"))
    {
        "logical_call_budget_exhausted"
    } else if transport {
        "hosted_transport_or_provider_error"
    } else {
        "executor_interrupted"
    };
    Ok(
        json!({"id":case.id,"scope":"input_journey_function_only","full_reading":false,
        "input_journey_observed":observed,"input_journey_pass":pass,"input_journey_score":pass.map(u8::from),
        "execution_status":status,"execution_error":(!errors.is_empty()).then(|| errors.join("; ")),
        "first_turn_execution_error":first["result"]["Err"],
        "supplying_turn_execution_error":final_state["follow_up"]["result"]["Err"],
        "infrastructure_error":if observed {Value::Null} else {json!({"kind":status,"errors":errors,"native":raw["infrastructure_error"]})},
        "grade":raw["grade"],"first_turn_grade":raw["grade"],"after_turn_grade":after,
        "supplying_turn_grade":final_state["follow_up"]["grade"],
        "first_turn_hurdles":first["hurdles"],"hurdles":raw["hurdles"],
        "follow_up":raw["follow_up"],"follow_up_pass":raw["follow_up_pass"],
        "follow_up_scripted":case.follow_up.is_some(),
        "first_turn_execution_completed":raw["first_turn_execution_completed"],
        "follow_up_execution_completed":raw["follow_up_execution_completed"],
        "physical_generation_attempts":input_journey_generation_attempts(calls),
        "physical_generation_attempts_complete":calls_complete,
        "logical_call_groups":logical_groups,"logical_calls":logical_providers,"logical_provider_calls":logical_providers,
        "model_calls":logical_providers,"native_model_calls":raw["model_calls"],
        "hosted_http_requests":input_journey_generation_attempts(calls),"new_generation_attempts":input_journey_generation_attempts(calls),
        "prompt_program_applied_calls":raw["prompt_program_applied_calls"],
        "rejected_attempts":raw["rejected_attempts"],"elapsed_ms":raw["elapsed_ms"],
        "deadline_cancelled":raw["deadline_cancelled"],"provider_stop":raw["provider_stop"],
        "decoder_mode":"hosted_unconstrained_text","reading_semantic_review":"not attempted; input journey only"}),
    )
}

#[test]
#[ignore = "Fresh hosted input journey: hash-bound HORARY_NEURAL_TASK and HORARY_EVAL_EVIDENCE; Google credential required only for measurement"]
fn real_model_input_journey_function() -> Result<(), String> {
    let task_bytes = fs::read(PathBuf::from(
        std::env::var_os("HORARY_NEURAL_TASK").ok_or("Set HORARY_NEURAL_TASK")?,
    ))
    .map_err(|error| error.to_string())?;
    let task: InputJourneyTask =
        serde_json::from_slice(&task_bytes).map_err(|error| error.to_string())?;
    let case = catalogue()?
        .into_iter()
        .find(|case| case.id == task.case_id)
        .ok_or("Input journey task names no authored catalogue case")?;
    let source_dir = task
        .source_case_directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if source_dir.file_name().and_then(|name| name.to_str()) != Some(case.id.as_str()) {
        return Err("Input journey source directory must name its authored case".into());
    }
    let request_path = if task.source_request_file.is_absolute() {
        task.source_request_file.clone()
    } else {
        source_dir.join(&task.source_request_file)
    }
    .canonicalize()
    .map_err(|error| error.to_string())?;
    let request_name = request_path
        .strip_prefix(&source_dir)
        .map_err(|_| "Focused source request must belong to its source case")?
        .to_str()
        .ok_or("Non-UTF8 source request path")?;
    let campaign = source_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Input journey source campaign unavailable")?;
    let request_bytes = classification_source_bytes(&request_path, &task.source_request_sha256)?;
    let initial_bytes = classification_source_bytes(
        &source_dir.join("initial.json"),
        &task.source_initial_sha256,
    )?;
    let fixture_bytes = classification_source_bytes(
        &source_dir.join("fixture.json"),
        &task.source_fixture_sha256,
    )?;
    let origin_bytes = classification_source_bytes(
        &campaign
            .join("case-origins")
            .join(format!("{}.json", case.id)),
        &task.source_origin_sha256,
    )?;
    let baseline_bytes = classification_source_bytes(
        &campaign.join("manifest.json"),
        &task.baseline_manifest_sha256,
    )?;
    let request: Value =
        serde_json::from_slice(&request_bytes).map_err(|error| error.to_string())?;
    let initial: Value =
        serde_json::from_slice(&initial_bytes).map_err(|error| error.to_string())?;
    let fixture: Value =
        serde_json::from_slice(&fixture_bytes).map_err(|error| error.to_string())?;
    let origin: Value = serde_json::from_slice(&origin_bytes).map_err(|error| error.to_string())?;
    let baseline: Value =
        serde_json::from_slice(&baseline_bytes).map_err(|error| error.to_string())?;
    let (matter, input) = input_journey_source_scope(
        &task,
        &case,
        &request,
        &initial,
        &fixture,
        &origin,
        request_name,
    )?;
    // The emitted starting bytes must match too, not merely an equivalent
    // decoded Session. This check happens before any client or paid request.
    let fresh_bytes = serde_json::to_vec_pretty(&input_journey_initial(&case))
        .map_err(|error| error.to_string())?;
    if fresh_bytes != initial_bytes {
        return Err("Current authored fresh initial serialization differs from the sealed source initial; no hosted call made".into());
    }
    let provenance = input_journey_source_fingerprint(&origin, &baseline)?;
    let program_bytes = task
        .program_file
        .as_ref()
        .map(fs::read)
        .transpose()
        .map_err(|error| error.to_string())?;
    let program = input_journey_program(&task, program_bytes.as_deref())?;
    let schema = crate::horary_step::response_schema_for(Stage::Intake, matter, &input, &[]);
    let (prompt, applied) = trial_prompt(Stage::Intake, matter, &input, &schema, program.as_ref())?;
    let messages: Value = serde_json::from_str(&prompt).map_err(|error| error.to_string())?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Repository root unavailable")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let dir = PathBuf::from(
        std::env::var_os("HORARY_EVAL_EVIDENCE")
            .ok_or("Set a fresh HORARY_EVAL_EVIDENCE directory")?,
    );
    fs::create_dir(&dir)
        .map_err(|error| format!("Input journey evidence must be fresh: {error}"))?;
    for (name, bytes) in [
        ("task.json", &task_bytes),
        ("source-request.json", &request_bytes),
        ("source-initial.json", &initial_bytes),
        ("source-fixture.json", &fixture_bytes),
        ("source-origin.json", &origin_bytes),
        ("source-manifest.json", &baseline_bytes),
    ] {
        atomic_evidence_file(&dir.join(name), true, |file| {
            file.write_all(bytes).map_err(|error| error.to_string())
        })?;
    }
    if task.inspect_prompt_only {
        if task.program_file.is_some() {
            return Err("Input journey prompt inspection cannot apply a program".into());
        }
        write_new(
            &dir.join("inspection.json"),
            &json!({"scope":"input_journey_prompt_inspection_only",
            "case_id":case.id,"target_method":task.target_method,"stage":"intake","recognition_phase":"complete_selected_program",
            "task_sha256":horary_prompt_program::digest(&task_bytes),"prompt":messages,"schema":schema,"input":input,
            "guide_sha256":horary_prompt_program::digest(messages[0]["content"].as_str().ok_or("No focused native guide")?),
            "prompt_sha256":horary_prompt_program::digest(&prompt),"schema_sha256":horary_prompt_program::digest(schema.to_string()),
            "input_sha256":horary_prompt_program::digest(input.to_string()),
            "source_request_sha256":task.source_request_sha256,"source_initial_sha256":task.source_initial_sha256,
            "source_fixture_sha256":task.source_fixture_sha256,"source_origin_sha256":task.source_origin_sha256,
            "baseline_manifest_sha256":task.baseline_manifest_sha256,"source_provenance":provenance,
            "repository_sha":git(&root,&["rev-parse","HEAD"])?.trim(),"sources":source_hashes(&root)?,
            "working_diff_sha256":horary_prompt_program::digest(git(&root,&["diff","--binary","HEAD"])?),
            "model_constructed":false,"new_generation_attempts":0,"full_reading":false,
            "end_to_end_qualified":false,"captured_input_used_for":"Focused prompt inspection data only"}),
        )?;
        return Ok(());
    }
    let max_calls = std::env::var("HORARY_EVAL_MAX_CALLS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(24);
    // The cap is native generation groups. Reserve two providers per group and
    // every completed-HTTP retry: 24 groups reserve 144 generation attempts.
    let limits = input_journey_call_limits(max_calls)?;
    let seconds = std::env::var("HORARY_EVAL_CASE_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(900);
    if seconds == 0 {
        return Err("Input journey deadline must be positive".into());
    }
    let keyfile = PathBuf::from(
        std::env::var_os("HORARY_GOOGLE_KEY_FILE")
            .ok_or("Fresh hosted Google is mandatory: set HORARY_GOOGLE_KEY_FILE")?,
    );
    if keyfile
        .canonicalize()
        .map_err(|_| "Hosted credential unavailable")?
        .starts_with(&root)
    {
        return Err("Keep the hosted credential outside the repository".into());
    }
    let hosted = Arc::new(crate::hosted_gemma_eval::Client::from_file(&keyfile, 1)?);
    if let Some(bytes) = &program_bytes {
        atomic_evidence_file(&dir.join("prompt-program.json"), true, |file| {
            file.write_all(bytes).map_err(|error| error.to_string())
        })?;
    }
    write_new(
        &dir.join("manifest.json"),
        &json!({"version":EVALUATOR_VERSION,
        "scope":"input_journey_function_only","case_id":case.id,"task":task,
        "task_sha256":horary_prompt_program::digest(&task_bytes),"source_provenance":provenance,
        "repository_sha":git(&root,&["rev-parse","HEAD"])?.trim(),
        "working_diff_sha256":horary_prompt_program::digest(git(&root,&["diff","--binary","HEAD"])?),
        "sources":source_hashes(&root)?,"model":hosted.metadata(),"limits":limits,
        "logical_model_call_cap":max_calls,"logical_model_call_cap_units":"native_generation_groups",
        "logical_call_group_cap":max_calls,"logical_provider_call_cap":limits["logical_provider_call_cap"],
        "physical_generation_attempt_reservation":limits["physical_generation_attempt_reservation"],
        "case_seconds":seconds,"program_sha256":program_bytes.as_ref().map(horary_prompt_program::digest),
        "target_signature":{"stage":"intake","recognition_phase":"complete_selected_program","method":task.target_method},
        "inspected_current_target_program":applied,"entry_point":"evaluate_case -> horary_pipeline::run_elicitation",
        "fresh_initial_verified_before_network":true,"captured_checkpoint_reused":false,
        "fresh_upstream_and_supplying_calls":true,"full_reading":false}),
    )?;
    fs::create_dir(dir.join("cases")).map_err(|error| error.to_string())?;
    let state = NativeLlamaState::default();
    let case_dir = dir.join("cases").join(&case.id);
    let run = evaluate_case(
        &state,
        &dir,
        case.clone(),
        CaseRun {
            full_reading: false,
            seconds,
            max_calls,
            dispatcher: None,
            shared_cancelled: None,
            group: None,
            program: program.map(Arc::new),
            rubrics: None,
            hosted: Some(hosted),
        },
    );
    let read = |name: &str| -> Result<Value, String> {
        let path = case_dir.join(name);
        if !path.exists() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    };
    let raw = run
        .as_ref()
        .ok()
        .cloned()
        .unwrap_or(json!({"full_reading":false}));
    let first = read("first-turn.json")?;
    let final_state = read("final.json")?;
    let (calls, calls_complete) = input_journey_calls(&case_dir)?;
    let initial_check =
        classification_source_bytes(&case_dir.join("initial.json"), &task.source_initial_sha256);
    let run_error = run
        .as_ref()
        .err()
        .cloned()
        .or_else(|| initial_check.as_ref().err().cloned());
    let mut outcome = input_journey_measurement(
        &case,
        &raw,
        &first,
        &final_state,
        &calls,
        calls_complete,
        run_error.as_deref(),
    )?;
    outcome["native_evidence_directory"] = json!(case_dir);
    outcome["trace"] = json!(format!("cases/{}/trace.html", case.id));
    outcome["logical_model_call_cap"] = json!(max_calls);
    outcome["logical_model_call_cap_units"] = json!("native_generation_groups");
    outcome["logical_call_group_cap"] = json!(max_calls);
    outcome["logical_provider_call_cap"] = limits["logical_provider_call_cap"].clone();
    outcome["physical_generation_attempt_reservation"] =
        limits["physical_generation_attempt_reservation"].clone();
    outcome["target_method"] = json!(task.target_method);
    outcome["source_provenance"] = provenance;
    outcome["source_request_sha256"] = json!(task.source_request_sha256);
    outcome["source_initial_sha256"] = json!(task.source_initial_sha256);
    outcome["source_fixture_sha256"] = json!(task.source_fixture_sha256);
    outcome["source_origin_sha256"] = json!(task.source_origin_sha256);
    outcome["baseline_manifest_sha256"] = json!(task.baseline_manifest_sha256);
    outcome["fresh_initial_sha256_verified"] = json!(initial_check.is_ok());
    outcome["target_signature_calls"] = json!(calls
        .iter()
        .filter(|call| {
            let signature = horary_prompt_program::signature("intake", &call["request"]["input"]);
            call["request"]["stage"] == "intake"
                && signature.recognition_phase == Some("complete_selected_program")
                && signature.method == Some(task.target_method.name())
        })
        .count());
    write_new(&dir.join("calls.json"), &calls)
        .map_err(|error| retain_execution_error(error, run_error.as_deref()))?;
    let mut final_copy = final_state;
    if !final_copy.is_object() {
        final_copy = json!({"session":null});
    }
    final_copy["scope"] = json!("input_journey_function_only");
    final_copy["full_reading"] = json!(false);
    final_copy["native_evidence_directory"] = json!(case_dir);
    final_copy["execution_error"] = outcome["execution_error"].clone();
    write_new(&dir.join("final.json"), &final_copy)
        .map_err(|error| retain_execution_error(error, run_error.as_deref()))?;
    write_new(&dir.join("outcome.json"), &outcome)
        .map_err(|error| retain_execution_error(error, run_error.as_deref()))?;
    println!("{}", outcome);
    run_error.map_or(Ok(()), Err)
}

fn input_journey_source_test_fixture() -> (Case, InputJourneyTask, Value, Value, Value, Value) {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "contact-explicit")
        .unwrap();
    let initial = input_journey_initial(&case);
    let fixture = serde_json::to_value(&case).unwrap();
    let mut consultation =
        serde_json::to_value(reading_contracts::Consultation::default()).unwrap();
    consultation["frame"] = json!({"state":"resolved","observation":{"value":{"method":"contact","facet":"event"},
        "evidence":{"source":"user","turn":1,"quote":case.words}}});
    let input = json!({"recognition_phase":"complete_selected_program","consultation":consultation,
        "latest_words":case.words,"legacy_user_fact_sources":[case.words],"chart_exists":false,
        "pending_requirement":null,"consultation_state":{"chart":null,"interpretation_exists":false,
            "completed_steps":[],"unfinished_step":null,"pending_user_requests":[]}});
    let schema = crate::horary_step::response_schema_for(Stage::Intake, Matter::Other, &input, &[]);
    let (prompt, _) = trial_prompt(Stage::Intake, Matter::Other, &input, &schema, None).unwrap();
    let request = json!({"sequence":2,"stage":"intake","matter":"other","input":input,"schema":schema,
        "prompt":serde_json::from_str::<Value>(&prompt).unwrap(),"prompt_program":null,
        "schema_sha256":horary_prompt_program::digest(schema.to_string()),"prompt_sha256":horary_prompt_program::digest(&prompt)});
    let task = InputJourneyTask {
        case_id: case.id.clone(),
        source_case_directory: PathBuf::from("cases/contact-explicit"),
        source_request_file: PathBuf::from("calls/0002-request.json"),
        source_request_sha256: horary_prompt_program::digest(
            serde_json::to_vec_pretty(&request).unwrap(),
        ),
        source_initial_sha256: horary_prompt_program::digest(
            serde_json::to_vec_pretty(&initial).unwrap(),
        ),
        source_fixture_sha256: horary_prompt_program::digest(
            serde_json::to_vec_pretty(&fixture).unwrap(),
        ),
        source_origin_sha256: "0".repeat(64),
        baseline_manifest_sha256: "1".repeat(64),
        target_method: Method::Contact,
        program_file: None,
        inspect_prompt_only: false,
    };
    let origin = json!({"case_id":case.id,"files":{"initial.json":task.source_initial_sha256,
        "fixture.json":task.source_fixture_sha256,"calls/0002-request.json":task.source_request_sha256}});
    (case, task, request, initial, fixture, origin)
}

#[test]
fn input_journey_scope_rejects_downstream_or_wrong_method_without_resetting() {
    let (case, task, request, initial, fixture, origin) = input_journey_source_test_fixture();
    let check = |request: &Value| {
        input_journey_source_scope(
            &task,
            &case,
            request,
            &initial,
            &fixture,
            &origin,
            "calls/0002-request.json",
        )
    };
    assert!(check(&request).is_ok());
    let mut repair = request.clone();
    repair["input"] = json!({"original_input":request["input"],"previous_worksheet":{"intent":"clarify"},"native_validation_error":"Retain a supplied actor"});
    assert_eq!(check(&repair).unwrap().1, repair["input"], "Inspection uses the actual current repair prompt, never an invented plain extraction input");
    for (pointer, value) in [
        ("/stage", json!("conversation")),
        ("/input/recognition_phase", json!("classify_question")),
        (
            "/input/consultation/frame/observation/value/method",
            json!("parcel"),
        ),
        ("/input/chart_exists", json!(true)),
        ("/input/latest_words", json!("She is my neighbour.")),
        (
            "/input/legacy_user_fact_sources",
            json!([case.words, "She is my neighbour."]),
        ),
    ] {
        let mut wrong = request.clone();
        *wrong.pointer_mut(pointer).unwrap() = value;
        assert!(check(&wrong).is_err(), "Must reject {pointer}");
    }
    let mut downstream = request.clone();
    downstream["input"]["reading_request"] = json!({"binding":{"frame":{"method":"contact"}}});
    assert!(check(&downstream).is_err());
}

#[test]
fn input_journey_source_identity_rejects_gold_initial_and_seal_tampering() {
    let (case, task, request, initial, fixture, origin) = input_journey_source_test_fixture();
    let mut wrong_fixture = fixture.clone();
    wrong_fixture["expected"]["facet"] = json!("timing");
    assert!(input_journey_source_scope(
        &task,
        &case,
        &request,
        &initial,
        &wrong_fixture,
        &origin,
        "calls/0002-request.json"
    )
    .is_err());
    let mut specialist = initial.clone();
    specialist["session"]["method"]["consultation"] = request["input"]["consultation"].clone();
    assert!(input_journey_source_scope(
        &task,
        &case,
        &request,
        &specialist,
        &fixture,
        &origin,
        "calls/0002-request.json"
    )
    .err()
    .unwrap()
    .contains("nothing is reset"));
    let mut wrong_origin = origin.clone();
    wrong_origin["files"]["fixture.json"] = json!("2".repeat(64));
    assert!(input_journey_source_scope(
        &task,
        &case,
        &request,
        &initial,
        &fixture,
        &wrong_origin,
        "calls/0002-request.json"
    )
    .is_err());
    let mut wrong_prompt = request.clone();
    wrong_prompt["prompt"][0]["content"] = json!("Changed source teaching");
    assert!(input_journey_source_scope(
        &task,
        &case,
        &wrong_prompt,
        &initial,
        &fixture,
        &origin,
        "calls/0002-request.json"
    )
    .is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("initial.json");
    write_new(&path, &initial).unwrap();
    classification_source_bytes(&path, &task.source_initial_sha256).unwrap();
    fs::write(&path, serde_json::to_vec_pretty(&specialist).unwrap()).unwrap();
    assert!(classification_source_bytes(&path, &task.source_initial_sha256).is_err());
    let mut replay_task = serde_json::to_value(task).unwrap();
    replay_task["replay_completed_capture"] = json!(true);
    assert!(
        serde_json::from_value::<InputJourneyTask>(replay_task).is_err(),
        "Whole input journeys have no archived replay mode"
    );
}

#[test]
fn input_journey_requires_correct_supplying_binding_and_preserves_first_failure() {
    let case = catalogue()
        .unwrap()
        .into_iter()
        .find(|case| case.id == "contact-missing")
        .unwrap();
    assert!(case.follow_up.is_some());
    let mut session = Session::default();
    session.method.consultation = Some(reading_contracts::Consultation::default());
    session.method.consultation.as_mut().unwrap().requested = Some(RequirementKey::Owner);
    assert!(
        !scripted_need_is_eligible(&case, &session),
        "An Owner ask cannot authorize a relationship supplying script"
    );
    let hurdles = json!({"classification":{"status":"pass"},"elicitation":{"status":"pass"},
        "extraction":{"status":"pass"},"reading":{"status":"not_run"}});
    let mut raw = json!({"full_reading":false,"grade":{"semantic_pass":true},"hurdles":hurdles,
        "first_turn_execution_completed":true,"follow_up_execution_completed":null,
        "follow_up_pass":true,"follow_up":{"status":"withheld because binding was wrong"},"model_calls":0});
    let first = json!({"result":{"Ok":null},"hurdles":hurdles});
    let final_state = json!({"follow_up":raw["follow_up"],"hurdles":hurdles});
    let withheld =
        input_journey_measurement(&case, &raw, &first, &final_state, &[], true, None).unwrap();
    assert_eq!(withheld["input_journey_observed"], true);
    assert_eq!(
        withheld["input_journey_score"], 0,
        "No supplying execution means no supplying pass, even if a stale pass flag was offered"
    );
    raw["grade"]["semantic_pass"] = json!(false);
    let mut explicit = case.clone();
    explicit.follow_up = None;
    explicit.follow_up_expected = None;
    assert_eq!(
        input_journey_measurement(&explicit, &raw, &first, &final_state, &[], true, None).unwrap()
            ["input_journey_score"],
        0,
        "The first-turn grade cannot be erased by later state"
    );
}

#[test]
fn input_journey_transport_and_supply_errors_are_unobserved_and_reading_is_forbidden() {
    let (case, _, _, _, _, _) = input_journey_source_test_fixture();
    let hurdles = json!({"classification":{"status":"pass"},"elicitation":{"status":"pass"},
        "extraction":{"status":"pass"},"reading":{"status":"not_run"}});
    let raw = json!({"full_reading":false,"grade":{"semantic_pass":true},"hurdles":hurdles,
        "first_turn_execution_completed":true,"follow_up_execution_completed":false,"model_calls":1});
    let first = json!({"result":{"Ok":null},"hurdles":hurdles});
    let final_state = json!({"follow_up":{"result":{"Err":"Unknown transport completion on supplying call"}},"hurdles":hurdles});
    let calls = vec![
        json!({"request":{"stage":"intake"},"result":{"Err":"Unknown transport completion"},
        "provider_receipt":{"generation_attempts":[{"submitted":true}]}}),
    ];
    let measured =
        input_journey_measurement(&case, &raw, &first, &final_state, &calls, true, None).unwrap();
    assert_eq!(measured["input_journey_score"], Value::Null);
    assert_eq!(
        measured["execution_status"],
        "hosted_transport_or_provider_error"
    );
    assert!(measured["execution_error"]
        .as_str()
        .unwrap()
        .contains("supplying"));
    assert_eq!(measured["physical_generation_attempts"], 1);
    assert_eq!(measured["full_reading"], false);
    assert_eq!(
        input_journey_call_limits(24).unwrap()["physical_generation_attempt_reservation"],
        144
    );
    let mut reading = raw.clone();
    reading["full_reading"] = json!(true);
    assert!(
        input_journey_measurement(&case, &reading, &first, &final_state, &calls, true, None)
            .is_err()
    );
    let reading_calls = vec![json!({"request":{"stage":"judgment"}})];
    let prohibited = input_journey_measurement(
        &case,
        &raw,
        &first,
        &final_state,
        &reading_calls,
        true,
        None,
    )
    .unwrap();
    assert_eq!(prohibited["input_journey_score"], Value::Null);
    assert_eq!(
        prohibited["execution_status"],
        "unsupported_input_call_or_batch"
    );
    let first_error = json!({"result":{"Err":"call budget exhausted"},"hurdles":hurdles});
    let capped =
        input_journey_measurement(&case, &raw, &first_error, &final_state, &[], true, None)
            .unwrap();
    assert_eq!(capped["input_journey_score"], Value::Null);
    assert_eq!(capped["execution_status"], "logical_call_budget_exhausted");
    assert!(capped["execution_error"]
        .as_str()
        .unwrap()
        .contains("call budget exhausted"));
    assert!(capped["execution_error"]
        .as_str()
        .unwrap()
        .contains("supplying"));
}

#[test]
fn input_journey_reserves_groups_branches_and_retries_separately() {
    let pair = json!({"request":{"tasks":[["place","other",{},{}],["moment","other",{},{}]]},
        "provider_receipts":[{"generation_attempts":[{"submitted":true},{"submitted":true},{"submitted":true}]},
            {"generation_attempts":[{"submitted":true},{"submitted":true},{"submitted":true}]}]});
    let calls = vec![
        json!({"request":{"stage":"intake"},"provider_receipt":{"generation_attempts":[{"submitted":true}]}}),
        pair,
    ];
    assert_eq!(input_journey_call_counts(&calls).unwrap(), (2, 3));
    assert_eq!(input_journey_generation_attempts(&calls), 7);
    let limits = input_journey_call_limits(24).unwrap();
    assert_eq!(limits["logical_call_group_cap"], 24);
    assert_eq!(limits["logical_provider_call_cap"], 48);
    assert_eq!(limits["physical_generation_attempt_reservation"], 144);
    assert_eq!(
        classification_call_limits(3).unwrap()["physical_generation_attempt_reservation"],
        9,
        "The independent classifier's bound must not change"
    );
    assert!(input_journey_call_limits(0).is_err());
    assert!(input_journey_call_limits(u64::MAX).is_err());
    let (case, _, _, _, _, _) = input_journey_source_test_fixture();
    let raw = json!({"full_reading":false,"grade":{"semantic_pass":true},"first_turn_execution_completed":true});
    let first = json!({"result":{"Ok":null}});
    for tasks in [
        json!([
            ["place", "other", {}, {}],
            ["moment", "other", {}, {}],
            ["moment", "other", {}, {}]
        ]),
        json!([["place", "other", {}, {}], ["judgment", "other", {}, {}]]),
        json!([["place", "other", {}, {}], ["place", "other", {}, {}]]),
    ] {
        let calls = vec![json!({"request":{"tasks":tasks}})];
        let interrupted =
            input_journey_measurement(&case, &raw, &first, &Value::Null, &calls, true, None)
                .unwrap();
        assert_eq!(interrupted["input_journey_score"], Value::Null);
        assert_eq!(
            interrupted["execution_status"],
            "unsupported_input_call_or_batch"
        );
    }
}

#[test]
fn input_journey_program_cannot_change_upstream_or_another_methods_teaching() {
    use horary_prompt_program::{digest, Evidence, Override, Program};
    let (case, task, request, _, _, _) = input_journey_source_test_fixture();
    let guide =
        crate::horary_contract::guide_for(Stage::Intake, Matter::Other, &request["input"]).unwrap();
    let program = Program {
        version: 1,
        id: "input-journey-selector-test".into(),
        baseline_manifest_sha256: task.baseline_manifest_sha256.clone(),
        overrides: vec![Override {
            stage: "intake".into(),
            recognition_phase: Some("complete_selected_program".into()),
            method: Some("contact".into()),
            expected_guide_sha256: digest(&guide),
            replacement_text: guide,
            edits: vec![],
        }],
        rationale: "Check only the typed override scope".into(),
        evidence: vec![Evidence {
            case_id: case.id.clone(),
            file: "trace.json".into(),
            json_pointer: "".into(),
            sha256: digest("trace"),
        }],
        training_case_ids: vec![case.id],
        holdout_case_ids: vec!["unused-validation-placeholder".into()],
    };
    let parse = |program: &Program| {
        input_journey_program(&task, Some(&serde_json::to_vec(program).unwrap()))
    };
    assert!(parse(&program).is_ok());
    let mut broad = program.clone();
    broad.overrides[0].method = None;
    assert!(parse(&broad).is_err());
    let mut upstream = program.clone();
    upstream.overrides[0].recognition_phase = Some("classify_question".into());
    assert!(parse(&upstream).is_err());
    let mut other = program.clone();
    other.overrides[0].method = Some("parcel".into());
    assert!(parse(&other).is_err());
    let mut two = program.clone();
    let mut conversation = two.overrides[0].clone();
    conversation.stage = "conversation".into();
    conversation.recognition_phase = None;
    two.overrides.push(conversation);
    assert!(parse(&two).is_err());
    let mut unbound = program;
    unbound.baseline_manifest_sha256 = "2".repeat(64);
    assert!(parse(&unbound).is_err());
}
