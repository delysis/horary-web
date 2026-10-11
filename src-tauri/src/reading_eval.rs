//! Four separately observable hurdles. A checked worksheet is not a correct
//! reading: source-specific interpretation still requires independent review.
#![forbid(unsafe_code)]

use crate::{
    conversation::Session,
    horary_lessons::{self, Matter, Stage},
    horary_step::Phase,
    reading_contracts::{self, Facet, Method, ReadingResult, ReadyReading},
    reading_method,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name { $(pub(crate) $field: $ty),* }
    };
}

record!(Source { printed_pages: String, rule_ids: Vec<String>, notes: String });
record!(Support { contract_status: String, native_recipe: String, missing_capabilities: Vec<String>, qualification: String });
record!(ContextRequirement {
    field: String,
    when: String,
    meaning: String
});
record!(RoleRequirement {
    role: String,
    house_or_derivation: String,
    when: String,
    evidence: String
});
record!(DecisiveTest {
    id: String,
    when: String,
    test: String,
    required_evidence: String,
    outcome_dependency: String
});
record!(Answer { must_address: String, conditional_conclusions: Vec<String>, uncertainty: String });
record!(Rubric {
    case_id: String,
    declared_method: Method,
    reading_method: Method,
    mode: String,
    facet: Facet,
    source: Source,
    support: Support,
    context_requirements: Vec<ContextRequirement>,
    required_roles: Vec<RoleRequirement>,
    decisive_tests: Vec<DecisiveTest>,
    answer: Answer,
    forbidden_inferences: Vec<String>,
    shared_recipe: String,
});

/// Runtime-loaded banks are frozen with the campaign, including ignored files.
/// There is no model access to this directory or to a case's expected answers.
pub(crate) fn banks(directory: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err(
                "Reading rubric banks must be ordinary files, not links or directories".into(),
            );
        }
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn catalogue(directory: &Path) -> Result<BTreeMap<String, Rubric>, String> {
    let passages = horary_lessons::passages()?;
    let mut all = BTreeMap::new();
    for path in banks(directory)? {
        let rows: Vec<Rubric> =
            serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Reading rubrics {}: {e}", path.display()))?;
        for row in rows {
            let value = serde_json::to_value(&row).map_err(|e| e.to_string())?;
            fn check_strings(value: &serde_json::Value) -> bool {
                match value {
                    serde_json::Value::String(s) => !s.trim().is_empty(),
                    serde_json::Value::Object(o) => o.values().all(check_strings),
                    serde_json::Value::Array(a) => a.iter().all(check_strings),
                    _ => true,
                }
            }
            if !check_strings(&value)
                || !["explicit", "implicit", "missing"].contains(&row.mode.as_str())
                || !["implemented", "expert_review"].contains(&row.support.contract_status.as_str())
                || row.decisive_tests.is_empty()
                || row.forbidden_inferences.is_empty()
            {
                return Err(format!("Incomplete reading rubric {}", row.case_id));
            }
            let mut ids = std::collections::BTreeSet::new();
            if row.decisive_tests.iter().any(|test| !ids.insert(&test.id)) {
                return Err(format!("Repeated reading obligation in {}", row.case_id));
            }
            for rule in &row.source.rule_ids {
                if !passages.iter().any(|passage| &passage.id == rule) {
                    return Err(format!("Unknown source passage {rule} in {}", row.case_id));
                }
            }
            let expected = match reading_contracts::contract(row.reading_method).coverage {
                reading_contracts::Coverage::Implemented => "implemented",
                reading_contracts::Coverage::ExpertReview => "expert_review",
            };
            if row.support.contract_status != expected {
                return Err(format!(
                    "Reading support differs from the executable contract in {}",
                    row.case_id
                ));
            }
            let id = row.case_id.clone();
            if all.insert(id.clone(), row).is_some() {
                return Err(format!("Repeated reading rubric {id}"));
            }
        }
    }
    if all.is_empty() {
        return Err("Reading rubric catalogue is empty".into());
    }
    Ok(all)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Pass,
    Fail,
    AwaitingInformation,
    Blocked,
    NotRun,
    StructurePassReviewPending,
}
record!(Gate { status: Status, failures: Vec<String>, basis: String });
record!(Hurdles {
    classification: Gate,
    elicitation: Gate,
    extraction: Gate,
    reading: Gate
});

impl Gate {
    pub(crate) fn checked(failures: Vec<String>, basis: &str) -> Self {
        Self {
            status: if failures.is_empty() {
                Status::Pass
            } else {
                Status::Fail
            },
            failures,
            basis: basis.into(),
        }
    }
}

/// This intentionally cannot certify the semantic content of an answer. It
/// proves a bound, completed procedure and supplies its witnesses to the judge.
pub(crate) fn reading_gate(
    session: &Session,
    attempted: bool,
    input_gates_pass: bool,
    error: Option<&str>,
) -> Gate {
    let basis = "Native completion, current input binding, role/ruler witnesses, all required worksheets, and delivered answer. Astrological inference must also pass the independent per-case source rubric.";
    if !attempted {
        return Gate {
            status: Status::NotRun,
            failures: Vec::new(),
            basis: "Elicitation-only campaign: no reading was attempted".into(),
        };
    }
    if !input_gates_pass {
        return Gate {
            status: Status::Blocked,
            failures: vec![
                "The authored input gates have not all passed; no reading can qualify".into(),
            ],
            basis: basis.into(),
        };
    }
    if let Some(error) = error {
        return Gate {
            status: Status::Fail,
            failures: vec![format!("Reading execution did not complete: {error}")],
            basis: basis.into(),
        };
    }
    let Some(ReadingResult::Judgment {
        binding,
        answer,
        evidence,
        worksheet,
        ..
    }) = session.method.result.as_ref()
    else {
        let (status, failure) = match session.method.result.as_ref() {
            Some(ReadingResult::NeedsInformation { .. }) => (
                Status::AwaitingInformation,
                "Still awaiting information; no completed interpretation",
            ),
            Some(ReadingResult::Limited { .. }) => (
                Status::Blocked,
                "Method or native capability boundary prevents a completed interpretation",
            ),
            _ => (Status::Fail, "No completed interpretation was produced"),
        };
        return Gate {
            status,
            failures: vec![failure.into()],
            basis: basis.into(),
        };
    };
    let mut failures = Vec::new();
    match session
        .method
        .consultation
        .as_ref()
        .zip(session.chart.as_ref())
        .zip(session.place.as_ref())
    {
        Some(((case, chart), place)) => {
            let anchor = reading_contracts::Anchor {
                timestamp_ms: chart["timestampMs"].as_f64().unwrap_or(f64::NAN),
                latitude: place.latitude,
                longitude: place.longitude,
                timezone: place.timezone.clone(),
            };
            match ReadyReading::prepare(case, anchor) {
                Ok(ready) if ready.binding() == binding => {}
                _ => failures.push(
                    "Judgment binding does not match the current ready consultation and chart"
                        .into(),
                ),
            }
        }
        None => {
            failures.push("Judgment lacks its current consultation, chart or reader place".into())
        }
    }
    if answer.trim().is_empty() || worksheet["answer"].as_str() != Some(answer) {
        failures.push("The final answer is empty or differs from its checked worksheet".into());
    }
    let all = reading_method::facts(session.chart.as_ref());
    let referenced: Vec<_> = evidence
        .iter()
        .map(String::as_str)
        .chain(
            worksheet["checks"]
                .as_object()
                .into_iter()
                .flatten()
                .flat_map(|(_, check)| check["evidence"].as_array().into_iter().flatten())
                .filter_map(serde_json::Value::as_str),
        )
        .collect();
    if referenced.is_empty()
        || referenced
            .iter()
            .any(|id| !all.iter().any(|fact| fact.id == *id))
    {
        failures.push("Final answer lacks existing native evidence".into());
    }
    let mut stages = vec![
        Stage::Significators,
        Stage::Condition,
        Stage::Reception,
        Stage::Contacts,
        Stage::Judgment,
    ];
    if matches!(
        session.method.brief.matter,
        Matter::LostObject | Matter::LostAnimal
    ) {
        stages.push(Stage::Location);
    }
    for stage in stages {
        let record = session
            .method
            .records
            .iter()
            .enumerate()
            .rev()
            .find(|(_, r)| {
                r.revision == session.revision
                    && r.stage == stage
                    && r.validation_error.is_none()
                    && r.worksheet.get("request_input").is_none()
            });
        let Some((index, record)) = record else {
            failures.push(format!(
                "No accepted {} worksheet for the current revision",
                stage.name()
            ));
            continue;
        };
        let job = session.method.flow.jobs.iter().find(|job| {
            job.revision == session.revision
                && job.stage == stage
                && matches!(job.phase(), Phase::Complete { record_index } if *record_index == index)
        });
        if job.is_none() {
            failures.push(format!(
                "{} worksheet has no completed native task witness",
                stage.name()
            ));
        }
        let section = session.sections.iter().find(|section| {
            section.revision == session.revision && section.method_stage == Some(stage)
        });
        match section {
            Some(section) if section.worksheet == record.worksheet => {
                if stage == Stage::Significators {
                    if section.roles.is_empty() {
                        failures.push("No main significator roles were retained".into());
                    }
                    for role in &section.roles {
                        if let Some(house) = role.house {
                            if !all.iter().any(|fact| {
                                fact.kind == "house"
                                    && fact.label == format!("House {house}")
                                    && fact.planets.contains(&role.planet)
                            }) {
                                failures.push(format!(
                                    "Role {} has a ruler inconsistent with the native house cusp",
                                    role.label
                                ));
                            }
                        }
                    }
                }
            }
            _ => failures.push(format!(
                "{} checked worksheet was not delivered as a current passage",
                stage.name()
            )),
        }
    }
    if !session.sections.iter().any(|section| {
        section.revision == session.revision
            && section.method_stage == Some(Stage::Judgment)
            && section.body == *answer
    }) {
        failures.push("The completed answer was not delivered to the unfolding document".into());
    }
    Gate {
        status: if failures.is_empty() {
            Status::StructurePassReviewPending
        } else {
            Status::Fail
        },
        failures,
        basis: basis.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_input_collection_cannot_be_called_a_completed_reading() {
        let session = Session::default();
        assert_eq!(
            reading_gate(&session, false, true, None).status,
            Status::NotRun
        );
        assert_eq!(
            reading_gate(&session, true, true, None).status,
            Status::Fail
        );
        assert_eq!(
            reading_gate(&session, true, false, None).status,
            Status::Blocked
        );
        assert_eq!(
            reading_gate(&session, true, true, Some("specialist interrupted")).status,
            Status::Fail
        );
    }
    #[test]
    fn a_method_boundary_is_blocked_not_a_passing_reading() {
        let mut session = Session::default();
        session.method.result = Some(ReadingResult::Limited {
            limitation: reading_contracts::LimitationRecord {
                code: "judgment_program_needs_review".into(),
                message: "No reviewed recipe".into(),
                printed_pages: "219".into(),
            },
        });
        assert_eq!(
            reading_gate(&session, true, true, None).status,
            Status::Blocked
        );
    }
    #[test]
    fn reading_rubric_parser_rejects_unknown_fields_and_incomplete_obligations() {
        let bad = serde_json::json!({"case_id":"lost_object-explicit", "made_up_verdict":"yes"});
        assert!(serde_json::from_value::<Rubric>(bad).is_err());
    }
    #[test]
    fn a_nonempty_answer_without_a_bound_procedure_is_not_a_reading() {
        let mut session = Session::default();
        session.method.result = Some(ReadingResult::Judgment {
            binding: reading_contracts::Binding {
                catalogue_version: reading_contracts::VERSION.into(),
                case_revision: 0,
                question: "Where is my ring?".into(),
                frame: reading_contracts::Frame {
                    method: Method::LostObject,
                    facet: Facet::Location,
                },
                input_sha256: "invented".into(),
            },
            verdict: "location".into(),
            answer: "A plausible paragraph is insufficient without its supporting procedure."
                .into(),
            evidence: Vec::new(),
            worksheet: serde_json::json!({"answer":"A plausible paragraph is insufficient without its supporting procedure."}),
        });
        let gate = reading_gate(&session, true, true, None);
        assert_eq!(gate.status, Status::Fail);
        assert!(gate
            .failures
            .iter()
            .any(|reason| reason.contains("worksheet")));
        assert!(gate
            .failures
            .iter()
            .any(|reason| reason.contains("evidence")));
    }
}
