//! Source-reviewed worked answers. Rubrics and provenance are not demonstrations.
use crate::{
    book::Book, catalogue, codex, functions::Fixture, google, local, optimize, preflight, reading,
    runner, signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Example, Module, Predict, SignatureSpec,
    example::Prediction,
    lm::{DynChatModel, LmConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc, time::Duration};

pub const AUTHOR_CALL_CAP: usize = 3360;
pub const OUTPUT_FIELDS: [&str; 5] = ["status", "answer", "analysis", "evidence", "unresolved"];
pub const STUDENT_INPUT_LIMIT: usize = 6000;
pub const CONCISE_BASELINE: &str = "Apply the declared family procedure in source_text to this actual question, accepted consultation and native chart. Preserve parties, roles, direction, supplied values and genuine calculation limits. Return ONLY the five requested field markers, starting with status: answer, need_information or method_limit. Give a useful qualified answer, decisive source-supported reasons and supporting native eIDs; keep the complete worked reading within 300 words. Do not echo inputs, invent missing facts, force an exact date or substitute a rubric/checklist. The demonstration is a worked example, not evidence for the current chart.";
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
    fn word_count(&self) -> usize {
        [
            &self.status,
            &self.answer,
            &self.analysis,
            &self.evidence,
            &self.unresolved,
        ]
        .into_iter()
        .map(|field| field.split_whitespace().count())
        .sum()
    }
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
    recovery: Option<(&str, bool)>,
    max_attempts: usize,
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
    for attempt in 1..=max_attempts {
        let target = directory.join(format!("attempt-{attempt}"));
        fs::create_dir(&target)?;
        let mut trace = Vec::new();
        let mut writer = author(&case.method, model.clone(), correction.as_deref())?;
        if let Some((notice, true)) = recovery {
            writer.signature.instructions.push_str(notice);
        }
        let outcome = writer
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
        if let Some((recovery, _)) = recovery {
            judge.signature.instructions.push_str(recovery);
        }
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
            let outcome = generate_case(case, &book, model, &target, None, 2).await;
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

/// A continuation carries physical charges, approvals and exact completed caches;
/// it never edits or resumes a prior provider operation.
fn continuation_inventory(prior: &Path) -> Result<Value> {
    let mut approvals = Vec::new();
    let mut operations = Vec::new();
    let run = prior.join("run");
    for family in fs::read_dir(&run)? {
        let family = family?.path();
        if !family.is_dir() {
            continue;
        }
        for case in fs::read_dir(&family)? {
            let case = case?.path();
            if !case.is_dir() {
                continue;
            }
            let gold = case.join("gold.json");
            if gold.exists() {
                let bytes = fs::read(&gold)?;
                let record: GoldRecord = serde_json::from_slice(&bytes)?;
                ensure!(
                    record.review["passed"] == true
                        && record.review["reading"] == "pass"
                        && record.review["answer_quality"] == "pass",
                    "Prior approval is not a complete received pass"
                );
                approvals.push(
                    json!({"path":gold,"sha256":google::digest(bytes),"case_id":record.case_id}),
                );
            }
            let teacher = case.join("teacher");
            if !teacher.exists() {
                continue;
            }
            for operation in fs::read_dir(&teacher)? {
                let operation = operation?.path();
                if !operation.join("request.json").exists() {
                    continue;
                }
                let mut files = BTreeMap::new();
                for name in [
                    "request.json",
                    "response.json",
                    "answer.txt",
                    "exit.json",
                    "events.jsonl",
                    "failure.json",
                ] {
                    let file = operation.join(name);
                    if file.exists() {
                        files.insert(name, google::digest(fs::read(file)?));
                    }
                }
                operations.push(json!({"path":operation,"files":files,"completed":operation.join("response.json").exists()}));
            }
        }
    }
    approvals.sort_by_key(|v| v["path"].as_str().unwrap_or_default().to_owned());
    operations.sort_by_key(|v| v["path"].as_str().unwrap_or_default().to_owned());
    ensure!(
        operations.len() <= AUTHOR_CALL_CAP,
        "Prior physical cap exhausted"
    );
    Ok(
        json!({"prior":prior,"prior_prepared_sha256":google::digest(fs::read(prior.join("prepared/plan.json"))?),"approved":approvals,"operations":operations,"inherited_physical_charges":operations.len(),"remaining_physical_cap":AUTHOR_CALL_CAP-operations.len(),"uncertain_operations_replayed":false,"replacement_cases_authorized_by_user":true}),
    )
}

pub fn prepare_continuation(
    examples: &Path,
    book: &Book,
    codex_path: &Path,
    prior: &Path,
    directory: &Path,
) -> Result<Value> {
    let original: Value = serde_json::from_slice(&fs::read(prior.join("prepared/plan.json"))?)?;
    let inventory = continuation_inventory(prior)?;
    let mut plan = prepare(examples, book, codex_path, directory)?;
    // Source, original case bytes, split and calculated inputs must still agree.
    ensure!(
        plan["families"] == original["families"] && plan["book_sha256"] == original["book_sha256"],
        "Original author inputs or source changed; do not silently replace gold"
    );
    plan["continuation"] = inventory;
    crate::queue::write_json_atomic(&directory.join("plan.json"), &plan)?;
    Ok(plan)
}

fn check_author_disk(directory: &Path) -> Result<()> {
    let output = std::process::Command::new("df")
        .arg("-Pk")
        .arg(directory)
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot check author checkpoint capacity"
    );
    let text = String::from_utf8(output.stdout)?;
    let available: u64 = text
        .lines()
        .last()
        .context("Missing free-space row")?
        .split_whitespace()
        .nth(3)
        .context("Missing available space")?
        .parse()?;
    ensure!(
        available >= 5 * 1024 * 1024,
        "Less than 5 GiB free; author stops before another provider request"
    );
    Ok(())
}

pub async fn continue_generation(
    examples: &Path,
    book: Arc<Book>,
    prepared: &Path,
    codex_path: &Path,
    prior: &Path,
    directory: &Path,
) -> Result<Value> {
    ensure!(!directory.exists(), "Preserve prior continuation outputs");
    let plan: Value = serde_json::from_slice(&fs::read(prepared.join("plan.json"))?)?;
    ensure!(
        plan["executable_sha256"] == google::digest(fs::read(std::env::current_exe()?)?)
            && plan["codex_executable_sha256"] == google::digest(fs::read(codex_path)?)
            && plan["book_sha256"] == crate::book::OCR_SHA256
            && plan["teaching_sources_sha256"] == json!(optimize::teaching_sources()),
        "Continuation source/executable identity differs"
    );
    let inventory = continuation_inventory(prior)?;
    ensure!(
        plan["continuation"] == inventory,
        "Prior approval or paid ledger changed after preparation"
    );
    let spent = inventory["inherited_physical_charges"]
        .as_u64()
        .context("Missing inherited physical count")? as usize;
    let budget = google::CallBudget::seeded(AUTHOR_CALL_CAP, spent)?;
    fs::create_dir(directory)?;
    let mut summaries = Vec::new();
    let mut total = 0;
    for method in family_order() {
        let cases = catalogue::cases(examples, method)?;
        let bound = plan["families"]
            .as_array()
            .context("Missing families")?
            .iter()
            .find(|v| v["method"] == method)
            .context("Missing bound family")?;
        ensure!(
            bound["case_bank_sha256"] == google::digest(serde_json::to_vec(&cases)?)
                && bound["reference_source_sha256"]
                    == google::digest(catalogue::reference_reader_source(&book, method)?),
            "Continuation corpus/source drift"
        );
        let family = directory.join(method);
        fs::create_dir(&family)?;
        let mut records = Vec::new();
        let mut withheld = Vec::new();
        for case in &cases {
            check_author_disk(directory)?;
            let expected = bound["cases"]
                .as_array()
                .context("Missing cases")?
                .iter()
                .find(|v| v["case_id"] == case.id)
                .context("Missing bound case")?;
            let (inputs, _) = inputs_and_rubric(case, &book)?;
            ensure!(
                expected["input_hash"] == optimize::input_key(&inputs)?
                    && expected["split"] == case.split,
                "Continuation input identity changed"
            );
            let target = family.join(&case.id);
            fs::create_dir(&target)?;
            let old = prior.join("run").join(method).join(&case.id);
            let gold = old.join("gold.json");
            if gold.exists() {
                let bytes = fs::read(&gold)?;
                let record: GoldRecord = serde_json::from_slice(&bytes)?;
                ensure!(
                    record.input_hash == optimize::input_key(&inputs)?
                        && record.case_id == case.id
                        && record.split == case.split,
                    "Saved approval input identity differs"
                );
                fs::write(target.join("gold.json"), &bytes)?;
                for name in ["student-inputs.json", "teacher-inputs.json", "rubric.json"] {
                    fs::copy(old.join(name), target.join(name))?;
                }
                crate::queue::write_json_atomic(
                    &target.join("reused-approval.json"),
                    &json!({"source":gold,"sha256":google::digest(bytes),"new_provider_calls":0}),
                )?;
                records.push(record);
            } else {
                let teacher = target.join("teacher");
                let old_teacher = old.join("teacher");
                let mut old_requests = 0;
                let mut incomplete_author = false;
                let mut incomplete_review = false;
                let mut old_authors = 0;
                if old_teacher.exists() {
                    for op in fs::read_dir(&old_teacher)? {
                        let op = op?.path();
                        if !op.join("request.json").exists() {
                            continue;
                        }
                        old_requests += 1;
                        let request = fs::read_to_string(op.join("request.json"))?;
                        let author = request.contains(AUTHOR_INSTRUCTION);
                        old_authors += usize::from(author);
                        if op.join("response.json").exists() {
                            codex::import_completed(&op, &teacher)?;
                        } else if author {
                            incomplete_author = true;
                        } else {
                            incomplete_review = true;
                        }
                    }
                }
                ensure!(
                    old_requests <= 4 && old_authors <= 2,
                    "Original per-case bound exceeded"
                );
                let notice = format!(
                    "\nINDEPENDENT REPLACEMENT CASE REVIEW, explicitly authorized after the original stopped campaign. Case {}. Prior incomplete operations remain charged and are not resumed or replayed. Assess the same original question, native facts and salient source; the recovery label supplies no semantic evidence.",
                    case.id
                );
                let recovery = (incomplete_author || incomplete_review)
                    .then_some((notice.as_str(), incomplete_author));
                crate::queue::write_json_atomic(
                    &target.join("continuation.json"),
                    &json!({"prior_case":old,"prior_physical_charges":old_requests,"prior_author_attempts":old_authors,"completed_operations_imported":old_requests-usize::from(incomplete_author)-usize::from(incomplete_review),"author_replacement":incomplete_author,"review_replacement":incomplete_review,"uncertain_replay":false}),
                )?;
                let model: Arc<dyn DynChatModel> = Arc::new(
                    codex::Codex::new(codex_path.to_path_buf(), teacher, 4 - old_requests)?
                        .with_budget(budget.clone()),
                );
                let max_attempts = if incomplete_author {
                    2 - old_authors
                } else {
                    2
                };
                match generate_case(case, &book, model, &target, recovery, max_attempts).await {
                    Ok(Some(record)) => {
                        eprintln!("gold {method} {} approved", case.id);
                        records.push(record);
                    }
                    outcome => {
                        let failure = json!({"case_id":case.id,"error":outcome.err().map(|e|e.to_string()),"unknown_is_not_failure_score":true,"automatic_retry":false});
                        crate::queue::write_json_atomic(
                            &target.join("case-withheld.json"),
                            &failure,
                        )?;
                        withheld.push(failure);
                    }
                }
            }
            total += usize::from(target.join("gold.json").exists());
            // A single completed case survives an interruption before its family closes.
            crate::queue::write_json_atomic(
                &directory.join("progress.json"),
                &json!({"last_case":case.id,"approved":total,"inherited_approved":inventory["approved"].as_array().map(Vec::len),"physical_teacher_calls":budget.spent(),"physical_teacher_cap":AUTHOR_CALL_CAP,"student_calls":0}),
            )?;
        }
        let lines = records
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n")
            + "\n";
        fs::write(family.join("answers.jsonl"), lines)?;
        let summary = json!({"method":method,"original_cases":20,"approved":records.len(),"failed_or_unknown":withheld,"ready_for_student":records.len()==20,"teacher_physical_spent_total":budget.spent(),"physical_teacher_cap":AUTHOR_CALL_CAP,"rubrics_are_not_demo_outputs":true});
        crate::queue::write_json_atomic(&family.join("result.json"), &summary)?;
        summaries.push(summary);
    }
    let result = json!({"families":summaries,"approved":total,"planned_cases":840,"teacher_physical_calls":budget.spent(),"inherited_teacher_physical_calls":spent,"physical_teacher_cap":AUTHOR_CALL_CAP,"student_calls":0,"automatic_uncertain_replays":0});
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

fn canonical_review_inputs(example: &Example) -> Result<Example> {
    let fields: Vec<(String, String)> = serde_json::from_value(
        example
            .get("__review_inputs")
            .cloned()
            .context("No canonical input witness for concise presentation")?,
    )?;
    ensure!(
        fields.len() == 4
            && ["source_text", "question", "consultation", "chart_evidence"]
                .iter()
                .all(|name| fields.iter().filter(|(field, _)| field == name).count() == 1),
        "Canonical review witness must contain exactly the four declared inputs"
    );
    Ok(Example::new(
        fields
            .iter()
            .map(|(name, value)| (name.clone(), Value::String(value.clone()))),
    )
    .with_inputs(["source_text", "question", "consultation", "chart_evidence"]))
}

pub(crate) fn verify_concise_fixture(example: &Example, method: &str) -> Result<()> {
    let canonical = canonical_review_inputs(example)?;
    let projected = catalogue::relevant_reader_inputs(&canonical, method)?;
    ensure!(
        same_named_inputs(&projected, &example.inputs()?),
        "Concise reader input does not match its canonical native/source witness"
    );
    Ok(())
}

fn same_named_inputs(first: &Example, second: &Example) -> bool {
    first.len() == 4
        && second.len() == 4
        && first.rendered().into_iter().collect::<BTreeMap<_, _>>()
            == second.rendered().into_iter().collect::<BTreeMap<_, _>>()
}

fn concise_fixtures(fixtures: &mut [Fixture], method: &str) -> Result<()> {
    for fixture in fixtures {
        let original = fixture.example.inputs()?;
        let projected = catalogue::relevant_reader_inputs(&original, method)?;
        fixture
            .example
            .set("__review_inputs", json!(original.rendered()));
        fixture
            .example
            .set("__concise_student_projection", json!(true));
        for (name, value) in projected.fields() {
            fixture.example.set(name, value.clone());
        }
        verify_concise_fixture(&fixture.example, method)?;
    }
    Ok(())
}

fn concise_predictor(method: &str, labels: &[Example], count: usize) -> Result<Predict> {
    let mut predictor = labeled_predictor(method, labels, count)?;
    predictor.signature.instructions = CONCISE_BASELINE.into();
    predictor = predictor.with_config(LmConfig {
        temperature: Some(0.),
        max_tokens: Some(2048),
        ..Default::default()
    });
    Ok(predictor)
}

fn relevant_demo_label(
    fixture: &Example,
    canonical: &Example,
    method: &str,
    answer: &WorkedAnswer,
) -> Result<Example> {
    let mut inputs = fixture.inputs()?;
    let labeled = answer.labeled_example(&inputs)?;
    let cited = crate::demo::cited_ids(&labeled)?;
    let chart: Value = serde_json::from_str(
        canonical
            .get("chart_evidence")
            .and_then(Value::as_str)
            .context("No canonical demo chart")?,
    )?;
    let consultation: Value = serde_json::from_str(
        canonical
            .get("consultation")
            .and_then(Value::as_str)
            .context("No canonical demo consultation")?,
    )?;
    inputs.set(
        "chart_evidence",
        json!(catalogue::relevant_reader_chart(
            &chart,
            &consultation,
            method,
            Some(&cited)
        )?),
    );
    answer.labeled_example(&inputs)
}

/// A cited-fact demo is valid only for its original training consultation and
/// the exact received five-field answer whose independent review was accepted.
pub(crate) fn labeled_inputs_match_training(
    label: &Example,
    fixture: &Example,
    method: &str,
) -> Result<bool> {
    if fixture.get("__concise_student_projection") != Some(&json!(true)) {
        return Ok(fixture.inputs()?.rendered() == label.inputs()?.rendered());
    }
    verify_concise_fixture(fixture, method)?;
    if label.len() != 9 || label.inputs()?.len() != 4 || label.labels()?.len() != 5 {
        return Ok(false);
    }
    let answer =
        WorkedAnswer::from_prediction(&Prediction::new(label.clone(), "Received worked label"))?;
    if fixture.get("__approved_demo_answer") != Some(&serde_json::to_value(&answer)?) {
        return Ok(false);
    }
    let canonical = canonical_review_inputs(fixture)?;
    let expected = relevant_demo_label(fixture, &canonical, method, &answer)?;
    Ok(same_named_inputs(&expected.inputs()?, &label.inputs()?))
}

fn bind_approved_concise_demo(
    fixtures: &mut [Fixture],
    labels: &[Example],
    method: &str,
) -> Result<()> {
    ensure!(
        labels.len() == 1,
        "The concise phase accepts exactly one received TRAIN answer"
    );
    let answer = WorkedAnswer::from_prediction(&Prediction::new(
        labels[0].clone(),
        "Source-reviewed concise answer",
    ))?;
    let fixture = fixtures
        .iter_mut()
        .find(|fixture| fixture.split == "train")
        .context("No original TRAIN fixture")?;
    fixture
        .example
        .set("__approved_demo_answer", serde_json::to_value(answer)?);
    ensure!(
        labeled_inputs_match_training(&labels[0], &fixture.example, method)?,
        "Received demo does not match its original TRAIN presentation"
    );
    Ok(())
}

async fn concise_label(
    fixtures: &[Fixture],
    method: &str,
    book: &Book,
    teacher: Arc<dyn DynChatModel>,
    directory: &Path,
) -> Result<Vec<Example>> {
    fs::create_dir(directory)?;
    let fixture = fixtures
        .iter()
        .find(|fixture| fixture.split == "train")
        .context("No original TRAIN example for the worked demo")?;
    let canonical = canonical_review_inputs(&fixture.example)?;
    let reference: WorkedAnswer = serde_json::from_str(
        fixture
            .example
            .get("__reference_answer")
            .and_then(Value::as_str)
            .context("No approved full worked answer")?,
    )?;
    let source = catalogue::reference_reader_source(book, method)?;
    let mut inputs = canonical.clone();
    inputs.set("source_text", json!(&source));
    let mut writer = author(method, teacher.clone(), None)?;
    writer.signature.instructions.push_str(&format!("\n\nCreate a concise actual worked version of this independently approved TRAIN reference. Keep the original question/parties, decisive native testimony and material limits. Use at most 250 words TOTAL across the five output fields. This is not a rubric or checklist. Do not invent a stronger verdict, drop decisive contrary evidence or copy input fields. APPROVED FULL REFERENCE DATA:\n{}",serde_json::to_string(&reference)?));
    let mut trace = Vec::new();
    let proposed = writer.forward_traced(inputs, &mut trace).await?;
    crate::queue::write_json_atomic(&directory.join("author-trace.json"), &json!(trace.iter().map(|step|json!({"predictor":step.predictor,"inputs":step.inputs.rendered(),"outputs":step.outputs.rendered()})).collect::<Vec<_>>()))?;
    let answer = WorkedAnswer::from_prediction(&proposed)?;
    crate::queue::write_json_atomic(&directory.join("answer.json"), &json!(answer))?;
    ensure!(
        answer.word_count() <= 250,
        "Received worked demo exceeds the fixed 250-word limit; do not truncate it"
    );
    let native = optimize::reading_native_failures(&canonical, &proposed)?;
    crate::queue::write_json_atomic(
        &directory.join("native-check.json"),
        &json!({"failures":native}),
    )?;
    ensure!(
        native.is_empty(),
        "Concise worked demo has native output/citation failure"
    );
    let mut judge = optimize::plain(signatures::JudgeFunction::signature(), teacher);
    judge.signature.instructions = REVIEW_INSTRUCTION.into();
    judge = judge.with_config(LmConfig {
        max_tokens: Some(8192),
        temperature: Some(0.),
        ..Default::default()
    });
    let review = judge
        .forward(review_input(
            &canonical,
            &answer,
            fixture
                .example
                .get("expected")
                .context("No private criteria")?,
            &source,
            Some(&reference),
        )?)
        .await?;
    crate::queue::write_json_atomic(&directory.join("review.json"), &review_fields(&review))?;
    ensure!(
        review_pass(&review)?,
        "Concise actual worked demo failed independent source/answer review"
    );
    let label = relevant_demo_label(&fixture.example, &canonical, method, &answer)?;
    crate::queue::write_json_atomic(
        &directory.join("label.json"),
        &json!(label.fields().collect::<Vec<_>>()),
    )?;
    let displayed_demo = crate::demo::compact(&label)?;
    crate::queue::write_json_atomic(
        &directory.join("demonstration.json"),
        &json!(displayed_demo.fields().collect::<Vec<_>>()),
    )?;
    crate::queue::write_json_atomic(
        &directory.join("source.json"),
        &json!({"case_id":fixture.example.get("case_id"),"split":"train","fixture_digest":optimize::fixtures_digest(fixtures)?,"book_sha256":crate::book::OCR_SHA256,"teacher_source_sha256":google::digest(&source),"canonical_input_sha256":optimize::input_key(&canonical)?,"projected_input_sha256":optimize::input_key(&fixture.example.inputs()?)?,"demonstration_input_sha256":optimize::input_key(&displayed_demo.inputs()?)?,"reference_answer_sha256":google::digest(serde_json::to_vec(&reference)?),"answer_sha256":google::digest(fs::read(directory.join("answer.json"))?),"review_sha256":google::digest(fs::read(directory.join("review.json"))?),"label_sha256":google::digest(fs::read(directory.join("label.json"))?),"demonstration_sha256":google::digest(fs::read(directory.join("demonstration.json"))?),"reading":"pass","answer_quality":"pass","max_output_words":250,"full_reference_preserved":true,"live_chart_evidence_filtered":true,"student_selection":"source_question_type_and_declared_roles","full_canonical_chart_private":true,"demo_evidence_subset_declared":true,"rubrics_are_not_demonstrations":true}),
    )?;
    Ok(vec![label])
}

fn load_concise_label(
    fixtures: &[Fixture],
    book: &Book,
    method: &str,
    directory: &Path,
) -> Result<Vec<Example>> {
    let source: Value = serde_json::from_slice(&fs::read(directory.join("source.json"))?)?;
    ensure!(
        source["fixture_digest"] == optimize::fixtures_digest(fixtures)?
            && source["book_sha256"] == crate::book::OCR_SHA256
            && source["teacher_source_sha256"]
                == google::digest(catalogue::reference_reader_source(book, method)?),
        "Prepared concise demo source/dataset differs"
    );
    let fixture = fixtures
        .iter()
        .find(|f| f.split == "train" && f.example.get("case_id") == Some(&source["case_id"]))
        .context("Prepared concise demo is not an original TRAIN case")?;
    let canonical = canonical_review_inputs(&fixture.example)?;
    ensure!(
        source["canonical_input_sha256"] == optimize::input_key(&canonical)?
            && source["projected_input_sha256"] == optimize::input_key(&fixture.example.inputs()?)?,
        "Prepared concise demo inputs differ"
    );
    let reference: WorkedAnswer = serde_json::from_str(
        fixture
            .example
            .get("__reference_answer")
            .and_then(Value::as_str)
            .context("No full reference")?,
    )?;
    ensure!(
        source["reference_answer_sha256"] == google::digest(serde_json::to_vec(&reference)?),
        "Prepared full reference differs"
    );
    for file in ["answer", "review", "label", "demonstration"] {
        ensure!(
            source[format!("{file}_sha256")]
                == google::digest(fs::read(directory.join(format!("{file}.json")))?),
            "Prepared concise {file} drift"
        );
    }
    let answer: WorkedAnswer = serde_json::from_slice(&fs::read(directory.join("answer.json"))?)?;
    ensure!(
        answer.word_count() <= 250,
        "Prepared worked demo exceeds 250 words"
    );
    let prediction = Prediction::new(
        answer.labeled_example(&canonical)?.labels()?,
        "Exact received concise worked answer",
    );
    ensure!(
        optimize::reading_native_failures(&canonical, &prediction)?.is_empty(),
        "Prepared worked demo has absent native citations or invalid status"
    );
    let review: Value = serde_json::from_slice(&fs::read(directory.join("review.json"))?)?;
    ensure!(
        review["passed"] == true
            && review["reading"] == "pass"
            && review["answer_quality"] == "pass",
        "Prepared demo review did not pass"
    );
    let label = relevant_demo_label(&fixture.example, &canonical, method, &answer)?;
    ensure!(
        json!(label.fields().collect::<Vec<_>>())
            == serde_json::from_slice::<Value>(&fs::read(directory.join("label.json"))?)?,
        "Prepared label differs from actual worked answer"
    );
    let displayed_demo = crate::demo::compact(&label)?;
    ensure!(
        source["demonstration_input_sha256"] == optimize::input_key(&displayed_demo.inputs()?)?
            && json!(displayed_demo.fields().collect::<Vec<_>>())
                == serde_json::from_slice::<Value>(&fs::read(
                    directory.join("demonstration.json")
                )?)?,
        "Prepared demonstration subset differs"
    );
    Ok(vec![label])
}

async fn count_labeled_inputs(
    fixtures: &[Fixture],
    method: &str,
    labels: &[Example],
    http: &reqwest::Client,
    listing: &Value,
    directory: &Path,
    student: &StudentSelection<'_>,
) -> Result<Value> {
    fs::create_dir(directory)?;
    let predictor = if student.concise {
        concise_predictor(method, labels, usize::from(!labels.is_empty()))?
    } else {
        labeled_predictor(method, labels, 2)?
    };
    let requests = preflight::reader_requests(fixtures, &predictor)?;
    crate::queue::write_json_atomic(&directory.join("requests.json"), &json!(requests))?;
    let limit = if student.local_url.is_some() {
        listing["local_context_limit"]
            .as_u64()
            .context("No resident slot context")? as usize
    } else {
        google::model_context_limit(listing, google::STUDENT)?
    };
    let mut counts = Vec::new();
    for (index, row) in requests.iter().enumerate() {
        let mut request: dsrust::lm::api::LmRequest =
            serde_json::from_value(row["request"].clone())?;
        request.model = if student.local_url.is_some() {
            student.model
        } else {
            google::STUDENT
        }
        .into();
        let target = directory.join(format!("count-{index:02}"));
        let count = if let Some(url) = student.local_url {
            local::count_tokens(http, url, &request, &target).await?
        } else {
            google::count_tokens(http, &request, &target).await?
        };
        let tokens = count["input_tokens"]
            .as_u64()
            .context("No exact provider count")? as usize;
        let output = if student.concise { 2048 } else { 8192 };
        let fits = tokens.saturating_add(output) <= limit
            && (!student.concise || tokens <= STUDENT_INPUT_LIMIT);
        counts.push(json!({"case_id":row["case_id"],"request_sha256":row["request_sha256"],"input_tokens":tokens,"generation_tokens":output,"fits":fits}));
    }
    let result = json!({"method":method,"optimizer":"upstream LabeledFewShot","labeled_training_cases":labels.len(),"demos":if student.concise{usize::from(!labels.is_empty())}else{2},"input_context_limit":limit,"strict_prompt_limit":if student.concise{Some(STUDENT_INPUT_LIMIT)}else{None},"generation_tokens":if student.concise{2048}else{8192},"counts":counts,"generation_submitted":false,"reference_and_rubric_private":true,"resident_tokenizer":student.local_url.is_some()});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        counts.iter().all(|row| row["fits"] == true),
        "One or more student wires exceed the fixed prompt/context limits; all exact counts retained and no generation submitted"
    );
    Ok(result)
}

pub struct StudentSelection<'a> {
    pub local_url: Option<&'a str>,
    pub model: &'a str,
    pub prior_closure: Option<&'a Path>,
    pub concise: bool,
    pub prepare_only: bool,
    pub prepared: Option<&'a Path>,
}

fn completed_teacher_seals(directory: &Path, ignored_key: Option<&str>) -> Result<Value> {
    let mut operations = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        ensure!(
            entry.file_type()?.is_dir()
                && name.len() == 64
                && name.bytes().all(|b| b.is_ascii_hexdigit()),
            "Unexpected preparation teacher carrier"
        );
        if Some(name.as_str()) == ignored_key {
            continue;
        }
        let path = entry.path();
        ensure!(
            serde_json::from_slice::<Value>(&fs::read(path.join("exit.json"))?)?["code"] == 0,
            "Preparation teacher did not complete"
        );
        codex::verify_events(&fs::read_to_string(path.join("events.jsonl"))?)?;
        let mut files = BTreeMap::new();
        for file in [
            "request.json",
            "prompt.txt",
            "answer.txt",
            "events.jsonl",
            "exit.json",
            "stderr.log",
            "response.json",
        ] {
            files.insert(file, google::digest(fs::read(path.join(file))?));
        }
        operations.push(json!({"source":path,"sealed_files":files}));
    }
    Ok(json!(operations))
}

fn prepared_consumption(
    directory: &Path,
    ancestry: &Value,
    author_plan_sha: &str,
    codex_path: &Path,
) -> Result<(usize, usize, Value)> {
    let plan_bytes = fs::read(directory.join("plan.json"))?;
    let plan: Value = serde_json::from_slice(&plan_bytes)?;
    ensure!(
        plan["concise_student"] == true
            && plan["prepare_student_only"] == true
            && plan["method_order"] == json!(["contact"])
            && plan["author_plan_sha256"] == author_plan_sha
            && plan["book_sha256"] == crate::book::OCR_SHA256
            && plan["executable_sha256"] == google::digest(fs::read(std::env::current_exe()?)?)
            && plan["teaching_sources_sha256"] == json!(optimize::teaching_sources())
            && plan["codex_executable_sha256"] == google::digest(fs::read(codex_path)?)
            && plan["prior_consumer"] == *ancestry
            && plan["per_family_physical_student_cap"] == 64
            && plan["per_family_physical_teacher_cap"] == 80,
        "Prepared concise baseline/source/ancestry differs"
    );
    let result_bytes = fs::read(directory.join("result.json"))?;
    let result: Value = serde_json::from_slice(&result_bytes)?;
    let family = &result["families"][0];
    ensure!(
        result["families"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1)
            && family["status"] == "prepared"
            && family["result"]["phase"] == "prepared_concise_student"
            && family["result"]["student_calls"] == 0
            && family["result"]["preflight"]["strict_prompt_limit"] == STUDENT_INPUT_LIMIT
            && family["result"]["preflight"]["counts"]
                .as_array()
                .is_some_and(|rows| rows.len() == 20
                    && rows.iter().all(|row| row["fits"] == true
                        && row["input_tokens"]
                            .as_u64()
                            .is_some_and(|tokens| tokens <= STUDENT_INPUT_LIMIT as u64))),
        "Prepared family lacks twenty admitted exact requests"
    );
    let student = result["student_physical_calls"]
        .as_u64()
        .context("No prepared student deductions")? as usize;
    let teacher = result["teacher_physical_calls"]
        .as_u64()
        .context("No prepared teacher deductions")? as usize;
    ensure!(
        student == ancestry["student_physical_calls"].as_u64().unwrap_or(0) as usize
            && teacher <= 80,
        "Prepared physical caps differ"
    );
    let operations = family["result"]["completed_teacher_operations"].clone();
    let completed = operations
        .as_array()
        .context("No completed preparation teacher carriers")?;
    let inherited_keys = ancestry["completed_teacher_operations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|operation| operation["source"].as_str())
        .filter_map(|source| Path::new(source).file_name())
        .collect::<std::collections::BTreeSet<_>>();
    let reused = completed
        .iter()
        .filter(|operation| {
            operation["source"]
                .as_str()
                .and_then(|source| Path::new(source).file_name())
                .is_some_and(|key| inherited_keys.contains(key))
        })
        .count();
    ensure!(
        teacher
            == ancestry["teacher_physical_calls"].as_u64().unwrap_or(0) as usize + completed.len()
                - reused,
        "Prepared teacher deductions differ from actual completed operations"
    );
    for operation in completed {
        let path = Path::new(
            operation["source"]
                .as_str()
                .context("No preparation operation source")?,
        );
        ensure!(
            path.parent() == Some(directory.join("teacher/contact").as_path()),
            "Prepared operation lies outside its teacher cache"
        );
        for (name, sha) in operation["sealed_files"]
            .as_object()
            .context("No paid preparation seals")?
        {
            ensure!(
                Path::new(name)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                    && sha == &json!(google::digest(fs::read(path.join(name))?)),
                "Preparation paid carrier drift: {name}"
            );
        }
    }
    Ok((
        student,
        teacher,
        json!({"path":directory,"plan_sha256":google::digest(plan_bytes),"result_sha256":google::digest(result_bytes),"completed_teacher_operations":operations,"student_physical_calls":student,"teacher_physical_calls":teacher,"scores_imported":false}),
    ))
}

fn inherited_consumer_calls(path: Option<&Path>, concise: bool) -> Result<(usize, usize, Value)> {
    let Some(path) = path else {
        return Ok((0, 0, Value::Null));
    };
    let bytes = fs::read(path)?;
    let closure: Value = serde_json::from_slice(&bytes)?;
    if closure["cumulative_student_physical_calls"] == 16
        && closure["cumulative_teacher_physical_calls"] == 15
    {
        ensure!(
            concise
                && closure["original_student_cap"] == 64
                && closure["original_teacher_cap"] == 80
                && closure["fresh_student_physical_calls"] == 13
                && closure["fresh_teacher_physical_calls"] == 12
                && closure["new_uncertain_paid_operations"] == 0
                && closure["successful_student_responses"] == 11
                && closure["exact_pids_absent"]
                    .as_array()
                    .is_some_and(|p| p.len() == 2),
            "Only the sealed completed Contact rewrite-limit failure may continue with these deductions"
        );
        let root = Path::new(
            closure["campaign"]
                .as_str()
                .context("No closed rewrite campaign")?,
        );
        verify_retained_seals(root, &closure)?;
        let prior = Path::new(
            closure["prior_consumer_closure"]["path"]
                .as_str()
                .context("No rewrite ancestry")?,
        );
        ensure!(
            closure["prior_consumer_closure"]["sha256"] == google::digest(fs::read(prior)?),
            "Rewrite ancestry changed"
        );
        let (s, t, mut ancestry) = inherited_consumer_calls(Some(prior), true)?;
        ensure!(s == 3 && t == 3, "Rewrite predecessor deductions differ");
        ensure!(
            closure["old_unknown_teacher_key_preserved_without_replay"]
                == ancestry["unsettled_teacher_key"],
            "Closed original unknown teacher key differs"
        );
        let plan: Value = serde_json::from_slice(&fs::read(root.join("run/plan.json"))?)?;
        ensure!(
            plan["author_plan_sha256"] == ancestry["author_plan_sha256"]
                && plan["codex_executable_sha256"] == ancestry["codex_executable_sha256"],
            "Rewrite teacher/author identity changed"
        );
        let operations = completed_teacher_seals(
            &root.join("run/teacher/contact"),
            ancestry["unsettled_teacher_key"].as_str(),
        )?;
        ensure!(
            operations.as_array().is_some_and(|ops| ops.len() == 14),
            "Closed teacher operation count differs"
        );
        for operation in operations
            .as_array()
            .context("No closed teacher operations")?
        {
            let source = Path::new(operation["source"].as_str().context("No teacher source")?);
            for (file, sha) in operation["sealed_files"]
                .as_object()
                .context("No teacher seals")?
            {
                let key = source
                    .strip_prefix(root)?
                    .join(file)
                    .to_string_lossy()
                    .into_owned();
                ensure!(
                    closure["sealed_files"][&key] == *sha,
                    "Unsealed received teacher carrier: {key}"
                );
            }
        }
        ancestry["settled_student_cache_seals"] = closure["sealed_files"].clone();
        ancestry["path"] = json!(path);
        ancestry["sha256"] = json!(google::digest(bytes));
        ancestry["student_physical_calls"] = json!(16);
        ancestry["teacher_physical_calls"] = json!(15);
        ancestry["completed_teacher_operations"] = operations;
        ancestry["settled_student_cache"] = json!(root.join("run/student"));
        ancestry["replay_closed_student_and_teacher_only"] = json!(true);
        return Ok((16, 15, ancestry));
    }
    if closure["cumulative_student_calls"] == 3 && closure["cumulative_teacher_calls"] == 3 {
        ensure!(
            concise
                && closure["exact_pids_absent"] == true
                && closure["fresh_student_calls"] == 0
                && closure["fresh_teacher_calls"] == 2
                && closure["inherited_student_calls"] == 3
                && closure["inherited_teacher_calls"] == 1
                && closure["new_unsettled_teacher_operations"] == 0
                && closure["old_unknown_key_not_replayed"] == true
                && closure["canonical_gold_unchanged"] == true
                && closure["concise_demo_source_approved"] == true
                && closure["all_one_demo_wires_withheld"] == true,
            "Only the closed received worked-answer preparation can supply this reuse"
        );
        let root = Path::new(
            closure["campaign"]
                .as_str()
                .context("No preparation campaign")?,
        );
        verify_retained_seals(root, &closure)?;
        let plan: Value = serde_json::from_slice(&fs::read(root.join("run/plan.json"))?)?;
        ensure!(
            plan["concise_student"] == true
                && plan["prepare_student_only"] == true
                && plan["method_order"] == json!(["contact"])
                && plan["book_sha256"] == crate::book::OCR_SHA256
                && plan["per_family_physical_student_cap"] == 64
                && plan["per_family_physical_teacher_cap"] == 80,
            "Received preparation scope/bounds differ"
        );
        let predecessor = Path::new(
            plan["prior_consumer"]["path"]
                .as_str()
                .context("No prior consumer receipt")?,
        );
        ensure!(
            plan["prior_consumer"]["sha256"] == google::digest(fs::read(predecessor)?),
            "Worked-answer preparation ancestry drift"
        );
        let (student, teacher, previous) = inherited_consumer_calls(Some(predecessor), true)?;
        ensure!(
            student == 3 && teacher == 1,
            "Earlier cumulative deductions differ"
        );
        let operations = completed_teacher_seals(
            &root.join("run/teacher/contact"),
            previous["unsettled_teacher_key"].as_str(),
        )?;
        for operation in operations
            .as_array()
            .context("No received teacher operations")?
        {
            let source = Path::new(
                operation["source"]
                    .as_str()
                    .context("No received teacher source")?,
            );
            let relative = source
                .strip_prefix(root)
                .context("Teacher carrier lies outside closed preparation")?;
            for (file, sha) in operation["sealed_files"]
                .as_object()
                .context("No received teacher seals")?
            {
                let key = relative.join(file).to_string_lossy().into_owned();
                ensure!(
                    closure["sealed_files"][&key] == *sha,
                    "Received teacher file was not sealed by its closure: {key}"
                );
            }
        }
        let keys = operations
            .as_array()
            .context("No received teacher carriers")?
            .iter()
            .map(|operation| {
                Path::new(
                    operation["source"]
                        .as_str()
                        .context("No received carrier path")?,
                )
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .context("No received key")
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let expected = closure["completed_teacher_operations"]
            .as_array()
            .context("No closed received keys")?
            .iter()
            .map(|key| {
                key.as_str()
                    .map(str::to_owned)
                    .context("Invalid received key")
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        ensure!(
            keys.len() == 2 && keys == expected,
            "Received teacher key set differs"
        );
        return Ok((
            3,
            3,
            json!({"path":path,"sha256":google::digest(bytes),"student_physical_calls":3,"teacher_physical_calls":3,
            "outputs_and_scores_imported":false,"completed_teacher_operations":operations,"completed_cache_only_demo":true,
            "codex_executable_sha256":plan["codex_executable_sha256"],"author_plan_sha256":plan["author_plan_sha256"],
            "unsettled_teacher_guard":previous["unsettled_teacher_guard"],"unsettled_teacher_key":previous["unsettled_teacher_key"]}),
        ));
    }
    let stopped_local = closure["student_physical_calls_cumulative"] == 3;
    if stopped_local {
        ensure!(
            concise
                && closure["exact_pids_absent"] == true
                && closure["reviewer_process_absent"] == true
                && closure["student_physical_calls_fresh"] == 1
                && closure["student_physical_calls_inherited_hosted"] == 2
                && closure["teacher_physical_calls_fresh"] == 1
                && closure["student_normalized_outputs"] == 1
                && closure["semantic_reviews"] == 0
                && closure["unsettled_teacher_operations"] == 1,
            "Only the explicitly stopped large-input comparison can supply these deductions to a fresh concise baseline"
        );
    } else {
        ensure!(
            closure["exact_pids_absent"] == true
                && closure["uncertain_requests"] == 0
                && closure["student_normalized_outputs"] == 0
                && closure["semantic_reviews"] == 0
                && closure["third_attempt_wait_generation_submitted"] == false,
            "Only a settled, explicitly stopped no-output consumer can supply deductions"
        );
        ensure!(
            closure["student_physical_calls"] == 2
                && closure["teacher_physical_calls"] == 0
                && closure["original_caps"]["student_per_family"] == 64
                && closure["original_caps"]["teacher_per_family"] == 80,
            "Prior consumer bounds or deductions differ"
        );
    }
    let root = Path::new(
        closure["campaign"]
            .as_str()
            .context("No stopped campaign")?,
    );
    verify_retained_seals(root, &closure)?;
    if stopped_local {
        let key = closure["reviewer_key"]
            .as_str()
            .context("No unsettled reviewer key")?;
        ensure!(
            key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "Malformed unsettled reviewer key"
        );
        let teacher = root.join("run/teacher/contact").join(key);
        ensure!(
            teacher.join("request.json").is_file()
                && !teacher.join("response.json").exists()
                && !teacher.join("exit.json").exists(),
            "Stopped reviewer is not the recorded unsettled request"
        );
        return Ok((
            3,
            1,
            json!({"path":path,"sha256":google::digest(bytes),"student_physical_calls":3,"teacher_physical_calls":1,"outputs_and_scores_imported":false,"unsettled_teacher_guard":teacher,"unsettled_teacher_key":key}),
        ));
    }
    let results = closure["results"]
        .as_array()
        .context("No stopped physical outcomes")?;
    ensure!(
        results.len() == 2
            && results
                .iter()
                .enumerate()
                .all(|(i, r)| r["response_complete"] == true
                    && r["http_status"] == 429
                    && r["physical_call"] == i + 1),
        "Stopped provider work was not fully received quota rejection"
    );
    Ok((
        2,
        0,
        json!({"path":path,"sha256":google::digest(bytes),"student_physical_calls":2,"teacher_physical_calls":0,"outputs_and_scores_imported":false}),
    ))
}

fn verify_retained_seals(root: &Path, receipt: &Value) -> Result<()> {
    for (name, digest) in receipt["sealed_files"]
        .as_object()
        .context("No retained carrier seals")?
    {
        let relative = Path::new(name);
        ensure!(
            relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "Unsafe retained carrier path"
        );
        ensure!(
            digest == &json!(google::digest(fs::read(root.join(relative))?)),
            "Retained carrier drift: {name}"
        );
    }
    Ok(())
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
    let reused = directory.join("reused-approval.json");
    if reused.exists() {
        let receipt: Value = serde_json::from_slice(&fs::read(reused)?)?;
        let source = Path::new(
            receipt["source"]
                .as_str()
                .context("Missing reused approval source")?,
        );
        let original = fs::read(source)?;
        let current = fs::read(directory.join("gold.json"))?;
        ensure!(
            receipt["new_provider_calls"] == 0
                && receipt["sha256"] == google::digest(&original)
                && original == current,
            "Reused approval ancestry or exact record identity differs"
        );
        let record: GoldRecord = serde_json::from_slice(&original)?;
        ensure!(
            record.answer == *answer
                && record.review["passed"] == true
                && record.review["reading"] == "pass"
                && record.review["answer_quality"] == "pass",
            "Reused approval lacks its exact passed answer"
        );
        let original_directory = source
            .parent()
            .context("Missing original approval directory")?;
        ensure!(
            !original_directory.join("reused-approval.json").exists(),
            "Nested approval ancestry is not supported"
        );
        return received_answer_matches(original_directory, answer);
    }
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
    student: StudentSelection<'_>,
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
    ensure!(
        student.prior_closure.is_none()
            || ((student.local_url.is_some() || student.concise) && methods[0] == "contact"),
        "Stopped hosted Contact work is deducted only from a fresh local Contact-first baseline"
    );
    let (mut student_spent, mut teacher_spent, ancestry) =
        inherited_consumer_calls(student.prior_closure, student.concise)?;
    ensure!(
        !student.prepare_only || (student.concise && selected != "all"),
        "Concise teacher/admission preparation requires one explicitly selected family"
    );
    ensure!(
        student.prepared.is_none() || student.concise,
        "Prepared student demonstrations require the concise baseline"
    );
    fs::create_dir(directory)?;
    let author_plan = fs::read(campaign.join("prepared/plan.json"))?;
    if ancestry["completed_cache_only_demo"] == true {
        ensure!(
            ancestry["codex_executable_sha256"] == google::digest(fs::read(codex_path)?)
                && ancestry["author_plan_sha256"] == google::digest(&author_plan),
            "Completed worked-example executable/author identity differs; no repurchase"
        );
    }
    if let Some(cache) = ancestry["settled_student_cache"].as_str() {
        let source = Path::new(cache);
        let destination = directory.join("student");
        fs::create_dir(&destination)?;
        let mut imported = Vec::new();
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(key) = name.strip_suffix(".response.json") {
                ensure!(
                    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Invalid settled student key"
                );
                for suffix in ["request.json", "response.json"] {
                    let file = format!("{key}.{suffix}");
                    ensure!(
                        ancestry["settled_student_cache_seals"][format!("run/student/{file}")]
                            == google::digest(fs::read(source.join(&file))?),
                        "Unsealed settled student cache file"
                    );
                    fs::copy(source.join(&file), destination.join(&file))?;
                }
                imported.push(key.to_owned());
            }
        }
        ensure!(
            imported.len() == 11,
            "Closed successful student cache count differs"
        );
        crate::queue::write_json_atomic(
            &directory.join("settled-student-import.json"),
            &json!({"source":source,"keys":imported,"fresh_calls":0,"scores_imported":false,"uncertain_requests_replayed":false}),
        )?;
    }
    let prepared_receipt = if let Some(prepared) = student.prepared {
        let (prior_student, prior_teacher, receipt) = prepared_consumption(
            prepared,
            &ancestry,
            &google::digest(&author_plan),
            codex_path,
        )?;
        student_spent = prior_student;
        teacher_spent = prior_teacher;
        receipt
    } else {
        Value::Null
    };
    let listing = if let Some(url) = student.local_url {
        local::validate_url(url)?;
        ensure!(
            student.model == local::MODEL,
            "Gold local baseline must name the resident QAT/MTP model"
        );
        let base = url.trim_end_matches('/').trim_end_matches("/v1");
        let slots: Value = http
            .get(format!("{base}/slots"))
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let rows = slots.as_array().context("No resident slots")?;
        ensure!(
            !rows.is_empty() && rows.iter().all(|row| row["is_processing"] == false),
            "Resident gold comparison requires idle reported slots"
        );
        let context = rows
            .iter()
            .map(|row| row["n_ctx"].as_u64().context("No resident context limit"))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .min()
            .unwrap();
        ensure!(context > 8192, "Resident context cannot fit a reading");
        let models: Value = http
            .get(url.trim_end_matches('/').to_owned() + "/models")
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        json!({"student":student.model,"local_url":url,"local_context_limit":context,"slots":slots,"models":models,"provider":"resident-port-OpenAiCompatible"})
    } else {
        google::models(&http).await?
    };
    crate::queue::write_json_atomic(&directory.join("models.json"), &listing)?;
    let student_id = if student.local_url.is_some() {
        student.model
    } else {
        google::STUDENT
    };
    let plan = json!({"method_order":methods,"author_plan_sha256":google::digest(&author_plan),"book_sha256":crate::book::OCR_SHA256,"executable_sha256":google::digest(fs::read(std::env::current_exe()?)?),"teaching_sources_sha256":optimize::teaching_sources(),"codex_executable_sha256":google::digest(fs::read(codex_path)?),"per_family_physical_student_cap":64,"per_family_physical_teacher_cap":80,"cumulative_student_cap":64*methods.len(),"cumulative_teacher_cap":80*methods.len(),"soft_metric_calls":24,"stock_optimizers":["LabeledFewShot","GEPA"],"demos":if student.concise{1}else{2},"original_split":"12/4/4","student":student_id,"student_local_url":student.local_url,"local_request_timeout_seconds":if student.local_url.is_some(){Some(600)}else{None},"prior_consumer":ancestry,"prepared_student":prepared_receipt,"inherited_student_physical_calls":student_spent,"inherited_teacher_physical_calls":teacher_spent,"concise_student":student.concise,"prepare_student_only":student.prepare_only,"strict_student_input_limit":if student.concise{Some(STUDENT_INPUT_LIMIT)}else{None},"student_output_tokens":if student.concise{2048}else{8192},"teacher":"codex-saved-login-tool-free-no-override","teacher_output_tokens":8192,"google_rate_hint":if student.local_url.is_none(){Some(14000)}else{None},"google_probe_ceiling":if student.concise && student.local_url.is_none(){Some(16000)}else{None},"student_concurrency":1,"approval_gate":approval_gate,"automatic_uncertain_replay":false,"application_promoted":false,"score_scope":"Declared reading component inputs; not extracted original-message application journeys"});
    crate::queue::write_json_atomic(&directory.join("plan.json"), &plan)?;
    let limiter = if student.concise {
        google::RateLimiter::with_probe_ceiling(14000, 16000)?
    } else {
        google::RateLimiter::new(14000)?
    };
    let mut results = Vec::new();
    for (family_index, method) in methods.iter().enumerate() {
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
        let student_budget = google::CallBudget::seeded(
            if family_index == 0 {
                64
            } else {
                student_spent + 64
            },
            student_spent,
        )?;
        let teacher_budget = google::CallBudget::seeded(
            if family_index == 0 {
                80
            } else {
                teacher_spent + 80
            },
            teacher_spent,
        )?;
        let outcome=async {
            let (mut fixtures,mut labels)=reviewed_family(examples,&book,campaign,method)?;
            let teacher_directory=directory.join("teacher").join(method);
            if let Some(guard)=ancestry["unsettled_teacher_guard"].as_str().filter(|_|*method=="contact") {
                let source=Path::new(guard);let destination=teacher_directory.join(source.file_name().context("No old reviewer key")?);
                fs::create_dir_all(&destination)?;
                for entry in fs::read_dir(source)? {let entry=entry?;ensure!(entry.file_type()?.is_file(),"Old unobserved guard must contain files only");fs::copy(entry.path(),destination.join(entry.file_name()))?;}
                crate::queue::write_json_atomic(&family.join("unsettled-review-guard.json"),&json!({"source":source,"destination":destination,"resubmission_allowed":false,"semantic_score_imported":false}))?;
            }
            if *method=="contact" && student.prepared.is_some() {
                for operation in prepared_receipt["completed_teacher_operations"].as_array().context("No prepared paid operations")? {
                    codex::import_completed(Path::new(operation["source"].as_str().context("No prepared paid operation path")?),&teacher_directory)?;
                }
            }
            if *method=="contact" && student.prepared.is_none() && ancestry["completed_cache_only_demo"] == true {
                for operation in ancestry["completed_teacher_operations"].as_array().context("No completed worked-example operations")? {
                    codex::import_completed(Path::new(operation["source"].as_str().context("No received worked-example path")?),&teacher_directory)?;
                }
            }
            let codex_teacher=Arc::new(codex::Codex::new(codex_path.to_path_buf(),teacher_directory.clone(),80)?.with_budget(teacher_budget.clone()));
            let teacher:Arc<dyn DynChatModel>=codex_teacher.clone();
            if student.concise {
                concise_fixtures(&mut fixtures,method)?;
                let initial=count_labeled_inputs(&fixtures,method,&[],&http,&listing,&family.join("authored-preflight"),&student).await?;
                ensure!(initial["counts"].as_array().is_some_and(|rows|rows.len()==20),"Concise current-case preflight must cover all twenty original cases");
                let demo_teacher:Arc<dyn DynChatModel>=if *method=="contact" && ancestry["completed_cache_only_demo"] == true {
                    Arc::new(codex::Codex::new(codex_path.to_path_buf(),teacher_directory,80)?.with_budget(teacher_budget.clone()).with_completed_cache_only())
                } else {teacher.clone()};
                labels=if let Some(prepared)=student.prepared.filter(|_|*method=="contact") {load_concise_label(&fixtures,&book,method,&prepared.join("contact/concise-demo"))?} else {concise_label(&fixtures,method,&book,demo_teacher,&family.join("concise-demo")).await?};
                bind_approved_concise_demo(&mut fixtures,&labels,method)?;
            }
            let preflight=count_labeled_inputs(&fixtures,method,&labels,&http,&listing,&family.join("preflight"),&student).await?;
            if student.prepare_only {
                let operations=completed_teacher_seals(&directory.join("teacher").join(method),ancestry["unsettled_teacher_key"].as_str())?;
                return Ok(json!({"phase":"prepared_concise_student","method":method,"fixture_digest":optimize::fixtures_digest(&fixtures)?,"demo_count":1,"input_limit":6000,"student_calls":0,"preflight":preflight,"teacher_physical_calls_total":teacher_budget.spent(),"completed_teacher_operations":operations}));
            }
            let selected:Arc<dyn DynChatModel>=if let Some(url)=student.local_url {
                let mut local=local::LocalStudent::new(url,student.model,&family.join("student"))?.with_budget(student_budget.clone()).with_timeout(600)?;
                if student.concise {local=local.with_input_limit(STUDENT_INPUT_LIMIT)?;}Arc::new(local)
            } else {
                let mut google=google::Google::from_listing(serde_json::from_value(listing["student"].clone())?,http.clone(),directory.join("student"),student_budget.clone(),limiter.clone())?;
                if student.concise {google=google.with_input_limit(STUDENT_INPUT_LIMIT)?;}Arc::new(google)
            };
            let teacher:Arc<dyn DynChatModel>=if student.concise && student.local_url.is_none() {Arc::new(crate::rewrite::CompactTeacher::new(codex_teacher,concise_predictor(method,&labels,1)?,fixtures.clone(),family.join("rewrite-admission"),http.clone()))} else {teacher};
            let models=runner::Models{student:selected,teacher,student_id:student_id.into(),teacher_id:"codex-saved-login-tool-free-no-override".into()};
            crate::queue::write_json_atomic(&family.join("started.json"),&json!({"gold_bank_sha256":google::digest(fs::read(campaign.join("run").join(method).join("answers.jsonl"))?),"fixture_digest":optimize::fixtures_digest(&fixtures)?,"preflight":preflight,"inherited_student_physical_calls":student_spent,"inherited_teacher_physical_calls":teacher_spent,"caps_carried_without_reset":true}))?;
            let baseline=if student.concise {CONCISE_BASELINE.to_owned()} else {reading::direct_reader_baseline(method)?};
            optimize::compile_direct_reader_from_gold(&fixtures,method,book.clone(),&models,&family.join("compile"),optimize::CompileOptions{max_metrics:24,bootstrap_demos:0},&baseline,&labels,if student.concise{1}else{2}).await
        }.await;
        student_spent = student_budget.spent();
        teacher_spent = teacher_budget.spent();
        let status = match outcome {
            Ok(result) => {
                json!({"method":method,"status":if student.prepare_only{"prepared"}else{"complete"},"result":result,"student_physical_calls_total":student_spent,"teacher_physical_calls_total":teacher_spent,"application_promoted":false})
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
    #[test]
    #[ignore = "Uses preserved local source-hashed campaign; no network or provider calls"]
    fn actual_closed_rewrite_ancestry_replays_only_sealed_completed_operations() {
        let closure = Path::new(
            "/Users/george/.codex/evidence/horary-working-loop-20261010/hosted-gold-contact-relevant-r2-586161e/closure.json",
        );
        let (student, teacher, ancestry) = inherited_consumer_calls(Some(closure), true).unwrap();
        assert_eq!((student, teacher), (16, 15));
        assert_eq!(
            ancestry["completed_teacher_operations"]
                .as_array()
                .unwrap()
                .len(),
            14
        );
        assert_eq!(
            ancestry["unsettled_teacher_key"],
            "387101e1e54227fea44b94a5efa0992c53f6f3be358bbd15b656a78e0d803026"
        );
        assert_eq!(ancestry["replay_closed_student_and_teacher_only"], true);
    }
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
    fn reused_approval_requires_exact_record_and_original_received_witness() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let current = temp.path().join("current");
        fs::create_dir_all(old.join("attempt-1")).unwrap();
        fs::create_dir(&current).unwrap();
        let record = GoldRecord {
            case_id: "synthetic-train".into(),
            split: "train".into(),
            input_hash: "bound-input".into(),
            answer: answer(),
            source_witnesses: json!({"book":"bound"}),
            review: json!({"passed":true,"reading":"pass","answer_quality":"pass"}),
        };
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(old.join("gold.json"), &bytes).unwrap();
        fs::write(current.join("gold.json"), &bytes).unwrap();
        fs::write(current.join("reused-approval.json"),serde_json::to_vec(&json!({"source":old.join("gold.json"),"sha256":google::digest(&bytes),"new_provider_calls":0})).unwrap()).unwrap();
        assert!(!received_answer_matches(&current, &record.answer).unwrap());
        fs::write(
            old.join("attempt-1/answer.json"),
            serde_json::to_vec(&record.answer).unwrap(),
        )
        .unwrap();
        assert!(received_answer_matches(&current, &record.answer).unwrap());
        fs::write(current.join("gold.json"), b"changed").unwrap();
        assert!(received_answer_matches(&current, &record.answer).is_err());
    }

    #[test]
    fn continuation_inventory_charges_unsettled_requests_and_binds_changes() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("prepared")).unwrap();
        fs::write(temp.path().join("prepared/plan.json"), "{}").unwrap();
        let operation = temp.path().join("run/contact/case/teacher/unknown-key");
        fs::create_dir_all(&operation).unwrap();
        fs::write(operation.join("request.json"), r#"{"submitted":true}"#).unwrap();
        let original = continuation_inventory(temp.path()).unwrap();
        assert_eq!(original["inherited_physical_charges"], 1);
        assert_eq!(original["remaining_physical_cap"], AUTHOR_CALL_CAP - 1);
        assert_eq!(original["operations"][0]["completed"], false);
        fs::write(operation.join("answer.txt"), "Later recovered bytes").unwrap();
        assert_ne!(original, continuation_inventory(temp.path()).unwrap());
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
    fn stopped_hosted_consumer_deducts_settled_calls_but_never_unknown_work() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("received.json"),
            b"Synthetic fully received carrier",
        )
        .unwrap();
        let mut value = json!({"campaign":directory.path(),"exact_pids_absent":true,"uncertain_requests":0,"student_normalized_outputs":0,"semantic_reviews":0,"third_attempt_wait_generation_submitted":false,"student_physical_calls":2,"teacher_physical_calls":0,"original_caps":{"student_per_family":64,"teacher_per_family":80},"sealed_files":{"received.json":google::digest(b"Synthetic fully received carrier")},"results":[{"physical_call":1,"response_complete":true,"http_status":429},{"physical_call":2,"response_complete":true,"http_status":429}]});
        let path = directory.path().join("closure.json");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let (student, teacher, receipt) = inherited_consumer_calls(Some(&path), false).unwrap();
        assert_eq!((student, teacher), (2, 0));
        assert_eq!(receipt["outputs_and_scores_imported"], false);
        value["results"][1]["response_complete"] = json!(false);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(inherited_consumer_calls(Some(&path), false).is_err());
        value["results"][1]["response_complete"] = json!(true);
        value["uncertain_requests"] = json!(1);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(inherited_consumer_calls(Some(&path), false).is_err());
    }

    #[test]
    fn stopped_large_input_consumer_charges_unknown_review_without_replaying_it() {
        let directory = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        let teacher = directory.path().join("run/teacher/contact").join(&key);
        fs::create_dir_all(&teacher).unwrap();
        fs::write(teacher.join("request.json"), b"Synthetic unknown request").unwrap();
        let relative = format!("run/teacher/contact/{key}/request.json");
        let closure = json!({"campaign":directory.path(),"exact_pids_absent":true,"reviewer_process_absent":true,"student_physical_calls_fresh":1,"student_physical_calls_inherited_hosted":2,"student_physical_calls_cumulative":3,"teacher_physical_calls_fresh":1,"student_normalized_outputs":1,"semantic_reviews":0,"unsettled_teacher_operations":1,"reviewer_key":key,"sealed_files":{relative:google::digest(b"Synthetic unknown request")}});
        let path = directory.path().join("closure.json");
        fs::write(&path, serde_json::to_vec(&closure).unwrap()).unwrap();
        assert!(inherited_consumer_calls(Some(&path), false).is_err());
        let (student, teacher_count, ancestry) =
            inherited_consumer_calls(Some(&path), true).unwrap();
        assert_eq!((student, teacher_count), (3, 1));
        assert_eq!(
            ancestry["unsettled_teacher_guard"],
            teacher.to_string_lossy().as_ref()
        );
        assert_eq!(ancestry["outputs_and_scores_imported"], false);
        fs::write(teacher.join("response.json"), b"Unexpected replacement").unwrap();
        assert!(inherited_consumer_calls(Some(&path), true).is_err());
    }

    #[test]
    fn canonical_judge_inputs_stay_separate_from_projected_student_and_reference_labels() {
        let canonical = dsrust::input! {source_text:"Canonical compact source",question:"Same actual question",consultation:"Canonical declared facts",chart_evidence:"Full original native table"};
        let mut fixture = dsrust::input! {source_text:"Short procedure",question:"Same actual question",consultation:"Canonical declared facts",chart_evidence:"Finite concise chart"};
        fixture.set("__review_inputs", json!(canonical.rendered()));
        fixture.set("__reference_answer", json!("Private full worked answer"));
        fixture.set("expected", json!("Private rubric"));
        assert_eq!(
            canonical_review_inputs(&fixture).unwrap().rendered(),
            canonical.rendered()
        );
        let student = fixture.inputs().unwrap();
        assert_eq!(student.len(), 4);
        assert!(student.get("__reference_answer").is_none());
        assert_ne!(
            student.get("chart_evidence"),
            canonical_review_inputs(&fixture)
                .unwrap()
                .get("chart_evidence")
        );
        fixture.set(
            "__review_inputs",
            json!([
                ["source_text", "s"],
                ["question", "q"],
                ["consultation", "c"],
                ["chart_evidence", "n"],
                ["expected", "Do not send this rubric"]
            ]),
        );
        assert!(canonical_review_inputs(&fixture).is_err());
    }

    #[test]
    fn named_four_input_guard_allows_field_order_but_not_extra_or_changed_values() {
        let first =
            dsrust::input! {question:"q",consultation:"c",chart_evidence:"n",source_text:"s"};
        let mut second =
            dsrust::input! {source_text:"s",question:"q",consultation:"c",chart_evidence:"n"};
        assert_ne!(first.rendered(), second.rendered());
        assert!(same_named_inputs(&first, &second));
        second.set("chart_evidence", json!("Changed native fact"));
        assert!(!same_named_inputs(&first, &second));
        second.set("chart_evidence", json!("n"));
        second.set("expected", json!("Private rubric"));
        assert!(!same_named_inputs(&first, &second));
    }

    #[test]
    #[ignore = "Requires the exact private book and approved twenty-case reference bank; no providers"]
    fn actual_contact_gold_projects_and_verifies_all_twenty_offline() {
        let book = Book::open(&std::path::PathBuf::from(
            std::env::var("HORARY_BOOK").unwrap(),
        ))
        .unwrap();
        let examples = std::path::PathBuf::from(std::env::var("HORARY_EXAMPLES").unwrap());
        let campaign = std::path::PathBuf::from(std::env::var("HORARY_GOLD_CAMPAIGN").unwrap());
        let method = std::env::var("HORARY_RELEVANT_METHOD").unwrap_or_else(|_| "contact".into());
        let (mut fixtures, _) = reviewed_family(&examples, &book, &campaign, &method).unwrap();
        assert_eq!(fixtures.len(), 20);
        concise_fixtures(&mut fixtures, &method).unwrap();
        for fixture in &fixtures {
            verify_concise_fixture(&fixture.example, &method).unwrap();
            let chart = fixture
                .example
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap();
            assert!(!catalogue::relevant_chart_ids(chart).unwrap().is_empty());
        }
        let requests =
            preflight::reader_requests(&fixtures, &concise_predictor(&method, &[], 0).unwrap())
                .unwrap();
        assert_eq!(requests.len(), 20);
        if let Ok(output) = std::env::var("HORARY_CONCISE_WIRES") {
            crate::queue::write_json_atomic(Path::new(&output),&json!({"cases":20,"fixture_digest":optimize::fixtures_digest(&fixtures).unwrap(),"requests":requests,"provider_calls":0,"generation_submitted":false})).unwrap();
        }
        println!(
            "All twenty original {method} inputs projected, native-validated and rendered without provider calls"
        );
    }

    #[tokio::test]
    #[ignore = "Requires retained private book/reference/cache artifacts; no providers"]
    async fn actual_completed_worked_answer_replays_under_new_presentation_without_paid_calls() {
        let book = Book::open(Path::new(&std::env::var("HORARY_BOOK").unwrap())).unwrap();
        let examples = std::path::PathBuf::from(std::env::var("HORARY_EXAMPLES").unwrap());
        let campaign = std::path::PathBuf::from(std::env::var("HORARY_GOLD_CAMPAIGN").unwrap());
        let retained =
            std::path::PathBuf::from(std::env::var("HORARY_RETAINED_PREPARATION").unwrap());
        let (student, teacher_calls, receipt) =
            inherited_consumer_calls(Some(&retained.join("closure.json")), true).unwrap();
        assert_eq!((student, teacher_calls), (3, 3));
        let codex_path = std::path::PathBuf::from("/opt/homebrew/bin/codex");
        assert_eq!(
            receipt["codex_executable_sha256"],
            google::digest(fs::read(&codex_path).unwrap())
        );
        let temporary = tempfile::tempdir().unwrap();
        let cache = temporary.path().join("teacher");
        for operation in receipt["completed_teacher_operations"].as_array().unwrap() {
            codex::import_completed(Path::new(operation["source"].as_str().unwrap()), &cache)
                .unwrap();
        }
        let budget = google::CallBudget::seeded(80, 3).unwrap();
        let teacher: Arc<dyn DynChatModel> = Arc::new(
            codex::Codex::new(codex_path, cache, 80)
                .unwrap()
                .with_budget(budget.clone())
                .with_completed_cache_only(),
        );
        let (mut fixtures, _) = reviewed_family(&examples, &book, &campaign, "contact").unwrap();
        concise_fixtures(&mut fixtures, "contact").unwrap();
        let label = concise_label(
            &fixtures,
            "contact",
            &book,
            teacher,
            &temporary.path().join("demo"),
        )
        .await
        .unwrap();
        let expected: WorkedAnswer = serde_json::from_slice(
            &fs::read(retained.join("run/contact/concise-demo/answer.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            WorkedAnswer::from_prediction(&Prediction::new(
                label[0].clone(),
                "Reused received answer"
            ))
            .unwrap(),
            expected
        );
        assert_eq!(budget.spent(), 3);
        let displayed = crate::demo::compact(&label[0]).unwrap();
        let chart = displayed.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(
            crate::demo::cited_ids(&displayed)
                .unwrap()
                .is_subset(&catalogue::relevant_chart_ids(chart).unwrap())
        );
        assert!(
            !labeled_inputs_match_training(&label[0], &fixtures[0].example, "contact").unwrap()
        );
        bind_approved_concise_demo(&mut fixtures, &label, "contact").unwrap();
        assert!(labeled_inputs_match_training(&label[0], &fixtures[0].example, "contact").unwrap());
        let mut changed = label[0].clone();
        changed.set(
            "analysis",
            json!("A different answer cannot inherit this approval"),
        );
        assert!(!labeled_inputs_match_training(&changed, &fixtures[0].example, "contact").unwrap());
        let mut changed = label[0].clone();
        changed.set(
            "chart_evidence",
            json!("<facts>\ne1|position|Changed supplied fact\n</facts>"),
        );
        assert!(!labeled_inputs_match_training(&changed, &fixtures[0].example, "contact").unwrap());
        let requests = preflight::reader_requests(
            &fixtures,
            &concise_predictor("contact", &label, 1).unwrap(),
        )
        .unwrap();
        assert_eq!(requests.len(), 20);
        if let Ok(output) = std::env::var("HORARY_CONCISE_WIRES") {
            crate::queue::write_json_atomic(Path::new(&output), &json!({"cases":20,"requests":requests,"teacher_calls":0,"student_calls":0,"inherited_student_calls":3,"inherited_teacher_calls":3,"answer_unchanged":true})).unwrap();
        }
        // Run the real compiler and native guards with offline stubs. These
        // deliberate grades check wiring only and establish no semantic result.
        let student = Arc::new(dsrust::DummyLM::new((0..64).map(|_| dsrust::example! {
            status:"method_limit",answer:"Offline construction fixture, not an actual reading",
            analysis:"e1 supplied test fact",evidence:"e1",unresolved:"Offline stub; no model quality claim"
        })));
        let judge = Arc::new(dsrust::DummyLM::new((0..64).map(|_| dsrust::example! {
            score:1.,passed:true,classification:"unobserved",extraction:"unobserved",reading:"pass",answer_quality:"pass",feedback:"Offline wiring stub, not semantic acceptance"
        })));
        let models = runner::Models {
            student: student.clone(),
            teacher: judge,
            student_id: "Offline no-network construction".into(),
            teacher_id: "Offline no-network construction".into(),
        };
        let result = optimize::compile_direct_reader_from_gold(
            &fixtures,
            "contact",
            Arc::new(book),
            &models,
            &temporary.path().join("compile"),
            optimize::CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            CONCISE_BASELINE,
            &label,
            1,
        )
        .await
        .unwrap();
        assert!(result.get("authored_development").is_some());
        assert!(!student.asked().is_empty());
    }

    #[test]
    fn concise_native_domain_includes_all_three_tables_and_rejects_source_corruption() {
        let chart = json!({"presentation":"concise-native-reader-1",
            "facts":{"columns":["id","kind","label","native_detail"],"rows":[["e1","boundary","Limits","Exact supplied limits"]]},
            "receptions":{"columns":["id","regarding_planet","regarded_planet","positive_qualities","negative_qualities"],"rows":[["e2","Mars","Saturn",["face"],["fall"]]]},
            "events":{"columns":["id","endpoint1","endpoint2","aspect","within_current_signs","astronomical_hours","native_current_angular_gap_or_detail","motion"],"rows":[["e3","Moon","Mars","square",true,1.25,"13.369°",null]]}});
        let mut input = dsrust::input! {chart_evidence:serde_json::to_string(&chart).unwrap()};
        let mut worked = answer();
        worked.evidence = "e1 e2 e3".into();
        let prediction = Prediction::new(
            worked.labeled_example(&input).unwrap().labels().unwrap(),
            "Synthetic received reading",
        );
        assert!(
            optimize::reading_native_failures(&input, &prediction)
                .unwrap()
                .is_empty()
        );
        for changed in ["duplicate", "header", "width"] {
            let mut corrupt = chart.clone();
            match changed {
                "duplicate" => corrupt["events"]["rows"][0][0] = json!("e2"),
                "header" => corrupt["receptions"]["columns"][0] = json!("guess"),
                _ => corrupt["facts"]["rows"][0] = json!(["e1", "boundary"]),
            };
            input.set(
                "chart_evidence",
                json!(serde_json::to_string(&corrupt).unwrap()),
            );
            assert!(optimize::reading_native_failures(&input, &prediction).is_err());
        }
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
