//! One acceptance, retry, wait and checkpoint path for single and batch steps.
#![forbid(unsafe_code)]
use crate::{
    conversation::{Message, Session},
    horary_contract::decode_json,
    horary_lessons::{self as lessons, Matter, Stage},
    horary_pipeline::{Record, Runtime},
    horary_step::{self as step, Checked, CheckedData},
    native_llama_worker::NativeGenerationResult,
    reading_method::Fact,
};
use serde_json::{json, Value};

fn generate(
    runtime: &impl Runtime,
    stage: Stage,
    matter: Matter,
    input: Value,
    facts: &[Fact],
    audio: Option<&[u8]>,
) -> Result<(Result<Checked, String>, Record), String> {
    runtime.check()?;
    let contract = step::response_schema_for(stage, matter, &input, facts);
    let result = runtime.generate(stage, matter, &input, &contract, audio)?;
    evaluate(stage, matter, input, contract, result, facts)
}

fn evaluate(
    stage: Stage,
    matter: Matter,
    input: Value,
    contract: Value,
    result: NativeGenerationResult,
    facts: &[Fact],
) -> Result<(Result<Checked, String>, Record), String> {
    let parsed = decode_json(&result.content);
    let checked = parsed
        .as_ref()
        .map_err(|e| e.clone())
        .and_then(|v| step::check(stage, matter, v, &input, facts));
    let validation_error = checked.as_ref().err().cloned();
    let value = parsed.unwrap_or(Value::Null);
    let record = Record {
        stage,
        revision: 0,
        guide_sha256: lessons::digest(&crate::horary_contract::guide_for(stage, matter, &input)?),
        schema_sha256: lessons::digest(&contract.to_string()),
        input_sha256: lessons::digest(&input.to_string()),
        input,
        raw: result.content.clone(),
        worksheet: value.clone(),
        generation: result,
        source_passages: stage.passages(matter).iter().map(|s| (*s).into()).collect(),
        validation_error,
    };
    Ok((checked, record))
}

fn keep(session: &mut Session, runtime: &impl Runtime, mut record: Record) -> Result<(), String> {
    record.revision = session.revision;
    let metric = crate::conversation::inference_measurement(&record.generation);
    if let Ok(progress) = crate::review_progress::record_with_inference(
        runtime.directory(),
        record.stage.name(),
        record.stage.activity(),
        metric.elapsed_ms,
        Some(metric),
    ) {
        session.progress.push(progress);
        if session.progress.len() > 160 {
            session.progress.remove(0);
        }
    }
    log::info!("horary stage={} revision={} guide={} prompt_tokens={} cached_tokens={} output_tokens={} elapsed_ms={} lesson_bank_hit={:?} lesson_prepare_ms={:?} valid={}",record.stage.name(),record.revision,record.guide_sha256,record.generation.prompt_tokens,record.generation.cached_prompt_tokens,record.generation.generated_tokens,record.generation.elapsed_ms,record.generation.lesson_bank_hit,record.generation.lesson_prepare_ms,record.validation_error.is_none());
    session.audit.push(json!({"event":"method_stage","stage":record.stage,"dependencies":record.stage.dependencies(),"bookOcrSha256":lessons::BOOK_OCR_SHA256,"revision":record.revision,"guideSha256":record.guide_sha256,"schemaSha256":record.schema_sha256,"inputSha256":record.input_sha256,"generation":record.generation}));
    session.method.records.push(record);
    runtime.publish(session)
}

pub(crate) fn task(
    session: &mut Session,
    runtime: &impl Runtime,
    stage: Stage,
    input: Value,
    facts: &[Fact],
) -> Result<Option<CheckedData>, String> {
    execute(session, runtime, stage, input, facts, None)
}

enum Received {
    Complete(Box<CheckedData>),
    AwaitingUser,
    Rejected { proposal: Value, error: String },
}

/// All actual model results, including batch cases, enter the same acceptance
/// path. No caller can mark a decoded JSON value as complete.
fn receive(
    session: &mut Session,
    runtime: &impl Runtime,
    key: &str,
    checked: Result<Checked, String>,
    mut record: Record,
) -> Result<Received, String> {
    let stage = record.stage;
    let proposal = record.worksheet.clone();
    let record_index = session.method.records.len();
    // A result may be well formed but belong to superseded consultation
    // inputs. Reject it through the same journal/receipt/repair boundary.
    let checked = checked.and_then(|value| {
        if let Some(case) = &session.method.consultation {
            let request = &step::original_input(&record.input)["reading_request"];
            if request.is_object()
                && (request["binding"]["catalogue_version"] != case.catalogue_version
                    || request["binding"]["case_revision"].as_u64() != Some(case.revision)
                    || request["binding"]["question"].as_str()
                        != case.question.resolved().map(String::as_str)
                    || request["binding"]["frame"]
                        != serde_json::to_value(case.frame.resolved()).map_err(|e| e.to_string())?)
            {
                return Err("This result belongs to older consultation inputs; this task has not completed.".into());
            }
        }
        Ok(value)
    });
    record.validation_error = checked.as_ref().err().cloned();
    keep(session, runtime, record)?;
    let (to, received) = match checked {
        Ok(Checked::Data(data)) => {
            session.method.flow.finish(key, &data, record_index)?;
            ("complete", Received::Complete(Box::new(data)))
        }
        Ok(Checked::NeedsInput { request }) => {
            if let Some(case) = session.method.consultation.as_mut() {
                let key = crate::reading_contracts::need_key(&request.field, case)?;
                case.require_information(key.clone(), request.reason.clone());
                if matches!(
                    key,
                    crate::reading_contracts::RequirementKey::Field(
                        crate::reading_contracts::Field::Context
                            | crate::reading_contracts::Field::SearchContext
                    )
                ) {
                    if let Some(need) = case.additional.iter_mut().find(|need| need.key == key) {
                        need.question = Some(request.question.clone());
                        need.reason = request.reason.clone();
                    }
                }
                let question = case.question_for(&key);
                session.method.result =
                    Some(crate::reading_contracts::ReadingResult::NeedsInformation {
                        need: crate::reading_contracts::InformationNeed {
                            key,
                            reason: request.reason.clone(),
                            question: Some(question),
                        },
                    });
            }
            session.method.flow.wait(key, request)?;
            ("awaiting_user", Received::AwaitingUser)
        }
        Err(error) => {
            session.method.flow.reject(key, error.clone())?;
            ("repairing", Received::Rejected { proposal, error })
        }
    };
    session.audit.push(json!({"event":"stage_transition","stage":stage,"key":key,"to":to,"recordIndex":record_index}));
    runtime.publish(session)?;
    Ok(received)
}

pub(crate) fn execute(
    session: &mut Session,
    runtime: &impl Runtime,
    stage: Stage,
    input: Value,
    facts: &[Fact],
    audio: Option<&[u8]>,
) -> Result<Option<CheckedData>, String> {
    let supplied = work_input(session, stage, input);
    let original = step::original_input(&supplied).clone();
    if audio.is_none() && !matches!(stage, Stage::Intake | Stage::Explanation) {
        if let Some((data, index)) = cached_data(session, stage, &original, facts)? {
            let key = work_key(session, stage, &original, facts)?;
            session
                .method
                .flow
                .reuse(key, session.revision, &data, index)?;
            session.audit.push(json!({"event":"stage_data_reused","stage":stage,"revision":session.revision,"inputSha256":lessons::digest(&original.to_string()),"basis":"The exact current input, lesson and contract match; native checks were rerun."}));
            runtime.publish(session)?;
            return Ok(Some(data));
        }
    }
    let mut key = work_key(session, stage, &original, facts)?;
    if matches!(stage, Stage::Intake | Stage::Explanation) {
        key.push_str(&format!("-{}", session.method.records.len()));
    }
    session.status = stage.activity().into();
    let mut input = supplied;
    loop {
        runtime.check()?;
        session.method.flow.begin(
            key.clone(),
            stage,
            session.revision,
            lessons::digest(&original.to_string()),
        )?;
        session
            .audit
            .push(json!({"event":"stage_transition","stage":stage,"key":key,"to":"running"}));
        if let Err(error) = step::prerequisites(stage, &original) {
            session.method.flow.pause(error.clone());
            runtime.publish(session)?;
            return Err(error);
        }
        runtime.publish(session)?;
        let (checked, record) = generate(
            runtime,
            stage,
            session.method.brief.matter,
            input.clone(),
            facts,
            audio,
        )?;
        match receive(session, runtime, &key, checked, record)? {
            Received::Complete(data) => return Ok(Some(*data)),
            Received::AwaitingUser => {
                ask_pending(session);
                return Ok(None);
            }
            Received::Rejected { proposal, error } => {
                input = repair_input(&original, proposal, error)
            }
        }
    }
}

fn repair_input(original: &Value, previous: Value, error: String) -> Value {
    json!({"original_input":original,"previous_worksheet":previous,"native_validation_error":error,
        "instruction":"This step has not completed. Only original_input has accepted authority; previous_worksheet was rejected and none of its proposed facts were saved. Correct against the original data and the specific native error. Supply required data, or use request_input for genuinely missing user context. Do not ask the user to repeat provided words or supply chart calculations. Earlier attempts remain in the receipts."})
}

fn work_input(session: &Session, stage: Stage, mut input: Value) -> Value {
    let replies: Vec<_> = session
        .method
        .replies
        .iter()
        .filter(|reply| reply.stage == stage)
        .collect();
    if !replies.is_empty() && input.get("original_input").is_none() {
        input["stage_user_replies"] = json!(replies);
    }
    input
}

fn work_key(
    session: &Session,
    stage: Stage,
    input: &Value,
    facts: &[Fact],
) -> Result<String, String> {
    Ok(lessons::digest(&json!({"stage":stage,"revision":session.revision,"guide":lessons::digest(&crate::horary_contract::guide_for(stage, session.method.brief.matter,input)?),"contract":step::response_schema_for(stage,session.method.brief.matter,input,facts),"input":input,"validationAuthority":step::validation_authority()}).to_string()))
}

fn cached_data(
    session: &Session,
    stage: Stage,
    input: &Value,
    facts: &[Fact],
) -> Result<Option<(CheckedData, usize)>, String> {
    let input_hash = lessons::digest(&input.to_string());
    let guide_hash = lessons::digest(&crate::horary_contract::guide_for(
        stage,
        session.method.brief.matter,
        input,
    )?);
    let schema_hash = lessons::digest(
        &step::response_schema_for(stage, session.method.brief.matter, input, facts).to_string(),
    );
    for (index, record) in session
        .method
        .records
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, record)| {
            record.stage == stage
                && record.revision == session.revision
                && record.validation_error.is_none()
                && record.guide_sha256 == guide_hash
                && record.schema_sha256 == schema_hash
        })
    {
        if lessons::digest(&step::original_input(&record.input).to_string()) == input_hash {
            if let Ok(Checked::Data(data)) = step::check(
                stage,
                session.method.brief.matter,
                &record.worksheet,
                input,
                facts,
            ) {
                return Ok(Some((data, index)));
            }
        }
    }
    Ok(None)
}

pub(crate) fn execute_batch(
    session: &mut Session,
    runtime: &impl Runtime,
    tasks: &[(Stage, Value, Vec<Fact>)],
) -> Result<Vec<Option<CheckedData>>, String> {
    if tasks.is_empty() || tasks.len() > 4 {
        return Err("An independent native batch needs one to four steps.".into());
    }
    let matter = session.method.brief.matter;
    let mut values: Vec<Option<CheckedData>> = (0..tasks.len()).map(|_| None).collect();
    let mut pending = Vec::new();
    // Revalidate cached data before beginning any uncached work. All running
    // cases then have a durable job before the single native batch call.
    for (index, (stage, input, facts)) in tasks.iter().enumerate() {
        let input = work_input(session, *stage, input.clone());
        step::prerequisites(*stage, &input)?;
        let key = work_key(session, *stage, &input, facts)?;
        if let Some((data, record_index)) = cached_data(session, *stage, &input, facts)? {
            session
                .method
                .flow
                .reuse(key, session.revision, &data, record_index)?;
            session.audit.push(json!({"event":"stage_data_reused","stage":stage,"revision":session.revision,"recordIndex":record_index}));
            values[index] = Some(data);
        } else {
            pending.push((index, key, *stage, input, facts));
        }
    }
    if pending.is_empty() {
        runtime.publish(session)?;
        return Ok(values);
    }
    for (_, key, stage, input, _) in &pending {
        session.method.flow.begin(
            key.clone(),
            *stage,
            session.revision,
            lessons::digest(&input.to_string()),
        )?;
        session.audit.push(json!({"event":"stage_transition","stage":stage,"key":key,"to":"running","batchSize":pending.len()}));
    }
    runtime.check()?;
    runtime.publish(session)?;
    let requests: Vec<_> = pending
        .iter()
        .map(|(_, _, stage, input, facts)| {
            (
                *stage,
                matter,
                input.clone(),
                step::response_schema_for(*stage, matter, input, facts),
            )
        })
        .collect();
    let batch = runtime.generate_batch(&requests)?;
    if batch.len() != pending.len() {
        return Err("An independent task is missing from the batch.".into());
    }
    let mut repairs = Vec::new();
    // Receive EVERY case before repairing one. Good sibling data and rejected
    // proposals survive cancellation or failure in a later repair.
    for ((index, key, stage, input, facts), generation) in pending.into_iter().zip(batch) {
        let contract = step::response_schema_for(stage, matter, &input, facts);
        let (checked, record) =
            evaluate(stage, matter, input.clone(), contract, generation, facts)?;
        match receive(session, runtime, &key, checked, record)? {
            Received::Complete(data) => values[index] = Some(*data),
            Received::AwaitingUser => {}
            Received::Rejected { proposal, error } => {
                repairs.push((index, stage, input, facts, proposal, error))
            }
        }
    }
    for (index, stage, input, facts, proposal, error) in repairs {
        values[index] = task(
            session,
            runtime,
            stage,
            repair_input(&input, proposal, error),
            facts,
        )?;
    }
    ask_pending(session);
    Ok(values)
}

pub(crate) fn ask_pending(session: &mut Session) {
    if let Some(case) = session.method.consultation.as_ref() {
        if let Some(key) = &case.requested {
            let native_anchor_question = session.method.flow.pending.iter().find(|pending| {
                matches!(key, crate::reading_contracts::RequirementKey::ChartPlace)
                    && pending.request.field == "chart_place"
                    || matches!(key, crate::reading_contracts::RequirementKey::ChartMoment)
                        && pending.request.field == "chart_moment"
            });
            let question = native_anchor_question.map_or_else(
                || case.question_for(key),
                |pending| pending.request.question.clone(),
            );
            if !session
                .messages
                .last()
                .is_some_and(|m| m.role == "assistant" && m.text == question)
            {
                session.messages.push(Message {
                    role: "assistant".into(),
                    text: question,
                });
            }
            return;
        }
    }
    let question = session
        .method
        .flow
        .pending
        .iter()
        .map(|pending| pending.request.question.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if !question.is_empty()
        && !session
            .messages
            .last()
            .is_some_and(|message| message.role == "assistant" && message.text == question)
    {
        session.messages.push(Message {
            role: "assistant".into(),
            text: question,
        });
    }
}
