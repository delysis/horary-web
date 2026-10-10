//! Authored fault injection, not model-quality or performance evidence.
use super::*;
use std::{collections::VecDeque, sync::Mutex};

struct Script {
    dir: tempfile::TempDir,
    outputs: Mutex<VecDeque<(Stage, String)>>,
    calls: Mutex<Vec<(Stage, Value)>>,
    focused: Mutex<Option<Value>>,
    split_intake: bool,
    cancel_after: Option<usize>,
    locations: Mutex<VecDeque<Result<Option<LocationCandidate>, String>>>,
    location_calls: std::sync::atomic::AtomicUsize,
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
            focused: Mutex::new(None),
            split_intake: true,
            cancel_after: None,
            locations: Mutex::new(VecDeque::new()),
            location_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn exact(outputs: Vec<(Stage, Value)>) -> Self {
        Self {
            split_intake: false,
            ..Self::new(outputs)
        }
    }
}
impl Runtime for Script {
    fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
        self.location_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.locations
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(None))
    }
    fn generate(
        &self,
        stage: Stage,
        _matter: Matter,
        input: &Value,
        _contract: &Value,
        _audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        self.calls.lock().unwrap().push((stage, input.clone()));
        let mut outputs = self.outputs.lock().unwrap();
        let (expected, mut content) = if self.split_intake
            && stage == Stage::Intake
            && crate::horary_step::original_input(input)["recognition_phase"]
                == "complete_selected_program"
        {
            // Authored scripts separate their initial classification from the
            // same-turn facts. This is fault injection, never model evidence.
            let mut facts = self
                .focused
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| turn(crate::reading_contracts::Intent::Clarify));
            facts["intent"] = json!("clarify");
            facts["question"] = Value::Null;
            facts["frame"] = Value::Null;
            facts["heard"] = json!("");
            (Stage::Intake, facts.to_string())
        } else if stage == Stage::Conversation
            && outputs
                .front()
                .is_none_or(|(s, _)| *s != Stage::Conversation)
        {
            let reminder = input["reminders"].as_array().and_then(|r| r.first());
            (Stage::Conversation, json!({"reply":reminder.map_or("Authored conversational fixture.", |n|
                if n["state"]=="unavailable" {"Authored fixture: detail is still unknown."}
                else { n["example_question"].as_str().unwrap_or("Authored conversational fixture.") }),
                "ask":reminder.map_or("",|n|n["id"].as_str().unwrap_or(""))}).to_string())
        } else {
            outputs.pop_front().expect("Unexpected extra model call")
        };
        assert_eq!(stage, expected);
        if self.split_intake && stage == Stage::Intake {
            let mut proposal: Value = serde_json::from_str(&content).unwrap();
            if crate::horary_step::original_input(input)["recognition_phase"] == "classify_question"
                || proposal["intent"] == "new_question"
            {
                *self.focused.lock().unwrap() = Some(proposal.clone());
                proposal["subject"] = Value::Null;
                proposal["people"] = json!([]);
                proposal["updates"] = json!([]);
                content = proposal.to_string();
            }
        }
        if stage == Stage::Condition {
            let mut value: Value = serde_json::from_str(&content).unwrap();
            if value["summary"] == "Authored fixture summary, not a model answer." {
                // This generic successful stub supplies native coverage from
                // the actual task. Explicit fault worksheets remain untouched.
                // It tests controller recovery, never model interpretation.
                let original = crate::horary_step::original_input(input);
                let facts: Vec<Fact> = serde_json::from_value(original["facts"].clone()).unwrap();
                let roles = original
                    .get("roles")
                    .or_else(|| original["question"].get("roles"));
                for (facet, key) in [
                    (
                        horary_ai_core::book_method::ConditionFacet::Essential,
                        "own_dignity",
                    ),
                    (
                        horary_ai_core::book_method::ConditionFacet::HouseCapacity,
                        "ability_to_act",
                    ),
                ] {
                    let ids: Vec<_> = facts
                        .iter()
                        .filter(|fact| {
                            fact.condition_facet == Some(facet)
                                && roles.and_then(Value::as_array).is_some_and(|roles| {
                                    roles.iter().any(|role| {
                                        fact.planets.iter().any(|planet| role["planet"] == *planet)
                                    })
                                })
                        })
                        .map(|fact| fact.id.clone())
                        .collect();
                    value["checks"][key]["evidence"] = json!(ids);
                }
                content = value.to_string();
            }
        }
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
        if self.cancel_after.is_some_and(|limit| {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(s, _)| *s != Stage::Conversation)
                .count()
                >= limit
        }) {
            Err("Cancelled during repair".into())
        } else {
            Ok(())
        }
    }
    fn directory(&self) -> &Path {
        self.dir.path()
    }
}

#[test]
fn omitted_job_condition_returns_to_repair_before_the_stage_can_complete() {
    let mut session = session();
    let chart = session.chart.as_ref().unwrap();
    let facts = reading_method::facts(Some(chart));
    let mut incomplete = worksheet(Stage::Condition, &facts);
    incomplete["summary"] = json!("Authored applicant-only failure, not model evidence.");
    for (facet, key) in [
        (
            horary_ai_core::book_method::ConditionFacet::Essential,
            "own_dignity",
        ),
        (
            horary_ai_core::book_method::ConditionFacet::HouseCapacity,
            "ability_to_act",
        ),
    ] {
        incomplete["checks"][key]["evidence"] = json!([facts
            .iter()
            .find(|fact| fact.condition_facet == Some(facet) && fact.planets == ["Jupiter"])
            .unwrap()
            .id]);
    }
    let input = json!({"roles":[
        {"label":"Applicant","house":1,"planet":"Jupiter","reason":"Authored worker role"},
        {"label":"Job","house":10,"planet":"Mercury","reason":"Authored external job role"}
    ],"facts":facts});
    let script = Script::exact(vec![
        (Stage::Condition, incomplete.clone()),
        (Stage::Condition, worksheet(Stage::Condition, &facts)),
    ]);
    let checked = task(
        &mut session,
        &script,
        Stage::Condition,
        input.clone(),
        &facts,
    )
    .unwrap()
    .expect("Repaired coverage must complete");
    assert_eq!(script.calls.lock().unwrap().len(), 2);
    assert_eq!(session.method.records[0].worksheet, incomplete);
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("Condition review omits Mercury from own_dignity"));
    assert!(session.method.records[1].validation_error.is_none());
    assert_eq!(
        crate::horary_step::original_input(&script.calls.lock().unwrap()[1].1),
        &input
    );
    assert!(matches!(
        session.method.flow.jobs.last().unwrap().phase(),
        step::Phase::Complete { .. }
    ));
    assert_eq!(checked.worksheet(), &session.method.records[1].worksheet);
    assert!(task(&mut session, &script, Stage::Condition, input, &facts)
        .unwrap()
        .is_some());
    assert_eq!(
        script.calls.lock().unwrap().len(),
        2,
        "Completed repaired data must be reused"
    );
}

fn brief(intent: &str) -> Value {
    json!({"intent":intent,"question":"How many books will Bob sell at the fair?","matter":"other","question_kind":"quantity","context":"Bob's book sales at a fair.","people":[{"id":"bob","label":"Bob","relationship":"unknown","source_quote":""}],"subject":{"name":"Books","kind":"movable","owner_id":"","source_quote":"How many books will Bob sell at the fair?"},"event_place":"Bozeman, Montana","event_time":"","place_request":"","time_request":"","horizon":"","clarification":"","focus":"moment","heard":"","restore_revision":null})
}
fn turn(intent: crate::reading_contracts::Intent) -> Value {
    serde_json::to_value(crate::reading_contracts::control(intent)).unwrap()
}
fn roles() -> Value {
    json!({"selections":[{"id":"querent.self","reason":"The first house represents the person asking."},{"id":"bob.self","reason":"Bob is the explicitly stated husband, seventh house."},{"id":"deal.counterparty","reason":"The unspecified buyer is seventh from the seller."}],"summary":"Authored role assignment for transaction completion; no unused goods-condition role.","unknowns":[]})
}

fn know_bob(session: &mut Session) {
    session.method.brief.people[0].relationship = "partner".into();
    session.method.brief.people[0].source_quote = "Bob is my husband".into();
    session.method.brief.subject.owner_id = "bob".into();
    if let Some(case) = session.method.consultation.as_mut() {
        case.people
            .insert("bob".into(), session.method.brief.people[0].clone());
        case.subject = crate::reading_contracts::Slot::Resolved {
            observation: crate::reading_contracts::Observation {
                value: session.method.brief.subject.clone(),
                evidence: crate::reading_contracts::Evidence::Migration {
                    detail: "Authored Bob relationship fixture".into(),
                },
            },
        };
    }
}
fn role_options(session: &Session) -> crate::horary_role_options::Options {
    crate::horary_role_options::build_for(
        session.method.consultation.as_ref().unwrap(),
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
    // This regression exercises a supported deal question; a numerical sales
    // count has its own explicit unsupported-facet regression below.
    session.question = "Will Bob sell his books at the fair?".into();
    session.method.brief.question = session.question.clone();
    session.method.brief.question_kind = "event".into();
    let mut case = crate::reading_contracts::migrate(&session.method.brief);
    use crate::reading_contracts::{Evidence, Facet, Field, Frame, Method, Observation, Slot};
    let observation = |value| Slot::Resolved {
        observation: Observation {
            value,
            evidence: Evidence::Migration {
                detail: "Authored controller fixture, not model output".into(),
            },
        },
    };
    case.frame = Slot::Resolved {
        observation: Observation {
            value: Frame {
                method: Method::MovableDeal,
                facet: Facet::Event,
            },
            evidence: Evidence::Migration {
                detail: "Authored supported deal frame".into(),
            },
        },
    };
    case.facts
        .insert(Field::DealCapacity, observation("sell".into()));
    case.facts.insert(Field::Seller, observation("bob".into()));
    session.method.consultation = Some(case);
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
        .contains("Missing required role: select exactly one")));
    let job = session.method.flow.jobs.last().unwrap();
    assert_eq!(job.attempts, 6);
    assert!(matches!(job.phase(), step::Phase::Complete { .. }));
    for (_, repair) in script.calls.lock().unwrap().iter().skip(1) {
        assert_eq!(repair["original_input"], input);
        assert!(repair["original_input"].get("original_input").is_none());
        let messages: Value = serde_json::from_str(
            &crate::horary_contract::prompt(
                Stage::Significators,
                Matter::Other,
                repair,
                &step::response_schema_for(Stage::Significators, Matter::Other, repair, &facts),
            )
            .unwrap(),
        )
        .unwrap();
        let accepted: Value =
            serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(accepted["input"], input);
        assert!(accepted["input"].get("previous_worksheet").is_none());
        assert_eq!(messages[2]["role"], "assistant");
        let rejected: Value =
            serde_json::from_str(messages[2]["content"].as_str().unwrap()).unwrap();
        assert_eq!(rejected, repair["previous_worksheet"]);
        assert_eq!(messages[3]["role"], "user");
        let feedback: Value =
            serde_json::from_str(messages[3]["content"].as_str().unwrap()).unwrap();
        assert_eq!(
            feedback["native_validation_error"],
            repair["native_validation_error"]
        );
    }
}

#[test]
fn a_scope_only_repair_keeps_its_proposed_frame_then_runs_the_new_extractor() {
    use crate::reading_contracts::{control, Facet, Frame, Intent, Method};
    let words = "My sister Rhea applied for the railway signal-engineer vacancy. They are still choosing applicants and have not offered her a post. Will she get that job?";
    let noted_moment = 1789387200000.;
    let later_receipt = noted_moment + 60_000.;
    let mut classification = control(Intent::Read);
    classification.question = Some(words.into());
    classification.frame = Some(Frame {
        method: Method::JobOffer,
        facet: Facet::Event,
    });
    let mut premature = control(Intent::Clarify);
    premature.frame = Some(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    premature.people.push(crate::horary_role_options::Person {
        id: "rhea".into(),
        label: "Rhea".into(),
        relationship: "sibling".into(),
        source_quote: "My sister Rhea".into(),
    });
    premature.subject = Some(crate::horary_role_options::Subject {
        name: "railway signal-engineer vacancy".into(),
        kind: "job".into(),
        owner_id: "rhea".into(),
        source_quote: "railway signal-engineer vacancy".into(),
    });
    let mut frame_only = control(Intent::Clarify);
    frame_only.frame = premature.frame.clone();
    let mut extraction = premature.clone();
    extraction.frame = None;
    let script = Script::exact(vec![
        (Stage::Intake, serde_json::to_value(classification).unwrap()),
        (Stage::Intake, serde_json::to_value(&premature).unwrap()),
        (Stage::Intake, serde_json::to_value(&frame_only).unwrap()),
        (Stage::Intake, serde_json::to_value(&extraction).unwrap()),
    ]);
    let mut session = Session {
        candidate_moment_ms: Some(noted_moment),
        candidates: vec![LocationCandidate {
            id: "device-location".into(),
            label: "Near Albany".into(),
            name: "Albany".into(),
            country: "US".into(),
            latitude: 42.6526,
            longitude: -73.7562,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        }],
        ..Default::default()
    };
    session.messages.push(Message {
        role: "user".into(),
        text: words.into(),
    });
    let previous = session.clone();
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        later_receipt,
        None,
        &previous,
    )
    .unwrap();

    let calls = script.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        4,
        "Four intake calls; input-only execution stops at ReadyReading"
    );
    assert!(calls[..4].iter().all(|(stage, _)| *stage == Stage::Intake));
    assert!(session
        .audit
        .iter()
        .any(|e| e["event"] == "input_evaluation_boundary" && e["boundary"] == "ready_reading"));
    let repair = &calls[2].1;
    assert_eq!(
        repair["previous_worksheet"],
        serde_json::to_value(&premature).unwrap()
    );
    let error = repair["native_validation_error"].as_str().unwrap();
    assert!(error.contains("frame-only refinement"));
    assert!(error.contains("You proposed {\"method\":\"new_job\",\"facet\":\"event\"}"));
    assert!(error.contains("premature and remain unsaved"));
    assert!(error.contains("still an unverified hypothesis"));
    let original = crate::horary_step::original_input(repair);
    assert_eq!(
        original["consultation"]["frame"]["observation"]["value"]["method"],
        "job_offer"
    );
    assert_eq!(original["consultation"]["people"], json!({}));
    assert_eq!(original["consultation"]["subject"]["state"], "missing");

    let newly_selected = &calls[3].1;
    assert!(newly_selected.get("previous_worksheet").is_none());
    assert_eq!(
        newly_selected["recognition_phase"],
        "complete_selected_program"
    );
    assert_eq!(
        newly_selected["consultation"]["frame"]["observation"]["value"]["method"],
        "new_job"
    );
    assert_eq!(newly_selected["consultation"]["people"], json!({}));
    assert_eq!(
        newly_selected["consultation"]["subject"]["state"],
        "missing"
    );
    assert_eq!(newly_selected["canonical_question"], words);
    let new_guide =
        crate::horary_contract::guide_for(Stage::Intake, Matter::Other, newly_selected).unwrap();
    let old_guide =
        crate::horary_contract::guide_for(Stage::Intake, Matter::Other, original).unwrap();
    assert_ne!(
        new_guide, old_guide,
        "The reroute must dispatch the newly selected teaching"
    );
    assert_eq!(
        session.method.records[3].guide_sha256,
        crate::horary_lessons::digest(&new_guide)
    );
    assert!(session.method.records[1].validation_error.is_some());
    assert!(session.method.records[2..]
        .iter()
        .all(|record| record.validation_error.is_none()));
    assert_eq!(
        session.method.records[1].worksheet,
        serde_json::to_value(&premature).unwrap()
    );
    assert_eq!(
        session.method.records[1].raw,
        serde_json::to_value(&premature).unwrap().to_string()
    );

    let case = session.method.consultation.as_ref().unwrap();
    assert_eq!(case.method(), Some(Method::NewJob));
    assert_eq!(case.question.resolved().map(String::as_str), Some(words));
    assert_eq!(case.people["rhea"].relationship, "sibling");
    assert_eq!(case.subject.resolved().unwrap().owner_id, "rhea");
    assert_eq!(case.changes.len(), 3);
    assert!(case.changes[..2]
        .iter()
        .all(|change| { change.proposal.people.is_empty() && change.proposal.subject.is_none() }));
    assert_eq!(
        serde_json::to_value(&case.changes[2].proposal).unwrap(),
        serde_json::to_value(&extraction).unwrap()
    );
    assert_eq!(session.question, words);
    assert_eq!(session.candidate_moment_ms, Some(noted_moment));
    assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], noted_moment);
    assert_eq!(
        session.messages.iter().filter(|m| m.role == "user").count(),
        1
    );
    assert!(session
        .audit
        .iter()
        .any(|event| event["event"] == "recognition_program_refined"
            && event["from"] == "job_offer"
            && event["to"] == "new_job"));
    assert!(session
        .audit
        .iter()
        .any(|event| event["event"] == "contract_handoff"
            && event["binding"]["frame"]["method"] == "new_job"));
    assert!(script.outputs.lock().unwrap().is_empty());
}

#[test]
fn asking_the_user_does_not_complete_roles_or_dispatch_testimony() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![]);
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
    assert!(script
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(s, _)| *s == Stage::Conversation));
    assert!(session.sections.is_empty());
    assert!(session
        .method
        .flow
        .jobs
        .iter()
        .all(|j| j.stage == Stage::Conversation));
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(crate::reading_contracts::RequirementKey::PersonRelationship("bob".into()))
    );
    assert_eq!(
        session.messages.last().unwrap().text,
        "Who is Bob to you in this question?"
    );
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
        (Stage::Intake, {
            let mut t = turn(crate::reading_contracts::Intent::Explain);
            t["focus"] = json!("moment");
            t
        }),
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
    let mut intake = turn(crate::reading_contracts::Intent::Explain);
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
    assert_eq!(
        script
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| *s != Stage::Conversation)
            .count(),
        1
    );
    assert!(session
        .audit
        .iter()
        .any(|r| r["event"] == "explanation_prerequisite"));
    assert!(script
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|(s, _)| *s == Stage::Conversation));
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
    let script = Script::new(vec![]);
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
    let mut reply = turn(crate::reading_contracts::Intent::Clarify);
    reply["people"] = json!([{"id":"bob","label":"Bob","relationship":"partner","source_quote":"Bob is my husband"}]);
    reply["subject"] = json!({"name":"Books","kind":"movable","owner_id":"bob","source_quote":"they're his books"});
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
    assert_eq!(calls[1].1["reading_request"]["subject"]["owner_id"], "bob");
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
    assert!(empty
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(s, _)| *s == Stage::Conversation));
    assert_eq!(session.sections.len(), 5);
}

#[test]
fn asking_the_device_really_retries_acquisition_and_uses_its_coordinates() {
    let mut session = session();
    know_bob(&mut session);
    let expected = session.place.take().unwrap();
    session.chart = None;
    session.method.device_attempted = true;
    session.messages.push(Message {
        role: "user".into(),
        text: "Ask this device where we are".into(),
    });
    let mut script = Script::new(vec![(
        Stage::Intake,
        turn(crate::reading_contracts::Intent::UseDevice),
    )]);
    script
        .locations
        .lock()
        .unwrap()
        .push_back(Ok(Some(expected.clone())));
    script.cancel_after = Some(1);
    let previous = session.clone();
    assert!(run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous
    )
    .is_err());
    assert_eq!(
        script
            .location_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(session.place.as_ref().unwrap().latitude, expected.latitude);
    assert_eq!(
        session.chart.as_ref().unwrap()["timestampMs"],
        1789387200000.
    );
    assert!(session
        .audit
        .iter()
        .any(|r| r["event"] == "contract_device_acquisition" && r["retry"] == true));
}

#[test]
fn explicit_reader_place_survives_an_empty_neural_patch_in_text_and_audio() {
    use crate::reading_contracts::{control, Field, Intent};
    let words = "I'm asking from Woodbridge, Virginia, United States.";
    for spoken in [false, true] {
        let mut session = session();
        session.chart = None;
        session.place = None;
        session.messages.push(Message {
            role: "user".into(),
            text: if spoken { "Spoken question" } else { words }.into(),
        });
        let mut patch = control(Intent::Clarify);
        if spoken {
            patch.heard = words.into();
        }
        let script = Script::new(vec![(Stage::Intake, serde_json::to_value(patch).unwrap())]);
        let previous = session.clone();
        run(
            &mut session,
            &script,
            &GeocodeState::default(),
            1789387200000.,
            spoken.then_some(&b"authored audio fixture"[..]),
            &previous,
        )
        .unwrap();
        assert_eq!(
            session
                .method
                .consultation
                .as_ref()
                .unwrap()
                .text(Field::ReaderPlace),
            Some("Woodbridge, Virginia, United States")
        );
        assert_eq!(session.method.brief.event_place, "Bozeman, MT");
        assert!(session.audit.iter().any(|receipt| receipt["event"]
            == "native_event_place_resolution"
            && receipt["before"]["observation"]["value"] == "Bozeman, Montana"
            && receipt["chart_anchor_changed"] == false));
        assert!(
            session.chart.is_none(),
            "Bob's still-unknown relationship must block roles"
        );
        assert!(session
            .audit
            .iter()
            .any(|receipt| receipt["rule"] == "explicit_reader_place_statement"));
        assert_eq!(
            script
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(s, _)| *s != Stage::Conversation)
                .count(),
            1
        );
    }
}

#[test]
fn an_explicit_count_cannot_be_accepted_as_a_yes_or_no_sale() {
    use crate::reading_contracts::{control, Facet, Frame, Intent, Method};
    let words = "How many books will Bob sell at the fair?";
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: words.into(),
    });
    let mut proposal = control(Intent::Read);
    proposal.question = Some(words.into());
    proposal.frame = Some(Frame {
        method: Method::MovableDeal,
        facet: Facet::Event,
    });
    let mut repair = proposal.clone();
    repair.frame.as_mut().unwrap().facet = Facet::Quantity;
    let script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(proposal).unwrap()),
        (Stage::Intake, serde_json::to_value(repair).unwrap()),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("HOW MANY"));
    assert_eq!(
        session
            .method
            .consultation
            .as_ref()
            .unwrap()
            .frame
            .resolved()
            .unwrap()
            .facet,
        Facet::Quantity
    );
    assert!(session.chart.is_none());
    assert!(matches!(
        session.method.result,
        Some(crate::reading_contracts::ReadingResult::Limited { .. })
    ));
    assert_eq!(
        script
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| *s != Stage::Conversation)
            .count(),
        3
    );
}

#[test]
fn a_historical_question_uses_user_civil_time_not_device_now_or_event_time() {
    use crate::reading_contracts::{Evidence, Field, Observation, Slot};
    let mut session = session();
    know_bob(&mut session);
    let case = session.method.consultation.as_mut().unwrap();
    case.facts.insert(
        Field::QuestionTime,
        Slot::Resolved {
            observation: Observation {
                value: "2026-01-14 at 14:30".into(),
                evidence: Evidence::User {
                    turn: 1,
                    quote: "use the question I understood on January 14 at 14:30".into(),
                },
            },
        },
    );
    case.facts.insert(
        Field::EventTime,
        Slot::Resolved {
            observation: Observation {
                value: "tomorrow at three".into(),
                evidence: Evidence::User {
                    turn: 1,
                    quote: "the fair is tomorrow at three".into(),
                },
            },
        },
    );
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let mut script = Script::new(vec![(
        Stage::Moment,
        json!({"mode":"explicit","local_time":"2026-01-14T14:30","occurrence":"","clarification":"","basis":"The user's explicitly earlier understanding, not the fair start."}),
    )]);
    script.cancel_after = Some(1);
    let previous = session.clone();
    assert!(run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous
    )
    .is_err());
    let expected =
        horary_ai_core::chart_input::resolve_chart_time("2026-01-14T14:30", "America/New_York", "")
            .unwrap();
    assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], expected);
    assert_eq!(
        script.calls.lock().unwrap()[0].1["requested_question_moment"],
        "2026-01-14 at 14:30"
    );
}

#[test]
fn unclear_core_is_refined_before_recording_its_moment() {
    use crate::reading_contracts::{self as contracts, Facet, Frame, Method};
    let mut ambiguous = contracts::control(contracts::Intent::Read);
    ambiguous.question = Some("Will it happen?".into());
    ambiguous.frame = Some(Frame {
        method: Method::Unclassified,
        facet: Facet::Event,
    });
    let mut clear = contracts::control(contracts::Intent::Clarify);
    clear.question = Some("Will I get the job?".into());
    clear.frame = Some(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    clear.subject = Some(crate::horary_role_options::Subject {
        name: "Job".into(),
        kind: "job".into(),
        owner_id: "querent".into(),
        source_quote: "Will I get the job?".into(),
    });
    let mut script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(ambiguous).unwrap()),
        (Stage::Intake, serde_json::to_value(clear).unwrap()),
    ]);
    script.cancel_after = Some(3);
    let mut session = Session::default();
    session
        .candidates
        .push(super::recovery_tests::session().place.unwrap());
    session.messages.push(Message {
        role: "user".into(),
        text: "Will it happen?".into(),
    });
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.candidate_moment_ms.is_none());
    assert!(session.chart.is_none());
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "Will I get the job?".into(),
    });
    assert!(run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous
    )
    .is_err());
    assert_eq!(
        session.chart.as_ref().unwrap()["timestampMs"],
        1789387260000.
    );
    assert_eq!(session.question, "Will I get the job?");
}

#[test]
fn exact_book_count_stops_at_a_named_method_limit_without_substituting_a_prediction() {
    let mut session = session();
    let question = "How many books will Bob sell at the fair?";
    session.question = question.into();
    session.method.brief.question = question.into();
    let case = session.method.consultation.as_mut().unwrap();
    if let crate::reading_contracts::Slot::Resolved { observation } = &mut case.question {
        observation.value = question.into();
    }
    if let crate::reading_contracts::Slot::Resolved { observation } = &mut case.frame {
        observation.value.facet = crate::reading_contracts::Facet::Quantity;
    }
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(script
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(s, _)| *s == Stage::Conversation));
    assert_eq!(session.question, question);
    assert!(
        matches!(&session.method.result,Some(crate::reading_contracts::ReadingResult::Limited{limitation}) if limitation.code=="unsupported_facet")
    );
}

#[test]
fn empty_audio_meaning_is_repaired_inside_acceptance_and_never_completed() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Your spoken question".into(),
    });
    let mut invalid = crate::reading_contracts::control(crate::reading_contracts::Intent::Resume);
    let mut repaired = invalid.clone();
    repaired.heard = "Continue".into();
    invalid.heard = String::new();
    let script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(invalid).unwrap()),
        (Stage::Intake, serde_json::to_value(repaired).unwrap()),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        Some(&[1, 2]),
        &previous,
    )
    .unwrap();
    assert_eq!(
        session
            .method
            .records
            .iter()
            .filter(|r| r.stage == Stage::Intake)
            .count(),
        2
    );
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("audio"));
    assert!(session.method.records[1].validation_error.is_none());
    assert_eq!(session.method.flow.jobs[0].attempts, 2);
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|m| m.text == "From your spoken words: Continue")
            .count(),
        1
    );
}

#[test]
fn a_stale_result_is_recorded_as_rejected_and_cannot_complete_a_job() {
    let mut session = session();
    let mut script = Script::new(vec![(
        Stage::Explanation,
        worksheet(Stage::Explanation, &[]),
    )]);
    script.cancel_after = Some(1);
    let input = json!({"follow_up_words":"Why that step?", "prior_worksheet":{},
        "reading_request":{"binding":{"catalogue_version":"obsolete", "case_revision":0}}});
    assert!(task(&mut session, &script, Stage::Explanation, input, &[]).is_err());
    assert_eq!(session.method.records.len(), 1);
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("older consultation"));
    assert!(matches!(
        session.method.flow.jobs[0].phase(),
        step::Phase::Repairing { .. }
    ));
    assert!(session.method.flow.active.is_empty());
}

#[test]
fn a_gap_in_civil_time_asks_the_native_specific_question_and_accepts_ignorance() {
    use crate::reading_contracts::{control, Evidence, Field, Observation, Slot};
    let mut session = session();
    know_bob(&mut session);
    session.chart = None;
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::QuestionTime,
        Slot::Resolved {
            observation: Observation {
                value: "2026-03-08T02:30".into(),
                evidence: Evidence::Migration {
                    detail: "Authored nonexistent civil time".into(),
                },
            },
        },
    );
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![
        (
            Stage::Moment,
            json!({"mode":"explicit","local_time":"2026-03-08T02:30","occurrence":"","clarification":"","basis":"Use the explicitly supplied civil time."}),
        ),
        (Stage::Intake, {
            let mut t = control(crate::reading_contracts::Intent::Clarify);
            t.unavailable_quote = "I don't know".into();
            serde_json::to_value(t).unwrap()
        }),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(
        session.messages.last().unwrap().text,
        "The clocks skipped that time. What time before or after the change should I use?"
    );
    assert!(session.chart.is_none());
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "I don't know".into(),
    });
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session
        .messages
        .last()
        .unwrap()
        .text
        .contains("detail is still unknown"));
    assert!(matches!(
        session.method.consultation.as_ref().unwrap().facts[&Field::QuestionTime],
        Slot::Unavailable { .. }
    ));
    assert_eq!(
        script
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| *s != Stage::Conversation)
            .count(),
        2
    );
}

#[test]
fn a_clock_overlap_waits_for_the_persons_choice_and_keeps_that_occurrence() {
    use crate::reading_contracts::{
        control, Evidence, Field, Observation, Slot, Update, UpdateMode,
    };
    let mut session = session();
    know_bob(&mut session);
    session.chart = None;
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::QuestionTime,
        Slot::Resolved {
            observation: Observation {
                value: "2026-11-01T01:30".into(),
                evidence: Evidence::Migration {
                    detail: "Authored overlapping civil time".into(),
                },
            },
        },
    );
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let explicit = |occurrence| json!({"mode":"explicit","local_time":"2026-11-01T01:30","occurrence":occurrence,"clarification":"","basis":"Use the supplied civil question moment."});
    let script = Script::new(vec![(Stage::Moment, explicit(""))]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.chart.is_none());
    assert!(session
        .messages
        .last()
        .unwrap()
        .text
        .contains("earlier occurrence or the later"));
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "The later occurrence".into(),
    });
    let mut t = control(crate::reading_contracts::Intent::Clarify);
    t.updates.push(Update {
        field: Field::TimeOccurrence,
        value: "later".into(),
        quote: "later".into(),
        mode: UpdateMode::Supply,
    });
    let mut script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(t).unwrap()),
        (Stage::Moment, explicit("later")),
    ]);
    script.cancel_after = Some(2); // Deliberately stop before reading the resolved chart.
    assert!(run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous
    )
    .is_err());
    assert_eq!(
        script.calls.lock().unwrap()[1].1["explicit_occurrence"],
        "later"
    );
    let expected = horary_ai_core::chart_input::resolve_chart_time(
        "2026-11-01T01:30",
        "America/New_York",
        "later",
    )
    .unwrap();
    assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], expected);
}

#[test]
fn a_moment_actors_question_cannot_be_mislabelled_as_missing_place() {
    use crate::reading_contracts::{Evidence, Field, Observation, RequirementKey, Slot};
    let mut session = session();
    know_bob(&mut session);
    session.chart = None;
    session.place = None;
    session.candidates.clear();
    session.method.device_attempted = true;
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::ReaderPlace,
        Slot::Resolved {
            observation: Observation {
                value: "London, United Kingdom".into(),
                evidence: Evidence::Migration {
                    detail: "Authored explicit place without a device fix".into(),
                },
            },
        },
    );
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::QuestionTime,
        Slot::Resolved {
            observation: Observation {
                value: "January 14, 2026 in the morning".into(),
                evidence: Evidence::Migration {
                    detail: "Authored earlier question date with genuinely unspecified clock time"
                        .into(),
                },
            },
        },
    );
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![(
        Stage::Moment,
        json!({"mode":"ask","local_time":"","occurrence":"",
        "clarification":"What local time on January 14 did you understand the question?","basis":"The earlier date is supplied but its clock time is not."}),
    )]);
    let previous = session.clone();
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.chart.is_none());
    assert_eq!(session.place.as_ref().unwrap().name, "London");
    assert!(session
        .method
        .flow
        .pending
        .iter()
        .any(|pending| pending.stage == Stage::Moment && pending.request.field == "chart_moment"));
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(RequirementKey::ChartMoment)
    );
    assert!(
        !crate::horary_conversation::clipboard(&session)["reminders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|need| need["key"] == json!({"kind":"chart_place"}))
    );
}

#[test]
fn losing_coordinates_preserves_a_typed_moment_wait_and_the_guru_selects_one_need() {
    use crate::reading_contracts::{Evidence, Field, Observation, RequirementKey, Slot};
    let mut session = session();
    know_bob(&mut session);
    session.chart = None;
    session.method.consultation.as_mut().unwrap().facts.insert(
        Field::QuestionTime,
        Slot::Resolved {
            observation: Observation {
                value: "January 14, 2026 in the morning".into(),
                evidence: Evidence::Migration {
                    detail: "Authored incomplete earlier clock time".into(),
                },
            },
        },
    );
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let question = "What local time on January 14 did you understand the question?";
    let script = Script::new(vec![
        (
            Stage::Moment,
            json!({"mode":"ask","local_time":"","occurrence":"","clarification":question,"basis":"The clock time is genuinely missing."}),
        ),
        (
            Stage::Conversation,
            json!({"reply":question,"ask":"need_0"}),
        ),
        (
            Stage::Conversation,
            json!({"reply":question,"ask":"need_1"}),
        ),
    ]);
    let previous = session.clone();
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(RequirementKey::ChartMoment)
    );
    let previous = session.clone();
    session.place = None;
    session.candidates.clear();
    session.method.device_attempted = true;
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.chart.is_none());
    let pending = &session.method.flow.pending;
    assert_eq!(pending.len(), 2);
    let moment = pending
        .iter()
        .find(|request| request.stage == Stage::Moment)
        .unwrap();
    assert_eq!(moment.request.field, "chart_moment");
    assert_eq!(moment.request.question, question);
    let place = pending
        .iter()
        .find(|request| request.stage == Stage::Place)
        .unwrap();
    assert_eq!(place.request.field, "chart_place");
    assert!(!place.request.question.is_empty());
    assert_ne!(place.request.question, question);
    let reminders = crate::horary_conversation::clipboard(&session)["reminders"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(reminders.len(), 2);
    assert!(reminders
        .iter()
        .any(|need| need["key"] == json!({"kind":"chart_place"})));
    assert!(reminders
        .iter()
        .any(|need| need["key"] == json!({"kind":"chart_moment"})
            && need["example_question"] == question));
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(RequirementKey::ChartMoment)
    );
    assert_eq!(session.messages.last().unwrap().text, question);
}

#[test]
fn a_saved_chart_without_place_metadata_recasts_with_acquired_coordinates_without_panicking() {
    let mut session = session();
    know_bob(&mut session);
    let acquired = session.place.take().unwrap();
    session.candidates.clear();
    session.method.device_attempted = false;
    let timestamp = session.chart.as_ref().unwrap()["timestampMs"]
        .as_f64()
        .unwrap();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![]);
    script
        .locations
        .lock()
        .unwrap()
        .push_back(Ok(Some(acquired.clone())));
    let previous = session.clone();
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        timestamp + 60_000.,
        None,
        &previous,
    )
    .unwrap();
    let place = session.place.as_ref().unwrap();
    assert_eq!(place.latitude, acquired.latitude);
    assert_eq!(place.longitude, acquired.longitude);
    assert_eq!(place.timezone, acquired.timezone);
    assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], timestamp);
    assert_eq!(session.revisions.len(), 1);
    assert_eq!(
        script
            .location_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(session.method.flow.pending.is_empty());
    assert!(session
        .audit
        .iter()
        .any(|receipt| receipt["event"] == "contract_handoff"));
}

#[test]
fn device_clock_metadata_does_not_prevent_native_coordinate_acquisition() {
    let mut session = session();
    know_bob(&mut session);
    let supplied = session.place.take().unwrap();
    session.chart = None;
    session.candidates.clear();
    session.device_context = Some(crate::conversation::DeviceContext {
        timezone: "America/New_York".into(),
        locale: "en-US".into(),
        latitude: None,
        longitude: None,
        accuracy_meters: None,
    });
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let mut script = Script::new(Vec::new());
    script
        .locations
        .get_mut()
        .unwrap()
        .push_back(Ok(Some(supplied)));
    let previous = session.clone();
    run_elicitation(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(
        script
            .location_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(session.chart.is_some());
    assert!(session.place.is_some());
}

#[test]
fn a_supplied_baseline_retires_the_prior_request_before_the_guru_replies() {
    let place = session().place.unwrap();
    let mut current = Session {
        candidates: vec![place],
        ..Session::default()
    };
    let question = "Will I get married within the next year?";
    current.messages.push(Message {
        role: "user".into(),
        text: question.into(),
    });
    let mut first = turn(crate::reading_contracts::Intent::Read);
    first["question"] = json!(question);
    first["frame"] = json!({"method":"relationship","facet":"event"});
    first["subject"] = json!({"name":"prospective partner","kind":"person","owner_id":"","source_quote":"get married"});
    let script = Script::new(vec![(Stage::Intake, first)]);
    let previous = current.clone();
    run_elicitation(
        &mut current,
        &script,
        &GeocodeState::default(),
        1791388800000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(matches!(
        current.method.result,
        Some(crate::reading_contracts::ReadingResult::NeedsInformation { .. })
    ));
    assert!(current.chart.is_none());

    let previous = current.clone();
    current.messages.push(Message {
        role: "user".into(),
        text: "I am single and no wedding is arranged.".into(),
    });
    let mut supplied = turn(crate::reading_contracts::Intent::Clarify);
    supplied["updates"] =
        json!([{"field":"baseline","value":"hoped_for","quote":"I am single","mode":"supply"}]);
    let script = Script::new(vec![(Stage::Intake, supplied)]);
    run_elicitation(
        &mut current,
        &script,
        &GeocodeState::default(),
        1791388860000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(current.chart.is_some());
    assert!(
        current.method.result.is_none(),
        "A satisfied request is history, not a live result"
    );
    assert!(crate::horary_conversation::clipboard(&current)["reminders"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(current.question, question);
    assert_eq!(
        current.chart.as_ref().unwrap()["timestampMs"],
        1791388800000.
    );
}

#[test]
fn focused_extraction_can_repair_but_cannot_replace_the_selected_question() {
    let input = json!({"consultation":crate::reading_contracts::Consultation::default(),
        "latest_words":"Will my own house sell?","spoken_input":false,
        "recognition_phase":"complete_selected_program"});
    let valid = turn(crate::reading_contracts::Intent::Clarify);
    assert!(step::check(Stage::Intake, Matter::Other, &valid, &input, &[]).is_ok());
    let mut changed = valid.clone();
    changed["question"] = json!("Will I buy a different house?");
    assert!(step::check(Stage::Intake, Matter::Other, &changed, &input, &[]).is_err());
    let mut changed = valid;
    changed["intent"] = json!("new_question");
    assert!(step::check(Stage::Intake, Matter::Other, &changed, &input, &[]).is_err());
}

#[test]
fn a_reader_cannot_reask_known_ownership_instead_of_using_its_handoff() {
    let mut session = session();
    know_bob(&mut session);
    let case = session.method.consultation.as_ref().unwrap();
    let anchor = crate::reading_contracts::Anchor {
        timestamp_ms: 1789387200000.,
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
    };
    let ready = crate::reading_contracts::ReadyReading::prepare(case, anchor).unwrap();
    let facts = reading_method::facts(session.chart.as_ref());
    let input = json!({"reading_request":ready.input(),"house_rulers_and_positions":facts,"native_role_options":role_options(&session)});
    let script = Script::new(vec![
        (
            Stage::Significators,
            json!({"request_input":{"field":"ownership","question":"Who owns these books?","reason":"The owner determines the house."}}),
        ),
        (Stage::Significators, roles()),
    ]);
    assert!(
        task(&mut session, &script, Stage::Significators, input, &facts)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        session
            .method
            .records
            .iter()
            .filter(|r| r.stage == Stage::Significators)
            .count(),
        2
    );
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("already resolved"));
    assert!(session.method.flow.pending.is_empty());
}

#[test]
fn a_readers_context_gap_returns_to_the_same_consultation_and_then_completes() {
    use crate::reading_contracts::{control, Field, Update, UpdateMode};
    let mut session = session();
    know_bob(&mut session);
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let facts = reading_method::facts(session.chart.as_ref());
    let script = Script::new(vec![
        (Stage::Significators, roles()),
        (
            Stage::Condition,
            json!({"request_input":{"field":"context","question":"Is this a one-off sale or an ongoing shop?","reason":"The kind of arrangement affects what successful completion means."}}),
        ),
        (Stage::Reception, worksheet(Stage::Reception, &facts)),
        (Stage::Contacts, worksheet(Stage::Contacts, &facts)),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(!session
        .sections
        .iter()
        .any(|s| s.method_stage == Some(Stage::Judgment)));
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(crate::reading_contracts::RequirementKey::Field(
            Field::Context
        ))
    );
    assert_eq!(
        session.messages.last().unwrap().text,
        "Is this a one-off sale or an ongoing shop?"
    );
    assert!(session
        .method
        .records
        .iter()
        .any(|r| r.stage == Stage::Reception && r.validation_error.is_none()));
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "It is a one-off sale".into(),
    });
    let mut reply = control(crate::reading_contracts::Intent::Clarify);
    reply.updates = vec![Update {
        field: Field::Context,
        value: "It is a one-off sale".into(),
        quote: "It is a one-off sale".into(),
        mode: UpdateMode::Supply,
    }];
    let script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(reply).unwrap()),
        (Stage::Significators, roles()),
        (Stage::Condition, worksheet(Stage::Condition, &facts)),
        (Stage::Reception, worksheet(Stage::Reception, &facts)),
        (Stage::Contacts, worksheet(Stage::Contacts, &facts)),
        (Stage::Judgment, worksheet(Stage::Judgment, &facts)),
    ]);
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(session.question, "Will Bob sell his books at the fair?");
    let context = session
        .method
        .consultation
        .as_ref()
        .unwrap()
        .text(Field::Context)
        .unwrap();
    assert!(context.contains("Bob's book sales") && context.contains("one-off sale"));
    assert!(matches!(
        session.method.result,
        Some(crate::reading_contracts::ReadingResult::Judgment { .. })
    ));
}

#[test]
fn unavailable_device_coordinates_use_the_same_pending_fact_and_ignorance_path() {
    use crate::horary_role_options::Subject;
    use crate::reading_contracts::{control, Facet, Frame, Intent, Method, RequirementKey};
    let words = "Will I get this job?";
    let mut session = Session::default();
    let mut initial = control(Intent::Read);
    initial.question = Some(words.into());
    initial.frame = Some(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    initial.subject = Some(Subject {
        name: "Job".into(),
        kind: "job".into(),
        owner_id: "querent".into(),
        source_quote: "job".into(),
    });
    let mut unknown = control(Intent::Clarify);
    unknown.unavailable_quote = "I don't know".into();
    let script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(initial).unwrap()),
        (Stage::Intake, serde_json::to_value(unknown).unwrap()),
    ]);
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: words.into(),
    });
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(RequirementKey::ChartPlace)
    );
    assert_eq!(
        script
            .location_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: "I don't know".into(),
    });
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387260000.,
        None,
        &previous,
    )
    .unwrap();
    assert!(session.chart.is_none());
    assert!(session
        .messages
        .last()
        .unwrap()
        .text
        .contains("detail is still unknown"));
    assert_eq!(
        script
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| *s != Stage::Conversation)
            .count(),
        3 // Classification, same-turn focused extraction, then the factual reply.
    );
}

#[test]
fn initial_recognition_cannot_treat_a_rejected_question_as_saved_context() {
    use crate::reading_contracts::{control, Facet, Frame, Intent, Method};
    let words = "Will I get this job?";
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: words.into(),
    });
    let mut initial = control(Intent::Read);
    initial.frame = Some(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    initial.subject = Some(crate::horary_role_options::Subject {
        name: "Job".into(),
        kind: "job".into(),
        owner_id: "querent".into(),
        source_quote: "job".into(),
    });
    let mut repaired = initial.clone();
    repaired.question = Some(words.into());
    let script = Script::new(vec![
        (Stage::Intake, serde_json::to_value(initial).unwrap()),
        (Stage::Intake, serde_json::to_value(repaired).unwrap()),
    ]);
    let previous = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )
    .unwrap();
    assert_eq!(
        session
            .method
            .records
            .iter()
            .filter(|r| r.stage == Stage::Intake)
            .count(),
        3 // Two classification attempts followed by focused extraction.
    );
    assert!(session.method.records[0]
        .validation_error
        .as_ref()
        .unwrap()
        .contains("unsaved question"));
    assert_eq!(session.question, words);
    assert_eq!(session.method.flow.jobs[0].attempts, 2);
}

#[test]
fn guru_speaks_and_selects_a_missing_fact_without_mutating_the_question_or_completing_roles() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![(
        Stage::Conversation,
        json!({
            "reply":"How are you connected to Bob?", "ask":"need_0"
        }),
    )]);
    let original = session.clone();
    run(
        &mut session,
        &script,
        &GeocodeState::default(),
        1789387800000.,
        None,
        &original,
    )
    .unwrap();
    assert_eq!(
        session.messages.last().unwrap().text,
        "How are you connected to Bob?"
    );
    assert_eq!(session.question, original.question);
    assert!(session.sections.is_empty());
    assert!(script
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(s, _)| *s == Stage::Conversation));
    assert_eq!(
        session.method.consultation.as_ref().unwrap().requested,
        Some(crate::reading_contracts::RequirementKey::PersonRelationship("bob".into()))
    );
    let calls = script.calls.lock().unwrap();
    assert_eq!(calls[0].1["canonical_question"], original.question);
    assert!(!calls[0].1["reminders"].as_array().unwrap().is_empty());
}

#[test]
fn an_invalid_guru_reminder_is_repaired_before_the_person_sees_a_reply() {
    let mut session = session();
    session.messages.push(Message {
        role: "user".into(),
        text: "Continue".into(),
    });
    let script = Script::new(vec![
        (
            Stage::Conversation,
            json!({"reply":"Invalid private reminder.","ask":"made_up"}),
        ),
        (
            Stage::Conversation,
            json!({"reply":"What is your connection to Bob?","ask":"need_0"}),
        ),
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
    assert!(session.method.records[0].validation_error.is_some());
    assert_eq!(session.method.flow.jobs[0].attempts, 2);
    assert_eq!(
        session.messages.last().unwrap().text,
        "What is your connection to Bob?"
    );
    assert!(!session
        .messages
        .iter()
        .any(|m| m.text == "Invalid private reminder."));
    assert!(matches!(
        session.method.flow.jobs[0].phase(),
        step::Phase::Complete { .. }
    ));
}
