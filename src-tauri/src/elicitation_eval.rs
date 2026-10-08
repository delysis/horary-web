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

const EVALUATOR_VERSION: &str = "horary-elicitation-evaluator-2026-10-08.7";
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

#[derive(Debug, Serialize)]
struct Grade {
    semantic_pass: bool,
    mismatches: Vec<String>,
    actual: Value,
    fluidity_review_flags: Vec<String>,
    human_fluidity_review: &'static str,
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
    let mut mismatches = Vec::new();
    if let Some(error) = error {
        mismatches.push(format!("Execution did not complete: {error}"));
    }
    let consultation = session.method.consultation.as_ref();
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
    if expected_needs.is_empty() && requested.is_some() {
        mismatches.push(format!(
            "The reader requested unnecessary information {requested:?}"
        ));
    }
    let reply = session
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .map(|message| message.text.as_str())
        .unwrap_or_default();
    if reply.trim().is_empty() {
        mismatches.push("No conversational reply was delivered".into());
    }
    let flags = fluidity_flags(session, reply, &needs);
    Grade {
        semantic_pass: mismatches.is_empty(),
        mismatches,
        actual: json!({"frame":frame,"question":session.question,"needs":needs,
            "requested":requested,"ready":ready,"plan":plan,"chart":session.chart,
            "reader_place":session.place,"anchor":anchor,"consultation":consultation,
            "reply":reply,"native_result":session.method.result}),
        fluidity_review_flags: flags,
        human_fluidity_review:
            "Not reviewed; flags are heuristics, not certification of conversational quality",
    }
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
    if fresh_reply.is_none() {
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
}
impl Reader<'_> {
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
            "prompt":messages,
            "schema":schema,"input":input,
            "guide_sha256":horary_lessons::digest(messages[0]["content"].as_str().ok_or("Missing actual system teaching")?),
            "baseline_guide_sha256":horary_lessons::digest(&crate::horary_contract::guide_for(stage,matter,input)?),
            "prompt_program":applied,
            "prompt_sha256":horary_lessons::digest(&prompt),
            "schema_sha256":horary_lessons::digest(&schema.to_string()),
            "decoder":if self.dispatcher.is_some(){"EXPLORATION unconstrained independent batch; not production-decoder qualification"}else{"production constrained single text generation"},
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
        let result = if let Some(dispatcher) = &self.dispatcher {
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
            "decoder":"production unconstrained independent analysis batch"});
        write_new(
            &self
                .dir
                .join("calls")
                .join(format!("{sequence:04}-request.json")),
            &request,
        )?;
        let start = Instant::now();
        let result = generate_native_batch(
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
        .map_err(|e| e.message);
        self.keep_call(
            sequence,
            json!({"request":request,"wall_ms":start.elapsed().as_millis(),
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
            .map_err(|error| format!("Evidence I/O failure at {}: {error}", path.display()))
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
    let mut html = format!("<!doctype html><meta charset=utf-8><title>{}</title><style>{STYLE}</style><h1>{}</h1><p class=note>Synthetic authored words and device context. Actual native model outputs. Elicitation grades do not qualify astrological judgment or microphone recognition.</p><h2>Conversation</h2>",escape(&case.id),escape(&case.id));
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
    let report = json!({"manifest":manifest,"campaign_state":campaign_state,"completed":results.len(),"semantic_passes":passes,
        "prompt_program_applied_calls":applied_calls,
        "semantic_failures":failures,"human_fluidity_review":"pending",
        "first_turn_execution_interruptions":interruptions,"follow_up_execution_interruptions":follow_up_interruptions,
        "follow_up_passes":journey_passes,"follow_up_failures":journey_failures,
        "scripted_followups_withheld":withheld,
        "follow_up_not_executed":results.len()-journey_passes-journey_failures-follow_up_interruptions,"campaign_failures":campaign_failures,
        "coverage_boundary":"Actual text elicitation and native readiness. Full readings only when explicitly selected; no speech/hardware or SME acceptance implied.","cases":results});
    write_index_json(&dir.join("report.json"), &report)?;
    let status = campaign_state["status"].as_str().unwrap_or("unqualified");
    let reason = campaign_state["reason"].as_str().unwrap_or("");
    let mut html = format!("<!doctype html><meta charset=utf-8><title>Horary scenario review</title><style>{STYLE}</style><h1>Horary scenario review</h1><p class=note>{} {}</p><p>{} completed · {passes} first-turn semantic passes · {failures} first-turn semantic failures · {interruptions} execution interruptions.</p><p>{journey_passes} follow-ups passed · {journey_failures} follow-ups failed · {follow_up_interruptions} follow-ups interrupted.</p><p class=note>Authored synthetic questions; actual model outputs. Conversational quality still needs human review. Every failed attempt is retained. These tests assess elicitation; no astrological answer is certified by a passing grade. First-turn passes are separate from follow-up passes; an unexecuted follow-up is not a successful journey. Batch4 uses unconstrained native generation and shared group cancellation; it is exploration, not production-decoder qualification. Interrupted cases remain unqualified and are counted separately from completed semantic failures.</p><table><thead><tr><th>Case</th><th>Method</th><th>Inputs</th><th>Result</th><th>Review flags</th></tr></thead><tbody>",escape(status),escape(reason),results.len());
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

fn source_hashes(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut names: BTreeSet<String> = git(
        root,
        &[
            "ls-files",
            "src-tauri/src",
            "src-tauri/Cargo.toml",
            "src-tauri/Cargo.lock",
            "src-tauri/test-fixtures/elicitation",
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
        let name = relative
            .to_str()
            .ok_or("A fixture bank needs a UTF-8 repository path")?;
        names.insert(name.to_owned());
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
#[ignore = "Actual resident Gemma across synthetic catalogue; fresh HORARY_EVAL_EVIDENCE and HORARY_NATIVE_LLAMA_TEST_MODEL required"]
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
    let mut evidence_error = result
        .as_ref()
        .err()
        .filter(|error| error.contains("Evidence I/O failure"))
        .cloned();
    let first = grade(&case, &session, result.as_ref().err().map(String::as_str));
    let first_turn_execution_completed = result.is_ok();
    write_new(
        &case_dir.join("first-turn.json"),
        &json!({"result":result,"grade":first,"session":session,"candidates":session.candidates}),
    )?;
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
            if let Some(error) = next
                .as_ref()
                .err()
                .filter(|error| error.contains("Evidence I/O failure"))
            {
                evidence_error = Some(error.clone());
            }
            follow_up_execution_completed = Some(next.is_ok());
            let next_grade = follow_up_grade(&case, words, &previous, &session, &next);
            follow_up_pass = next_grade["pass"].as_bool();
            follow_up = json!({"status":if proposal {"executed after one eligible authored proposal"} else {"executed after matching elicitation"},"words":words,"result":next,
                "consultation":session.method.consultation,"native_result":session.method.result,
                "chart":session.chart,"first_turn_grade_retained":true,"grade":next_grade});
        } else {
            follow_up = json!({"status":"withheld because the intended fact or single eligible proposal was not correctly elicited","words":words,
                "bound_need_matched":bound_need,"proposal_eligible":proposal});
        }
    }
    drop(deadline);
    let calls = reader.calls.into_inner().map_err(|e| e.to_string())?;
    write_new(
        &case_dir.join("final.json"),
        &json!({"session":session,"candidates":session.candidates,"follow_up":follow_up}),
    )?;
    write_index_text(
        &case_dir.join("trace.html"),
        &trace_html(&case, &first, &session, &calls, &follow_up),
    )?;
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
    let outcome = json!({"id":case.id,"method":case.method,"mode":case.mode,"grade":first,
        "prompt_program_applied_calls":applied_calls,
        "elapsed_ms":started.elapsed().as_millis(),"model_calls":calls.len(),"rejected_attempts":repairs,
        "deadline_cancelled":reader.cancelled.load(Ordering::Acquire),"follow_up":follow_up,
        "follow_up_pass":follow_up_pass,"follow_up_scripted":case.follow_up.is_some(),
        "first_turn_execution_completed":first_turn_execution_completed,
        "follow_up_execution_completed":follow_up_execution_completed,
        "exploration_group":config.group,
        "decoder_mode":if config.dispatcher.is_some(){"exploration_unconstrained_batch"}else{"production_constrained_single"},
        "group_cancelled":config.group.is_some() && reader.cancelled.load(Ordering::Acquire),
        "infrastructure_error":evidence_error,
        "execution_status":if evidence_error.is_some(){"evidence_io_interrupted"}
            else if config.group.is_some() && reader.cancelled.load(Ordering::Acquire){"exploration_group_interrupted"}
            else if reader.cancelled.load(Ordering::Acquire){"deadline_cancelled"}
            else if reader.sequence.load(Ordering::Acquire)>=max_calls && result.is_err(){"call_budget_exhausted"}
            else if result.is_err(){"execution_error"}else{"completed"},
        "health":native_llama_health(state).map_err(|e|e.message),"trace":format!("cases/{}/trace.html",case.id)});
    write_new(&case_dir.join("outcome.json"), &outcome)?;
    println!(
        "{}: {} ({} calls, {} rejected attempts, {} ms)",
        case.id,
        if !first_turn_execution_completed {
            "EXECUTION INTERRUPTED"
        } else if first.semantic_pass {
            "semantic pass"
        } else {
            "SEMANTIC FAIL"
        },
        calls.len(),
        repairs,
        started.elapsed().as_millis()
    );
    Ok(outcome)
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
    if batch_size == 4 && full_reading {
        return Err("HORARY_EVAL_BATCH=4 is an elicitation exploration; HORARY_EVAL_FULL requires exact production batch=1".into());
    }
    let model = PathBuf::from(
        std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL")
            .ok_or("Set HORARY_NATIVE_LLAMA_TEST_MODEL")?,
    );
    fs::create_dir(dir.join("cases")).map_err(|e| e.to_string())?;
    let all_cases = catalogue()?;
    let filter = std::env::var("HORARY_EVAL_FILTER").unwrap_or_default();
    let filters: Vec<_> = filter
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let cases: Vec<_> = all_cases
        .iter()
        .filter(|case| case_selected(case, &filters))
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
    let metadata = fs::metadata(&model).map_err(|e| e.to_string())?;
    let manifest = json!({"version":EVALUATOR_VERSION,"authorship":"All case words, expectations and device data are authored synthetic fixtures; actual Gemma outputs are separately traced",
        "campaign_phase":std::env::var("HORARY_EVAL_PHASE").unwrap_or_else(|_|"exploration".into()),
        "prompt_program":program_manifest,
        "prompt_experiment":prompt_experiment,
        "holdout_status":"No blind qualification implied; selected authored cases are visible during prompt development",
        "repository_sha":git(root,&["rev-parse","HEAD"] )?.trim(),
        "working_diff_sha256":horary_lessons::digest(&git(root,&["diff","--binary","HEAD"])?),
        "sources":sources,"catalogue_version":reading_contracts::VERSION,
        "book_ocr_sha256":horary_lessons::BOOK_OCR_SHA256,
        "model":{"path":model,"bytes":metadata.len(),"sha256":model_digest(&model)?},
        "all_fixture_count":all_cases.len(),"selected_count":cases.len(),"selection_filter":filter,
        "fixture_sha256":horary_lessons::digest(&serde_json::to_string(&all_cases).map_err(|e|e.to_string())?),
        "entry_point":if full_reading {"horary_pipeline::run"}else{"horary_pipeline::run_elicitation"},
        "decoder":if batch_size==1{"exact production single constrained text; production independent batching only inside full reading"}else{"EXPLORATION unconstrained native independent batches of up to4; exact production executor validates each proposal"},
        "case_flow_parallelism":batch_size,
        "cancellation_scope":if batch_size==1{"individual case"}else{"shared four-case group; deadline cancellation affects every peer and is an infrastructure interruption"},
        "frozen_clock":{"local":"2026-10-07T12:00","timezone":"America/New_York","timestamp_ms":frozen_moment()},
        "device":device(),"temperature":0,"seed":0,"ctx_tokens":crate::native_llama_worker::READING_CONTEXT_TOKENS,
        "case_deadline_seconds":if batch_size==1{Some(seconds)}else{None},
        "group_deadline_seconds":if batch_size==4{Some(seconds)}else{None},
        "case_max_calls":max_calls,"expected_answers_sent_to_model":false});
    progress.manifest = Some(manifest.clone());
    write_new(&dir.join("manifest.json"), &manifest)?;
    write_new(&dir.join("fixtures.json"), &all_cases)?;
    let state = NativeLlamaState::default();
    struct Stop<'a>(&'a NativeLlamaState);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            let _ = stop_native_llama(self.0);
        }
    }
    start_native_llama_from_path(&state,"horary-catalogue-eval".into(),String::new(),model,PathBuf::new(),
        serde_json::from_value(json!({"modelId":"horary-catalogue-eval","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"})).map_err(|e|e.to_string())?)
        .map_err(|e|e.message)?;
    let _stop = Stop(&state);
    save_report(dir, &manifest, &progress.results)?;
    let config = CaseRun {
        full_reading,
        seconds,
        max_calls,
        dispatcher: None,
        shared_cancelled: None,
        group: None,
        program,
    };
    for (group_index, group) in cases.chunks(batch_size).enumerate() {
        progress.active_case_ids = group.iter().map(|case| case.id.clone()).collect();
        if source_hashes(root)? != sources {
            return Err("Application source changed during this frozen campaign; earlier receipts remain. Start a new campaign for the changed source.".into());
        }
        let outcomes = if batch_size == 1 {
            vec![evaluate_case(&state, dir, group[0].clone(), config.clone())]
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
                    progress.results.push(outcome);
                }
                Err(error) => infrastructure_errors.push(error),
            }
        }
        save_report(dir, &manifest, &progress.results)?;
        if !infrastructure_errors.is_empty() {
            return Err(infrastructure_errors.join("; "));
        }
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
fn a_loaded_supplement_is_fingerprinted_even_when_git_ignores_it() {
    use sha2::{Digest, Sha256};
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--quiet"]).unwrap();
    fs::write(
        repository.path().join(".gitignore"),
        "invariant-adversarial.json\n",
    )
    .unwrap();
    let directory = repository
        .path()
        .join("src-tauri/test-fixtures/elicitation");
    fs::create_dir_all(&directory).unwrap();
    let data = include_str!("../test-fixtures/elicitation/invariant-adversarial.json");
    fs::write(directory.join("invariant-adversarial.json"), data).unwrap();
    let before = source_hashes(repository.path()).unwrap();
    let key = "src-tauri/test-fixtures/elicitation/invariant-adversarial.json";
    assert_eq!(before[key], format!("{:x}", Sha256::digest(data)));
    fs::write(
        directory.join("invariant-adversarial.json"),
        format!("{data}\n"),
    )
    .unwrap();
    assert_ne!(before[key], source_hashes(repository.path()).unwrap()[key]);
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
