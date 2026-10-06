//! Authored fault injection, not model-quality or performance evidence.
use super::*;
use std::{collections::VecDeque, sync::Mutex};

struct Script {
    dir: tempfile::TempDir,
    outputs: Mutex<VecDeque<(Stage, String)>>,
    calls: Mutex<Vec<(Stage, Value)>>,
    cancel_after: Option<usize>,
}
impl Script {
    fn new(outputs: Vec<(Stage, Value)>) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            outputs: Mutex::new(
                outputs
                    .into_iter()
                    .map(|(stage, value)| (stage, value.to_string()))
                    .collect(),
            ),
            calls: Mutex::new(Vec::new()),
            cancel_after: None,
        }
    }
}
impl Runtime for Script {
    fn generate(
        &self,
        stage: Stage,
        _matter: Matter,
        input: &Value,
        _contract: &Value,
        _audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        self.calls.lock().unwrap().push((stage, input.clone()));
        let (expected, content) = self
            .outputs
            .lock()
            .unwrap()
            .pop_front()
            .expect("Unexpected extra model call");
        assert_eq!(stage, expected);
        Ok(NativeGenerationResult {
            content,
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
            .map(|(stage, matter, input, contract)| {
                self.generate(*stage, *matter, input, contract, None)
            })
            .collect()
    }
    fn publish(&self, session: &mut Session) -> Result<(), String> {
        crate::reading_store::write(&self.dir.path().join("reading.json"), session, false)
    }
    fn check(&self) -> Result<(), String> {
        if self
            .cancel_after
            .is_some_and(|limit| self.calls.lock().unwrap().len() >= limit)
        {
            Err("Cancelled during repair".into())
        } else {
            Ok(())
        }
    }
    fn directory(&self) -> &Path {
        self.dir.path()
    }
}

fn brief(intent: &str) -> Value {
    json!({"intent":intent,"question":"How many books will Bob sell at the fair?","matter":"other","question_kind":"quantity","context":"Bob's book sales at a fair.","people":[{"id":"bob","label":"Bob","relationship":"unknown","source_quote":""}],"subject":{"name":"Books","kind":"movable","owner_id":"","source_quote":"How many books will Bob sell at the fair?"},"event_place":"Bozeman, Montana","event_time":"","place_request":"","time_request":"","horizon":"","clarification":"","focus":"moment","heard":"","restore_revision":null})
}
fn roles() -> Value {
    json!({"selections":[{"id":"querent.self","reason":"The first house represents the person asking."},{"id":"bob.self","reason":"Bob is the explicitly stated husband, seventh house."},{"id":"subject.primary","reason":"These books are his possessions; use his turned second."}],"summary":"Authored role assignment for controller testing.","unknowns":[]})
}

fn know_bob(session: &mut Session) {
    session.method.brief.people[0].relationship = "partner".into();
    session.method.brief.people[0].source_quote = "Bob is my husband".into();
    session.method.brief.subject.owner_id = "bob".into();
}
fn role_options(session: &Session) -> crate::horary_role_options::Options {
    crate::horary_role_options::build(
        session.method.brief.matter,
        &session.method.brief.people,
        &session.method.brief.subject,
    )
}
fn worksheet(stage: Stage, facts: &[Fact]) -> Value {
    let checks = stage.checks().iter().map(|key|((*key).into(),json!({"state":"unestablished","evidence":[],"finding":"Authored fault-injection finding, not an astrological judgment."}))).collect::<serde_json::Map<_,_>>();
    let mut value = json!({"checks":checks,"summary":"Authored fixture summary, not a model answer.","unknowns":[]});
    if stage == Stage::Contacts {
        value["basis"] = json!("no_candidate_covered");
        value["candidate_ids"] = json!([]);
        value["candidate_signs"] = json!([]);
    }
    if stage == Stage::Judgment {
        value["verdict"] = json!("unresolved");
        value["answer"] =
            json!("Authored test answer showing that an accepted judgment reaches the document.");
        value["evidence"] = json!([]);
        value["checks"]["scope_of_answer"]["evidence"] = json!([facts
            .iter()
            .find(|fact| fact.kind == "boundary")
            .unwrap()
            .id]);
    }
    value
}
fn session() -> Session {
    let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
    let mut session = Session {
        chart: Some(chart),
        revision: 1,
        question: "How many books will Bob sell at the fair?".into(),
        place: Some(LocationCandidate {
            id: "device".into(),
            label: "Here".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        }),
        ..Default::default()
    };
    session.method.brief = serde_json::from_value(brief("read")).unwrap();
    session
}

#[test]
fn every_role_rejection_returns_to_the_same_step_until_data_is_delivered() {
    let mut session = session();
    know_bob(&mut session);
    let facts = reading_method::facts(session.chart.as_ref());
    let input = json!({"brief":reading_brief(&session.method.brief),"house_rulers_and_positions":facts,"native_role_options":role_options(&session)});
    let mut invalid = roles();
    invalid["selections"].as_array_mut().unwrap().remove(1);
    let script = Script::new(
        (0..5)
            .map(|_| (Stage::Significators, invalid.clone()))
            .chain([(Stage::Significators, roles())])
            .collect(),
    );
    let data = task(
        &mut session,
        &script,
        Stage::Significators,
        input.clone(),
        &facts,
    )
    .unwrap()
    .unwrap();
    assert_eq!(data.roles().len(), 3);
    assert_eq!(session.method.records.len(), 6);
    assert!(session.method.records[..5].iter().all(|record| record
        .validation_error
        .as_ref()
        .unwrap()
        .contains("every required")));
    let job = session.method.flow.jobs.last().unwrap();
    assert_eq!(job.attempts, 6);
    assert!(matches!(job.phase(), step::Phase::Complete { .. }));
    for (_, repair) in script.calls.lock().unwrap().iter().skip(1) {
        assert_eq!(repair["original_input"], input);
        assert!(repair["original_input"].get("original_input").is_none());
    }
}

#[test]
fn asking_the_user_does_not_complete_roles_or_dispatch_testimony() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![(
        Stage::Significators,
        json!({"request_input":{"field":"subject_relationship","question":"Who is Bob to you?","reason":"His relationship determines his house and whose stock is being judged."}}),
    )]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387800000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(script.calls.lock().unwrap().len(), 1);
    assert!(session.sections.is_empty());
    assert_eq!(session.method.flow.pending[0].stage, Stage::Significators);
    assert!(matches!(
        session.method.flow.jobs.last().unwrap().phase(),
        step::Phase::AwaitingUser
    ));
    assert_eq!(session.messages.last().unwrap().text, "Who is Bob to you?");
}

#[test]
fn explanations_receive_the_follow_up_and_actual_native_moment_before_any_interpretation() {
    let mut session = session();
    let words =
        "How did you know what time to cast the chart? Don't you need to know when the fair is?";
    session.messages.push(Message {
        role: "user".into(),
        text: words.into(),
    });
    let mut explanation = worksheet(Stage::Explanation, &[]);
    explanation["checks"]["evidence_used"] = json!({"state":"supported","evidence":["chart.moment"],"finding":"Use the chart's actual recorded question instant."});
    let script = Script::new(vec![
        (Stage::Intake, brief("explain")),
        (Stage::Explanation, explanation),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387800000.,
        None,
        &previous,
    )
    .unwrap();
    let calls = script.calls.lock().unwrap();
    let input = &calls[1].1;
    assert_eq!(input["follow_up_words"], words);
    assert_eq!(input["chart_context"]["timestamp_ms"], 1789387200000.);
    assert_eq!(input["chart_context"]["timezone"], "America/New_York");
    assert_eq!(input["reading_complete"], false);
    assert!(input["prior_worksheet"].is_null());
    assert!(input["facts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|fact| fact["id"] == "chart.moment"));
}

#[test]
fn missing_internal_explanation_data_is_not_sent_to_a_model_or_requested_from_a_user() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Why did you say that about his sales?".into(),
    });
    let mut intake = brief("explain");
    intake["focus"] = json!("judgment");
    let script = Script::new(vec![(Stage::Intake, intake)]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387800000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(script.calls.lock().unwrap().len(), 1);
    assert!(session
        .messages
        .last()
        .unwrap()
        .text
        .contains("haven't completed"));
    assert!(!session
        .messages
        .last()
        .unwrap()
        .text
        .contains("provide the chart"));
}

#[test]
fn all_batch_receipts_survive_cancellation_while_one_case_is_being_repaired() {
    let mut session = session();
    know_bob(&mut session);
    let facts = reading_method::facts(session.chart.as_ref());
    let assignments =
        crate::horary_role_options::resolve(&role_options(&session), &roles(), &facts).unwrap();
    let task_input =
        json!({"brief":reading_brief(&session.method.brief),"roles":assignments,"facts":facts});
    let tasks: Vec<_> = [Stage::Condition, Stage::Reception, Stage::Contacts]
        .into_iter()
        .map(|stage| (stage, task_input.clone(), facts.clone()))
        .collect();
    let mut script = Script::new(vec![
        (Stage::Condition, json!({"bad":"missing checks"})),
        (Stage::Reception, worksheet(Stage::Reception, &facts)),
        (Stage::Contacts, worksheet(Stage::Contacts, &facts)),
    ]);
    script.cancel_after = Some(3);
    assert!(execute_batch(&mut session, &script, &tasks).is_err());
    session.method.flow.pause("Cancelled".into());
    assert_eq!(session.method.records.len(), 3);
    assert!(session.method.records[0].validation_error.is_some());
    assert!(session.method.records[1..]
        .iter()
        .all(|record| record.validation_error.is_none()));
    assert!(matches!(
        session
            .method
            .flow
            .jobs
            .iter()
            .find(|job| job.stage == Stage::Condition)
            .unwrap()
            .phase(),
        step::Phase::Paused { .. }
    ));
    assert!(session
        .method
        .flow
        .jobs
        .iter()
        .filter(|job| matches!(job.stage, Stage::Reception | Stage::Contacts))
        .all(|job| matches!(job.phase(), step::Phase::Complete { .. })));
}

#[test]
fn a_reply_resumes_the_waiting_stage_and_an_explicit_resume_reuses_completed_data() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![(
        Stage::Significators,
        json!({"request_input":{"field":"subject_relationship","question":"Who is Bob to you?","reason":"Needed to select a house."}}),
    )]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387800000.,
        None,
        &previous,
    )
    .unwrap();
    let mut reply = brief("clarify");
    reply["people"][0]["relationship"] = json!("partner");
    reply["people"][0]["source_quote"] = json!("Bob is my husband");
    reply["subject"]["owner_id"] = json!("bob");
    reply["context"] = json!("Bob is the querent's husband; they are his books.");
    let facts = reading_method::facts(session.chart.as_ref());
    let script = Script::new(vec![
        (Stage::Intake, reply),
        (Stage::Significators, roles()),
        (Stage::Condition, worksheet(Stage::Condition, &facts)),
        (Stage::Reception, worksheet(Stage::Reception, &facts)),
        (Stage::Contacts, worksheet(Stage::Contacts, &facts)),
        (Stage::Judgment, worksheet(Stage::Judgment, &facts)),
    ]);
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "Bob is my husband; they're his books.".into(),
    });
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789388400000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.method.flow.pending.is_empty());
    assert_eq!(session.sections.len(), 5);
    assert_eq!(
        session.chart.as_ref().unwrap()["timestampMs"],
        1789387200000.
    );
    let calls = script.calls.lock().unwrap();
    assert_eq!(
        calls[1].1["stage_user_replies"][0]["words"],
        "Bob is my husband; they're his books."
    );
    drop(calls);
    let empty = Script::new(vec![]);
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "Cast the chart.".into(),
    });
    run(
        &mut session,
        &empty,
        &GeocodeState::default(),
        1789389000000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(empty.calls.lock().unwrap().is_empty());
    assert_eq!(session.sections.len(), 5);
}
