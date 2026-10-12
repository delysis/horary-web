//! One acceptance, retry, wait and checkpoint path for single and batch steps.
#![forbid(unsafe_code)]
use crate::{
    conversation::Session,
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
    let guide_sha256 = lessons::digest(&runtime.guide_for(stage, matter, &input)?);
    let contract = step::response_schema_for(stage, matter, &input, facts);
    let result = runtime.generate(stage, matter, &input, &contract, audio)?;
    evaluate(stage, matter, input, contract, result, facts, guide_sha256)
}

fn evaluate(
    stage: Stage,
    matter: Matter,
    input: Value,
    contract: Value,
    result: NativeGenerationResult,
    facts: &[Fact],
    guide_sha256: String,
) -> Result<(Result<Checked, String>, Record), String> {
    let parsed = decode_json(&result.content);
    let checked = parsed
        .as_ref()
        .map_err(|e| e.clone())
        .and_then(|v| step::check(stage, matter, v, &input, facts));
    let validation_error = checked.as_ref().err().cloned();
    let value = parsed.unwrap_or(Value::Null);
    let source_passages = crate::horary_contract::source_passages_for(stage, matter, &input)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let record = Record {
        stage,
        revision: 0,
        guide_sha256,
        schema_sha256: lessons::digest(&contract.to_string()),
        input_sha256: lessons::digest(&input.to_string()),
        input,
        raw: result.content.clone(),
        worksheet: value.clone(),
        generation: result,
        source_passages,
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
    NoProgress,
}

/// Pipeline error handlers must not turn a proved local stop into a pause of
/// unrelated work. A string alone is insufficient: retain the current native
/// witness and reject stale witnesses followed by another dispatch.
pub(crate) fn pause_after_error(session: &mut Session, error: String) {
    let local_stop = error == horary_prompt_program::rejection_cycle::STOP
        && session
            .audit
            .iter()
            .rposition(|event| event["event"] == "stage_no_progress")
            .is_some_and(|index| {
                !session.audit[index + 1..]
                    .iter()
                    .any(|event| event["event"] == "stage_transition" && event["to"] == "running")
                    && serde_json::to_value(&*session).ok().is_some_and(|value| {
                        horary_prompt_program::rejection_cycle::verify(
                            &value,
                            &session.audit[index],
                        )
                        .is_ok_and(|proof| proof.revision == session.revision)
                    })
            });
    if !local_stop {
        session.method.flow.pause(error);
    }
}

fn repeated_rejection(session: &Session, key: &str, index: usize) -> Result<Option<Value>, String> {
    use horary_prompt_program::rejection_cycle::{same_rejection, Witness};
    let repeated = session
        .method
        .records
        .get(index)
        .ok_or("Missing new rejection record")?;
    let value = serde_json::to_value(repeated).map_err(|error| error.to_string())?;
    for transition in &session.audit {
        if transition["event"] != "stage_transition"
            || transition["key"] != key
            || transition["to"] != "repairing"
        {
            continue;
        }
        let Some(first_index) = transition["recordIndex"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < index)
        else {
            continue;
        };
        let Some(first) = session.method.records.get(first_index) else {
            continue;
        };
        if !same_rejection(
            &serde_json::to_value(first).map_err(|error| error.to_string())?,
            &value,
        ) {
            continue;
        }
        let job = session
            .method
            .flow
            .jobs
            .iter()
            .find(|job| job.key == key)
            .ok_or("No native owner of the repeated rejection")?;
        return serde_json::to_value(Witness {
            event: "stage_no_progress".into(),
            version: 1,
            key: key.into(),
            stage: repeated.stage.name().into(),
            revision: repeated.revision,
            first_record_index: first_index,
            record_index: index,
            input_sha256: job.input_sha256.clone(),
            guide_sha256: repeated.guide_sha256.clone(),
            schema_sha256: repeated.schema_sha256.clone(),
            first_raw_sha256: lessons::digest(&first.raw),
            raw_sha256: lessons::digest(&repeated.raw),
            validation_error_sha256: lessons::digest(
                repeated
                    .validation_error
                    .as_deref()
                    .ok_or("No repeated native error")?,
            ),
        })
        .map(Some)
        .map_err(|error| error.to_string());
    }
    Ok(None)
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
                let question = if matches!(
                    key,
                    crate::reading_contracts::RequirementKey::ChartPlace
                        | crate::reading_contracts::RequirementKey::ChartMoment
                        | crate::reading_contracts::RequirementKey::Field(
                            crate::reading_contracts::Field::TimeOccurrence
                        )
                ) {
                    request.question.clone()
                } else {
                    case.question_for(&key)
                };
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
            if let Some(witness) = repeated_rejection(session, key, record_index)? {
                session
                    .method
                    .flow
                    .pause_job(key, horary_prompt_program::rejection_cycle::STOP.into())?;
                session.audit.push(witness);
                ("paused", Received::NoProgress)
            } else {
                ("repairing", Received::Rejected { proposal, error })
            }
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
    if audio.is_none()
        && !matches!(
            stage,
            Stage::Intake | Stage::Explanation | Stage::Conversation
        )
    {
        if let Some((data, index)) = cached_data(session, runtime, stage, &original, facts)? {
            let key = work_key(session, runtime, stage, &original, facts)?;
            session
                .method
                .flow
                .reuse(key, session.revision, &data, index)?;
            session.audit.push(json!({"event":"stage_data_reused","stage":stage,"revision":session.revision,"inputSha256":lessons::digest(&original.to_string()),"basis":"The exact current input, lesson and contract match; native checks were rerun."}));
            runtime.publish(session)?;
            return Ok(Some(data));
        }
    }
    let mut key = work_key(session, runtime, stage, &original, facts)?;
    if matches!(
        stage,
        Stage::Intake | Stage::Explanation | Stage::Conversation
    ) {
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
                input = repair_input(stage, &original, proposal, error)
            }
            Received::NoProgress => return Err(horary_prompt_program::rejection_cycle::STOP.into()),
        }
    }
}

fn repair_input(stage: Stage, original: &Value, previous: Value, error: String) -> Value {
    let instruction = match stage {
        Stage::Intake => "This extraction has not completed. Only original_input has accepted authority; previous_worksheet was rejected and none of its proposed facts were saved. Return a corrected Turn using the actual source words and this phase's output contract. Fix the specific native error. Leave genuinely absent facts missing for the conversational reader; do not invent a quote, repeat provided questions, or ask the person to correct your worksheet.",
        Stage::Conversation => "This conversational reply has not completed. Only original_input has accepted authority; previous_worksheet was rejected. Return a corrected reply and ask binding using the supplied reminders and output contract. A rejected reply is not evidence of a chart finding or a new user fact.",
        _ => "This step has not completed. Only original_input has accepted authority; previous_worksheet was rejected and none of its proposed facts were saved. Correct against the original data and the specific native error. Supply required data, or use the output contract's genuine missing-input path. Do not ask the user to repeat provided words or supply chart calculations. Earlier attempts remain in the receipts.",
    };
    json!({"original_input":original,"previous_worksheet":previous,"native_validation_error":error,
        "instruction":instruction})
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
    runtime: &impl Runtime,
    stage: Stage,
    input: &Value,
    facts: &[Fact],
) -> Result<String, String> {
    Ok(lessons::digest(&json!({"stage":stage,"revision":session.revision,"guide":lessons::digest(&runtime.guide_for(stage, session.method.brief.matter,input)?),"contract":step::response_schema_for(stage,session.method.brief.matter,input,facts),"input":input,"validationAuthority":step::validation_authority()}).to_string()))
}

fn cached_data(
    session: &Session,
    runtime: &impl Runtime,
    stage: Stage,
    input: &Value,
    facts: &[Fact],
) -> Result<Option<(CheckedData, usize)>, String> {
    let input_hash = lessons::digest(&input.to_string());
    let guide_hash =
        lessons::digest(&runtime.guide_for(stage, session.method.brief.matter, input)?);
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
        let key = work_key(session, runtime, *stage, &input, facts)?;
        if let Some((data, record_index)) = cached_data(session, runtime, *stage, &input, facts)? {
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
    // Capture each actual runtime guide before dispatch. A stale teaching
    // program must stop before any native or hosted generation is submitted.
    let guide_hashes = requests
        .iter()
        .map(|(stage, matter, input, _)| {
            runtime
                .guide_for(*stage, *matter, input)
                .map(|guide| lessons::digest(&guide))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let batch = runtime.generate_batch(&requests)?;
    if batch.len() != pending.len() {
        return Err("An independent task is missing from the batch.".into());
    }
    let mut repairs = Vec::new();
    let mut no_progress = false;
    // Receive EVERY case before repairing one. Good sibling data and rejected
    // proposals survive cancellation or failure in a later repair.
    for (((index, key, stage, input, facts), generation), guide_sha256) in
        pending.into_iter().zip(batch).zip(guide_hashes)
    {
        let contract = step::response_schema_for(stage, matter, &input, facts);
        let (checked, record) = evaluate(
            stage,
            matter,
            input.clone(),
            contract,
            generation,
            facts,
            guide_sha256,
        )?;
        match receive(session, runtime, &key, checked, record)? {
            Received::Complete(data) => values[index] = Some(*data),
            Received::AwaitingUser => {}
            Received::Rejected { proposal, error } => {
                repairs.push((index, stage, input, facts, proposal, error))
            }
            Received::NoProgress => no_progress = true,
        }
    }
    if no_progress {
        // Every paid sibling is now journaled. Halt before further generation;
        // checked siblings remain complete and other rejections unfinished.
        return Err(horary_prompt_program::rejection_cycle::STOP.into());
    }
    for (index, stage, input, facts, proposal, error) in repairs {
        values[index] = task(
            session,
            runtime,
            stage,
            repair_input(stage, &input, proposal, error),
            facts,
        )?;
    }
    ask_pending(session);
    Ok(values)
}

pub(crate) fn ask_pending(session: &mut Session) {
    // Workers request data from the clipboard. Only the reader's conversation
    // stage turns a reminder into words for the person.
    if let Some(case) = session.method.consultation.as_mut() {
        if case.requested.is_none() {
            if let Some(pending) = session.method.flow.pending.first() {
                case.requested =
                    crate::reading_contracts::need_key(&pending.request.field, case).ok();
            }
        }
    }
}

#[cfg(test)]
mod effective_guide_tests {
    use super::*;
    use std::{path::Path, sync::Mutex};

    struct TestRuntime {
        dir: tempfile::TempDir,
        program: Option<horary_prompt_program::Program>,
        prompts: Mutex<Vec<Value>>,
    }
    impl TestRuntime {
        fn new(program: Option<horary_prompt_program::Program>) -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
                program,
                prompts: Mutex::new(vec![]),
            }
        }
    }
    fn worksheet(stage: Stage) -> Value {
        match stage {
            Stage::Place => {
                json!({"mode":"select","place_id":"synthetic-place","query":"","clarification":"","basis":"A supplied synthetic candidate"})
            }
            Stage::Moment => {
                json!({"mode":"now","local_time":"","occurrence":"","clarification":"","basis":"The supplied synthetic receipt moment"})
            }
            _ => panic!("Unexpected synthetic stage"),
        }
    }
    impl Runtime for TestRuntime {
        fn guide_for(&self, stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
            crate::horary_contract::resolved_guide_for(stage, matter, input, self.program.as_ref())
                .map(|(guide, _)| guide)
        }
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            let (prompt, _) = crate::horary_contract::prompt_with_program(
                stage,
                matter,
                input,
                schema,
                self.program.as_ref(),
            )?;
            self.prompts
                .lock()
                .unwrap()
                .push(serde_json::from_str(&prompt).unwrap());
            Ok(NativeGenerationResult {
                content: worksheet(stage).to_string(),
                prompt_tokens: 0,
                generated_tokens: 0,
                elapsed_ms: 0,
                total_wall_ms: None,
                batch_size: None,
                lesson_bank_hit: None,
                lesson_prepare_ms: None,
                tokens_per_second: 0.,
                prompt_cache_hit: false,
                cached_prompt_tokens: 0,
                prefilled_prompt_tokens: 0,
                first_token_ms: None,
                cold_cache_bytes: None,
            })
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            tasks
                .iter()
                .map(|(stage, matter, input, schema)| {
                    self.generate(*stage, *matter, input, schema, None)
                })
                .collect()
        }
        fn publish(&self, _session: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn check(&self) -> Result<(), String> {
            Ok(())
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    fn input() -> Value {
        json!({"candidates":[{"id":"synthetic-place"}]})
    }

    #[test]
    fn changed_effective_teaching_invalidates_work_and_accepted_data_without_changing_input() {
        let input = input();
        let prior = TestRuntime::new(Some(crate::horary_contract::synthetic_teaching_program(
            Stage::Place,
            &input,
            "earlier selected teaching",
        )));
        let current = TestRuntime::new(Some(crate::horary_contract::synthetic_teaching_program(
            Stage::Place,
            &input,
            "later selected teaching",
        )));
        let (checked, record) = generate(
            &prior,
            Stage::Place,
            Matter::Other,
            input.clone(),
            &[],
            None,
        )
        .unwrap();
        assert!(matches!(checked.unwrap(), Checked::Data(_)));
        let mut session = Session::default();
        session.method.records.push(record);
        assert!(cached_data(&session, &prior, Stage::Place, &input, &[])
            .unwrap()
            .is_some());
        assert!(cached_data(&session, &current, Stage::Place, &input, &[])
            .unwrap()
            .is_none());
        assert_ne!(
            work_key(&session, &prior, Stage::Place, &input, &[]).unwrap(),
            work_key(&session, &current, Stage::Place, &input, &[]).unwrap()
        );
        assert_eq!(session.method.records[0].input, input);
        assert!(current.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn single_repair_and_batch_records_hash_each_actual_system_message() {
        let original = input();
        let mut program = crate::horary_contract::synthetic_teaching_program(
            Stage::Place,
            &original,
            "place lesson",
        );
        program.overrides.extend(
            crate::horary_contract::synthetic_teaching_program(
                Stage::Moment,
                &original,
                "moment lesson",
            )
            .overrides,
        );
        let runtime = TestRuntime::new(Some(program));
        let repair = repair_input(
            Stage::Place,
            &original,
            json!({"wrong":"rejected"}),
            "Synthetic native rejection".into(),
        );
        let (_, record) =
            generate(&runtime, Stage::Place, Matter::Other, repair, &[], None).unwrap();
        let requests = runtime.prompts.lock().unwrap();
        assert_eq!(
            record.guide_sha256,
            lessons::digest(requests[0][0]["content"].as_str().unwrap())
        );
        assert_eq!(requests[0][2]["role"], "assistant");
        drop(requests);
        runtime.prompts.lock().unwrap().clear();
        let mut session = Session::default();
        let values = execute_batch(
            &mut session,
            &runtime,
            &[
                (Stage::Place, original.clone(), vec![]),
                (Stage::Moment, original, vec![]),
            ],
        )
        .unwrap();
        assert!(values.iter().all(Option::is_some));
        let requests = runtime.prompts.lock().unwrap();
        assert_eq!(session.method.records.len(), 2);
        for (record, request) in session.method.records.iter().zip(requests.iter()) {
            assert_eq!(
                record.guide_sha256,
                lessons::digest(request[0]["content"].as_str().unwrap())
            );
        }
        assert_ne!(
            session.method.records[0].guide_sha256,
            session.method.records[1].guide_sha256
        );
    }

    #[test]
    fn stale_effective_teaching_stops_single_and_batch_before_generation() {
        let original = input();
        let mut program = crate::horary_contract::synthetic_teaching_program(
            Stage::Place,
            &original,
            "stale lesson",
        );
        program.overrides[0].expected_guide_sha256 = lessons::digest("Changed original teaching");
        let runtime = TestRuntime::new(Some(program));
        assert!(generate(
            &runtime,
            Stage::Place,
            Matter::Other,
            original.clone(),
            &[],
            None
        )
        .err()
        .unwrap()
        .contains("stale"));
        assert!(execute_batch(
            &mut Session::default(),
            &runtime,
            &[(Stage::Place, original, vec![])]
        )
        .err()
        .unwrap()
        .contains("stale"));
        assert!(runtime.prompts.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod repair_tests {
    use super::*;

    #[test]
    fn intake_repair_retains_the_original_phase_and_never_offers_a_foreign_action() {
        let original = json!({
            "recognition_phase":"complete_selected_program",
            "consultation":crate::reading_contracts::Consultation::default(),
            "latest_words":"Will I marry within a year?"
        });
        let rejected = json!({"recognition_phase":"classify_question","question":"I'm single"});
        let repair = repair_input(
            Stage::Intake,
            &original,
            rejected.clone(),
            "The subject kind is not allowed.".into(),
        );
        assert_eq!(step::original_input(&repair), &original);
        assert_eq!(repair["previous_worksheet"], rejected);
        assert!(!repair["instruction"]
            .as_str()
            .unwrap()
            .contains("request_input"));
        let schema = step::response_schema_for(Stage::Intake, Matter::Other, &repair, &[]);
        assert_eq!(schema["properties"]["intent"]["enum"], json!(["clarify"]));
        assert_eq!(schema["properties"]["question"], json!({"type":"null"}));
    }
}

#[cfg(test)]
mod no_progress_tests {
    use super::*;
    use std::{collections::VecDeque, path::Path, sync::Mutex};

    struct Fixture {
        directory: tempfile::TempDir,
        answers: Mutex<VecDeque<Result<String, String>>>,
        calls: Mutex<usize>,
    }
    impl Fixture {
        fn new(answers: &[&str]) -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
                answers: Mutex::new(answers.iter().map(|s| Ok((*s).into())).collect()),
                calls: Mutex::new(0),
            }
        }
    }
    impl Runtime for Fixture {
        fn generate(
            &self,
            _stage: Stage,
            _matter: Matter,
            _input: &Value,
            _schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            *self.calls.lock().unwrap() += 1;
            let content = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or("Unexpected extra generation")??;
            serde_json::from_value(json!({"content":content,"promptTokens":0,"generatedTokens":0,"elapsedMs":0,
                "tokensPerSecond":0.0,"promptCacheHit":false,"cachedPromptTokens":0,"prefilledPromptTokens":0,
                "firstTokenMs":null,"coldCacheBytes":null})).map_err(|e| e.to_string())
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            tasks
                .iter()
                .map(|(s, m, i, c)| self.generate(*s, *m, i, c, None))
                .collect()
        }
        fn publish(&self, _session: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn check(&self) -> Result<(), String> {
            Ok(())
        }
        fn directory(&self) -> &Path {
            self.directory.path()
        }
    }
    fn input() -> Value {
        json!({"candidates":[{"id":"synthetic-place"}]})
    }
    const A: &str = "{ incomplete A";
    const B: &str = "[ incomplete B";

    #[test]
    fn repeated_rejection_and_two_state_cycle_pause_without_accepting_or_extra_call() {
        for answers in [vec![A, A], vec![A, B, A]] {
            let runtime = Fixture::new(&answers);
            let mut session = Session::default();
            assert_eq!(
                execute(&mut session, &runtime, Stage::Place, input(), &[], None)
                    .err()
                    .unwrap(),
                horary_prompt_program::rejection_cycle::STOP
            );
            assert_eq!(*runtime.calls.lock().unwrap(), answers.len());
            assert_eq!(session.method.records.len(), answers.len());
            assert!(session
                .method
                .records
                .iter()
                .all(|record| record.validation_error.is_some()));
            assert!(matches!(
                session.method.flow.jobs[0].phase(),
                step::Phase::Paused { .. }
            ));
            assert!(session.method.flow.active.is_empty());
            let witness = session
                .audit
                .iter()
                .find(|event| event["event"] == "stage_no_progress")
                .unwrap();
            let proof = horary_prompt_program::rejection_cycle::verify(
                &serde_json::to_value(&session).unwrap(),
                witness,
            )
            .unwrap();
            assert_eq!(proof.first_record_index, 0);
            assert_eq!(proof.record_index, answers.len() - 1);
            assert!(session.method.result.is_none());
        }
    }
    #[test]
    fn paid_batch_siblings_are_journaled_and_completed_before_cycle_halt() {
        let prior = Fixture::new(&[A, A]);
        let mut session = Session::default();
        assert!(execute(&mut session, &prior, Stage::Place, input(), &[], None).is_err());
        let moment = r#"{"mode":"now","local_time":"","occurrence":"","clarification":"","basis":"Synthetic receipt moment"}"#;
        let runtime = Fixture::new(&[A, moment, B]);
        assert_eq!(
            execute_batch(
                &mut session,
                &runtime,
                &[
                    (Stage::Place, input(), vec![]),
                    (Stage::Moment, input(), vec![]),
                    (Stage::Intake, input(), vec![])
                ]
            )
            .err()
            .unwrap(),
            horary_prompt_program::rejection_cycle::STOP
        );
        assert_eq!(*runtime.calls.lock().unwrap(), 3);
        assert_eq!(session.method.records.len(), 5);
        assert!(session.method.records[2].validation_error.is_some());
        assert!(session.method.records[3].validation_error.is_none());
        assert!(session.method.records[4].validation_error.is_some());
        pause_after_error(
            &mut session,
            horary_prompt_program::rejection_cycle::STOP.into(),
        );
        assert!(matches!(
            session
                .method
                .flow
                .jobs
                .iter()
                .find(|job| job.stage == Stage::Place)
                .unwrap()
                .phase(),
            step::Phase::Paused { .. }
        ));
        assert!(matches!(
            session
                .method
                .flow
                .jobs
                .iter()
                .find(|job| job.stage == Stage::Moment)
                .unwrap()
                .phase(),
            step::Phase::Complete { record_index: 3 }
        ));
        assert!(matches!(
            session
                .method
                .flow
                .jobs
                .iter()
                .find(|job| job.stage == Stage::Intake)
                .unwrap()
                .phase(),
            step::Phase::Repairing { .. }
        ));
        let witness = session
            .audit
            .iter()
            .rev()
            .find(|e| e["event"] == "stage_no_progress")
            .unwrap();
        horary_prompt_program::rejection_cycle::verify(
            &serde_json::to_value(&session).unwrap(),
            witness,
        )
        .unwrap();
        pause_after_error(&mut session, "Explicit cancellation".into());
        assert!(
            matches!(session.method.flow.jobs.iter().find(|job|job.stage==Stage::Intake).unwrap().phase(),step::Phase::Paused{error} if error=="Explicit cancellation")
        );
    }
    #[test]
    fn backend_uncertainty_is_not_a_second_native_rejection() {
        let runtime = Fixture::new(&[A]);
        runtime
            .answers
            .lock()
            .unwrap()
            .push_back(Err("Uncertain transport".into()));
        let mut session = Session::default();
        assert_eq!(
            execute(&mut session, &runtime, Stage::Place, input(), &[], None)
                .err()
                .unwrap(),
            "Uncertain transport"
        );
        assert_eq!(session.method.records.len(), 1);
        assert!(!session
            .audit
            .iter()
            .any(|event| event["event"] == "stage_no_progress"));
    }
    #[test]
    fn new_work_binding_can_repair_and_complete_after_an_old_cycle() {
        let runtime = Fixture::new(&[A, A]);
        let mut session = Session::default();
        assert!(execute(&mut session, &runtime, Stage::Place, input(), &[], None).is_err());
        let valid = r#"{"mode":"select","place_id":"synthetic-place","query":"","clarification":"","basis":"A supplied synthetic candidate"}"#;
        let runtime = Fixture::new(&[A, valid]);
        let mut changed = input();
        changed["new_context"] = json!("Genuine changed task input");
        assert!(
            execute(&mut session, &runtime, Stage::Place, changed, &[], None)
                .unwrap()
                .is_some()
        );
        assert_eq!(*runtime.calls.lock().unwrap(), 2);
        assert!(session
            .method
            .records
            .last()
            .unwrap()
            .validation_error
            .is_none());
    }

    #[test]
    fn matching_stop_string_or_stale_witness_cannot_suppress_global_pause() {
        for stale in [false, true] {
            let mut session = Session::default();
            if stale {
                let runtime = Fixture::new(&[A, A]);
                assert!(execute(&mut session, &runtime, Stage::Place, input(), &[], None).is_err());
            }
            session
                .method
                .flow
                .begin(
                    "unrelated".into(),
                    Stage::Moment,
                    session.revision,
                    "hash".into(),
                )
                .unwrap();
            session.audit.push(json!({"event":"stage_transition","stage":"moment","key":"unrelated","to":"running"}));
            pause_after_error(
                &mut session,
                horary_prompt_program::rejection_cycle::STOP.into(),
            );
            assert!(matches!(
                session
                    .method
                    .flow
                    .jobs
                    .iter()
                    .find(|job| job.key == "unrelated")
                    .unwrap()
                    .phase(),
                step::Phase::Paused { .. }
            ));
        }
    }
}
