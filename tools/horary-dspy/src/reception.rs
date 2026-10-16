//! One reusable judgment function, evaluated by the port's ordinary Predict.
//! Actor binding and reception calculations are supplied native inputs.
use crate::{book::Book, catalogue, functions::Fixture, optimize};
use anyhow::{Context, Result, ensure};
use dsrust::{Evaluate, Example, Module, SignatureSpec, example::Prediction, lm::LmConfig};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc};

pub const PILOT_CASES: [&str; 4] = [
    "dspy-contact-first-004",
    "dspy-contact-first-006",
    "dspy-contact-first-008",
    "dspy-contact-first-011",
];
pub const SOURCE_PAGES: [usize; 6] = [44, 71, 72, 73, 74, 75];
pub const GEPA_METRICS: usize = 24;
pub const GEPA_LOCAL_CALLS: usize = 32;
pub const GEPA_TEACHER_CALLS: usize = 48;
pub const TRANSFER_CASES: [&str; 4] = [
    "dspy-contact-first-004",
    "dspy-contact-first-006",
    "dspy-contact-first-008",
    "dspy-movable_deal-first-003",
];
pub const TRANSFER_STUDENT_CALLS: usize = 16;
pub const TRANSFER_TEACHER_CALLS: usize = 12;
const TRANSFER_OUTPUT_TOKENS: u32 = 2048;

#[derive(dsrust::Signature)]
/// Interpret ONLY the supplied directed reception, under the supplied primary text. The guest planet regards the host planet; never reverse that direction or assume reciprocity. Preserve all matching accepted role IDs when a planet has several roles. Report the exact native positive and negative dignity names separately, without summing or cancelling them. Give a concise qualified contextual implication about the regarding actor's inclination toward the regarded actor. Reception alone establishes neither ability nor an occasion, an action, delivery, an outcome or timing. Do not infer a psychological diagnosis, qualifications or an identity conflict. Return ONLY the seven requested output fields, beginning with edge_id; never reproduce source_text, question, accepted_roles or native_edge.
pub struct InterpretReception {
    /// Printed pp. 44 and 71–75, including minor qualities and ambivalence, with exact primary passages and OCR witnesses.
    #[input]
    pub source_text: String,
    /// Actual enquiry; contextual meaning is about this enquiry, not a generic horoscope.
    #[input]
    pub question: String,
    /// AcceptedReceptionRole-shaped rows: role_index, selected_id, label, house and planet. These are declared component inputs, not an observed acquisition journey.
    #[input]
    pub accepted_roles: String,
    /// Exactly one fact_id and unchanged native DirectedReception (guestPlanet, hostPlanet, qualities). No reverse edge or event evidence is implied.
    #[input]
    pub native_edge: String,
    /// Copy the supplied native fact_id exactly.
    #[output]
    pub edge_id: String,
    /// JSON array of ALL accepted selected_id values for the guest planet. IDs, not labels.
    #[output]
    pub regarding_actor_ids: String,
    /// JSON array of ALL accepted selected_id values for the host planet. IDs, not labels.
    #[output]
    pub regarded_actor_ids: String,
    /// JSON array of exactly the supplied positive dignity names, e.g. domicile or termRuler. No new strength calculation.
    #[output]
    pub positive_qualities: String,
    /// JSON array of exactly the supplied negative dignity names, e.g. detriment or fall.
    #[output]
    pub negative_qualities: String,
    /// One or two sentences: who regards whom, qualified contextual meaning and contrary qualities. Cite printed pages for the interpretation.
    #[output]
    pub interpretation: String,
    /// What cannot be inferred from this single edge, including no automatic reciprocity, occurrence or timing.
    #[output]
    pub limitations: String,
}

#[derive(dsrust::Signature)]
/// Grade ONLY qualified contextual interpretation of this single native reception. Rust independently checks exact edge identity, regarding/regarded accepted IDs and supplied qualities; a semantic pass cannot compensate for those failures. Use printed pp. 44/71–75 and concrete prediction quotes. Do not demand classification, extraction, full chart judgment, an answer, an event, a deadline or timing: these are outside this signature. Do not reward an event prediction made from reception alone. Distinguish actual regard direction, its contextual target and mixed positive/negative qualities; no invented strength sum, cancellation, diagnosis, qualifications or certainty. Return ONLY score, passed, contextual_meaning and feedback, beginning with score.
pub struct JudgeReception {
    #[input]
    pub inputs: String,
    #[input]
    pub prediction: String,
    #[input]
    pub rubric: String,
    #[input]
    pub source_text: String,
    #[output]
    pub score: f64,
    #[output]
    pub passed: bool,
    /// pass, fail, or unobserved for contextual interpretation only.
    #[output]
    pub contextual_meaning: String,
    /// Observed-output quotations and precise printed source witnesses.
    #[output]
    pub feedback: String,
}

/// Mirrors the application clipboard's AcceptedReceptionRole representation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedRole {
    pub role_index: usize,
    pub selected_id: String,
    pub label: String,
    pub house: Option<u8>,
    pub planet: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEdge {
    pub fact_id: String,
    pub reception: crate::native_evidence::DirectedReception,
}

fn supplied(input: &Example) -> Result<(Vec<AcceptedRole>, NativeEdge)> {
    let roles: Vec<AcceptedRole> = serde_json::from_str(
        input
            .get("accepted_roles")
            .and_then(Value::as_str)
            .context("No accepted roles")?,
    )?;
    let edge: NativeEdge = serde_json::from_str(
        input
            .get("native_edge")
            .and_then(Value::as_str)
            .context("No native edge")?,
    )?;
    let mut ids = BTreeSet::new();
    let mut indices = BTreeSet::new();
    ensure!(
        !roles.is_empty()
            && roles.iter().all(|r| {
                !r.selected_id.is_empty()
                    && !r.label.is_empty()
                    && !r.planet.is_empty()
                    && ids.insert(&r.selected_id)
                    && indices.insert(r.role_index)
            }),
        "Ambiguous or missing accepted role identity"
    );
    ensure!(!edge.fact_id.is_empty(), "Missing native edge identity");
    Ok((roles, edge))
}

fn role_ids(roles: &[AcceptedRole], planet: &str) -> BTreeSet<String> {
    roles
        .iter()
        .filter(|r| r.planet == planet)
        .map(|r| r.selected_id.clone())
        .collect()
}

fn qualities(edge: &NativeEdge, polarity: &str) -> Result<BTreeSet<String>> {
    let value = serde_json::to_value(&edge.reception.qualities)?;
    value
        .as_array()
        .context("Native qualities are not an array")?
        .iter()
        .filter(|q| q["polarity"] == polarity)
        .map(|q| {
            q["dignity"]
                .as_str()
                .context("Missing native dignity")
                .map(str::to_owned)
        })
        .collect::<Result<_>>()
}

fn check_array(
    prediction: &Prediction,
    field: &str,
    expected: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    let parsed = optimize::text(prediction, field)
        .and_then(|text| serde_json::from_str::<Vec<String>>(text).map_err(anyhow::Error::from));
    match parsed {
        Ok(values)
            if values.iter().collect::<BTreeSet<_>>().len() == values.len()
                && values.iter().cloned().collect::<BTreeSet<_>>() == *expected => {}
        Ok(_) => errors.push(format!(
            "{field}: differs from exact supplied native direction/qualities"
        )),
        Err(error) => errors.push(format!(
            "{field}: native JSON string-array rejected: {error}"
        )),
    }
}

/// Compare against actual native inputs only. A received semantic/format error
/// is observed rejection, while invalid source inputs remain authoring errors.
pub fn native_failures(input: &Example, prediction: &Prediction) -> Result<Vec<String>> {
    let (roles, edge) = supplied(input)?;
    let mut errors = Vec::new();
    if !optimize::text(prediction, "edge_id").is_ok_and(|id| id == edge.fact_id) {
        errors.push("edge_id: differs from the supplied native fact".into());
    }
    check_array(
        prediction,
        "regarding_actor_ids",
        &role_ids(&roles, &edge.reception.guest_planet),
        &mut errors,
    );
    check_array(
        prediction,
        "regarded_actor_ids",
        &role_ids(&roles, &edge.reception.host_planet),
        &mut errors,
    );
    check_array(
        prediction,
        "positive_qualities",
        &qualities(&edge, "positive")?,
        &mut errors,
    );
    check_array(
        prediction,
        "negative_qualities",
        &qualities(&edge, "negative")?,
        &mut errors,
    );
    for field in ["interpretation", "limitations"] {
        if !optimize::text(prediction, field).is_ok_and(|text| !text.trim().is_empty()) {
            errors.push(format!(
                "{field}: missing contextual interpretation or limits"
            ));
        }
    }
    Ok(errors)
}

pub fn source(book: &Book) -> Result<String> {
    book.pages(&BTreeSet::from(SOURCE_PAGES))
}

/// Selection policy: the target person's native planet regarding the principal
/// person's native planet. Neither case gold/branch nor a desired quality chooses
/// the edge. The four initial probes retain their original training membership.
pub fn pilot_fixtures(cases: &[catalogue::Case], book: &Book) -> Result<Vec<Fixture>> {
    PILOT_CASES.iter().map(|id| {
        let case = cases.iter().find(|c| c.id == *id).context("Missing original training probe")?;
        ensure!(case.split == "train" && case.method == "contact", "Reception pilot membership changed");
        let reading = catalogue::example(case, "reading", book)?;
        let chart: Value = serde_json::from_str(reading.get("chart_evidence").and_then(Value::as_str).context("No native chart")?)?;
        let view = &chart["role_reference"];
        let rows = view["role_rows"].as_array().context("No calculated role references")?;
        let planet = |role: &str| -> Result<&str> {
            rows.iter().find(|r| r["role_id"] == role).context("Missing declared role")?["ruler_planet"].as_str().context("Unresolved role planet")
        };
        let guest = planet("target.person")?;
        let host = planet("principal.person")?;
        let edges: Vec<_> = view["directed_reception"].as_array().context("No directed native edges")?.iter()
            .filter(|e| e["guest_planet"] == guest && e["host_planet"] == host).collect();
        ensure!(edges.len() == 1, "Target-to-principal native edge is unavailable or ambiguous");
        let e = edges[0];
        let roles = view["aliases"].as_array().context("No native actor aliases")?.iter().enumerate().map(|(role_index, a)| {
            Ok(AcceptedRole { role_index,
                selected_id:a["role_id"].as_str().context("No accepted alias ID")?.into(),
                label:a["label"].as_str().context("No accepted label")?.into(),
                house:a["significator_house"].as_u64().map(u8::try_from).transpose()?,
                planet:a["planet"].as_str().context("No accepted alias planet")?.into() })
        }).collect::<Result<Vec<_>>>()?;
        let edge = NativeEdge {fact_id:e["fact_id"].as_str().context("No native edge ID")?.into(),
            reception:serde_json::from_value(json!({"guestPlanet":e["guest_planet"],"hostPlanet":e["host_planet"],"qualities":e["qualities"]}))?};
        let source = source(book)?;
        let mut example = dsrust::input!{source_text:source,question:case.question,accepted_roles:serde_json::to_string(&roles)?,native_edge:serde_json::to_string(&edge)?};
        example.set("case_id", json!(case.id));
        example.set("expected", json!({"source_pages":SOURCE_PAGES,
            "task":"Qualified contextual interpretation of one supplied directed native reception",
            "must_explain":["the regarding actor's inclination toward the regarded actor, preserving direction and context", "mixed native positive and negative qualities without cancellation or a fabricated strength sum", "no automatic reciprocity, ability, occurrence, outcome or timing"],
            "must_not":["reverse or silently replace the edge", "infer an event, deadline or delivery", "invent qualifications, a psychological diagnosis or an identity conflict"]}));
        example.set("__review_source", json!(source));
        example.set("__input_authority", json!("Independently supplied component consultation and native calculated role references; no classification/extraction journey observed"));
        Ok(Fixture {split:case.split.clone(),example})
    }).collect()
}

pub fn signature() -> dsrust::signature::Signature {
    InterpretReception::signature()
}
pub fn judge_signature() -> dsrust::signature::Signature {
    JudgeReception::signature()
}

pub fn prepare(cases: &[catalogue::Case], book: &Book, directory: &Path) -> Result<Value> {
    prepare_impl(cases, book, directory, false)
}

/// These diagnostic round pools contain only the four original training probes.
/// Original case membership is neither changed nor represented as held-out gold.
pub fn optimization_fixtures(cases: &[catalogue::Case], book: &Book) -> Result<Vec<Fixture>> {
    let mut fixtures = pilot_fixtures(cases, book)?;
    for (fixture, split) in fixtures
        .iter_mut()
        .zip(["train", "train", "development", "test"])
    {
        fixture.split = split.to_owned();
    }
    Ok(fixtures)
}

pub fn prepare_optimization(
    cases: &[catalogue::Case],
    book: &Book,
    directory: &Path,
) -> Result<Value> {
    prepare_impl(cases, book, directory, true)
}

fn prepare_impl(
    cases: &[catalogue::Case],
    book: &Book,
    directory: &Path,
    optimizing: bool,
) -> Result<Value> {
    ensure!(
        !directory.exists(),
        "Preserve the earlier reception preparation"
    );
    let fixtures = if optimizing {
        optimization_fixtures(cases, book)?
    } else {
        pilot_fixtures(cases, book)?
    };
    let predictor = optimize::plain(signature(), Arc::new(dsrust::DummyLM::new(vec![])))
        .with_config(LmConfig {
            max_tokens: Some(1024),
            temperature: Some(0.),
            ..Default::default()
        });
    let requests = crate::preflight::reader_requests(&fixtures, &predictor)?;
    let inputs = fixtures
        .iter()
        .map(|f| {
            Ok(json!({"case_id":f.example.get("case_id"),
        "split":f.split,"inputs":f.example.inputs()?.rendered()}))
        })
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir(directory)?;
    crate::queue::write_json_atomic(&directory.join("wire-preflight.json"), &json!(requests))?;
    crate::queue::write_json_atomic(&directory.join("declared-inputs.json"), &json!(inputs))?;
    fs::write(
        directory.join("baseline-instructions.txt"),
        &predictor.signature.instructions,
    )?;
    let plan = json!({"phase":if optimizing{"reception_gepa_preparation"}else{"reception_preparation"},"function":"reception.interpretation",
        "case_ids":PILOT_CASES,"original_bank_count":cases.len(),
        "original_bank_sha256":crate::google::digest(serde_json::to_vec(cases)?),
        "fixtures_sha256":optimize::fixtures_digest(&fixtures)?,"book_sha256":crate::book::OCR_SHA256,
        "executable_sha256":crate::google::digest(fs::read(std::env::current_exe()?)?),
        "teaching_sources_sha256":optimize::teaching_sources(),
        "wire_preflight_sha256":crate::google::digest(fs::read(directory.join("wire-preflight.json"))?),
        "declared_inputs_sha256":crate::google::digest(fs::read(directory.join("declared-inputs.json"))?),
        "baseline_instructions_sha256":crate::google::digest(&predictor.signature.instructions),
        "source_pages":SOURCE_PAGES,"edge_selection":"Unique target.person planet regarding principal.person planet from the native helper; not a gold edge ID or desired quality",
        "actor_id_contract":"AcceptedReceptionRole.selected_id; all planet aliases remain allowed/required, including collisions",
        "input_authority":"Declared component consultation and calculated role references, not an observed classifier/extractor journey",
        "metric":"Exact native ID/direction/quality checks are noncompensating; teacher grades contextual_meaning only",
        "student_generation_tokens":1024,"physical_bounds":{"student":if optimizing{GEPA_LOCAL_CALLS}else{4},"teacher":if optimizing{GEPA_TEACHER_CALLS}else{8}},
        "max_metric_calls_soft":if optimizing{Some(GEPA_METRICS)}else{None},
        "round_pools":if optimizing{json!({"reflection":&PILOT_CASES[..2],"development_selection":[PILOT_CASES[2]],"final_diagnostic_check":[PILOT_CASES[3]]})}else{Value::Null},
        "original_training_only":true,"original_development_or_reserved_validation_used":false,
        "round_pool_limit":"One-case selection/check pools are diagnostic; no untouched validation claim. Original bank and original membership are preserved.",
        "bootstrap_calls":0,"reflection_calls":0,"selection_calls":0,"provider_calls":0,
        "original_split_preserved":true,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("preparation.json"), &plan)?;
    Ok(plan)
}

fn verify_preparation(
    cases: &[catalogue::Case],
    fixtures: &[Fixture],
    prepared: &Path,
    optimizing: bool,
) -> Result<()> {
    let plan: Value = serde_json::from_slice(&fs::read(prepared.join("preparation.json"))?)?;
    let phase = if optimizing {
        "reception_gepa_preparation"
    } else {
        "reception_preparation"
    };
    ensure!(
        plan["phase"] == phase
            && plan["case_ids"] == json!(PILOT_CASES)
            && plan["original_bank_sha256"] == crate::google::digest(serde_json::to_vec(cases)?)
            && plan["fixtures_sha256"] == optimize::fixtures_digest(fixtures)?
            && plan["executable_sha256"]
                == crate::google::digest(fs::read(std::env::current_exe()?)?)
            && plan["teaching_sources_sha256"] == json!(optimize::teaching_sources())
            && plan["baseline_instructions_sha256"]
                == crate::google::digest(signature().instructions)
            && plan["source_pages"] == json!(SOURCE_PAGES),
        "Prepared reception sources or inputs differ"
    );
    for (file, key) in [
        ("wire-preflight.json", "wire_preflight_sha256"),
        ("declared-inputs.json", "declared_inputs_sha256"),
    ] {
        ensure!(
            plan[key] == crate::google::digest(fs::read(prepared.join(file))?),
            "Prepared reception artifact differs"
        );
    }
    Ok(())
}

/// A thin call into the existing stock GEPA compiler. No new search, recovery,
/// demonstration, or grading path is introduced here.
pub async fn optimize(
    cases: &[catalogue::Case],
    book: Arc<Book>,
    prepared: &Path,
    models: &crate::runner::Models,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !directory.join("result.json").exists() && !directory.join("observations.jsonl").exists(),
        "Preserve the earlier reception optimization"
    );
    let fixtures = optimization_fixtures(cases, &book)?;
    verify_preparation(cases, &fixtures, prepared, true)?;
    crate::queue::write_json_atomic(
        &directory.join("round-scope.json"),
        &json!({"source_coverage_repair":[74,75],"fresh_baseline":true,
            "preparation_sha256":crate::google::digest(fs::read(prepared.join("preparation.json"))?),
            "original_training_only":true,"reflection_cases":&PILOT_CASES[..2],
            "selection_case":PILOT_CASES[2],"final_diagnostic_check_case":PILOT_CASES[3],
            "one_case_selection_and_check_are_not_reserved_validation":true,
            "student_call_cap":GEPA_LOCAL_CALLS,"teacher_call_cap":GEPA_TEACHER_CALLS,
            "max_metric_calls_soft":GEPA_METRICS,"bootstrap_demos":0,
            "evaluation_concurrency":1,"whole_reading_unobserved":true,
            "previous_scores_or_optimizer_state_imported":false,"application_promoted":false}),
    )?;
    optimize::compile_fixtures(
        &fixtures,
        "common",
        "reception",
        book,
        models,
        directory,
        optimize::CompileOptions {
            max_metrics: GEPA_METRICS,
            bootstrap_demos: 0,
        },
    )
    .await
}

pub async fn evaluate(
    cases: &[catalogue::Case],
    book: Arc<Book>,
    prepared: &Path,
    models: &crate::runner::Models,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !directory.join("result.json").exists() && !directory.join("observations.jsonl").exists(),
        "Preserve the earlier reception evaluation"
    );
    let fixtures = pilot_fixtures(cases, &book)?;
    verify_preparation(cases, &fixtures, prepared, false)?;
    let all = fixtures
        .iter()
        .map(|f| f.example.clone())
        .collect::<Vec<_>>();
    let mut program = optimize::Graded::new(
        "common",
        "reception",
        &all,
        models.student.clone(),
        models.teacher.clone(),
        Some(book),
    )?
    .with_stream(directory.join("observations.jsonl"))?;
    program.save(&directory.join("baseline-program.json"))?;
    let evaluation = Evaluate::new(
        all,
        |input| program.forward(input),
        optimize::Graded::metric,
    )
    .run()
    .await;
    program.save_observations(&directory.join("observations.json"))?;
    let grades = optimize::evaluation_receipt(&evaluation);
    let review_consistent = evaluation.results.iter().all(|r| {
        r.prediction
            .as_ref()
            .is_ok_and(|p| p.get("__review_summary_consistent") == Some(&json!(true)))
    });
    let full_pass = evaluation
        .results
        .iter()
        .filter(|r| {
            r.prediction.as_ref().is_ok_and(|p| {
                p.get("__passed") == Some(&json!(true))
                    && p.get("__review_summary_consistent") == Some(&json!(true))
            })
        })
        .count();
    let result = json!({"function":"reception.interpretation","optimizer":"stock Evaluate; no search",
        "teacher":models.teacher_id,"student":models.student_id,"case_ids":PILOT_CASES,"original_training_only":true,
        "grades":grades,"preparation_sha256":crate::google::digest(fs::read(prepared.join("preparation.json"))?),
        "execution_observed":evaluation.failure_count()==0,"bootstrap_calls":0,"reflection_calls":0,"selection_calls":0,
        "independent_review_consistent":review_consistent,"full_function_passes":full_pass,
        "whole_reading_unobserved":true,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        evaluation.failure_count() == 0,
        "An execution/review error remains unknown; no invented semantic zero"
    );
    Ok(result)
}

/// Declared, source-known component roles, resolved by the application's actual
/// native house/ruler assignment. This never reads Case.expected or claims an
/// observed classification, extraction, or native selection journey.
pub fn transfer_fixtures(
    contact: &[catalogue::Case],
    movable: &[catalogue::Case],
    book: &Book,
) -> Result<Vec<Fixture>> {
    use crate::native_evidence::{RoleChoice, assign_from_facts, facts, relationship_house};
    let mut result = Vec::new();
    for (index, id) in TRANSFER_CASES.iter().enumerate() {
        let bank = if index < 3 { contact } else { movable };
        let case = bank
            .iter()
            .find(|c| c.id == *id)
            .context("Missing original transfer probe")?;
        ensure!(
            case.split == "train"
                && case.method == if index < 3 { "contact" } else { "movable_deal" },
            "Transfer probe original membership differs"
        );
        let (quotes, declarations, guest_id, host_id) = if index < 3 {
            let (actor, label, relationship, quote) = match index {
                0 => (
                    "dena",
                    "Dena",
                    "neighbor",
                    "Dena is the next-door neighbour",
                ),
                1 => (
                    "finn",
                    "Finn",
                    "employer",
                    "Finn supervises my work and is my direct boss",
                ),
                _ => ("hana", "Hana", "partner", "Hana, my wife"),
            };
            let target = format!("{actor}.self");
            let house = relationship_house(relationship)
                .context("Unresolved native operative relationship")?;
            (
                vec![quote],
                vec![
                    ("querent.self".to_owned(), "You".to_owned(), 1),
                    (target.clone(), label.into(), house),
                ],
                if index == 0 {
                    target.clone()
                } else {
                    "querent.self".into()
                },
                if index == 0 {
                    "querent.self".into()
                } else {
                    target
                },
            )
        } else {
            // Native sell-profit policy: stock and the actual recipient's money
            // are distinct roles in the same second house. No extra actor is
            // invented. Both source clauses explicitly identify this speaker.
            (
                vec![
                    "I own and will sell my handmade lamps",
                    "I keep the proceeds",
                    "worthwhile profit for me",
                ],
                vec![
                    ("querent.self".into(), "You".into(), 1),
                    ("subject.primary".into(), "The handmade lamps".into(), 2),
                    ("deal.pocket".into(), "Your recipient money".into(), 2),
                ],
                "querent.self".into(),
                "subject.primary".into(),
            )
        };
        ensure!(
            quotes.iter().all(|quote| case.question.contains(quote)),
            "Declared operative role lacks its exact original source quote"
        );
        let at = chrono::DateTime::parse_from_rfc3339(&case.chart.utc)?;
        let chart = horary_ai_core::astronomy::chart(
            at.timestamp_millis() as f64,
            case.chart.latitude,
            case.chart.longitude,
        )
        .map_err(anyhow::Error::msg)?;
        let native = facts(Some(&chart));
        let choices = declarations.iter().map(|(_, label, house)| RoleChoice {
            label:label.clone(), house:Some(*house), natural:None,
            reason:"Declared source-known operative role; native house/ruler assignment, not an expected answer.".into(),
        }).collect();
        let bound = assign_from_facts(&native, choices).map_err(anyhow::Error::msg)?;
        ensure!(
            bound.len() == declarations.len(),
            "Native role assignment changed cardinality"
        );
        let roles = bound
            .into_iter()
            .zip(&declarations)
            .enumerate()
            .map(|(role_index, (role, (id, _, _)))| AcceptedRole {
                role_index,
                selected_id: id.clone(),
                label: role.label,
                house: role.house,
                planet: role.planet,
            })
            .collect::<Vec<_>>();
        let planet = |id: &str| {
            roles
                .iter()
                .find(|r| r.selected_id == id)
                .map(|r| r.planet.as_str())
                .context("Missing declared edge endpoint")
        };
        let guest = planet(&guest_id)?;
        let host = planet(&host_id)?;
        if index == 3 {
            ensure!(
                planet("subject.primary")? == planet("deal.pocket")?,
                "The genuine second-house shared ruler is unavailable"
            );
        }
        let edges = native
            .iter()
            .filter(|fact| {
                fact.kind == "reception"
                    && fact
                        .reception
                        .as_ref()
                        .is_some_and(|edge| edge.guest_planet == guest && edge.host_planet == host)
            })
            .collect::<Vec<_>>();
        ensure!(
            edges.len() == 1,
            "{}: declared native edge {guest} → {host} has {} rows; do not fabricate it",
            case.id,
            edges.len()
        );
        let fact = edges[0];
        let edge = NativeEdge {
            fact_id: fact.id.clone(),
            reception: fact
                .reception
                .clone()
                .context("No typed native reception")?,
        };
        let source = source(book)?;
        let mut example = dsrust::input! {source_text:source,question:case.question,
        accepted_roles:serde_json::to_string(&roles)?,native_edge:serde_json::to_string(&edge)?};
        example.set("case_id", json!(case.id));
        example.set("expected", json!({"source_pages":SOURCE_PAGES,"task":"Qualified contextual meaning of only this supplied directed reception",
            "must_explain":["Preserve exact regarding/regarded roles and distinguish all native qualities", "Use qualified context; preserve object/money roles without literal human emotions", "Regard toward a person is not evidence of interest in a separate request or task"],
            "must_not":["Infer reciprocity, ability, action, an event verdict or timing", "Invent actor identity or merge shared-ruler roles", "Cancel contrary qualities or invent a net strength"]}));
        example.set("__review_source", json!(source));
        example.set("__input_authority", json!("Explicitly declared source-known component roles resolved through native assign_from_facts; not observed acquisition/selection"));
        example.set("__derivation", json!({"original_case_id":case.id,"original_split":case.split,"original_question_sha256":crate::google::digest(&case.question),
            "exact_source_quotes":quotes,"native_anchor":case.chart,"guest_role_id":guest_id,"host_role_id":host_id,
            "native_house_facts":native.iter().filter(|fact|fact.kind=="house"&&roles.iter().any(|role|role.house.is_some_and(|h|fact.label==format!("House {h}")))).collect::<Vec<_>>(),
            "role_policy":"Contact persons use actual operative relationship houses; sell-profit stock and recipient pocket both use their actual owner's/recipient's second house. IDs are declared production role IDs, never copied from Case.expected.",
            "role_policy_source_sha256":crate::google::digest(include_bytes!("../../../src-tauri/src/horary_role_options.rs")),
            "house_assignment_source_sha256":crate::google::digest(include_bytes!("../../../src-tauri/src/reading_method.rs")),
            "role_assignment_authority":"Declared operative roles from the exact original question and pinned native role/house policy; not an observed full selection clipboard or a new house-classification grade"}));
        result.push(Fixture {
            split: case.split.clone(),
            example,
        });
    }
    Ok(result)
}

fn transfer_instruction(program: &Path, closure: &Path) -> Result<String> {
    let bytes = fs::read(program)?;
    let closed: Value = serde_json::from_slice(&fs::read(closure)?)?;
    ensure!(
        closed["exit_code"] == 0
            && closed["unsettled_calls"] == 0
            && closed["application_promoted"] == false
            && closed["compiled_sha256"] == crate::google::digest(&bytes),
        "Candidate is not bound to its settled, preserved closure"
    );
    let state: dsrust::module::ProgramState = serde_json::from_slice(&bytes)?;
    ensure!(
        state.predictors.len() == 1,
        "Transfer only this single reception instruction"
    );
    let predictor = state
        .predictors
        .get("reception.interpretation")
        .context("Candidate is not the reception function")?;
    ensure!(
        predictor.demos.is_empty()
            && predictor.signature.fields
                == dsrust::module::SignatureState::of(&signature()).fields,
        "No demonstrations, incompatible fields, or foreign optimizer state may be imported"
    );
    let text = &predictor.signature.instructions;
    ensure!(
        !text.trim().is_empty()
            && closed["selected_instruction_sha256"] == crate::google::digest(text),
        "Selected instruction identity differs"
    );
    Ok(text.clone())
}

fn transfer_predictor(instruction: &str) -> dsrust::Predict {
    let mut sig = signature();
    sig.instructions = instruction.to_owned();
    optimize::plain(sig, Arc::new(dsrust::DummyLM::new(vec![]))).with_config(LmConfig {
        max_tokens: Some(TRANSFER_OUTPUT_TOKENS),
        temperature: Some(0.),
        ..Default::default()
    })
}

/// Render both ordinary ChatAdapter arms. Only the learned instruction differs;
/// all questions, declared native roles, exact edges and sources are identical.
pub fn prepare_transfer(
    contact: &[catalogue::Case],
    movable: &[catalogue::Case],
    book: &Book,
    candidate: &Path,
    closure: &Path,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !directory.exists(),
        "Preserve the earlier transfer preparation"
    );
    let fixtures = transfer_fixtures(contact, movable, book)?;
    let candidate_text = transfer_instruction(candidate, closure)?;
    let mut requests = Vec::new();
    for (arm, instruction) in [
        ("baseline", signature().instructions),
        ("candidate", candidate_text.clone()),
    ] {
        for mut request in
            crate::preflight::reader_requests(&fixtures, &transfer_predictor(&instruction))?
        {
            request["arm"] = json!(arm);
            requests.push(request);
        }
    }
    fs::create_dir(directory)?;
    crate::queue::write_json_atomic(&directory.join("wire-preflight.json"), &json!(requests))?;
    crate::queue::write_json_atomic(&directory.join("declared-inputs.json"), &json!(fixtures.iter().map(|f| Ok(json!({"case_id":f.example.get("case_id"),"split":f.split,"inputs":f.example.inputs()?.rendered(),"derivation":f.example.get("__derivation")}))).collect::<Result<Vec<_>>>()?))?;
    fs::write(
        directory.join("candidate-instructions.txt"),
        &candidate_text,
    )?;
    fs::write(
        directory.join("baseline-instructions.txt"),
        &signature().instructions,
    )?;
    let plan = json!({"phase":"hosted_reception_transfer_preparation","function":"reception.interpretation","case_ids":TRANSFER_CASES,
        "original_contact_bank_count":contact.len(),"original_movable_deal_bank_count":movable.len(),
        "contact_bank_sha256":crate::google::digest(serde_json::to_vec(contact)?),"movable_deal_bank_sha256":crate::google::digest(serde_json::to_vec(movable)?),
        "fixtures_sha256":optimize::fixtures_digest(&fixtures)?,"book_sha256":crate::book::OCR_SHA256,"source_pages":SOURCE_PAGES,
        "executable_sha256":crate::google::digest(fs::read(std::env::current_exe()?)?),"teaching_sources_sha256":optimize::teaching_sources(),
        "candidate_program_sha256":crate::google::digest(fs::read(candidate)?),"candidate_closure_sha256":crate::google::digest(fs::read(closure)?),
        "candidate_instruction_sha256":crate::google::digest(&candidate_text),"baseline_instruction_sha256":crate::google::digest(signature().instructions),
        "wire_preflight_sha256":crate::google::digest(fs::read(directory.join("wire-preflight.json"))?),"declared_inputs_sha256":crate::google::digest(fs::read(directory.join("declared-inputs.json"))?),
        "student":crate::google::STUDENT,"teacher":"codex-saved-login-contextual-review","logical_student_calls":8,
        "physical_bounds":{"student":TRANSFER_STUDENT_CALLS,"teacher":TRANSFER_TEACHER_CALLS},"initial_input_tokens_per_minute":14000,
        "student_output_tokens":TRANSFER_OUTPUT_TOKENS,"teacher_metric":"contextual_meaning only; exact native direction/IDs/qualities are noncompensating",
        "provider_schema":false,"bootstrap_calls":0,"reflection_calls":0,"selection_calls":0,"generation_calls":0,"teacher_calls":0,
        "original_training_only":true,"original_bank_and_split_unchanged":true,"old_scores_or_optimizer_state_imported":false,
        "component_inputs":"Explicit declared source-known native roles/edges, not an observed classification/extraction or full native selection journey",
        "only_treatment":"Authored versus selected instruction; same four actual native inputs",
        "whole_reading_unobserved":true,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("preparation.json"), &plan)?;
    Ok(plan)
}

fn verify_transfer(
    contact: &[catalogue::Case],
    movable: &[catalogue::Case],
    book: &Book,
    prepared: &Path,
) -> Result<Vec<Fixture>> {
    let fixtures = transfer_fixtures(contact, movable, book)?;
    let plan: Value = serde_json::from_slice(&fs::read(prepared.join("preparation.json"))?)?;
    ensure!(
        plan["phase"] == "hosted_reception_transfer_preparation"
            && plan["case_ids"] == json!(TRANSFER_CASES)
            && plan["contact_bank_sha256"] == crate::google::digest(serde_json::to_vec(contact)?)
            && plan["movable_deal_bank_sha256"]
                == crate::google::digest(serde_json::to_vec(movable)?)
            && plan["fixtures_sha256"] == optimize::fixtures_digest(&fixtures)?
            && plan["book_sha256"] == crate::book::OCR_SHA256
            && plan["executable_sha256"]
                == crate::google::digest(fs::read(std::env::current_exe()?)?)
            && plan["teaching_sources_sha256"] == json!(optimize::teaching_sources())
            && plan["source_pages"] == json!(SOURCE_PAGES),
        "Transfer source/input identity differs"
    );
    ensure!(
        plan["student"] == crate::google::STUDENT
            && plan["physical_bounds"]
                == json!({"student":TRANSFER_STUDENT_CALLS,"teacher":TRANSFER_TEACHER_CALLS})
            && plan["logical_student_calls"] == 8
            && plan["student_output_tokens"] == TRANSFER_OUTPUT_TOKENS
            && plan["initial_input_tokens_per_minute"] == 14000
            && plan["provider_schema"] == false,
        "Transfer fixed provider configuration differs"
    );
    for (file, key) in [
        ("wire-preflight.json", "wire_preflight_sha256"),
        ("declared-inputs.json", "declared_inputs_sha256"),
        ("candidate-instructions.txt", "candidate_instruction_sha256"),
        ("baseline-instructions.txt", "baseline_instruction_sha256"),
    ] {
        ensure!(
            plan[key] == crate::google::digest(fs::read(prepared.join(file))?),
            "Prepared transfer artifact differs"
        );
    }
    Ok(fixtures)
}

fn verify_transfer_counts(prepared: &Path, counted: &Value) -> Result<()> {
    ensure!(
        counted["all_fit"] == true
            && counted["counted_requests"] == 8
            && counted["student"] == crate::google::STUDENT
            && counted["generation_calls"] == 0
            && counted["teacher_calls"] == 0
            && counted["preparation_sha256"]
                == crate::google::digest(fs::read(prepared.join("preparation.json"))?)
            && counted["wire_preflight_sha256"]
                == crate::google::digest(fs::read(prepared.join("wire-preflight.json"))?),
        "No matching exact Google token preflight"
    );
    let wires: Vec<Value> =
        serde_json::from_slice(&fs::read(prepared.join("wire-preflight.json"))?)?;
    let rows = counted["requests"]
        .as_array()
        .context("Missing counted requests")?;
    ensure!(
        wires.len() == 8 && rows.len() == 8,
        "Transfer count membership differs"
    );
    for (wire, row) in wires.iter().zip(rows) {
        let mut request: dsrust::lm::api::LmRequest =
            serde_json::from_value(wire["request"].clone())?;
        request.model = crate::google::STUDENT.into();
        ensure!(
            row["case_id"] == wire["case_id"]
                && row["arm"] == wire["arm"]
                && row["normalized_request_sha256"] == wire["request_sha256"]
                && row["provider_request_key"] == request.cache_key(crate::google::STUDENT)
                && row["generation_tokens"] == TRANSFER_OUTPUT_TOKENS
                && row["within_listed_limits"] == true
                && row["generation_submitted"] == false
                && row["teacher_called"] == false
                && row["input_tokens"].as_u64().is_some_and(|n| n > 0),
            "Google token count is not bound to this exact transfer request"
        );
    }
    Ok(())
}

pub async fn count_transfer(
    contact: &[catalogue::Case],
    movable: &[catalogue::Case],
    book: &Book,
    prepared: &Path,
    http: &reqwest::Client,
    directory: &Path,
) -> Result<Value> {
    verify_transfer(contact, movable, book, prepared)?;
    ensure!(
        !directory.exists(),
        "Preserve the earlier token count attempt"
    );
    fs::create_dir(directory)?;
    let listing = crate::google::models(http).await?;
    crate::queue::write_json_atomic(&directory.join("model-listing.json"), &listing)?;
    let info: crate::google::ModelInfo = serde_json::from_value(listing["student"].clone())?;
    let wires: Vec<Value> =
        serde_json::from_slice(&fs::read(prepared.join("wire-preflight.json"))?)?;
    let mut rows = Vec::new();
    for wire in wires {
        let mut request: dsrust::lm::api::LmRequest =
            serde_json::from_value(wire["request"].clone())?;
        request.model = crate::google::STUDENT.into();
        let count =
            crate::google::count_tokens(http, &request, &directory.join("requests")).await?;
        let tokens = count["input_tokens"]
            .as_u64()
            .context("No provider count")?;
        rows.push(json!({"case_id":wire["case_id"],"arm":wire["arm"],"normalized_request_sha256":wire["request_sha256"],
            "provider_request_key":request.cache_key(crate::google::STUDENT),"input_tokens":tokens,"generation_tokens":TRANSFER_OUTPUT_TOKENS,
            "within_listed_limits":tokens <= info.input_token_limit as u64 && TRANSFER_OUTPUT_TOKENS as usize <= info.output_token_limit,
            "generation_submitted":false,"teacher_called":false}));
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let result = json!({"preparation_sha256":crate::google::digest(fs::read(prepared.join("preparation.json"))?),
        "wire_preflight_sha256":crate::google::digest(fs::read(prepared.join("wire-preflight.json"))?),
        "student":crate::google::STUDENT,"model_input_limit":info.input_token_limit,"model_output_limit":info.output_token_limit,
        "counted_requests":rows.len(),"requests":rows,"all_fit":rows.iter().all(|r|r["within_listed_limits"]==true),
        "generation_calls":0,"teacher_calls":0,"provider_schema":false});
    crate::queue::write_json_atomic(&directory.join("token-preflight.json"), &result)?;
    ensure!(
        result["all_fit"] == true && result["counted_requests"] == 8,
        "Counted transfer request does not fit; no generation submitted"
    );
    Ok(result)
}

pub async fn compare_transfer(
    contact: &[catalogue::Case],
    movable: &[catalogue::Case],
    book: Arc<Book>,
    prepared: &Path,
    token_preflight: &Path,
    models: &crate::runner::Models,
    directory: &Path,
) -> Result<Value> {
    let fixtures = verify_transfer(contact, movable, &book, prepared)?;
    let counted: Value = serde_json::from_slice(&fs::read(token_preflight)?)?;
    verify_transfer_counts(prepared, &counted)?;
    ensure!(
        !directory.join("result.json").exists()
            && !directory.join("baseline.json").exists()
            && !directory.join("baseline-observations.jsonl").exists(),
        "Preserve the earlier transfer evaluation"
    );
    fs::create_dir_all(directory)?;
    let examples = fixtures
        .iter()
        .map(|f| f.example.clone())
        .collect::<Vec<_>>();
    let mut grades = Vec::new();
    for arm in ["baseline", "candidate"] {
        let instruction = fs::read_to_string(prepared.join(format!("{arm}-instructions.txt")))?;
        let mut program = optimize::Graded::new(
            "common",
            "reception",
            &examples,
            models.student.clone(),
            models.teacher.clone(),
            Some(book.clone()),
        )?
        .with_stream(directory.join(format!("{arm}-observations.jsonl")))?;
        let predictors = program.named_predictors();
        ensure!(
            predictors.len() == 1 && predictors[0].name == "reception.interpretation",
            "Unexpected reception program"
        );
        for p in predictors {
            p.signature.instructions = instruction.clone();
            p.config.max_tokens = Some(TRANSFER_OUTPUT_TOKENS);
        }
        program.save(&directory.join(format!("{arm}-program.json")))?;
        let evaluation = Evaluate::new(
            examples.clone(),
            |input| program.forward(input),
            optimize::Graded::metric,
        )
        .run()
        .await;
        let receipt = optimize::evaluation_receipt(&evaluation);
        crate::queue::write_json_atomic(&directory.join(format!("{arm}.json")), &receipt)?;
        program.save_observations(&directory.join(format!("{arm}-observations.json")))?;
        ensure!(
            evaluation.failure_count() == 0,
            "Received execution/review failure is preserved and unqualified; later arm withheld, not fabricated as zero"
        );
        grades.push(json!({"arm":arm,"grades":receipt,"full_function_passes":evaluation.results.iter().filter(|row|row.prediction.as_ref().is_ok_and(|p|p.get("__passed")==Some(&json!(true))&&p.get("__review_summary_consistent")==Some(&json!(true)))).count()}));
    }
    let result = json!({"function":"reception.interpretation","optimizer":"stock Evaluate, no search","student":models.student_id,"teacher":models.teacher_id,
        "case_ids":TRANSFER_CASES,"arms":grades,"preparation_sha256":crate::google::digest(fs::read(prepared.join("preparation.json"))?),
        "token_preflight_sha256":crate::google::digest(fs::read(token_preflight)?),"logical_student_calls":8,
        "physical_bounds":{"student":TRANSFER_STUDENT_CALLS,"teacher":TRANSFER_TEACHER_CALLS},"evaluation_concurrency":1,
        "old_scores_or_optimizer_state_imported":false,"reflection_calls":0,"selection_calls":0,"bootstrap_calls":0,"original_training_only":true,
        "whole_reading_unobserved":true,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Example {
        let roles = json!([
            {"role_index":0,"selected_id":"person.self","label":"Person","house":7,"planet":"Mars"},
            {"role_index":1,"selected_id":"querent.self","label":"Querent","house":1,"planet":"Saturn"},
            {"role_index":2,"selected_id":"reply","label":"Reply","house":3,"planet":"Saturn"}]);
        let edge = json!({"fact_id":"e42","reception":{"guestPlanet":"Mars","hostPlanet":"Saturn",
            "qualities":[{"dignity":"termRuler","polarity":"positive"},{"dignity":"fall","polarity":"negative"}]}});
        let mut input = dsrust::input! {source_text:"SYNTHETIC mechanics fixture, not a book quotation",question:"Will the person reply?",
        accepted_roles:serde_json::to_string(&roles).unwrap(),native_edge:serde_json::to_string(&edge).unwrap()};
        input.set("case_id", json!("synthetic-reception"));
        input.set("expected", json!({"task":"contextual meaning only"}));
        input
    }
    fn output() -> Example {
        dsrust::example! {edge_id:"e42",regarding_actor_ids:r#"["person.self"]"#,
        regarded_actor_ids:r#"["querent.self","reply"]"#,positive_qualities:r#"["termRuler"]"#,
        negative_qualities:r#"["fall"]"#,interpretation:"Synthetic contextual interpretation, not qualified source truth.",
        limitations:"This edge alone establishes neither reciprocity nor an event or timing."}
    }
    fn prediction() -> Prediction {
        Prediction::new(output(), "Synthetic test output")
    }

    #[test]
    fn native_direction_and_all_shared_role_ids_are_noncompensating() {
        let input = input();
        let mut p = prediction();
        assert!(native_failures(&input, &p).unwrap().is_empty());
        p.example
            .set("regarding_actor_ids", json!(r#"["querent.self","reply"]"#));
        p.example
            .set("regarded_actor_ids", json!(r#"["person.self"]"#));
        assert_eq!(native_failures(&input, &p).unwrap().len(), 2);
        p = prediction();
        p.example
            .set("regarded_actor_ids", json!(r#"["querent.self"]"#));
        assert_eq!(native_failures(&input, &p).unwrap().len(), 1);
        p = prediction();
        p.example
            .set("regarded_actor_ids", json!(r#"["reply","querent.self"]"#));
        assert!(native_failures(&input, &p).unwrap().is_empty());
    }
    #[test]
    fn qualities_cannot_be_added_cancelled_or_duplicated() {
        let input = input();
        let mut p = prediction();
        p.example
            .set("positive_qualities", json!(r#"["domicile"]"#));
        p.example.set("negative_qualities", json!("[]"));
        assert_eq!(native_failures(&input, &p).unwrap().len(), 2);
        p = prediction();
        p.example
            .set("positive_qualities", json!(r#"["termRuler","termRuler"]"#));
        assert_eq!(native_failures(&input, &p).unwrap().len(), 1);
        p = prediction();
        p.example.set("edge_id", json!("e41"));
        assert_eq!(native_failures(&input, &p).unwrap().len(), 1);
    }
    #[tokio::test]
    async fn scoped_semantic_pass_needs_no_whole_reading_or_answer() {
        let student = Arc::new(dsrust::DummyLM::new(vec![output()]));
        let teacher = Arc::new(dsrust::DummyLM::new(vec![dsrust::example! {
        score:1.,passed:true,contextual_meaning:"pass",feedback:"Synthetic scoped criterion met; not source qualification."}]));
        let program =
            optimize::Graded::new("common", "reception", &[input()], student, teacher, None)
                .unwrap();
        let p = program.forward(input()).await.unwrap();
        assert_eq!(p.get("__score"), Some(&json!(1.)));
        assert_eq!(p.get("__passed"), Some(&json!(true)));
    }
    #[tokio::test]
    async fn fluent_teacher_pass_cannot_compensate_for_reversed_native_roles() {
        let mut out = output();
        out.set("regarding_actor_ids", json!(r#"["querent.self","reply"]"#));
        let student = Arc::new(dsrust::DummyLM::new(vec![out]));
        let teacher = Arc::new(dsrust::DummyLM::new(vec![dsrust::example! {
        score:1.,passed:true,contextual_meaning:"pass",feedback:"Synthetic teacher false positive retained."}]));
        let program =
            optimize::Graded::new("common", "reception", &[input()], student, teacher, None)
                .unwrap();
        let p = program.forward(input()).await.unwrap();
        assert_eq!(p.get("__score"), Some(&json!(0.)));
        assert_eq!(p.get("__passed"), Some(&json!(false)));
        assert!(
            p.get("__feedback")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("regarding_actor_ids")
        );
    }
    #[tokio::test]
    async fn unobserved_context_is_not_invented_partial_credit() {
        let student = Arc::new(dsrust::DummyLM::new(vec![output()]));
        let teacher = Arc::new(dsrust::DummyLM::new(vec![dsrust::example! {
        score:0.8,passed:false,contextual_meaning:"unobserved",feedback:"No contextual observation available."}]));
        let program =
            optimize::Graded::new("common", "reception", &[input()], student, teacher, None)
                .unwrap();
        assert!(
            program
                .forward(input())
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("unobserved")
        );
        assert!(
            program
                .forward(input())
                .await
                .unwrap_err()
                .to_string()
                .contains("no new call submitted")
        );
    }
    #[test]
    fn derived_training_probes_do_not_receive_branch_verdict_or_event_gold() {
        let book = Book::fixture();
        let mut cases = catalogue::cases(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .as_path(),
            "contact",
        )
        .unwrap();
        let before = pilot_fixtures(&cases, &book).unwrap();
        assert_eq!(before.len(), 4);
        assert!(before.iter().all(|f| f.split == "train"));
        for case in &mut cases {
            case.expected.facet = "arbitrary-private-facet".into();
            case.expected.must_explain = vec!["arbitrary private verdict".into()];
            case.expected.must_not = vec!["private".into()];
            case.expected.source_pages = vec![137, 138];
        }
        let after = pilot_fixtures(&cases, &book).unwrap();
        for (a, b) in before.iter().zip(&after) {
            let a = a.example.inputs().unwrap();
            let b = b.example.inputs().unwrap();
            assert_eq!(a.rendered(), b.rendered());
            assert_eq!(a.fields().count(), 4);
            assert!(a.get("expected").is_none());
            assert!(
                a.get("native_edge")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .contains("guestPlanet")
            );
            assert!(
                a.get("source_text")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .contains("printed_page=\"71\"")
            );
        }
        let c = cases.iter_mut().find(|c| c.id == PILOT_CASES[0]).unwrap();
        c.split = "development".into();
        assert!(
            pilot_fixtures(&cases, &book)
                .err()
                .unwrap()
                .to_string()
                .contains("membership")
        );
    }

    #[test]
    fn gepa_round_pools_preserve_original_training_membership_and_exact_inputs() {
        let book = Book::fixture();
        let cases = catalogue::cases(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"),
            "contact",
        )
        .unwrap();
        let original = serde_json::to_vec(&cases).unwrap();
        let pilot = pilot_fixtures(&cases, &book).unwrap();
        let round = optimization_fixtures(&cases, &book).unwrap();
        assert_eq!(cases.len(), 20);
        assert_eq!(
            round.iter().map(|f| f.split.as_str()).collect::<Vec<_>>(),
            ["train", "train", "development", "test"]
        );
        for (a, b) in pilot.iter().zip(&round) {
            assert_eq!(a.split, "train");
            assert_eq!(
                a.example.inputs().unwrap().rendered(),
                b.example.inputs().unwrap().rendered()
            );
            assert_eq!(a.example.get("expected"), b.example.get("expected"));
        }
        assert_eq!(serde_json::to_vec(&cases).unwrap(), original);
        assert_eq!(
            source(&book).unwrap(),
            book.pages(&BTreeSet::from([44, 71, 72, 73, 74, 75]))
                .unwrap()
        );
    }

    #[test]
    fn gepa_preparation_is_bound_to_its_round_phase_sources_and_native_inputs() {
        let book = Book::fixture();
        let cases = catalogue::cases(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"),
            "contact",
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("prepared");
        let prepared = prepare_optimization(&cases, &book, &directory).unwrap();
        assert_eq!(prepared["phase"], "reception_gepa_preparation");
        assert_eq!(
            prepared["physical_bounds"],
            json!({"student":32,"teacher":48})
        );
        assert_eq!(prepared["max_metric_calls_soft"], 24);
        assert_eq!(prepared["provider_calls"], 0);
        assert_eq!(prepared["original_training_only"], true);
        let mut fixtures = optimization_fixtures(&cases, &book).unwrap();
        verify_preparation(&cases, &fixtures, &directory, true).unwrap();
        assert!(verify_preparation(&cases, &fixtures, &directory, false).is_err());
        fixtures[0]
            .example
            .set("native_edge", json!("changed native input"));
        assert!(verify_preparation(&cases, &fixtures, &directory, true).is_err());
        assert!(prepare_optimization(&cases, &book, &directory).is_err());
    }

    fn transfer_banks() -> (Vec<catalogue::Case>, Vec<catalogue::Case>) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        (
            catalogue::cases(&path, "contact").unwrap(),
            catalogue::cases(&path, "movable_deal").unwrap(),
        )
    }

    #[test]
    fn transfer_uses_source_native_production_ids_reverse_edges_and_genuine_shared_roles() {
        let (contact, movable) = transfer_banks();
        let fixtures = transfer_fixtures(&contact, &movable, &Book::fixture()).unwrap();
        assert_eq!(fixtures.len(), 4);
        for fixture in &fixtures {
            assert_eq!(fixture.split, "train");
            assert_eq!(
                fixture
                    .example
                    .inputs()
                    .unwrap()
                    .fields()
                    .map(|(k, _)| k)
                    .collect::<Vec<_>>(),
                ["source_text", "question", "accepted_roles", "native_edge"]
            );
            let (roles, edge) = supplied(&fixture.example).unwrap();
            assert!(!role_ids(&roles, &edge.reception.guest_planet).is_empty());
            assert!(!role_ids(&roles, &edge.reception.host_planet).is_empty());
            assert!(
                !roles.iter().any(
                    |r| r.selected_id == "target.person" || r.selected_id == "principal.person"
                )
            );
        }
        let (roles, edge) = supplied(&fixtures[0].example).unwrap();
        assert_eq!(
            role_ids(&roles, &edge.reception.guest_planet),
            BTreeSet::from(["dena.self".into()])
        );
        let (roles, edge) = supplied(&fixtures[1].example).unwrap();
        assert_eq!(
            role_ids(&roles, &edge.reception.guest_planet),
            BTreeSet::from(["querent.self".into()])
        );
        assert_eq!(
            role_ids(&roles, &edge.reception.host_planet),
            BTreeSet::from(["finn.self".into()])
        );
        let (roles, edge) = supplied(&fixtures[2].example).unwrap();
        assert_eq!(
            role_ids(&roles, &edge.reception.host_planet),
            BTreeSet::from(["hana.self".into()])
        );
        let (roles, edge) = supplied(&fixtures[3].example).unwrap();
        assert_eq!(
            role_ids(&roles, &edge.reception.host_planet),
            BTreeSet::from(["subject.primary".into(), "deal.pocket".into()])
        );
        let mut prediction = dsrust::example! {edge_id:edge.fact_id,
        regarding_actor_ids:serde_json::to_string(&role_ids(&roles,&edge.reception.guest_planet)).unwrap(),
        regarded_actor_ids:serde_json::to_string(&role_ids(&roles,&edge.reception.host_planet)).unwrap(),
        positive_qualities:serde_json::to_string(&qualities(&edge,"positive").unwrap()).unwrap(),
        negative_qualities:serde_json::to_string(&qualities(&edge,"negative").unwrap()).unwrap(),
        interpretation:"Synthetic mechanics text, not a qualified judgment",limitations:"Synthetic mechanics text"};
        assert!(
            native_failures(
                &fixtures[3].example,
                &Prediction::new(prediction.clone(), "Synthetic mechanics response")
            )
            .unwrap()
            .is_empty()
        );
        prediction.set("regarded_actor_ids", json!(r#"["subject.primary"]"#));
        assert!(
            !native_failures(
                &fixtures[3].example,
                &Prediction::new(prediction, "Synthetic mechanics response")
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn transfer_inputs_ignore_all_private_gold_but_require_original_source_quotes() {
        let (mut contact, mut movable) = transfer_banks();
        let book = Book::fixture();
        let original = transfer_fixtures(&contact, &movable, &book).unwrap();
        for case in contact.iter_mut().chain(&mut movable) {
            case.expected.facts.clear();
            case.expected.missing = vec!["Private fake missing fact".into()];
            case.expected.facet = "Private fake facet".into();
            case.expected.source_pages = vec![99];
            case.expected.must_explain = vec!["Private fake role or verdict".into()];
        }
        let changed = transfer_fixtures(&contact, &movable, &book).unwrap();
        for (a, b) in original.iter().zip(changed) {
            assert_eq!(
                a.example.inputs().unwrap().rendered(),
                b.example.inputs().unwrap().rendered()
            );
        }
        contact
            .iter_mut()
            .find(|c| c.id == TRANSFER_CASES[1])
            .unwrap()
            .question = "Finn has an unspecified relationship.".into();
        assert!(
            transfer_fixtures(&contact, &movable, &book)
                .err()
                .unwrap()
                .to_string()
                .contains("source quote")
        );
    }

    #[tokio::test]
    async fn transfer_preflight_is_the_actual_fixed_2048_predict_request() {
        let (contact, movable) = transfer_banks();
        let fixtures = transfer_fixtures(&contact, &movable, &Book::fixture()).unwrap();
        struct Capture(std::sync::Mutex<Option<dsrust::lm::api::LmRequest>>);
        impl dsrust::lm::ChatModel for Capture {
            async fn forward(
                &self,
                _: &reqwest::Client,
                request: &dsrust::lm::api::LmRequest,
            ) -> Result<dsrust::lm::api::LmResponse> {
                *self.0.lock().unwrap() = Some(request.clone());
                Ok(dsrust::lm::api::LmResponse {
                    outputs: vec![dsrust::lm::api::LmOutput::text(
                        "[[ ## edge_id ## ]]\nsynthetic\n[[ ## regarding_actor_ids ## ]]\n[]\n[[ ## regarded_actor_ids ## ]]\n[]\n[[ ## positive_qualities ## ]]\n[]\n[[ ## negative_qualities ## ]]\n[]\n[[ ## interpretation ## ]]\nSynthetic format check only\n[[ ## limitations ## ]]\nSynthetic format check only\n[[ ## completed ## ]]",
                    )],
                    ..Default::default()
                })
            }
            async fn capabilities(&self, _: &reqwest::Client) -> dsrust::lm::Capabilities {
                dsrust::lm::Capabilities::default()
            }
        }
        let lm = Arc::new(Capture(std::sync::Mutex::new(None)));
        let predictor = transfer_predictor("Synthetic candidate instruction").with_lm(lm.clone());
        let expected = crate::preflight::reader_requests(&fixtures, &predictor).unwrap();
        predictor
            .forward(fixtures[0].example.clone())
            .await
            .unwrap();
        let actual = lm.0.lock().unwrap().clone().unwrap();
        assert_eq!(actual.config.max_tokens, Some(2048));
        assert_eq!(expected[0]["generation_tokens"], 2048);
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            expected[0]["request"]
        );
        assert!(
            actual.model.is_empty() && actual.tools.is_empty() && actual.output_schema().is_none()
        );
    }

    #[test]
    fn transfer_counts_reject_changed_order_request_identity_and_generation_allowance() {
        let (contact, movable) = transfer_banks();
        let fixtures = transfer_fixtures(&contact, &movable, &Book::fixture()).unwrap();
        let mut wires = Vec::new();
        for arm in ["baseline", "candidate"] {
            for mut wire in
                crate::preflight::reader_requests(&fixtures, &transfer_predictor(arm)).unwrap()
            {
                wire["arm"] = json!(arm);
                wires.push(wire);
            }
        }
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("preparation.json"),
            b"Synthetic metadata check",
        )
        .unwrap();
        let wire_bytes = serde_json::to_vec(&wires).unwrap();
        fs::write(root.path().join("wire-preflight.json"), &wire_bytes).unwrap();
        let rows = wires.iter().map(|wire| {
            let mut request: dsrust::lm::api::LmRequest = serde_json::from_value(wire["request"].clone()).unwrap();
            request.model = crate::google::STUDENT.into();
            json!({"case_id":wire["case_id"],"arm":wire["arm"],"normalized_request_sha256":wire["request_sha256"],
                "provider_request_key":request.cache_key(crate::google::STUDENT),"generation_tokens":2048,
                "within_listed_limits":true,"generation_submitted":false,"teacher_called":false,"input_tokens":100})
        }).collect::<Vec<_>>();
        let receipt = json!({"all_fit":true,"counted_requests":8,"student":crate::google::STUDENT,
            "generation_calls":0,"teacher_calls":0,"requests":rows,
            "preparation_sha256":crate::google::digest(b"Synthetic metadata check"),
            "wire_preflight_sha256":crate::google::digest(&wire_bytes)});
        verify_transfer_counts(root.path(), &receipt).unwrap();
        for field in [
            "arm",
            "case_id",
            "provider_request_key",
            "normalized_request_sha256",
            "generation_tokens",
        ] {
            let mut changed = receipt.clone();
            changed["requests"][0][field] = json!("Changed synthetic metadata");
            assert!(
                verify_transfer_counts(root.path(), &changed).is_err(),
                "{field}"
            );
        }
        let mut changed = receipt.clone();
        changed["requests"].as_array_mut().unwrap().swap(0, 1);
        assert!(verify_transfer_counts(root.path(), &changed).is_err());
    }
}
