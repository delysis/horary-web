//! Source-reviewed worked answers. Rubrics and provenance are not demonstrations.
use crate::{
    book::Book, catalogue, codex, functions::Fixture, google, optimize, preflight, reading, runner,
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Example, Module, Predict,
    example::Prediction,
    lm::{DynChatModel, LmConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc, time::Duration};

pub const AUTHOR_CALL_CAP: usize = 3360;
pub const OUTPUT_FIELDS: [&str; 5] = ["status", "answer", "analysis", "evidence", "unresolved"];
const AUTHOR_INSTRUCTION: &str = "Produce the actual worked reading, not a rubric or checklist. Preserve the original enquiry and declared parties/capacities. Bind ordinary/turned/natural roles to supplied cusps and literal planets, separating role house from physical house and resolving Moon/shared-ruler collisions. Weigh relevant planet condition/motion and directed mixed receptions. Apply this branch's event/state/location/choice procedure: for events assess relevant occasions, prevention and order, without inventing intermediary identities, missing calculations or later receptions. An approximate or after-ingress candidate is conditional, not automatic occurrence. Time only after establishing an event using moving-perfection travel and defensible contextual units; astronomicalHours are not civil timing. Give a useful qualified direct answer first, concise decisive reasons in analysis, actual native IDs and printed citations in evidence, and genuine remaining limits in unresolved. Honest unavailable calculations can coexist with a useful provisional judgment. Do not force status=answer or exact dates. Return ONLY status, answer, analysis, evidence, unresolved in the requested field markers; no input echoes, rubric reproduction or hidden reasoning transcript. This is a declared component consultation, not an observed application journey.";
const REVIEW_INSTRUCTION: &str = "Review this actual worked reading against the supplied native rows and full primary book context. Reference_answer and rubric are distinct: the reference is an independently reviewed worked answer, the rubric is a set of criteria. During reference authoring no previous approved reference exists, so assess the proposed answer independently. Accept substantive equivalent readings and legitimate source-supported alternatives, not verbatim matching. Judge correct parties/roles, actual planet-bound conditions, directed/mixed receptions, this branch's relevant procedure and prevention, defensible timing and a coherent useful answer with material limits. Do not require absent calculations, every unrelated technique or every rubric sentence. Do not force an aspect for state/quality questions, exact calendar precision or status=answer. Do not fail a correct reading solely for Frawley's contextual love metaphor or style. Classification/extraction are supplied component inputs and unobserved here: only reading and answer_quality decide passed. If original rubric/software coverage expectations conflict with primary source or available calculations, preserve and identify that conflict in feedback, rather than falsifying the worked answer.";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkedAnswer {
    pub status: String,
    pub answer: String,
    pub analysis: String,
    pub evidence: String,
    pub unresolved: String,
}
impl WorkedAnswer {
    pub fn from_prediction(prediction: &Prediction) -> Result<Self> {
        serde_json::from_value(json!({
            "status":optimize::text(prediction,"status")?,"answer":optimize::text(prediction,"answer")?,
            "analysis":optimize::text(prediction,"analysis")?,"evidence":optimize::text(prediction,"evidence")?,
            "unresolved":optimize::text(prediction,"unresolved")?
        })).map_err(Into::into)
    }
    pub fn labeled_example(&self, student_inputs: &Example) -> Result<Example> {
        let mut example = student_inputs.inputs()?;
        for (name, value) in serde_json::to_value(self)?
            .as_object()
            .context("Answer is not an object")?
        {
            example.set(name, value.clone());
        }
        Ok(example)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoldRecord {
    pub case_id: String,
    pub split: String,
    pub input_hash: String,
    pub answer: WorkedAnswer,
    pub source_witnesses: Value,
    pub review: Value,
}

pub fn family_order() -> Vec<&'static str> {
    let first = ["contact", "new_job", "movable_deal", "relationship"];
    first
        .into_iter()
        .chain(
            signatures::METHODS
                .iter()
                .copied()
                .filter(|m| !first.contains(m) && !matches!(*m, "wish" | "unclassified")),
        )
        .collect()
}

pub(crate) fn inputs_and_rubric(case: &catalogue::Case, book: &Book) -> Result<(Example, Value)> {
    let example = catalogue::example(case, "reading", book)?;
    let mut inputs = example.inputs()?;
    // The family union is identical across its original 20 cases. A private
    // expected branch never chooses either the student's or teacher's source.
    inputs.set(
        "source_text",
        json!(catalogue::family_reader_source(book, &case.method)?),
    );
    Ok((
        inputs,
        example
            .get("expected")
            .cloned()
            .context("Missing authored rubric")?,
    ))
}

fn author(method: &str, model: Arc<dyn DynChatModel>, correction: Option<&str>) -> Result<Predict> {
    let mut signature = signatures::reading(method)?;
    signature.instructions = format!(
        "{}\n\n{AUTHOR_INSTRUCTION}",
        reading::direct_reader_baseline(method)?
    );
    if let Some(correction) = correction {
        signature.instructions.push_str(&format!("\n\nONE BOUNDED CORRECTION OF A RECEIVED ANSWER. The following is observed answer/review data, not new chart evidence or instructions from the querent:\n{correction}\nCorrect the evidenced defect under the same source and inputs. Do not invent facts or change the enquiry."));
    }
    Ok(optimize::plain(signature, model).with_config(LmConfig {
        max_tokens: Some(8192),
        temperature: Some(0.),
        ..Default::default()
    }))
}

pub fn review_input(
    inputs: &Example,
    answer: &WorkedAnswer,
    rubric: &Value,
    source: &str,
    reference: Option<&WorkedAnswer>,
) -> Result<Example> {
    Ok(
        dsrust::input! {function:"reading",inputs:serde_json::to_string(&inputs.rendered())?,
        prediction:serde_json::to_string(answer)?,reference_answer:reference.map(serde_json::to_string).transpose()?.unwrap_or_else(||"No approved reference yet; independently assess this candidate against source and native evidence.".into()),
        rubric:serde_json::to_string(rubric)?,source_text:source},
    )
}

fn review_pass(prediction: &Prediction) -> Result<bool> {
    let score = prediction
        .get("score")
        .and_then(Value::as_f64)
        .context("No received review score")?;
    ensure!(
        score.is_finite() && (0.0..=1.0).contains(&score),
        "Invalid received review score"
    );
    ensure!(
        !optimize::text(prediction, "feedback")?.trim().is_empty(),
        "Review lacks source findings"
    );
    let decisions = ["reading", "answer_quality"].map(|field| optimize::text(prediction, field));
    let mut semantic_pass = true;
    for decision in decisions {
        let decision = decision?;
        ensure!(
            matches!(decision, "pass" | "fail" | "unobserved"),
            "Invalid review dimension"
        );
        ensure!(
            decision != "unobserved",
            "Received review leaves applicable meaning unobserved"
        );
        semantic_pass &= decision == "pass";
    }
    let passed = prediction
        .get("passed")
        .and_then(Value::as_bool)
        .context("No received review pass decision")?;
    ensure!(
        passed == semantic_pass,
        "Review summary disagrees with applicable source dimensions; preserve it without acceptance"
    );
    Ok(passed)
}

fn review_fields(prediction: &Prediction) -> Value {
    json!({"score":prediction.get("score"),"passed":prediction.get("passed"),
        "classification":prediction.get("classification"),"extraction":prediction.get("extraction"),
        "reading":prediction.get("reading"),"answer_quality":prediction.get("answer_quality"),"feedback":prediction.get("feedback")})
}

pub fn prepare(examples: &Path, book: &Book, codex_path: &Path, directory: &Path) -> Result<Value> {
    ensure!(
        !directory.exists(),
        "Preserve the earlier gold authoring plan"
    );
    let mut families = Vec::new();
    for method in family_order() {
        let cases = catalogue::cases(examples, method)?;
        ensure!(
            cases.len() == 20,
            "Keep exactly twenty original cases per family"
        );
        let source = catalogue::reference_reader_source(book, method)?;
        let inputs = cases.iter().map(|case| {
            let (input,_) = inputs_and_rubric(case,book)?;
            Ok(json!({"case_id":case.id,"split":case.split,"input_hash":optimize::input_key(&input)?}))
        }).collect::<Result<Vec<_>>>()?;
        families.push(json!({"method":method,"case_bank_sha256":google::digest(serde_json::to_vec(&cases)?),
            "reference_source_sha256":google::digest(&source),"source_pages":catalogue::reference_reader_pages(method)?,"source_excerpt_pages":catalogue::reference_reader_excerpt_pages(method),"cases":inputs}));
    }
    ensure!(
        families.len() == 42,
        "Keep all forty-two substantive families"
    );
    fs::create_dir(directory)?;
    let plan = json!({"phase":"source-reviewed worked gold readings","families":families,"case_count":840,
        "executable_sha256":google::digest(fs::read(std::env::current_exe()?)?),"teaching_sources_sha256":optimize::teaching_sources(),
        "book_sha256":crate::book::OCR_SHA256,"physical_teacher_cap":AUTHOR_CALL_CAP,"max_author_attempts_per_case":2,
        "teacher":"codex-saved-login-no-override-tool-free","output_fields":OUTPUT_FIELDS,"teacher_output_tokens":8192,
        "codex_executable_sha256":google::digest(fs::read(codex_path)?),
        "source_scope":"Teacher receives full salient primary pages; student inputs retain the declared compact family source.",
        "rubrics_are_separate":true,"original_banks_and_splits_unchanged":true,"student_calls":0,
        "unknown_case_is_retained_while_distinct_cases_continue":true,"uncertain_keys_replayed":false});
    crate::queue::write_json_atomic(&directory.join("plan.json"), &plan)?;
    Ok(plan)
}

async fn generate_case(
    case: &catalogue::Case,
    book: &Book,
    model: Arc<dyn DynChatModel>,
    directory: &Path,
) -> Result<Option<GoldRecord>> {
    let (student_inputs, rubric) = inputs_and_rubric(case, book)?;
    let source = catalogue::reference_reader_source(book, &case.method)?;
    let mut teacher_inputs = student_inputs.clone();
    teacher_inputs.set("source_text", json!(source));
    crate::queue::write_json_atomic(
        &directory.join("student-inputs.json"),
        &json!(student_inputs.rendered()),
    )?;
    crate::queue::write_json_atomic(
        &directory.join("teacher-inputs.json"),
        &json!(teacher_inputs.rendered()),
    )?;
    crate::queue::write_json_atomic(&directory.join("rubric.json"), &rubric)?;
    let mut correction = None;
    for attempt in 1..=2 {
        let target = directory.join(format!("attempt-{attempt}"));
        fs::create_dir(&target)?;
        let mut trace = Vec::new();
        let outcome = author(&case.method, model.clone(), correction.as_deref())?
            .forward_traced(teacher_inputs.clone(), &mut trace)
            .await;
        let prediction = match outcome {
            Ok(prediction) => prediction,
            Err(error) => {
                let received = error.downcast_ref::<dsrust::predict::ObservedOutputFailure>();
                crate::queue::write_json_atomic(
                    &target.join("failure.json"),
                    &json!({"error":error.to_string(),"received_output":received.map(|v|&v.response),"semantic_grade":"unobserved","automatic_retry":false}),
                )?;
                if attempt == 1
                    && let Some(received) = received
                {
                    correction = Some(format!(
                        "Received output formatting failure: {}\nOriginal raw reply:\n{}",
                        received.reason,
                        received.response.first_text()
                    ));
                    continue;
                }
                return Ok(None);
            }
        };
        let answer = WorkedAnswer::from_prediction(&prediction)?;
        crate::queue::write_json_atomic(
            &target.join("answer.json"),
            &serde_json::to_value(&answer)?,
        )?;
        crate::queue::write_json_atomic(
            &target.join("trace.json"),
            &json!(trace.iter().map(|step| json!({"predictor":step.predictor,"inputs":step.inputs.rendered(),"outputs":step.outputs.rendered()})).collect::<Vec<_>>()),
        )?;
        let native = optimize::reading_native_failures(&student_inputs, &prediction)?;
        crate::queue::write_json_atomic(
            &target.join("native-check.json"),
            &json!({"failures":native}),
        )?;
        let mut judge = optimize::plain(signatures::auxiliary("judge")?, model.clone())
            .with_config(LmConfig {
                max_tokens: Some(8192),
                temperature: Some(0.),
                ..Default::default()
            });
        judge
            .signature
            .instructions
            .push_str(&format!("\n\n{REVIEW_INSTRUCTION}"));
        let review = judge
            .forward(review_input(
                &student_inputs,
                &answer,
                &rubric,
                &source,
                None,
            )?)
            .await?;
        let reviewed = review_fields(&review);
        crate::queue::write_json_atomic(&target.join("review.json"), &reviewed)?;
        let accepted = review_pass(&review)? && native.is_empty();
        if accepted {
            let record = GoldRecord {
                case_id: case.id.clone(),
                split: case.split.clone(),
                input_hash: optimize::input_key(&student_inputs)?,
                answer,
                source_witnesses: json!({"book_sha256":crate::book::OCR_SHA256,"printed_pages":catalogue::reference_reader_pages(&case.method)?,"excerpt_printed_pages":catalogue::reference_reader_excerpt_pages(&case.method),"teacher_source_sha256":google::digest(&source),"teacher_input_hash":optimize::input_key(&teacher_inputs)?,"student_source_sha256":google::digest(student_inputs.get("source_text").and_then(Value::as_str).context("No student source")?),"declared_component_inputs":true}),
                review: reviewed,
            };
            crate::queue::write_json_atomic(
                &directory.join("gold.json"),
                &serde_json::to_value(&record)?,
            )?;
            return Ok(Some(record));
        }
        correction = Some(format!(
            "Received worked answer:\n{}\nNative failures:\n{}\nIndependent source review:\n{}",
            serde_json::to_string(&answer)?,
            serde_json::to_string(&native)?,
            serde_json::to_string(&reviewed)?
        ));
    }
    Ok(None)
}

/// One ordinary serial authoring pass. Fresh per-case clients share the same
/// physical cap; an uncertain case never makes unrelated cases a replay.
pub async fn generate(
    examples: &Path,
    book: Arc<Book>,
    prepared: &Path,
    codex_path: &Path,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !directory.exists(),
        "Preserve previous gold outputs and uncertain operations"
    );
    let plan: Value = serde_json::from_slice(&fs::read(prepared.join("plan.json"))?)?;
    ensure!(
        plan["executable_sha256"] == google::digest(fs::read(std::env::current_exe()?)?)
            && plan["teaching_sources_sha256"] == json!(optimize::teaching_sources())
            && plan["book_sha256"] == crate::book::OCR_SHA256
            && plan["codex_executable_sha256"] == google::digest(fs::read(codex_path)?)
            && plan["physical_teacher_cap"] == AUTHOR_CALL_CAP,
        "Gold source/executable/cap identity differs"
    );
    let budget = google::CallBudget::new(AUTHOR_CALL_CAP);
    fs::create_dir(directory)?;
    let mut results = Vec::new();
    for method in family_order() {
        let cases = catalogue::cases(examples, method)?;
        let bound = plan["families"]
            .as_array()
            .context("No family plan")?
            .iter()
            .find(|p| p["method"] == method)
            .context("Missing planned family")?;
        ensure!(
            bound["case_bank_sha256"] == google::digest(serde_json::to_vec(&cases)?)
                && bound["reference_source_sha256"]
                    == google::digest(catalogue::reference_reader_source(&book, method)?),
            "Gold family source/corpus drift"
        );
        let family = directory.join(method);
        fs::create_dir(&family)?;
        let mut approved = Vec::new();
        let mut failures = Vec::new();
        for case in &cases {
            let expected = bound["cases"]
                .as_array()
                .context("No planned case inputs")?
                .iter()
                .find(|planned| planned["case_id"] == case.id)
                .context("Missing planned case")?;
            let (inputs, _) = inputs_and_rubric(case, &book)?;
            ensure!(
                expected["split"] == case.split
                    && expected["input_hash"] == optimize::input_key(&inputs)?,
                "Gold case input/split drift before authoring"
            );
            let target = family.join(&case.id);
            fs::create_dir(&target)?;
            let model: Arc<dyn DynChatModel> = Arc::new(
                codex::Codex::new(codex_path.to_path_buf(), target.join("teacher"), 4)?
                    .with_budget(budget.clone()),
            );
            let outcome = generate_case(case, &book, model, &target).await;
            match outcome {
                Ok(Some(record)) => {
                    eprintln!("gold {method} {} approved", case.id);
                    approved.push(record);
                }
                other => {
                    let failure = json!({"case_id":case.id,"split":case.split,"approved":false,"error":other.err().map(|e|e.to_string()),"source_or_semantic_grade":"retained in case attempts; absent evidence remains unobserved","uncertain_key_replayed":false});
                    crate::queue::write_json_atomic(&target.join("case-withheld.json"), &failure)?;
                    failures.push(failure);
                    eprintln!(
                        "gold {method} {} withheld; continuing distinct case",
                        case.id
                    );
                }
            }
        }
        let records = approved
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n")
            + "\n";
        fs::write(family.join("answers.jsonl"), records)?;
        let status = json!({"method":method,"original_cases":cases.len(),"approved":approved.len(),"failed_or_unknown":failures,"ready_for_student":approved.len()==20,"teacher_physical_spent_total":budget.spent(),"physical_teacher_cap":AUTHOR_CALL_CAP,"rubrics_are_not_demo_outputs":true});
        crate::queue::write_json_atomic(&family.join("result.json"), &status)?;
        results.push(status);
        crate::queue::write_json_atomic(
            &directory.join("progress.json"),
            &json!({"families":results,"teacher_physical_calls":budget.spent(),"student_calls":0}),
        )?;
    }
    let result = json!({"families":results,"teacher_physical_calls":budget.spent(),"physical_teacher_cap":AUTHOR_CALL_CAP,"student_calls":0,"planned_cases":840,"automatic_uncertain_replays":0});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    Ok(result)
}

/// Bind each worked reference to its unchanged real student inputs. Metadata
/// remains private; only TRAIN's four inputs and five actual outputs are labels.
pub fn reviewed_family(
    examples: &Path,
    book: &Book,
    campaign: &Path,
    method: &str,
) -> Result<(Vec<Fixture>, Vec<Example>)> {
    let cases = catalogue::cases(examples, method)?;
    ensure!(cases.len() == 20, "Keep twenty original family cases");
    let planned: Value = serde_json::from_slice(&fs::read(campaign.join("prepared/plan.json"))?)?;
    let bank = planned["families"]
        .as_array()
        .context("No original author family plan")?
        .iter()
        .find(|f| f["method"] == method)
        .context("Family not in author plan")?;
    ensure!(
        planned["book_sha256"] == crate::book::OCR_SHA256
            && bank["case_bank_sha256"] == google::digest(serde_json::to_vec(&cases)?),
        "Original author book/corpus differs"
    );
    let family = campaign.join("run").join(method);
    let result: Value = serde_json::from_slice(&fs::read(family.join("result.json"))?)?;
    ensure!(
        result["ready_for_student"] == true && result["approved"] == 20,
        "This family does not have twenty approved answers"
    );
    let source = catalogue::reference_reader_source(book, method)?;
    ensure!(
        bank["reference_source_sha256"] == google::digest(&source),
        "Reference teacher book context differs"
    );
    let records = fs::read_to_string(family.join("answers.jsonl"))?
        .lines()
        .filter(|line| !line.is_empty())
        .map(serde_json::from_str::<GoldRecord>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        records.len() == 20,
        "Keep all twenty independently reviewed actual answers"
    );
    let mut index = BTreeMap::new();
    for record in records {
        ensure!(
            index.insert(record.case_id.clone(), record).is_none(),
            "Duplicate worked answer identity"
        );
    }
    let mut fixtures = Vec::new();
    let mut labels = Vec::new();
    for case in &cases {
        let record = index
            .remove(&case.id)
            .context("Missing original case reference")?;
        let (inputs, _) = inputs_and_rubric(case, book)?;
        let mut teacher_inputs = inputs.clone();
        teacher_inputs.set("source_text", json!(&source));
        ensure!(
            record.split == case.split
                && record.input_hash == optimize::input_key(&inputs)?
                && record.source_witnesses["book_sha256"] == crate::book::OCR_SHA256
                && record.source_witnesses["teacher_source_sha256"] == google::digest(&source),
            "Worked answer input/split/source differs"
        );
        ensure!(
            record.source_witnesses["teacher_input_hash"] == optimize::input_key(&teacher_inputs)?
                && record.source_witnesses["student_source_sha256"]
                    == google::digest(
                        inputs
                            .get("source_text")
                            .and_then(Value::as_str)
                            .context("No student source")?
                    ),
            "Worked reference teacher/student source identities differ"
        );
        ensure!(
            record.review["passed"] == true
                && record.review["reading"] == "pass"
                && record.review["answer_quality"] == "pass",
            "Unapproved reference cannot label a student"
        );
        let disk: GoldRecord =
            serde_json::from_slice(&fs::read(family.join(&case.id).join("gold.json"))?)?;
        ensure!(
            serde_json::to_value(&disk)? == serde_json::to_value(&record)?,
            "Family bank differs from its individual approved record"
        );
        ensure!(
            received_answer_matches(&family.join(&case.id), &record.answer)?,
            "Worked reference lacks its received answer witness"
        );
        let label = record.answer.labeled_example(&inputs)?;
        let prediction = Prediction::new(label.clone(), "Source-reviewed worked reference");
        ensure!(
            optimize::reading_native_failures(&inputs, &prediction)?.is_empty(),
            "Worked reference has native format/citation failure"
        );
        if case.split == "train" {
            labels.push(label);
        }
        let mut example = catalogue::example(case, "reading", book)?;
        for (name, value) in inputs.fields() {
            example.set(name, value.clone());
        }
        example.set("__review_source", json!(&source));
        example.set(
            "__reference_answer",
            json!(serde_json::to_string(&record.answer)?),
        );
        fixtures.push(Fixture {
            split: case.split.clone(),
            example,
        });
    }
    ensure!(
        index.is_empty()
            && labels.len() == 12
            && fixtures.iter().filter(|f| f.split == "development").count() == 4
            && fixtures.iter().filter(|f| f.split == "test").count() == 4,
        "Original12/4/4 split differs"
    );
    Ok((fixtures, labels))
}

fn labeled_predictor(method: &str, labels: &[Example], count: usize) -> Result<Predict> {
    let mut predictor = Predict::from_signature(signatures::reading(method)?)
        .with_adapter(dsrust::adapter::ChatAdapter::without_json_fallback())
        .with_config(LmConfig {
            max_tokens: Some(8192),
            temperature: Some(0.),
            ..Default::default()
        });
    predictor.signature.instructions = reading::direct_reader_baseline(method)?;
    dsrust::LabeledFewShot::new(count).compile(&mut predictor, labels);
    for demo in &mut predictor.demos {
        *demo = crate::demo::compact(demo)?;
    }
    Ok(predictor)
}

async fn count_labeled_inputs(
    fixtures: &[Fixture],
    method: &str,
    labels: &[Example],
    http: &reqwest::Client,
    listing: &Value,
    directory: &Path,
) -> Result<Value> {
    fs::create_dir(directory)?;
    let predictor = labeled_predictor(method, labels, 2)?;
    let requests = preflight::reader_requests(fixtures, &predictor)?;
    crate::queue::write_json_atomic(&directory.join("requests.json"), &json!(requests))?;
    let limit = google::model_context_limit(listing, google::STUDENT)?;
    let mut counts = Vec::new();
    for (index, row) in requests.iter().enumerate() {
        let mut request: dsrust::lm::api::LmRequest =
            serde_json::from_value(row["request"].clone())?;
        request.model = google::STUDENT.into();
        let count =
            google::count_tokens(http, &request, &directory.join(format!("count-{index:02}")))
                .await?;
        let tokens = count["input_tokens"]
            .as_u64()
            .context("No exact provider count")? as usize;
        ensure!(
            tokens.saturating_add(8192) <= limit,
            "Two worked examples and generation do not fit the listed context"
        );
        counts.push(json!({"case_id":row["case_id"],"request_sha256":row["request_sha256"],"input_tokens":tokens,"generation_tokens":8192,"fits":true}));
    }
    let result = json!({"method":method,"optimizer":"upstream LabeledFewShot","labeled_training_cases":labels.len(),"demos":2,"input_context_limit":limit,"generation_tokens":8192,"counts":counts,"generation_submitted":false,"reference_and_rubric_private":true});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    Ok(result)
}

fn first_family_gate(campaign: &Path, gate: &Path) -> Result<()> {
    let receipt: Value = serde_json::from_slice(&fs::read(gate)?)?;
    ensure!(
        receipt["approved"] == true
            && receipt["book_sha256"] == crate::book::OCR_SHA256
            && receipt["author_plan_sha256"]
                == google::digest(fs::read(campaign.join("plan.json"))?),
        "Initial gold source approval differs"
    );
    let approvals = receipt["training_answers"]
        .as_object()
        .context("No three-training-answer approval")?;
    for case in [
        "dspy-contact-first-001",
        "dspy-contact-first-002",
        "dspy-contact-first-003",
    ] {
        let directory = campaign.join("run/contact").join(case);
        let bytes = fs::read(directory.join("gold.json"))?;
        let record: GoldRecord = serde_json::from_slice(&bytes)?;
        ensure!(
            record.split == "train",
            "Source gate may not use development/test content"
        );
        ensure!(
            approvals[case] == google::digest(bytes),
            "Initial training answer approval does not match the accepted answer"
        );
        ensure!(
            received_answer_matches(&directory, &record.answer)?,
            "Approved gold does not match a received answer"
        );
    }
    Ok(())
}

fn received_answer_matches(directory: &Path, answer: &WorkedAnswer) -> Result<bool> {
    for attempt in ["attempt-1", "attempt-2"] {
        let path = directory.join(attempt).join("answer.json");
        if path.exists() && serde_json::from_slice::<WorkedAnswer>(&fs::read(path)?)? == *answer {
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
pub async fn optimize_families(
    examples: &Path,
    book: Arc<Book>,
    campaign: &Path,
    approval_gate: &Path,
    selected: &str,
    http: reqwest::Client,
    codex_path: &Path,
    directory: &Path,
    watch: bool,
) -> Result<Value> {
    ensure!(
        !directory.exists(),
        "Preserve prior actual-answer runs; no uncertain replay"
    );
    let methods = if selected == "all" {
        family_order()
    } else {
        ensure!(
            family_order().contains(&selected),
            "Unknown substantive family"
        );
        vec![selected]
    };
    fs::create_dir(directory)?;
    let author_plan = fs::read(campaign.join("prepared/plan.json"))?;
    let listing = google::models(&http).await?;
    crate::queue::write_json_atomic(&directory.join("models.json"), &listing)?;
    let info: google::ModelInfo = serde_json::from_value(listing["student"].clone())?;
    let plan = json!({"method_order":methods,"author_plan_sha256":google::digest(&author_plan),"book_sha256":crate::book::OCR_SHA256,"executable_sha256":google::digest(fs::read(std::env::current_exe()?)?),"teaching_sources_sha256":optimize::teaching_sources(),"per_family_physical_student_cap":64,"per_family_physical_teacher_cap":80,"cumulative_student_cap":64*methods.len(),"cumulative_teacher_cap":80*methods.len(),"soft_metric_calls":24,"stock_optimizers":["LabeledFewShot","GEPA"],"demos":2,"original_split":"12/4/4","student":google::STUDENT,"student_output_tokens":8192,"teacher":"codex-saved-login-tool-free-no-override","teacher_output_tokens":8192,"google_rate_hint":14000,"student_concurrency":1,"approval_gate":approval_gate,"automatic_uncertain_replay":false,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("plan.json"), &plan)?;
    let limiter = google::RateLimiter::new(14000)?;
    let mut student_spent = 0;
    let mut teacher_spent = 0;
    let mut results = Vec::new();
    for method in &methods {
        let family_gold = campaign.join("run").join(method).join("result.json");
        while !family_gold.exists() {
            if !watch {
                anyhow::bail!("Family references not yet closed; no student calls");
            }
            if campaign.join("owner-exit.json").exists() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        if !family_gold.exists() {
            results.push(json!({"method":method,"status":"blocked","reason":"Author owner closed without this family's completed reference bank","student_calls":0}));
            crate::queue::write_json_atomic(
                &directory.join("progress.json"),
                &json!({"families":results,"student_physical_calls":student_spent,"teacher_physical_calls":teacher_spent}),
            )?;
            continue;
        }
        let ready: Value = serde_json::from_slice(&fs::read(&family_gold)?)?;
        if ready["ready_for_student"] != true {
            let failure = json!({"method":method,"status":"blocked","reason":"Not all twenty source-reviewed actual answers are approved","student_calls":0});
            results.push(failure);
            crate::queue::write_json_atomic(
                &directory.join("progress.json"),
                &json!({"families":results,"student_physical_calls":student_spent,"teacher_physical_calls":teacher_spent}),
            )?;
            continue;
        }
        while !approval_gate.exists() {
            if !watch {
                anyhow::bail!("Initial source approval not yet received");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        first_family_gate(campaign, approval_gate)?;
        let family = directory.join(method);
        fs::create_dir(&family)?;
        let student_budget = google::CallBudget::seeded(student_spent + 64, student_spent)?;
        let teacher_budget = google::CallBudget::seeded(teacher_spent + 80, teacher_spent)?;
        let outcome=async {
            let (fixtures,labels)=reviewed_family(examples,&book,campaign,method)?;
            let preflight=count_labeled_inputs(&fixtures,method,&labels,&http,&listing,&family.join("preflight")).await?;
            let models=runner::Models{student:Arc::new(google::Google::from_listing(info.clone(),http.clone(),directory.join("student"),student_budget.clone(),limiter.clone())?),teacher:Arc::new(codex::Codex::new(codex_path.to_path_buf(),directory.join("teacher").join(method),80)?.with_budget(teacher_budget.clone())),student_id:google::STUDENT.into(),teacher_id:"codex-saved-login-tool-free-no-override".into()};
            crate::queue::write_json_atomic(&family.join("started.json"),&json!({"gold_bank_sha256":google::digest(fs::read(campaign.join("run").join(method).join("answers.jsonl"))?),"fixture_digest":optimize::fixtures_digest(&fixtures)?,"preflight":preflight,"inherited_student_physical_calls":student_spent,"inherited_teacher_physical_calls":teacher_spent,"caps_carried_without_reset":true}))?;
            optimize::compile_direct_reader_from_gold(&fixtures,method,book.clone(),&models,&family.join("compile"),optimize::CompileOptions{max_metrics:24,bootstrap_demos:0},&reading::direct_reader_baseline(method)?,&labels,2).await
        }.await;
        student_spent = student_budget.spent();
        teacher_spent = teacher_budget.spent();
        let status = match outcome {
            Ok(result) => {
                json!({"method":method,"status":"complete","result":result,"student_physical_calls_total":student_spent,"teacher_physical_calls_total":teacher_spent,"application_promoted":false})
            }
            Err(error) => {
                json!({"method":method,"status":"unqualified","error":error.to_string(),"student_physical_calls_total":student_spent,"teacher_physical_calls_total":teacher_spent,"uncertain_key_replayed":false,"semantic_zero_for_missing_work":false})
            }
        };
        crate::queue::write_json_atomic(&family.join("status.json"), &status)?;
        results.push(status);
        crate::queue::write_json_atomic(
            &directory.join("progress.json"),
            &json!({"families":results,"student_physical_calls":student_spent,"teacher_physical_calls":teacher_spent,"limiter":limiter.snapshot().await}),
        )?;
    }
    let result = json!({"families":results,"student_physical_calls":student_spent,"teacher_physical_calls":teacher_spent,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn answer() -> WorkedAnswer {
        WorkedAnswer {
            status: "method_limit".into(),
            answer: "Synthetic useful answer".into(),
            analysis: "Synthetic decisive e1 reason".into(),
            evidence: "e1".into(),
            unresolved: "Synthetic missing calculation".into(),
        }
    }
    #[test]
    fn labeled_answers_have_exactly_four_inputs_and_five_answers_without_rubric_or_metadata() {
        let input = dsrust::input! {source_text:"Synthetic source",question:"Synthetic question",consultation:"{}",chart_evidence:"{}"};
        let mut with_private = input.clone();
        with_private.set("expected", json!("PRIVATE RUBRIC"));
        with_private.set("case_id", json!("PRIVATE ID"));
        with_private.set("source_witnesses", json!("PRIVATE METADATA"));
        let labeled = answer().labeled_example(&with_private).unwrap();
        assert_eq!(labeled.fields().count(), 9);
        assert_eq!(labeled.inputs().unwrap().rendered(), input.rendered());
        assert!(
            labeled.get("expected").is_none()
                && labeled.get("case_id").is_none()
                && labeled.get("source_witnesses").is_none()
        );
        assert_eq!(
            serde_json::to_value(answer())
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            5
        );
    }
    #[test]
    fn review_keeps_reference_answer_and_criteria_distinct_and_accepts_honest_limits() {
        let inputs = dsrust::input! {question:"Synthetic enquiry"};
        let proposed = answer();
        let reference = answer();
        let rubric = json!({"must_explain":["Synthetic criterion, not a worked answer"]});
        let input = review_input(
            &inputs,
            &proposed,
            &rubric,
            "Synthetic full source",
            Some(&reference),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<WorkedAnswer>(
                input.get("reference_answer").unwrap().as_str().unwrap()
            )
            .unwrap(),
            reference
        );
        assert_eq!(
            serde_json::from_str::<Value>(input.get("rubric").unwrap().as_str().unwrap()).unwrap(),
            rubric
        );
        let review = Prediction::new(
            dsrust::example! {score:1.,passed:true,classification:"unobserved",extraction:"unobserved",reading:"pass",answer_quality:"pass",feedback:"Synthetic source witness"},
            "Synthetic mechanics review",
        );
        assert!(review_pass(&review).unwrap());
    }
    #[test]
    fn all_forty_two_original_families_keep_the_requested_start_order() {
        let order = family_order();
        assert_eq!(order.len(), 42);
        assert_eq!(
            &order[..4],
            ["contact", "new_job", "movable_deal", "relationship"]
        );
        assert_eq!(
            order
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            42
        );
    }
    #[test]
    fn source_gate_binds_complete_gold_records_and_their_received_answers() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("plan.json"), b"Synthetic author plan").unwrap();
        let mut approvals = serde_json::Map::new();
        for case in [
            "dspy-contact-first-001",
            "dspy-contact-first-002",
            "dspy-contact-first-003",
        ] {
            let target = directory.path().join("run/contact").join(case);
            fs::create_dir_all(target.join("attempt-1")).unwrap();
            let record = GoldRecord {
                case_id: case.into(),
                split: "train".into(),
                input_hash: "Synthetic only".into(),
                answer: answer(),
                source_witnesses: json!({"synthetic":true}),
                review: json!({"synthetic":true}),
            };
            let bytes = serde_json::to_vec(&record).unwrap();
            fs::write(target.join("gold.json"), &bytes).unwrap();
            fs::write(
                target.join("attempt-1/answer.json"),
                serde_json::to_vec(&record.answer).unwrap(),
            )
            .unwrap();
            approvals.insert(case.into(), json!(google::digest(bytes)));
        }
        let gate = directory.path().join("gate.json");
        crate::queue::write_json_atomic(&gate,&json!({"approved":true,"book_sha256":crate::book::OCR_SHA256,"author_plan_sha256":google::digest(b"Synthetic author plan"),"training_answers":approvals})).unwrap();
        assert!(first_family_gate(directory.path(), &gate).is_ok());
        let target = directory
            .path()
            .join("run/contact/dspy-contact-first-002/attempt-1/answer.json");
        let mut changed = answer();
        changed.answer = "Changed received answer".into();
        fs::write(target, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(first_family_gate(directory.path(), &gate).is_err());
    }
    #[test]
    fn stock_labeled_fewshot_uses_real_answer_fields_and_not_private_metadata() {
        let inputs = dsrust::input! {source_text:"Synthetic source",question:"Synthetic question",consultation:"{}",chart_evidence:r#"{"facts":"ID\tKIND\tLABEL\tCALCULATED TESTIMONY\ne1\tboundary\tKnown\tSynthetic supplied fact\n"}"#};
        let labels = vec![answer().labeled_example(&inputs).unwrap()];
        let predictor = labeled_predictor("contact", &labels, 2).unwrap();
        assert_eq!(predictor.demos.len(), 1);
        let demo = &predictor.demos[0];
        assert!(
            demo.get("source_text").is_none()
                && demo.get("expected").is_none()
                && demo.get("__reference_answer").is_none()
        );
        assert_eq!(demo.get("answer"), Some(&json!("Synthetic useful answer")));
        assert_eq!(predictor.config().max_tokens, Some(8192));
    }
    #[test]
    fn semantic_review_summary_disagreement_cannot_accept_an_answer() {
        let review = Prediction::new(
            dsrust::example! {score:1.,passed:true,classification:"unobserved",extraction:"unobserved",reading:"fail",answer_quality:"pass",feedback:"Synthetic material source error"},
            "Synthetic inconsistent raw review",
        );
        assert!(review_pass(&review).is_err());
    }
    #[test]
    fn declared_native_guard_checks_all_answer_fields_without_forcing_status_answer() {
        let input = dsrust::input! {chart_evidence:r#"{"facts":"ID\tKIND\tLABEL\tCALCULATED TESTIMONY\ne1\tsynthetic\tx\tSupplied\n"}"#};
        let valid = Prediction::new(
            dsrust::example! {status:"method_limit",answer:"Useful e1 provisional answer",analysis:"e1",evidence:"e1",unresolved:"Unknown calculation"},
            "Synthetic reading",
        );
        assert!(
            optimize::reading_native_failures(&input, &valid)
                .unwrap()
                .is_empty()
        );
        let mut absent = valid.clone();
        absent.example.set("answer", json!("Claims absent e2"));
        assert_eq!(
            optimize::reading_native_failures(&input, &absent).unwrap(),
            ["answer: cites absent native evidence e2"]
        );
    }
}
