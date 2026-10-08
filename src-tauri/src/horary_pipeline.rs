//! Explicit horary tasks. Native calculations own facts; worksheets own proposals.
#![forbid(unsafe_code)]
#[cfg(test)]
use crate::conversation::Message;
use crate::{
    conversation::{Section, Session},
    geocode::{geocode_with_cache, GeocodeRequest, GeocodeState, LocationCandidate},
    horary_executor::{ask_pending, execute, execute_batch, task},
    horary_lessons::{self as lessons, Matter, Stage},
    horary_step::{self as step, CheckedData},
    native_llama_worker::NativeGenerationResult,
    reading_method::{self, Fact, Role, Step},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[cfg(test)]
pub use crate::horary_contract::{decode_json, schema, schema_for, validate};
pub use crate::horary_contract::{prompt, Brief};

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct MethodState {
    #[serde(default)]
    pub consultation: Option<crate::reading_contracts::Consultation>,
    #[serde(default)]
    pub result: Option<crate::reading_contracts::ReadingResult>,
    #[serde(default)]
    pub device_attempted: bool,
    pub brief: Brief,
    pub records: Vec<Record>,
    #[serde(default)]
    #[serde(skip_serializing_if = "step::Journal::is_empty")]
    pub flow: step::Journal,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<UserReply>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct UserReply {
    pub stage: Stage,
    pub question: String,
    pub words: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub stage: Stage,
    pub revision: u64,
    pub guide_sha256: String,
    pub schema_sha256: String,
    pub input_sha256: String,
    pub input: Value,
    pub raw: String,
    pub worksheet: Value,
    pub generation: NativeGenerationResult,
    pub source_passages: Vec<String>,
    #[serde(default)]
    pub validation_error: Option<String>,
}

pub trait Runtime: Sync {
    fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
        Ok(None)
    }
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String>;
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String>;
    fn publish(&self, session: &mut Session) -> Result<(), String>;
    fn check(&self) -> Result<(), String>;
    fn directory(&self) -> &Path;
}

fn reading_brief(brief: &Brief) -> Value {
    // A stable matter is different from the command on the latest turn. Analysis
    // tasks must not inherit "explain", a prior focus, or a prior clarification.
    json!({"question":brief.question,"matter":brief.matter,"question_kind":brief.question_kind,
        "context":brief.context,"people":brief.people,"subject":brief.subject,"event_place":brief.event_place,"event_time":brief.event_time,"horizon":brief.horizon})
}

fn follow_up_words(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| {
            message
                .text
                .strip_prefix("From your spoken words: ")
                .unwrap_or(&message.text)
                .to_string()
        })
        .unwrap_or_default()
}

fn is_resume_request(words: &str) -> bool {
    let words = words
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_lowercase();
    matches!(
        words.as_str(),
        "continue"
            | "go on"
            | "resume"
            | "try again"
            | "cast the chart"
            | "finish the reading"
            | "continue the reading"
    )
}

fn chart_context(session: &Session) -> Value {
    let Some(chart) = session.chart.as_ref() else {
        return Value::Null;
    };
    let moment = chart["timestampMs"].as_f64();
    let zone = session.place.as_ref().map(|place| place.timezone.as_str());
    let local = moment
        .zip(zone)
        .and_then(|(moment, zone)| horary_ai_core::chart_input::local_clock(moment, zone).ok());
    json!({"status":"cast","timestamp_ms":moment,"local_civil_time":local,"timezone":zone,
        "reader_place":session.place,"candidate_question_moment_ms":session.candidate_moment_ms,
        "basis":"Horary uses the reader's place and the moment the question was understood, not the place or starting time of the event being asked about.",
        "event_context":{"place":session.method.brief.event_place,"time":session.method.brief.event_time,"other":session.method.brief.context}})
}

fn intake_input(session: &Session, spoken: bool, instant: f64) -> Value {
    let device = session
        .candidates
        .iter()
        .find(|place| place.provider == "device");
    let zone = session
        .place
        .as_ref()
        .or(device)
        .map(|place| place.timezone.as_str())
        .or_else(|| {
            session
                .device_context
                .as_ref()
                .map(|context| context.timezone.as_str())
        });
    let clock = zone.and_then(|zone| horary_ai_core::chart_input::local_clock(instant, zone).ok());
    let completed: Vec<_> = session
        .sections
        .iter()
        .filter_map(|section| section.method_stage)
        .collect();
    let unfinished = session
        .method
        .flow
        .jobs
        .iter()
        .rev()
        .find(|job| {
            job.revision == session.revision
                && !matches!(
                    job.stage,
                    Stage::Intake | Stage::Explanation | Stage::Conversation
                )
                && !matches!(job.phase(), step::Phase::Complete { .. })
        })
        .map(|job| json!({"stage":job.stage,"phase":job.phase()}));
    let legacy_error = if session.chart.is_some() && session.sections.is_empty() {
        session
            .audit
            .iter()
            .rev()
            .find_map(|receipt| receipt.get("interruption"))
    } else {
        None
    };
    let legacy_sources: Vec<_> = if session.method.brief.subject.is_empty() {
        session
            .messages
            .iter()
            .filter(|message| message.role == "user")
            .rev()
            .take(8)
            .map(|message| message.text.as_str())
            .collect()
    } else {
        Vec::new()
    };
    let classify = session
        .method
        .consultation
        .as_ref()
        .and_then(|case| case.method())
        .is_none_or(|method| {
            matches!(
                method,
                crate::reading_contracts::Method::Wish
                    | crate::reading_contracts::Method::Unclassified
            )
        });
    json!({"recognition_phase":if classify {"classify_question"} else {"update_selected_program"},
        "legacy_user_fact_sources":legacy_sources,"canonical_question":session.question,"chart_exists":session.chart.is_some(),
        "consultation":session.method.consultation.as_ref().map(|c|c.recognition_snapshot()),"pending_requirement":session.method.consultation.as_ref().and_then(|c|c.requested.as_ref()),
        "consultation_state":{"chart":chart_context(session),"completed_steps":completed,
            "interpretation_exists":completed.contains(&Stage::Judgment),"unfinished_step":unfinished,
            "pending_user_requests":session.method.flow.pending,"legacy_interruption":legacy_error},
        "device_place_available":device.is_some(),"device_place":device,
        "receipt_instant_ms":instant,"current_local_clock":clock,"current_timezone":zone,
        "context_authority":"The native clock resolves relative dates; the device place supports nearby-place inference. Neither is evidence of citizenship, an event venue, or a historical chart override.",
        "native_clock_available":clock.is_some(),
        "available_revisions":session.revisions.iter().map(|revision|json!({"number":revision.number,"question":revision.question})).collect::<Vec<_>>(),
        "latest_words":session.messages.last().map(|message|message.text.as_str()),
        "last_reader_question":session.messages.iter().rev().find(|message|message.role == "assistant").map(|message|message.text.as_str()),
        "spoken_input":spoken})
}

fn explain(session: &mut Session, runtime: &impl Runtime, words: &str) -> Result<(), String> {
    let all = reading_method::facts(session.chart.as_ref());
    let focus = session.method.brief.focus.clone();
    let target = match focus.as_str() {
        "roles" => Stage::Significators,
        "condition" => Stage::Condition,
        "reception" => Stage::Reception,
        "contacts" => Stage::Contacts,
        "location" => Stage::Location,
        "timing" => Stage::Timing,
        "place" => Stage::Place,
        "moment" => Stage::Moment,
        _ => Stage::Judgment,
    };
    let native_context = if matches!(target, Stage::Place | Stage::Moment) {
        chart_context(session)
    } else {
        Value::Null
    };
    let record = session.method.records.iter().rev().find(|record| {
        record.stage == target
            && record.revision == session.revision
            && record.validation_error.is_none()
            && record.worksheet.get("request_input").is_none()
    });
    let passage = session
        .sections
        .iter()
        .find(|section| section.method_stage == Some(target))
        .or_else(|| {
            session.sections.iter().find(|section| {
                target == Stage::Significators && section.step == Some(Step::Significators)
            })
        });
    if native_context.is_null() && passage.is_none() {
        session.audit.push(json!({"event":"explanation_prerequisite","target":target,"result":"not_yet_available"}));
        ask_pending(session);
        return Ok(());
    }
    let supplied: Vec<_> = record
        .into_iter()
        .flat_map(|record| {
            record.worksheet["checks"]
                .as_object()
                .into_iter()
                .flatten()
                .flat_map(|(_, check)| check["evidence"].as_array().into_iter().flatten())
        })
        .filter_map(Value::as_str)
        .collect();
    let mut facts: Vec<_> = all
        .iter()
        .filter(|fact| {
            supplied.contains(&fact.id.as_str())
                || passage.is_some_and(|section| section.evidence.contains(&fact.id))
        })
        .cloned()
        .collect();
    if !native_context.is_null() {
        facts.push(Fact { id:"chart.moment".into(),kind:"chart_context".into(),label:"The chart's actual moment".into(),detail:format!("The existing chart uses {} in {} (instant {}). This is the question's recorded moment, not the reported event date.",native_context["local_civil_time"],native_context["timezone"],native_context["timestamp_ms"]),planets:Vec::new(),condition_facet:None,event:None });
        facts.push(Fact { id:"chart.place".into(),kind:"chart_context".into(),label:"The chart's actual place".into(),detail:format!("The existing chart was cast for {}. A place mentioned as the event venue is not confirmation that the reader is there.",native_context["reader_place"]["label"]),planets:Vec::new(),condition_facet:None,event:None });
    }
    let input = json!({"brief":reading_brief(&session.method.brief),"follow_up_words":words,"focus":focus,
        "chart_context":native_context,"reading_complete":session.sections.iter().any(|section|section.method_stage == Some(Stage::Judgment)),
        "prior_worksheet":record.map(|record|&record.worksheet).or_else(||passage.map(|section|&section.worksheet)),
        "prior_assignment":passage.map(|section|&section.roles),"pending_user_requests":session.method.flow.pending,"facts":facts});
    let _ = task(session, runtime, Stage::Explanation, input, &facts)?;
    ask_pending(session);
    Ok(())
}

fn relevant(facts: &[Fact], roles: &[Role], kinds: &[&str]) -> Vec<Fact> {
    facts
        .iter()
        .filter(|f| {
            kinds.contains(&f.kind.as_str())
                && (f.planets.is_empty()
                    || if f.kind == "condition" {
                        f.planets
                            .iter()
                            .any(|p| roles.iter().any(|r| &r.planet == p))
                    } else {
                        f.planets
                            .iter()
                            .all(|p| roles.iter().any(|r| &r.planet == p))
                    })
        })
        .cloned()
        .collect()
}

pub fn native_place(
    brief: &Brief,
    candidates: &[LocationCandidate],
    existing: Option<&LocationCandidate>,
    geocode: &GeocodeState,
) -> Result<(Option<LocationCandidate>, Vec<LocationCandidate>), String> {
    if brief.place_request.trim().is_empty() {
        return Ok((
            existing
                .cloned()
                .or_else(|| candidates.iter().find(|p| p.provider == "device").cloned()),
            Vec::new(),
        ));
    }
    let matches = geocode_with_cache(
        geocode,
        GeocodeRequest {
            query: brief.place_request.clone(),
            limit: Some(50),
        },
    )
    .map_err(|e| e.message)?;
    let chosen = crate::geocode::uniquely_qualified_city(&brief.place_request, &matches)
        .or_else(|| (matches.len() == 1).then(|| matches[0].clone()))
        .or_else(|| {
            let device = candidates.iter().find(|place| place.provider == "device")?;
            crate::geocode::nearby_unique_city(&matches, device)
        });
    Ok((chosen, matches))
}

/// Resolve a supplied event city with the same offline tools as a reader city.
/// The event stays separate from the chart anchor, and the original quoted
/// observation remains in the receipt. Ambiguous nearby towns are not guessed.
fn resolve_event_places(session: &mut Session, geocode: &GeocodeState) -> Result<(), String> {
    use crate::reading_contracts::{Evidence, Field, Observation, Slot};
    for field in [Field::TargetPlace, Field::EventPlace] {
        let observed = session
            .method
            .consultation
            .as_ref()
            .and_then(|case| case.facts.get(&field))
            .and_then(|slot| match slot {
                Slot::Resolved { observation } => Some(observation.clone()),
                _ => None,
            });
        let Some(observed) = observed else {
            continue;
        };
        let brief = Brief {
            place_request: observed.value.clone(),
            ..Default::default()
        };
        let (chosen, matches) = native_place(&brief, &session.candidates, None, geocode)?;
        let Some(place) = chosen else {
            // A venue description with no city match remains contextual text.
            // A recognized but ambiguous CITY remains a proposed fact.
            if matches.len() > 1 {
                let case = session
                    .method
                    .consultation
                    .as_mut()
                    .expect("Consultation installed");
                case.facts.insert(
                    field,
                    Slot::Proposed {
                        observation: observed.clone(),
                    },
                );
                case.revision = case
                    .revision
                    .checked_add(1)
                    .ok_or("Case revision exhausted")?;
                session.audit.push(json!({"event":"native_event_place_ambiguous","field":field,
                    "before_revision":case.revision-1,"after_revision":case.revision,
                    "before":{"state":"resolved","observation":observed},
                    "after":case.facts.get(&field),"candidates":matches,"chart_anchor_changed":false}));
            }
            continue;
        };
        if observed.value == place.label {
            continue;
        }
        let case = session
            .method
            .consultation
            .as_mut()
            .expect("Consultation installed");
        let replacement = Observation { value:place.label.clone(), evidence:Evidence::NativePlace {
            query:observed.value.clone(), candidate_id:place.id.clone(),
            context:"Offline candidates; explicitly qualified municipality, unique city or unique nearby municipality from actual device fix. Event place never replaces reader place.".into(),
            original:Box::new(observed.evidence.clone()),
        } };
        case.facts.insert(
            field,
            Slot::Resolved {
                observation: replacement.clone(),
            },
        );
        case.revision = case
            .revision
            .checked_add(1)
            .ok_or("Case revision exhausted")?;
        session.audit.push(
            json!({"event":"native_event_place_resolution","field":field,
            "before_revision":case.revision-1,"after_revision":case.revision,
            "before":{"state":"resolved","observation":observed},
            "after":{"state":"resolved","observation":replacement},
            "candidate":place,"chart_anchor_changed":false}),
        );
    }
    Ok(())
}

fn resolve_moment(
    moment: &Value,
    chart: Option<&Value>,
    instant: f64,
    zone: &str,
) -> Result<f64, String> {
    match moment["mode"].as_str() {
        Some("now") => Ok(instant),
        Some("keep") => Ok(chart
            .and_then(|c| c["timestampMs"].as_f64())
            .unwrap_or(instant)),
        Some("explicit") => horary_ai_core::chart_input::resolve_chart_time(
            moment["local_time"].as_str().unwrap_or(""),
            zone,
            moment["occurrence"].as_str().unwrap_or(""),
        ),
        _ => Err("The question moment remains unresolved.".into()),
    }
}

fn restore_revision(session: &mut Session, number: u64) -> Result<(), String> {
    let old = session
        .revisions
        .iter()
        .find(|r| r.number == number)
        .cloned()
        .ok_or("Choose a listed earlier chart revision.")?;
    let next = session
        .revision
        .checked_add(1)
        .ok_or("Reading revision exhausted")?;
    session.revisions.push(crate::conversation::Revision {
        number: session.revision,
        question: session.question.clone(),
        chart: session.chart.clone(),
        sections: session.sections.clone(),
        place: session.place.clone(),
        brief: session.method.brief.clone(),
        consultation: session.method.consultation.clone(),
    });
    session.revision = next;
    session.question = old.question;
    session.chart = old.chart;
    session.place = old.place;
    session.sections = old.sections;
    session.method.brief = old.brief;
    session.method.consultation = old.consultation;
    session.method.result = None;
    session.method.brief.question = session.question.clone();
    session.method.flow.pending.clear();
    session.method.replies.clear();
    session.chart_after_message = session.messages.len();
    for section in &mut session.sections {
        section.revision = next;
        section.after_message = session.messages.len();
    }
    session
        .audit
        .push(json!({"event":"restore_revision","restored":number,"activation_revision":next}));
    Ok(())
}

#[cfg(test)]
#[path = "horary_recovery_tests.rs"]
mod recovery_tests;

fn section(session: &mut Session, stage: Stage, value: &Value, facts: &[Fact], roles: Vec<Role>) {
    let ids: Vec<String> = if stage == Stage::Significators {
        facts
            .iter()
            .filter(|f| {
                f.kind == "house"
                    && roles.iter().any(|role| {
                        role.house
                            .is_some_and(|house| f.label == format!("House {house}"))
                    })
            })
            .map(|f| f.id.clone())
            .collect()
    } else {
        let mut ids: Vec<_> = value["checks"]
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(_, v)| v["evidence"].as_array().into_iter().flatten())
            .chain(value["evidence"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let body = value["answer"]
        .as_str()
        .or_else(|| value["summary"].as_str())
        .unwrap_or("")
        .to_string();
    let step = match stage {
        Stage::Significators => Some(Step::Significators),
        Stage::Judgment => Some(Step::Judgment),
        _ => Some(Step::Testimony),
    };
    let title = match stage {
        Stage::Significators => "The people and the matter",
        Stage::Judgment => "An answer taking shape",
        Stage::Location => "Where to look",
        Stage::Reception => "What draws them together",
        Stage::Contacts => "What could bring it about",
        _ => stage.title(),
    };
    let rules = lessons::passages()
        .ok()
        .into_iter()
        .flatten()
        .filter(|p| {
            stage
                .passages(session.method.brief.matter)
                .contains(&p.id.as_str())
        })
        .map(|p| reading_method::BookRule {
            id: p.id.clone(),
            title: "From the textbook".into(),
            explanation: p.quote.clone(),
            pages: if p.printed_first == p.printed_last {
                p.printed_first.to_string()
            } else {
                format!("{}–{}", p.printed_first, p.printed_last)
            },
            quoted: true,
        })
        .collect();
    let section = Section {
        title: title.into(),
        body,
        evidence: ids.clone(),
        revision: session.revision,
        after_message: session.messages.len(),
        step,
        rules,
        because: String::new(),
        roles,
        facts: facts
            .iter()
            .filter(|f| ids.contains(&f.id))
            .cloned()
            .collect(),
        draft: String::new(),
        worksheet: value.clone(),
        method_stage: Some(stage),
    };
    if let Some(existing) = session
        .sections
        .iter_mut()
        .find(|section| section.method_stage == Some(stage) && section.revision == session.revision)
    {
        *existing = section;
    } else {
        session.sections.push(section);
    }
}

fn contract_question(session: &mut Session, need: &crate::reading_contracts::Need) {
    use crate::reading_contracts::{InformationNeed, ReadingResult};
    let case = session
        .method
        .consultation
        .as_mut()
        .expect("Consultation installed");
    case.requested = Some(need.key.clone());
    session.method.result = Some(ReadingResult::NeedsInformation {
        need: InformationNeed {
            key: need.key.clone(),
            reason: need.reason.clone(),
            question: Some(need.question.clone()),
        },
    });
}

fn contract_limitation(session: &mut Session, limitation: &crate::reading_contracts::Limitation) {
    session.method.result = Some(crate::reading_contracts::ReadingResult::Limited {
        limitation: limitation.into(),
    });
}

fn capture_declared_reader_place(
    session: &mut Session,
    words: &str,
    spoken: bool,
) -> Result<(), String> {
    use crate::reading_contracts as contracts;
    if session.chart.is_some()
        || session
            .method
            .consultation
            .as_ref()
            .is_none_or(|case| case.text(contracts::Field::ReaderPlace).is_some())
    {
        return Ok(());
    }
    let Some(update) = contracts::reader_place_statement(words) else {
        return Ok(());
    };
    let mut patch = contracts::control(contracts::Intent::Clarify);
    patch.updates.push(update);
    if spoken {
        patch.heard = words.into();
    }
    session
        .method
        .consultation
        .as_mut()
        .expect("Consultation installed")
        .apply(&patch, session.messages.len(), words, spoken)?;
    session.audit.push(json!({"event":"native_fact_observation","rule":"explicit_reader_place_statement","words":words,"spoken":spoken,"patch":patch}));
    Ok(())
}

pub fn run(
    session: &mut Session,
    runtime: &impl Runtime,
    geocode: &GeocodeState,
    instant: f64,
    audio: Option<&[u8]>,
    previous: &Session,
) -> Result<(), String> {
    let result = run_inner(session, runtime, geocode, instant, audio, previous).and_then(|()| {
        if session.method.brief.intent != "pause" {
            crate::horary_conversation::respond(session, runtime)?;
        }
        Ok(())
    });
    if let Err(error) = &result {
        session.method.flow.pause(error.clone());
        runtime.publish(session)?;
    }
    result
}

fn run_inner(
    session: &mut Session,
    runtime: &impl Runtime,
    geocode: &GeocodeState,
    instant: f64,
    audio: Option<&[u8]>,
    previous: &Session,
) -> Result<(), String> {
    if let Some(ready) = prepare_reading(session, runtime, geocode, instant, audio, previous)? {
        generate_reading(session, runtime, ready)?;
    }
    Ok(())
}

/// The campaign exercises the same input phase and guru as production, without
/// claiming that collecting sufficient inputs qualifies an interpretation.
#[cfg(test)]
pub(crate) fn run_elicitation(
    session: &mut Session,
    runtime: &impl Runtime,
    geocode: &GeocodeState,
    instant: f64,
    audio: Option<&[u8]>,
    previous: &Session,
) -> Result<(), String> {
    let result =
        prepare_reading(session, runtime, geocode, instant, audio, previous).and_then(|_| {
            if session.method.brief.intent != "pause" {
                crate::horary_conversation::respond(session, runtime)?;
            }
            Ok(())
        });
    if let Err(error) = &result {
        session.method.flow.pause(error.clone());
        runtime.publish(session)?;
    }
    result
}

fn prepare_reading(
    session: &mut Session,
    runtime: &impl Runtime,
    geocode: &GeocodeState,
    instant: f64,
    audio: Option<&[u8]>,
    previous: &Session,
) -> Result<Option<crate::reading_contracts::ReadyReading>, String> {
    use crate::reading_contracts::{self as contracts, Intent, RequirementKey};
    if session.method.consultation.is_none() {
        session.method.consultation = Some(contracts::migrate(&session.method.brief));
    }
    let old_case_revision = session.method.consultation.as_ref().map(|c| c.revision);
    let old_method = session
        .method
        .consultation
        .as_ref()
        .and_then(|c| c.method());
    if audio.is_none() {
        let words = follow_up_words(session);
        capture_declared_reader_place(session, &words, false)?;
    }
    let record_start = session.method.records.len();
    let pending = session.method.flow.pending.clone();
    let typed_resume = audio.is_none()
        && is_resume_request(&follow_up_words(session))
        && !session.question.is_empty()
        && session.method.brief.question == session.question
        && session
            .method
            .consultation
            .as_ref()
            .is_some_and(|c| c.method().is_some());
    let acquire_device = session.place.is_none()
        && !session.candidates.iter().any(|p| p.provider == "device")
        && !session.method.device_attempted;
    let input = intake_input(session, audio.is_some(), instant);
    // Acquisition and recognition have independent inputs. The native location
    // worker runs beside the model, and both are joined before planning.
    let (recognition, location) = std::thread::scope(|scope| {
        let location = acquire_device.then(|| scope.spawn(|| runtime.device_location()));
        let recognition = if typed_resume {
            Ok(Some(contracts::control(Intent::Resume)))
        } else {
            execute(session, runtime, Stage::Intake, input, &[], audio).and_then(|data| {
                data.map(|data| {
                    data.turn()
                        .cloned()
                        .ok_or_else(|| "Recognition completion lacks its checked fact patch".into())
                })
                .transpose()
            })
        };
        (
            recognition,
            location.map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("Native location acquisition interrupted".into()))
            }),
        )
    });
    if let Some(location) = location {
        session.method.device_attempted = true;
        session
            .audit
            .push(json!({"event":"contract_device_acquisition","result":location}));
        if let Ok(Some(place)) = location {
            session.candidates.push(place);
        }
    }
    let Some(mut turn) = recognition? else {
        return Ok(None);
    };
    let words = if audio.is_some() {
        turn.heard.clone()
    } else {
        follow_up_words(session)
    };
    if is_resume_request(&words) && !session.question.is_empty() {
        session.audit.push(json!({"event":"native_resume_command","proposedIntent":turn.intent,"effectiveIntent":"resume","basis":"Explicit request to continue the current matter; an existing chart is retained."}));
        turn.intent = Intent::Resume;
    }
    if turn.intent == Intent::NewQuestion {
        let decision_records = session.method.records[record_start..].to_vec();
        let last = session.messages.last().cloned();
        let sequence = session.snapshot_id;
        let device_candidates = session
            .candidates
            .iter()
            .filter(|p| p.provider == "device")
            .cloned()
            .collect();
        let device_attempted = session.method.device_attempted;
        *session = crate::reading_store::fresh(runtime.directory(), previous, None)?;
        session.snapshot_id = sequence;
        if let Some(last) = last {
            session.messages.push(last);
        }
        session.device_context = previous.device_context.clone();
        session.method.device_attempted = device_attempted;
        // The new leaf keeps its actual classification receipt, including its
        // predecessor context, while downstream prompts use only the new brief.
        session.method.records = decision_records;
        session.candidates = device_candidates;
    }
    if audio.is_some() {
        if let Some(last) = session.messages.last_mut() {
            last.text = format!("From your spoken words: {}", turn.heard);
        }
    }
    session
        .method
        .consultation
        .get_or_insert_with(contracts::Consultation::default)
        .apply(&turn, session.messages.len(), &words, audio.is_some())?;
    // Direct audio has no text until recognition. Its checked meaning summary
    // can supply the same literal fact even if the model omitted the update.
    capture_declared_reader_place(session, &words, audio.is_some())?;
    // Classification selects the small program. Give that program the same
    // words before declaring a fact absent: its focused lesson knows the
    // situational fields that the general classifier does not teach in detail.
    // This is private extraction, not a second conversational turn or a new
    // question. Its output enters the normal acceptance/repair journal.
    let mut selected = session
        .method
        .consultation
        .as_ref()
        .and_then(|c| c.method());
    let focus_selected = selected.is_some_and(|method| {
        !matches!(
            method,
            contracts::Method::Wish | contracts::Method::Unclassified
        )
    }) && (selected != old_method || turn.intent == Intent::NewQuestion)
        && matches!(
            turn.intent,
            Intent::Read | Intent::Clarify | Intent::Correct | Intent::NewQuestion
        );
    while focus_selected
        && selected.is_some_and(|method| {
            !matches!(
                method,
                contracts::Method::Wish | contracts::Method::Unclassified
            )
        })
    {
        runtime.check()?;
        let mut input = intake_input(session, false, instant);
        input["canonical_question"] = json!(session
            .method
            .consultation
            .as_ref()
            .and_then(|c| c.question.resolved()));
        input["latest_words"] = json!(words);
        input["recognition_phase"] = json!("complete_selected_program");
        input["frame_authority"] = json!("The frame in consultation is only the classifier's tentative hypothesis in this phase, even though its stored slot says resolved. Verify both method and facet independently against the actual question. The accepted question and other sourced facts remain authoritative.");
        input["instruction"] = json!("Complete the selected program from the SAME actual words, not a fresh question and not a conversational reply. Use intent=clarify and question=null. Verify the tentative method and facet first; the stored frame is not an immutable fact. If the method is wrong, return only the corrected frame with subject=null, people=[], updates=[] so its own lesson can run. If only the facet is wrong, correct it and extract this method's facts now. WILL within a period is event plus horizon; WHEN is timing; retain an actual count as quantity. Null subject means unchanged only if the consultation already contains the actual subject: otherwise extract the stated target or role with this method's permitted kind. Preserve existing sourced observations, supply omitted facts with exact quotes from the actual words or retained question, and leave absent information missing. Examples are not user evidence. Explicit earlier consultation place/time overrides must be extracted separately from an event venue/date. Do not speak to the user.");
        let Some(data) = execute(session, runtime, Stage::Intake, input, &[], None)? else {
            return Ok(None);
        };
        let patch = data
            .turn()
            .ok_or("Focused recognition lacks its checked fact patch")?;
        let case = session
            .method
            .consultation
            .as_mut()
            .expect("Consultation installed");
        // The native boundary permits this refinement only in the focused
        // same-turn program. Its question is fixed; a changed method carries
        // no facts until the new lesson has verified it. Ordinary user updates
        // retain the usual conflict/correction semantics.
        if patch.frame.is_some() {
            case.frame = contracts::Slot::Missing;
        }
        case.apply(patch, session.messages.len(), &words, false)?;
        let confirmed = case.method();
        if confirmed == selected {
            break;
        }
        session.audit.push(json!({"event":"recognition_program_refined","from":selected,"to":confirmed,
            "basis":"Focused verification of the same source words; no new user question or chart moment."}));
        selected = confirmed;
    }
    resolve_event_places(session, geocode)?;
    let case = session
        .method
        .consultation
        .as_ref()
        .expect("Consultation installed");
    session.method.brief = contracts::brief(case, &turn);
    session.question = session.method.brief.question.clone();
    let semantic_change =
        old_case_revision != session.method.consultation.as_ref().map(|c| c.revision);
    if semantic_change && session.chart.is_some() && !session.sections.is_empty() {
        session.revisions.push(crate::conversation::Revision {
            number: session.revision,
            question: previous.question.clone(),
            chart: session.chart.clone(),
            sections: session.sections.clone(),
            place: session.place.clone(),
            brief: previous.method.brief.clone(),
            consultation: previous.method.consultation.clone(),
        });
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or("Reading revision exhausted")?;
        session.sections.clear();
        session.method.result = None;
    }
    if !matches!(
        session.method.brief.intent.as_str(),
        "explain" | "new_question" | "restore"
    ) && !is_resume_request(&words)
    {
        for request in pending {
            session.method.replies.push(UserReply {
                stage: request.stage,
                question: request.request.question,
                words: words.clone(),
            });
        }
    }
    if session.chart.is_none()
        && session.candidate_moment_ms.is_none()
        && session
            .method
            .consultation
            .as_ref()
            .is_some_and(|c| c.understood())
    {
        session.candidate_moment_ms = Some(instant);
        session.audit.push(json!({"event":"question_moment_candidate","timestampMs":instant,"basis":"Receipt of the understood question; a later place-only clarification keeps this instant."}));
    }
    if turn.intent == Intent::Restore {
        restore_revision(
            session,
            session
                .method
                .brief
                .restore_revision
                .ok_or("Choose one listed revision to restore.")?,
        )?;
        return Ok(None);
    }
    if session.method.brief.intent == "explain" && session.chart.is_some() {
        explain(session, runtime, &words)?;
        return Ok(None);
    }
    if turn.intent == Intent::Explain {
        return Ok(None);
    }
    if turn.intent == Intent::Pause {
        return Ok(None);
    }
    if turn.intent == Intent::UseDevice && !acquire_device {
        let location = runtime.device_location();
        session
            .audit
            .push(json!({"event":"contract_device_acquisition","retry":true,"result":location}));
        session.method.device_attempted = true;
        if let Ok(Some(place)) = location {
            session.candidates.retain(|p| p.provider != "device");
            session.candidates.push(place);
        }
    }
    let plan = session
        .method
        .consultation
        .as_ref()
        .expect("Consultation installed")
        .plan(None);
    session.audit.push(json!({"event":"contract_plan","catalogueVersion":contracts::VERSION,"caseRevision":session.method.consultation.as_ref().map(|c|c.revision),"plan":plan}));
    if plan
        .limitation
        .as_ref()
        .is_some_and(|l| l.code == "unsupported_facet")
    {
        contract_limitation(
            session,
            plan.limitation.as_ref().expect("Checked limitation"),
        );
        return Ok(None);
    }
    if let Some(need) = plan.needs.iter().find(|need| {
        need.key != RequirementKey::ChartPlace || need.state != "native_acquisition_pending"
    }) {
        contract_question(session, need);
        return Ok(None);
    }
    if let Some(limitation) = &plan.limitation {
        contract_limitation(session, limitation);
        return Ok(None);
    }
    // Place lookup and moment sufficiency have no dependency on each other.
    // Civil-time resolution waits for the selected place's actual time zone.
    let b = &session.method.brief;
    let (place_result, moment_result) = std::thread::scope(|scope| {
        let place = scope.spawn(|| {
            native_place(
                b,
                &session.candidates,
                if b.intent == "use_device" {
                    None
                } else {
                    session.place.as_ref()
                },
                geocode,
            )
        });
        let moment=scope.spawn(|| json!({"mode":if b.time_request.is_empty(){if session.chart.is_some(){"keep"}else{"now"}}else{"needs_civil_time"},"local_time":"","occurrence":"","clarification":"","basis":"The understood question uses the recorded receipt instant; the same matter keeps its chart."}));
        // Keep the two arms typed separately below; no chart mutation occurs
        // until both branches have joined successfully.
        (
            place.join().map_err(|_| "Place check interrupted"),
            moment.join().map_err(|_| "Moment check interrupted"),
        )
    });
    let (mut place, candidates) = place_result.map_err(str::to_string)??;
    let mut moment = moment_result.map_err(str::to_string)?;
    let mut place_inquiry = None;
    if place.is_none() && !candidates.is_empty() {
        let Some(data) = task(
            session,
            runtime,
            Stage::Place,
            json!({"brief":session.method.brief,"candidates":candidates}),
            &[],
        )?
        else {
            return Ok(None);
        };
        let value = data.worksheet();
        if value["mode"] == "select" {
            place = candidates
                .iter()
                .find(|p| Some(p.id.as_str()) == value["place_id"].as_str())
                .cloned();
            if place.is_none() {
                return Err("Choose a supplied place candidate ID.".into());
            }
        }
        if place.is_none() {
            place_inquiry = value["clarification"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
        }
    }
    if let Some(place) = &place {
        // A verified place is independently complete even when the moment
        // actor still needs information. Do not mix new place metadata into
        // an existing rendered chart before its recast is validated.
        if session.chart.is_none() {
            session.place = Some(place.clone());
        }
        session
            .method
            .flow
            .pending
            .retain(|pending| pending.stage != Stage::Place);
    }
    if moment["mode"] == "needs_civil_time" && place.is_some() {
        let zone = place.as_ref().map(|p| p.timezone.as_str());
        let clock = zone
            .map(|z| horary_ai_core::chart_input::local_clock(instant, z))
            .transpose()?;
        let Some(data) = task(
            session,
            runtime,
            Stage::Moment,
            json!({"requested_question_moment":session.method.brief.time_request,"explicit_occurrence":session.method.consultation.as_ref().and_then(|c|c.text(contracts::Field::TimeOccurrence)),"question_context":session.method.brief.context,"selected_timezone":zone,"current_local_clock":clock,"existing_chart_moment":session.chart.as_ref().map(|c|&c["timestampMs"])}),
            &[],
        )?
        else {
            return Ok(None);
        };
        moment = data.worksheet().clone();
    }
    if place.is_none() {
        // Actor inquiries already return typed NeedsInput through task().
        // This branch owns only failed native coordinate acquisition; it
        // must preserve a separate pending moment request.
        let need = contracts::Need {
            key: RequirementKey::ChartPlace,
            state: "native_acquisition_missing".into(),
            question: place_inquiry.unwrap_or_else(|| {
                session.method.consultation.as_ref().expect("Consultation installed")
                    .question_for(&RequirementKey::ChartPlace)
            }),
            reason: "The device did not supply usable coordinates, and no reader location has been resolved.".into(),
        };
        session
            .method
            .flow
            .pending
            .retain(|pending| pending.stage != Stage::Place);
        session.method.flow.pending.push(step::PendingInput {
            stage: Stage::Place,
            request: step::InputRequest {
                field: "chart_place".into(),
                question: need.question.clone(),
                reason: need.reason.clone(),
            },
        });
        session.audit.push(json!({"event":"native_stage_wait","stage":"place","reason":"reader_location_unresolved"}));
        contract_question(session, &need);
        return Ok(None);
    }
    let place = place.ok_or("A chart needs its reader's place")?;
    if session.chart.is_none() {
        session.place = Some(place.clone());
    }
    session
        .method
        .flow
        .pending
        .retain(|pending| pending.stage != Stage::Place);
    let default_instant = if session.method.brief.time_request.is_empty() {
        session.candidate_moment_ms.unwrap_or(instant)
    } else {
        instant
    };
    let timestamp = match resolve_moment(
        &moment,
        session.chart.as_ref(),
        default_instant,
        &place.timezone,
    ) {
        Ok(t) => t,
        Err(error) => {
            session
                .audit
                .push(json!({"event":"moment_validation","error":error}));
            let need = contracts::Need {
                key: RequirementKey::ChartMoment,
                state: "invalid_civil_time".into(),
                question: "Which earlier date and local time should this question use?".into(),
                reason: error,
            };
            session
                .method
                .flow
                .pending
                .retain(|pending| pending.stage != Stage::Moment);
            session.method.flow.pending.push(step::PendingInput {
                stage: Stage::Moment,
                request: step::InputRequest {
                    field: "chart_moment".into(),
                    question: need.question.clone(),
                    reason: need.reason.clone(),
                },
            });
            contract_question(session, &need);
            return Ok(None);
        }
    };
    session
        .method
        .flow
        .pending
        .retain(|pending| pending.stage != Stage::Moment);
    if session.chart.is_none()
        || session
            .chart
            .as_ref()
            .is_some_and(|chart| chart["timestampMs"].as_f64() != Some(timestamp))
        || session
            .place
            .as_ref()
            .is_none_or(|old| old.latitude != place.latitude || old.longitude != place.longitude)
    {
        let chart = horary_ai_core::astronomy::chart(timestamp, place.latitude, place.longitude)?;
        if session.chart.is_some() {
            session.revisions.push(crate::conversation::Revision {
                number: session.revision,
                question: previous.question.clone(),
                chart: session.chart.clone(),
                sections: session.sections.clone(),
                place: session.place.clone(),
                brief: previous.method.brief.clone(),
                consultation: previous.method.consultation.clone(),
            });
        }
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or("Reading revision exhausted")?;
        session.chart = Some(chart);
        session.place = Some(place);
        session.chart_after_message = session.messages.len();
        session.sections.clear();
        session.status = "The sky at this question’s moment…".into();
        runtime.publish(session)?;
    }
    let anchor = contracts::Anchor {
        timestamp_ms: timestamp,
        latitude: session.place.as_ref().expect("Place installed").latitude,
        longitude: session.place.as_ref().expect("Place installed").longitude,
        timezone: session
            .place
            .as_ref()
            .expect("Place installed")
            .timezone
            .clone(),
    };
    let ready = match contracts::ReadyReading::prepare(
        session
            .method
            .consultation
            .as_ref()
            .expect("Consultation installed"),
        anchor,
    ) {
        Ok(ready) => ready,
        Err(plan) => {
            if let Some(need) = plan.needs.first() {
                contract_question(session, need);
            } else if let Some(limitation) = &plan.limitation {
                contract_limitation(session, limitation);
            }
            return Ok(None);
        }
    };
    let reading_request = ready.input();
    session.audit.push(
        json!({"event":"contract_handoff","binding":ready.binding(),"request":reading_request}),
    );
    // Readiness was checked against current sourced facts and a verified
    // anchor. A request from the previous turn is history, not a live need.
    if matches!(
        session.method.result,
        Some(
            contracts::ReadingResult::NeedsInformation { .. }
                | contracts::ReadingResult::Limited { .. }
        )
    ) {
        session.method.result = None;
    }
    // A semantic correction invalidates interpretation without replacing the
    // understood question's sky. Archived chart revisions are separate.
    if session.method.result.as_ref().is_some_and(|result|matches!(result,contracts::ReadingResult::Judgment{binding,..} if binding!=ready.binding())) {
        session.sections.clear();
        session.method.result=None;
    }
    Ok(Some(ready))
}

fn generate_reading(
    session: &mut Session,
    runtime: &impl Runtime,
    ready: crate::reading_contracts::ReadyReading,
) -> Result<(), String> {
    use crate::reading_contracts as contracts;
    let chart = session.chart.as_ref().ok_or("A chart is required")?.clone();
    let reading_request = ready.input();
    let reading_brief = reading_brief(&ready.brief());
    let all = reading_method::facts(Some(&chart));
    let houses: Vec<_> = all
        .iter()
        .filter(|f| f.kind == "house" || f.kind == "position")
        .cloned()
        .collect();
    let options = crate::horary_role_options::build_for(
        session
            .method
            .consultation
            .as_ref()
            .expect("Consultation installed"),
        session.method.brief.matter,
        &session.method.brief.people,
        &session.method.brief.subject,
    );
    let Some(assignment) = task(
        session,
        runtime,
        Stage::Significators,
        json!({"reading_request":reading_request,"brief":reading_brief,"house_rulers_and_positions":houses,"native_role_options":options}),
        &houses,
    )?
    else {
        return Ok(());
    };
    let choices = assignment.worksheet();
    let roles = assignment.roles().to_vec();
    section(
        session,
        Stage::Significators,
        choices,
        &houses,
        roles.clone(),
    );
    runtime.publish(session)?;
    let condition = relevant(&all, &roles, &["condition", "position", "boundary"]);
    let reception = relevant(&all, &roles, &["reception", "boundary"]);
    let events = relevant(&all, &roles, &["event", "moon", "boundary"]);
    let positions = relevant(&all, &roles, &["position", "boundary"]);
    let matter = session.method.brief.matter;
    let common = json!({"brief":reading_brief,"roles":roles});
    session.status = "Following feeling, possibility, and the paths between them…".into();
    runtime.publish(session)?;
    // Independent judgments are submitted together. The resident model owns
    // inference scheduling; it does not load duplicate copies of the weights.
    let mut tasks = vec![
        (
            Stage::Condition,
            json!({"reading_request":reading_request,"question":common,"facts":condition}),
            condition.clone(),
        ),
        (
            Stage::Reception,
            json!({"reading_request":reading_request,"question":common,"facts":reception}),
            reception.clone(),
        ),
        (
            Stage::Contacts,
            json!({"reading_request":reading_request,"brief":reading_brief,"roles":roles,"facts":events}),
            events.clone(),
        ),
    ];
    if matches!(matter, Matter::LostObject | Matter::LostAnimal) {
        tasks.push((
            Stage::Location,
            json!({"reading_request":reading_request,"brief":reading_brief,"roles":roles,"facts":positions}),
            positions.clone(),
        ));
    }
    let values = execute_batch(session, runtime, &tasks)?;
    for ((stage, _, facts), value) in tasks.iter().zip(&values) {
        if let Some(data) = value {
            section(session, *stage, data.worksheet(), facts, Vec::new());
        }
    }
    runtime.publish(session)?;
    if values.iter().any(Option::is_none) {
        ask_pending(session);
        return Ok(());
    }
    let c = values[0].as_ref().map(CheckedData::worksheet);
    let r = values[1].as_ref().map(CheckedData::worksheet);
    let contact = values[2].as_ref().map(CheckedData::worksheet);
    let location = values
        .get(3)
        .and_then(Option::as_ref)
        .map(CheckedData::worksheet);
    // Current ephemeris supplies brackets, not the applying planet's travel
    // to exact perfection. Skip speculative timing inference entirely.
    let timing = json!({"timing_status":"unestablished","reason":"The native calculation has not supplied travel to exact perfection. Astronomical hours are not symbolic calendar timing."});
    session
        .audit
        .push(json!({"event":"native_stage","stage":"timing","result":timing}));
    let facts = relevant(
        &all,
        &roles,
        &[
            "condition",
            "reception",
            "event",
            "position",
            "moon",
            "boundary",
        ],
    );
    let Some(judgment) = task(
        session,
        runtime,
        Stage::Judgment,
        json!({"reading_request":reading_request,"brief":reading_brief,"roles":roles,"condition":c,"reception":r,"contacts":contact,"location":location,"timing":timing,"facts":facts}),
        &facts,
    )?
    else {
        return Ok(());
    };
    ready.validate_current(
        session
            .method
            .consultation
            .as_ref()
            .expect("Consultation installed"),
    )?;
    let answer = judgment.worksheet();
    session.method.result = Some(contracts::ReadingResult::Judgment {
        binding: ready.binding().clone(),
        verdict: answer["verdict"].as_str().unwrap_or("unresolved").into(),
        answer: answer["answer"].as_str().unwrap_or("").into(),
        evidence: answer["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        worksheet: answer.clone(),
    });
    section(
        session,
        Stage::Judgment,
        judgment.worksheet(),
        &facts,
        Vec::new(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_city_resolution_keeps_quoted_provenance_and_never_changes_reader_anchor() {
        use crate::reading_contracts::{Consultation, Evidence, Field, Observation, Slot};
        let mut case = Consultation::default();
        case.facts.insert(
            Field::TargetPlace,
            Slot::Resolved {
                observation: Observation {
                    value: "Springfield".into(),
                    evidence: Evidence::User {
                        turn: 1,
                        quote: "Springfield".into(),
                    },
                },
            },
        );
        let device = LocationCandidate {
            id: "device".into(),
            label: "Woodbridge, VA".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        let mut session = Session {
            candidates: vec![device.clone()],
            place: Some(device),
            chart: Some(json!({"timestampMs":1791388800000.})),
            ..Default::default()
        };
        session.method.consultation = Some(case.clone());
        resolve_event_places(&mut session, &GeocodeState::default()).unwrap();
        let resolved = session
            .method
            .consultation
            .as_ref()
            .unwrap()
            .facts
            .get(&Field::TargetPlace)
            .unwrap();
        assert!(resolved.resolved().unwrap().contains("VA"));
        assert!(
            matches!(resolved,Slot::Resolved { observation:Observation { evidence:Evidence::NativePlace { original,.. },.. } }
            if matches!(original.as_ref(),Evidence::User { quote,.. } if quote=="Springfield"))
        );
        assert_eq!(session.place.as_ref().unwrap().label, "Woodbridge, VA");
        assert_eq!(
            session.chart.as_ref().unwrap()["timestampMs"],
            1791388800000.
        );
        let mut without_device = Session::default();
        without_device.method.consultation = Some(case);
        resolve_event_places(&mut without_device, &GeocodeState::default()).unwrap();
        assert!(
            matches!(
                without_device
                    .method
                    .consultation
                    .as_ref()
                    .unwrap()
                    .facts
                    .get(&Field::TargetPlace),
                Some(Slot::Proposed { .. })
            ),
            "Ambiguous city without proximity context must stay unresolved"
        );
    }
    #[test]
    fn recognition_sees_actual_native_clock_and_device_position() {
        let device = LocationCandidate {
            id: "device".into(),
            label: "Woodbridge, VA".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        let session = Session {
            candidates: vec![device],
            ..Default::default()
        };
        let instant = horary_ai_core::chart_input::resolve_chart_time(
            "2026-10-07T12:00",
            "America/New_York",
            "",
        )
        .unwrap();
        let input = intake_input(&session, false, instant);
        assert_eq!(input["device_place"]["name"], "Woodbridge");
        assert_eq!(input["current_local_clock"], "2026-10-07T12:00");
        assert_eq!(input["current_timezone"], "America/New_York");
    }
    #[test]
    fn nearby_springfield_uses_device_context_but_explicit_region_wins() {
        let device = LocationCandidate {
            id: "device".into(),
            label: "Woodbridge, VA".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        let geocode = GeocodeState::default();
        let query = Brief {
            place_request: "Springfield".into(),
            ..Default::default()
        };
        let (selected, candidates) =
            native_place(&query, std::slice::from_ref(&device), None, &geocode).unwrap();
        assert!(candidates.iter().any(|place| place.label.contains("VA")));
        assert!(selected.unwrap().label.contains("VA"));
        let explicit = Brief {
            place_request: "Springfield, Illinois".into(),
            ..Default::default()
        };
        let (selected, candidates) = native_place(&explicit, &[device], None, &geocode).unwrap();
        assert!(candidates.iter().all(|place| !place.label.contains("VA")));
        assert!(selected.is_none_or(|place| !place.label.contains("VA")));
        let (selected, _) = native_place(&query, &[], None, &geocode).unwrap();
        assert!(
            selected.is_none(),
            "No proximity inference without a usable device location"
        );
    }
    #[test]
    fn role_assignment_failure_is_a_repairable_validation_error() {
        let chart =
            json!({"houses":[{"number":1,"sign":"Cancer"},{"number":7,"sign":"Capricorn"}]});
        let facts = reading_method::facts(Some(&chart));
        let worksheet = json!({"roles":[{"label":"Querent","house":1,"natural":"Moon","reason":"The first house represents the person asking."}],"summary":"The querent is represented by the first house.","unknowns":[]});
        assert!(validate(Stage::Significators, &worksheet, &facts).is_err());
    }
    #[test]
    fn device_defaults_do_not_require_geocoding_or_invent_coordinates() {
        let device = LocationCandidate {
            id: "device".into(),
            label: "Here".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.65,
            longitude: -77.24,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        let geo = GeocodeState::default();
        assert_eq!(
            native_place(&Brief::default(), std::slice::from_ref(&device), None, &geo)
                .unwrap()
                .0
                .unwrap()
                .id,
            "device"
        );
        assert!(native_place(&Brief::default(), &[], None, &geo)
            .unwrap()
            .0
            .is_none());
        let explicit = Brief {
            place_request: "London, United Kingdom".into(),
            ..Default::default()
        };
        let (_, found) = native_place(&explicit, &[device], None, &geo).unwrap();
        assert!(found.iter().all(|p| p.provider != "device"));
    }
    #[test]
    fn fabricated_evidence_and_empty_answer_are_not_valid_worksheets() {
        let v = json!({"checks":{},"summary":"Fine","unknowns":[],"answer":"Yes","evidence":["invented"],"verdict":"likely_yes"});
        assert!(validate(Stage::Judgment, &v, &[]).is_err());
    }
    #[test]
    fn contact_sign_checks_cannot_change_native_event_conditions() {
        let chart = json!({"derived":{"eventSearch":{"events":[{"planet1":"Moon","planet2":"Venus","aspectName":"Sextile","withinCurrentSigns":false,"estimatedPerfectsWithinHours":123.}]}}});
        let facts = reading_method::facts(Some(&chart));
        let event = facts.iter().find(|f| f.kind == "event").unwrap();
        let checks = Stage::Contacts.checks().iter().map(|key|((*key).to_string(),json!({"state":"unestablished","evidence":[],"finding":"The contact needs contextual judgment."}))).collect::<serde_json::Map<_,_>>();
        let mut worksheet = json!({"checks":checks,"summary":"A candidate after a sign change.","unknowns":[],"basis":"direct_candidate","candidate_ids":[event.id],"candidate_signs":[{"id":event.id,"within_current_signs":true}]});
        assert!(validate(Stage::Contacts, &worksheet, &facts)
            .unwrap_err()
            .contains("sign-change"));
        worksheet["candidate_signs"][0]["within_current_signs"] = json!(false);
        validate(Stage::Contacts, &worksheet, &facts).unwrap();
        let valid = worksheet.clone();
        worksheet["candidate_ids"] = json!([]);
        worksheet["candidate_signs"] = json!([]);
        assert!(validate(Stage::Contacts, &worksheet, &facts).is_err());
        worksheet = valid;
        worksheet["basis"] = json!("no_candidate_covered");
        assert!(validate(Stage::Contacts, &worksheet, &facts).is_err());
        worksheet["basis"] = json!("direct_candidate");
        worksheet["candidate_ids"] = json!([event.id, event.id]);
        worksheet["candidate_signs"] = json!([
            worksheet["candidate_signs"][0].clone(),
            worksheet["candidate_signs"][0].clone()
        ]);
        assert!(validate(Stage::Contacts, &worksheet, &facts).is_err());
    }
    #[test]
    fn payment_role_coverage_preserves_all_three_native_receipt_routes_for_contacts() {
        use crate::reading_contracts::{
            Consultation, Evidence, Facet, Field, Frame, Method, Observation, Slot,
        };
        fn resolved<T>(value: T) -> Slot<T> {
            Slot::Resolved {
                observation: Observation {
                    value,
                    evidence: Evidence::Migration {
                        detail: "Authored payment-route inclusion regression, not a model result"
                            .into(),
                    },
                },
            }
        }
        let case = Consultation {
            frame: resolved(Frame {
                method: Method::Money,
                facet: Facet::Event,
            }),
            facts: std::collections::BTreeMap::from([
                (Field::PrincipalMode, resolved("self".into())),
                (Field::MoneySource, resolved("job".into())),
            ]),
            ..Default::default()
        };
        let subject = crate::horary_role_options::Subject {
            name: "salary".into(),
            kind: "money".into(),
            owner_id: "querent".into(),
            source_quote: "my salary".into(),
        };
        let options = crate::horary_role_options::build_for(&case, Matter::Money, &[], &subject);
        let events: Vec<_> = ["Jupiter", "Moon", "Saturn", "Mercury"]
            .iter()
            .map(|recipient| {
                json!({"planet1":recipient,"planet2":"Venus","aspectName":"Sextile",
            "withinCurrentSigns":true,"estimatedPerfectsWithinHours":0.5})
            })
            .collect();
        let chart = json!({"houses":[{"number":1,"sign":"Sagittarius","degree":0.0},{"number":2,"sign":"Capricorn","degree":0.0},
            {"number":11,"sign":"Libra","degree":0.0}],"derived":{"eventSearch":{"events":events}}});
        let facts = reading_method::facts(Some(&chart));
        let selections: Vec<_> = [
            "querent.self",
            "subject.primary",
            "money.recipient_pocket",
            "moon.contextual",
        ]
        .iter()
        .map(
            |id| json!({"id":id,"reason":"Preserve this distinct source-permitted receipt route."}),
        )
        .collect();
        let roles = crate::horary_role_options::resolve(&options,
            &json!({"selections":selections,"summary":"The amount is distinct from the arrival.","unknowns":[]}), &facts).unwrap();
        let retained = relevant(&facts, &roles, &["event"]);
        assert_eq!(
            retained.len(),
            3,
            "Unselected incidental Mercury testimony stays excluded"
        );
        for recipient in ["Jupiter", "Moon", "Saturn"] {
            assert!(
                retained
                    .iter()
                    .any(|fact| fact.planets == [recipient, "Venus"]),
                "The source-permitted {recipient} receipt route must reach the contacts worker"
            );
        }
    }
    #[test]
    fn job_event_role_coverage_keeps_native_moon_to_job_contact_candidates() {
        use crate::reading_contracts::{
            Consultation, Evidence, Facet, Frame, Method, Observation, Slot,
        };
        for method in [Method::NewJob, Method::ReturnToJob] {
            for facet in [Facet::Event, Facet::Timing] {
                let case = Consultation {
                    frame: Slot::Resolved {
                        observation: Observation {
                            value: Frame { method, facet },
                            evidence: Evidence::Migration {
                                detail: "Authored job contact coverage, not a predicted event."
                                    .into(),
                            },
                        },
                    },
                    ..Default::default()
                };
                let subject = crate::horary_role_options::Subject {
                    name: "The applicant's job".into(),
                    kind: "job".into(),
                    owner_id: "querent".into(),
                    source_quote: "my application or return to my former job".into(),
                };
                let options =
                    crate::horary_role_options::build_for(&case, Matter::Work, &[], &subject);
                let events: Vec<_> = ["Jupiter", "Moon", "Mars"]
                    .iter()
                    .map(|worker| {
                        json!({"planet1":worker,"planet2":"Venus","aspectName":"Sextile",
                            "withinCurrentSigns":true,"estimatedPerfectsWithinHours":0.5})
                    })
                    .collect();
                let chart = json!({"houses":[
                    {"number":1,"sign":"Sagittarius","degree":0.0},
                    {"number":10,"sign":"Libra","degree":0.0}
                ],"derived":{"eventSearch":{"events":events}}});
                let facts = reading_method::facts(Some(&chart));
                let mut worksheet = json!({"selections":[
                    {"id":"querent.self","reason":"The actual principal is the worker."},
                    {"id":"subject.primary","reason":"The worker's relevant job."}
                ],"summary":"These are contact candidates, not a guaranteed outcome or date.","unknowns":[]});
                assert!(
                    crate::horary_role_options::resolve(&options, &worksheet, &facts)
                        .unwrap_err()
                        .contains("Missing conditional role moon.contextual")
                );
                worksheet["selections"].as_array_mut().unwrap().push(json!({
                    "id":"moon.contextual","reason":"The unclaimed genuine-worker co-significator."
                }));
                let roles =
                    crate::horary_role_options::resolve(&options, &worksheet, &facts).unwrap();
                let retained = relevant(&facts, &roles, &["event"]);
                assert_eq!(retained.len(), 2, "Incidental Mars remains excluded");
                for worker in ["Jupiter", "Moon"] {
                    assert!(retained
                        .iter()
                        .any(|fact| fact.planets == [worker, "Venus"]));
                }
                let main_roles: Vec<_> = roles.into_iter().filter(|r| r.house.is_some()).collect();
                assert_eq!(
                    relevant(&facts, &main_roles, &["event"]).len(),
                    1,
                    "Omitting the contextual Moon is the original contact-filter loss"
                );
            }
        }
    }
    #[test]
    fn relationship_role_contract_has_no_lost_object_fields() {
        let contract = schema_for(Stage::Significators, Matter::Relationship, &[]);
        assert!(contract["properties"].get("object_candidates").is_none());
        assert!(contract["properties"].get("owner_house").is_none());
        assert!(
            schema_for(Stage::Significators, Matter::LostObject, &[])["properties"]
                .get("object_candidates")
                .is_some()
        );
    }
    #[test]
    fn scheduler_submits_contact_checks_with_condition_and_reception() {
        let requests = process_examples().unwrap();
        let examples = requests["examples"].as_array().unwrap();
        for stage in ["condition", "reception", "contacts"] {
            let task = examples.iter().find(|v| v["stage"] == stage).unwrap();
            assert_eq!(
                task["batchStages"],
                json!(["condition", "reception", "contacts"])
            );
        }
    }
    #[test]
    fn delayed_city_clarification_keeps_the_understood_question_moment() {
        let evidence = process_examples_with_place(false).unwrap();
        assert_eq!(evidence["chartMomentMs"], json!(1789387200000.));
        assert_eq!(
            evidence["canonicalQuestion"],
            "Will I get married in the next year?"
        );
        assert_eq!(evidence["horizon"], "within the next year");
        assert!(evidence["examples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["messages"][1]["content"]
                .as_str()
                .unwrap_or("")
                .contains("Woodbridge, Virginia, United States.")));
    }
    #[test]
    fn prompt_does_not_carry_unrelated_history_or_lessons() {
        let input = json!({"requested_question_moment":"2026-01-14 14:30"});
        let p = prompt(
            Stage::Moment,
            Matter::Work,
            &input,
            &schema(Stage::Moment, &[]),
        )
        .unwrap();
        assert!(p.contains("2026-01-14"));
        assert!(!p.contains("reception_triplicity"));
        assert!(!p.contains("Lord 7 signifies"));
    }
    #[test]
    fn native_moments_distinguish_receipt_existing_and_user_civil_time() {
        let old = json!({"timestampMs":42.});
        assert_eq!(
            resolve_moment(&json!({"mode":"now"}), None, 100., "America/New_York").unwrap(),
            100.
        );
        assert_eq!(
            resolve_moment(
                &json!({"mode":"keep"}),
                Some(&old),
                100.,
                "America/New_York"
            )
            .unwrap(),
            42.
        );
        let explicit = json!({"mode":"explicit","local_time":"2026-01-14T14:30","occurrence":""});
        let london = resolve_moment(&explicit, None, 100., "Europe/London").unwrap();
        let new_york = resolve_moment(&explicit, None, 100., "America/New_York").unwrap();
        assert_eq!(new_york - london, 5. * 60. * 60. * 1000.);
        assert!(resolve_moment(
            &json!({"mode":"explicit","local_time":"2026-03-08T02:30"}),
            None,
            100.,
            "America/New_York"
        )
        .is_err());
        assert!(resolve_moment(
            &json!({"mode":"explicit","local_time":"2026-11-01T01:30","occurrence":""}),
            None,
            100.,
            "America/New_York"
        )
        .is_err());
    }
    #[test]
    fn duplicate_or_unframed_junk_cannot_be_a_tool_worksheet() {
        assert!(decode_json(r#"{"mode":"now","mode":"explicit"}"#).is_err());
        assert!(decode_json("```json\n{\"mode\":\"now\"}\n```\nextra").is_err());
        assert_eq!(
            decode_json("```json\n{\"mode\":\"now\"}\n```").unwrap()["mode"],
            "now"
        );
    }
    #[test]
    fn actual_scheduler_shortcuts_defaults_and_delivers_the_answer() {
        let fixture = process_examples().unwrap();
        let calls = fixture["examples"].as_array().unwrap();
        assert!(!calls
            .iter()
            .any(|r| r["stage"] == "place" || r["stage"] == "moment"));
        assert!(calls.iter().any(|r| r["stage"] == "judgment"));
        assert!(fixture["visibleProposedAnswer"]
            .as_str()
            .unwrap()
            .contains("main document"));
    }
}

#[cfg(test)]
mod restoration_tests {
    use super::*;
    #[test]
    fn restoring_a_listed_revision_keeps_both_chart_histories() {
        let mut session = Session {
            revision: 2,
            question: "Corrected question".into(),
            chart: Some(json!({"timestampMs":20})),
            revisions: vec![crate::conversation::Revision {
                number: 1,
                question: "Earlier question".into(),
                chart: Some(json!({"timestampMs":10})),
                sections: Vec::new(),
                place: None,
                consultation: None,
                brief: Brief {
                    question: "Earlier question".into(),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        assert!(restore_revision(&mut session, 99).is_err());
        assert_eq!(session.revision, 2);
        restore_revision(&mut session, 1).unwrap();
        assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], 10);
        assert_eq!(session.question, "Earlier question");
        assert_eq!(session.revision, 3);
        assert!(session
            .revisions
            .iter()
            .any(|r| r.number == 2 && r.chart.as_ref().unwrap()["timestampMs"] == 20));
    }
}

#[cfg(all(test, feature = "native-llama"))]
mod real_reading {
    use super::*;
    use crate::native_llama_worker::{
        generate_native, generate_native_batch, start_native_llama_from_path, stop_native_llama,
        NativeGenerateOptions, NativeLlamaState,
    };
    use std::sync::Mutex;
    struct Reader {
        state: NativeLlamaState,
        dir: std::path::PathBuf,
        records: Mutex<Vec<Value>>,
        stop_before_batch: bool,
        deadline: Option<std::time::Instant>,
    }
    impl Runtime for Reader {
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            contract: &Value,
            audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            let prompt = prompt(stage, matter, input, contract)?;
            let result = generate_native(
                &self.state,
                prompt.clone(),
                NativeGenerateOptions {
                    max_tokens: if stage == Stage::Judgment { 1400 } else { 1000 },
                    temperature: 0.,
                    cache_lesson: audio.is_none(),
                    audio: audio.map(<[u8]>::to_vec),
                    response_schema: audio.is_none().then(|| contract.to_string()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.message);
            self.records.lock().unwrap().push(json!({"stage":stage,"prompt":serde_json::from_str::<Value>(&prompt).unwrap(),"result":result.as_ref().map_err(|e|e.as_str())}));
            std::fs::write(
                self.dir.join("calls.json"),
                serde_json::to_vec_pretty(&*self.records.lock().unwrap()).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            result
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            if self.stop_before_batch {
                return Err("Synthetic probe stops before testimony".into());
            }
            let prompts = tasks
                .iter()
                .map(|(stage, matter, input, contract)| {
                    prompt(*stage, *matter, input, contract).map(|p| (p, 1000))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let result = generate_native_batch(
                &self.state,
                prompts,
                NativeGenerateOptions {
                    temperature: 0.,
                    cache_lesson: true,
                    ..Default::default()
                },
            )
            .map_err(|e| e.message);
            self.records.lock().unwrap().push(json!({"batchStages":tasks.iter().map(|t|t.0).collect::<Vec<_>>(),"inputs":tasks,"result":result.as_ref().map_err(|e|e.as_str())}));
            std::fs::write(
                self.dir.join("calls.json"),
                serde_json::to_vec_pretty(&*self.records.lock().unwrap()).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            result
        }
        fn publish(&self, session: &mut Session) -> Result<(), String> {
            crate::reading_store::write(&self.dir.join("reading.json"), session, false)
        }
        fn check(&self) -> Result<(), String> {
            if self
                .deadline
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                Err("Synthetic probe deadline reached".into())
            } else {
                Ok(())
            }
        }
        fn directory(&self) -> &Path {
            &self.dir
        }
    }
    #[test]
    #[ignore = "Real Gemma conversational reframing and affirmative recognition; immutable evidence directory required"]
    fn real_guru_proposes_one_reframing_and_accepts_the_persons_yes() {
        use crate::reading_contracts::{self as contracts, Facet, Method, ReadingResult};
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let dir = std::path::PathBuf::from(
            std::env::var_os("HORARY_READING_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&dir).expect("Preserve every earlier probe");
        let reader = Reader {
            state: NativeLlamaState::default(),
            dir,
            records: Mutex::new(Vec::new()),
            stop_before_batch: true,
            deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(180)),
        };
        start_native_llama_from_path(
            &reader.state,
            "reframing-probe".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(json!({"modelId":"reframing-probe",
                "ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}))
            .unwrap(),
        )
        .unwrap();
        let question = "How many fish will Bob sell at the market on Friday?";
        let turn: contracts::Turn = serde_json::from_value(json!({
            "intent":"read","question":question,"frame":{"method":"movable_deal","facet":"quantity"},
            "people":[{"id":"bob","label":"Bob","relationship":"unknown","source_quote":"Bob"}],
            "subject":{"name":"fish","kind":"movable","owner_id":"","source_quote":"fish"},
            "updates":[{"field":"deal_capacity","value":"sell","quote":"sell","mode":"supply"},
                {"field":"seller","value":"bob","quote":"Bob","mode":"supply"},
                {"field":"event_time","value":"Friday","quote":"Friday","mode":"supply"}],
            "heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null
        })).unwrap();
        let mut case = contracts::Consultation::default();
        case.apply(&turn, 1, question, false).unwrap();
        let mut session = Session {
            question: question.into(),
            candidate_moment_ms: Some(1789387200000.),
            ..Session::default()
        };
        session.method.brief = contracts::brief(&case, &turn);
        session.method.result = Some(ReadingResult::Limited {
            limitation: case.plan(None).limitation.as_ref().unwrap().into(),
        });
        session.method.consultation = Some(case);
        session.messages.push(Message {
            role: "user".into(),
            text: question.into(),
        });
        session.candidates.push(LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        });
        crate::horary_conversation::respond(&mut session, &reader).unwrap();
        std::fs::write(reader.dir.join("proposal.json"), serde_json::to_vec_pretty(&json!({
            "authorship":"Synthetic checked intake; actual Gemma conversation output", "session":session
        })).unwrap()).unwrap();
        assert_eq!(
            session.question, question,
            "An invitation cannot change the question"
        );
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
        assert_eq!(
            session.method.records.last().unwrap().worksheet["ask"],
            "",
            "Offer the reframing before interrogating irrelevant missing-role inputs"
        );
        assert!(session.chart.is_none());
        let previous = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "Yes, that is what I want to know.".into(),
        });
        let result = run(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1789387260000.,
            None,
            &previous,
        );
        std::fs::write(reader.dir.join("accepted.json"), serde_json::to_vec_pretty(&json!({
            "authorship":"Actual Gemma recognition and reply to an authored affirmative turn", "result":result,"session":session
        })).unwrap()).unwrap();
        result.unwrap();
        let case = session.method.consultation.as_ref().unwrap();
        assert_eq!(case.method(), Some(Method::MovableDeal));
        assert!(matches!(
            case.frame.resolved().unwrap().facet,
            Facet::Profit | Facet::Situation | Facet::Event
        ));
        assert_ne!(session.question, question);
        assert!(session.question.contains("Bob") && session.question.contains("Friday"));
        assert_eq!(case.people["bob"].relationship, "unknown");
        assert_eq!(case.subject.resolved().unwrap().owner_id, "");
        assert_eq!(
            case.requested,
            Some(contracts::RequirementKey::PersonRelationship("bob".into()))
        );
        assert!(session.chart.is_none() && session.sections.is_empty());
        assert_eq!(session.candidate_moment_ms, Some(1789387200000.));
        assert_eq!(session.messages[0].text, question);
        assert!(reader
            .records
            .lock()
            .unwrap()
            .iter()
            .all(|r| matches!(r["stage"].as_str(), Some("conversation" | "intake"))));
        stop_native_llama(&reader.state).unwrap();
    }

    #[test]
    #[ignore = "Real Gemma elicitation and conversation; synthetic fish-sale case; immutable evidence directory required"]
    fn real_guru_keeps_the_count_question_and_converses_with_device_defaults() {
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let dir = std::path::PathBuf::from(
            std::env::var_os("HORARY_READING_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&dir).expect("Preserve every earlier probe");
        let reader = Reader {
            state: NativeLlamaState::default(),
            dir,
            records: Mutex::new(Vec::new()),
            stop_before_batch: true,
            deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(240)),
        };
        start_native_llama_from_path(
            &reader.state,
            "guru-probe".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(json!({"modelId":"guru-probe",
                "ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}))
            .unwrap(),
        )
        .unwrap();
        let mut session = Session::default();
        session.candidates.push(LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        });
        let question = "How many fish will Bob sell at the market in Bozeman, Montana, on Friday?";
        for (index, words) in [
            question,
            "Bob is my husband. They are his fish.",
            "Do you need both locations, mine and the market's?",
        ]
        .into_iter()
        .enumerate()
        {
            let previous = session.clone();
            session.messages.push(Message {
                role: "user".into(),
                text: words.into(),
            });
            let result = run(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387200000. + index as f64 * 60000.,
                None,
                &previous,
            );
            std::fs::write(reader.dir.join(format!("turn-{index}.json")),serde_json::to_vec_pretty(&json!({
                "authorship":"Actual Gemma outputs over authored synthetic inputs", "result":result,"session":session
            })).unwrap()).unwrap();
            result.unwrap();
            let case = session.method.consultation.as_ref().unwrap();
            assert_eq!(
                case.frame.resolved().unwrap().facet,
                crate::reading_contracts::Facet::Quantity
            );
            assert_eq!(
                case.method(),
                Some(crate::reading_contracts::Method::MovableDeal)
            );
            assert_eq!(session.question, question);
            assert!(case
                .text(crate::reading_contracts::Field::EventPlace)
                .unwrap()
                .contains("Bozeman"));
            assert!(case
                .text(crate::reading_contracts::Field::ReaderPlace)
                .is_none());
            assert!(
                session.sections.is_empty(),
                "Unsupported counts must not enter a substitute event judgment"
            );
            let response = session.messages.last().unwrap();
            assert_eq!(response.role, "assistant");
            let record = session.method.records.last().unwrap();
            assert_eq!(record.stage, Stage::Conversation);
            assert_eq!(response.text, record.worksheet["reply"].as_str().unwrap());
            let lower = response.text.to_lowercase();
            assert!(
                !lower.contains("catalogue")
                    && !lower.contains("eileen")
                    && !lower.contains("for review")
            );
            assert!(!lower.contains("which city") && !lower.contains("where are you asking"));
            assert!(case
                .text(crate::reading_contracts::Field::Baseline)
                .is_none());
            if index == 0 {
                assert!(
                    case.subject.resolved().unwrap().owner_id.is_empty(),
                    "Selling does not establish ownership"
                );
            }
            if index >= 1 {
                assert_eq!(case.people["bob"].relationship, "partner");
            }
            eprintln!("GURU turn={index}: {}", response.text);
        }
        stop_native_llama(&reader.state).unwrap();
    }

    #[test]
    #[ignore = "Actual full Gemma reading over synthetic question/device data; preserves failures in HORARY_READING_EVIDENCE"]
    fn real_reader_reaches_an_interpreted_answer_with_device_defaults() {
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let dir = std::path::PathBuf::from(
            std::env::var_os("HORARY_READING_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&dir).expect("Preserve previous attempts");
        let mut reader = Reader {
            state: NativeLlamaState::default(),
            dir,
            records: Mutex::new(Vec::new()),
            stop_before_batch: false,
            deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(300)),
        };
        start_native_llama_from_path(
            &reader.state,
            "reading-probe".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(
                json!({"modelId":"reading-probe","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let question =
            "I'm single, and there's no arranged wedding. Will I get married in the next year?";
        let seed = Session {
            reading_id: "synthetic-reading".into(),
            messages: vec![Message {
                role: "user".into(),
                text: question.into(),
            }],
            ..Default::default()
        };
        let device = LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        // Keep the SAME native owner across both complete readings. A restarted
        // owner cannot establish a warm lesson-bank result.
        for phase in ["cold", "warm"] {
            reader.deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(300));
            let mut session = seed.clone();
            session.candidates.push(device.clone());
            let previous = session.clone();
            let start = std::time::Instant::now();
            let result = run(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387200000.,
                None,
                &previous,
            );
            std::fs::write(reader.dir.join(format!("completion-{phase}.json")),serde_json::to_vec_pretty(&json!({"authorship":"Actual Gemma generation over explicitly synthetic device/question/moment; not Eileen's reading","phase":phase,"result":result,"wallMs":start.elapsed().as_millis(),"session":session})).unwrap()).unwrap();
            if result.is_err() {
                stop_native_llama(&reader.state).unwrap();
            }
            result.unwrap();
            assert!(session.chart.is_some());
            assert_eq!(session.place.as_ref().unwrap().id, "device-location");
            assert_eq!(session.question, question);
            assert!(!session
                .method
                .records
                .iter()
                .any(|r| matches!(r.stage, Stage::Place | Stage::Moment)));
            let answer = session
                .sections
                .iter()
                .find(|s| s.step == Some(Step::Judgment))
                .expect("The person must receive an interpretation");
            assert!(answer.body.len() > 70);
            assert_eq!(answer.body, answer.worksheet["answer"].as_str().unwrap());
            if phase == "warm" {
                assert!(session
                    .method
                    .records
                    .iter()
                    .all(|r| r.generation.cached_prompt_tokens > 0));
            }
            eprintln!(
                "FULL READING {phase}: wall_ms={} stages={} answer={}",
                start.elapsed().as_millis(),
                session.method.records.len(),
                answer.body
            );
        }
        stop_native_llama(&reader.state).unwrap();
    }

    #[test]
    #[ignore = "Real multi-turn Gemma recovery probe, no audio; preserves every output in a new HORARY_READING_EVIDENCE directory"]
    fn real_reader_preserves_fair_context_and_waits_for_required_roles() {
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let dir = std::path::PathBuf::from(
            std::env::var_os("HORARY_READING_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&dir).expect("Preserve previous attempts");
        let reader = Reader {
            state: NativeLlamaState::default(),
            dir,
            records: Mutex::new(Vec::new()),
            stop_before_batch: true,
            deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(300)),
        };
        start_native_llama_from_path(
            &reader.state,
            "fair-recovery-probe".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(
                json!({"modelId":"fair-recovery-probe","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let mut session = Session::default();
        let geocode = GeocodeState::default();
        let question = "Will Bob sell his books at the fair?";
        let turns=[question,"The fair is in Bozeman, Montana.","I'm asking from Woodbridge, Virginia, United States.","How did you know what time to cast the chart? Don't you need to know when the fair is?","OK, the fair is tomorrow at 3 o'clock.","Cast the chart.","Bob is my husband. They are his books."];
        let mut observations = Vec::new();
        for (index, words) in turns.into_iter().enumerate() {
            let previous = session.clone();
            session.messages.push(Message {
                role: "user".into(),
                text: words.into(),
            });
            let result = run(
                &mut session,
                &reader,
                &geocode,
                1789387200000. + index as f64 * 60000.,
                None,
                &previous,
            );
            observations.push(json!({"turn":index,"words":words,"result":result,"brief":session.method.brief,"chartMoment":session.chart.as_ref().map(|chart|&chart["timestampMs"]),"place":session.place,"sections":session.sections.iter().map(|section|section.method_stage).collect::<Vec<_>>(),"pending":session.method.flow.pending}));
            std::fs::write(
                reader.dir.join("observations.json"),
                serde_json::to_vec_pretty(&observations).unwrap(),
            )
            .unwrap();
            if index < 6 {
                result.unwrap();
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    "Synthetic probe stops before testimony"
                );
            }
            assert_eq!(
                session.question, question,
                "The original deal question must survive every turn"
            );
            if index == 1 {
                assert!(
                    session.chart.is_none(),
                    "An event venue cannot stand in for an unknown reader place"
                );
                assert!(session.method.brief.place_request.is_empty());
                assert!(session
                    .method
                    .brief
                    .event_place
                    .to_lowercase()
                    .contains("bozeman"));
            }
            if index == 6 {
                assert_eq!(
                    session.chart.as_ref().unwrap()["timestampMs"],
                    1789387200000.
                );
                assert!(session.place.as_ref().unwrap().name.contains("Woodbridge"));
            }
            if matches!(index, 2 | 4 | 5) {
                assert!(
                    session.sections.is_empty(),
                    "Roles cannot complete before the missing relationship is supplied"
                );
                assert!(session
                    .method
                    .consultation
                    .as_ref()
                    .unwrap()
                    .plan(None)
                    .needs
                    .iter()
                    .any(|need| matches!(
                        need.key,
                        crate::reading_contracts::RequirementKey::Owner
                            | crate::reading_contracts::RequirementKey::PersonRelationship(_)
                            | crate::reading_contracts::RequirementKey::Field(
                                crate::reading_contracts::Field::Seller
                            )
                    )));
            }
            if index == 3 {
                let response = session.messages.last().unwrap().text.to_lowercase();
                assert!(!response.contains("no chart") && !response.contains("provide the chart"));
            }
            if index >= 4 {
                assert!(
                    session.method.brief.time_request.is_empty(),
                    "The fair's time is not the question moment"
                );
                assert!(session.method.brief.event_time.contains('3'));
            }
            if index == 6 {
                let roles = &session
                    .sections
                    .iter()
                    .find(|section| section.method_stage == Some(Stage::Significators))
                    .unwrap()
                    .roles;
                assert!(
                    roles.iter().any(|role| role.house == Some(7)),
                    "The explicitly supplied husband must have a seventh-house role"
                );
                assert!(
                    roles.iter().any(|role| role.house == Some(8)),
                    "His movable stock is his second, absolute eighth"
                );
                assert!(!roles.iter().any(
                    |role| role.label.to_lowercase().contains("book") && role.house == Some(3)
                ));
            }
        }
        stop_native_llama(&reader.state).unwrap();
    }
}

/// An authored fixture drives the real scheduler and captures its exact inputs.
/// It deliberately supplies no model judgment or qualification claim.
#[cfg(test)]
pub(crate) fn process_examples() -> Result<Value, String> {
    process_examples_with_place(true)
}

#[cfg(test)]
fn process_examples_with_place(device_available: bool) -> Result<Value, String> {
    use std::sync::Mutex;
    struct Fixture {
        dir: tempfile::TempDir,
        requests: Mutex<Vec<Value>>,
    }
    impl Runtime for Fixture {
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            contract: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            self.requests.lock().unwrap().push(json!({"stage":stage,"messages":serde_json::from_str::<Value>(&prompt(stage,matter,input,contract)?).map_err(|e|e.to_string())?,"responseSchema":contract}));
            let answer = match stage {
                Stage::Conversation => {
                    json!({"reply":"Authored conversational fixture, not a model response.",
                    "ask":input["reminders"].as_array().and_then(|r|r.first()).map_or("",|n|n["id"].as_str().unwrap_or(""))})
                }
                Stage::Intake => {
                    if crate::horary_step::original_input(input)["recognition_phase"]
                        == "complete_selected_program"
                    {
                        json!({"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"Prospective partner","kind":"person","owner_id":"","source_quote":"Will I get married in the next year?"},"updates":[{"field":"baseline","value":"hoped_for","quote":"Will I get married in the next year?","mode":"supply"},{"field":"horizon","value":"within the next year","quote":"in the next year","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
                    } else if input["latest_words"] == "Woodbridge, Virginia, United States." {
                        json!({"intent":"clarify","question":null,"frame":null,"people":[],"subject":null,"updates":[{"field":"reader_place","value":"Woodbridge, Virginia, United States.","quote":"Woodbridge, Virginia, United States.","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
                    } else {
                        json!({"intent":"read","question":"Will I get married in the next year?","frame":{"method":"relationship","facet":"event"},"people":[],"subject":null,"updates":[],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
                    }
                }
                Stage::Significators => {
                    json!({"selections":[{"id":"querent.self","reason":"The first house represents the person asking."},{"id":"subject.primary","reason":"The seventh house represents a prospective partner without inventing an existing person."},{"id":"moon.contextual","reason":"The Moon has a contextual role in the question."}],"summary":"Authored fixture: identifies roles to inspect subsequent requests.","unknowns":[]})
                }
                Stage::Place => {
                    let place = input["candidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|p| {
                            p["latitude"]
                                .as_f64()
                                .is_some_and(|n| (38.0..39.0).contains(&n))
                        })
                        .ok_or("The authored Virginia fixture needs a real candidate")?;
                    json!({"mode":"select","place_id":place["id"],"query":"","clarification":"","basis":"Authored fixture selects the real Virginia candidate, not invented coordinates."})
                }
                _ => {
                    let mut checks = serde_json::Map::new();
                    for key in stage.checks() {
                        checks.insert((*key).into(),json!({"state":"unestablished","evidence":[],"finding":"Authored fixture, not a model assessment."}));
                    }
                    let mut answer = json!({"checks":checks,"summary":"Authored fixture; no astrology interpretation is asserted.","unknowns":[]});
                    if stage == Stage::Condition {
                        // The source-export fixture satisfies native coverage
                        // without inventing an interpretation of any fact.
                        let original = crate::horary_step::original_input(input);
                        for (facet, key) in [
                            ("essential", "own_dignity"),
                            ("house_capacity", "ability_to_act"),
                        ] {
                            let ids: Vec<_> = original["facts"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|fact| fact["condition_facet"] == facet)
                                .map(|fact| fact["id"].clone())
                                .collect();
                            answer["checks"][key]["evidence"] = json!(ids);
                        }
                    }
                    if stage == Stage::Contacts {
                        answer["basis"] = json!("no_candidate_covered");
                        answer["candidate_ids"] = json!([]);
                        answer["candidate_signs"] = json!([]);
                    }
                    if stage == Stage::Judgment {
                        answer["verdict"] = json!("unresolved");
                        answer["answer"]=json!("This is authored fixture text showing that the proposed answer reaches the main document; it is not a model judgment.");
                        answer["evidence"] = json!([]);
                        answer["checks"]["scope_of_answer"]["evidence"] = json!([input["facts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|f| f["kind"] == "boundary")
                            .ok_or("Fixture boundary missing")?["id"]]);
                    }
                    answer
                }
            };
            Ok(NativeGenerationResult {
                content: answer.to_string(),
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
        fn publish(&self, _session: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            let results = tasks
                .iter()
                .map(|(stage, matter, input, contract)| {
                    self.generate(*stage, *matter, input, contract, None)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let stages: Vec<_> = tasks.iter().map(|t| t.0).collect();
            let mut requests = self.requests.lock().map_err(|e| e.to_string())?;
            let start = requests
                .len()
                .checked_sub(tasks.len())
                .ok_or("Missing batch fixture requests")?;
            for request in &mut requests[start..] {
                request["batchStages"] = json!(stages);
            }
            Ok(results)
        }
        fn check(&self) -> Result<(), String> {
            if self.requests.lock().map_err(|e| e.to_string())?.len() > 32 {
                Err("Authored documentation fixture rejected repeatedly; inspect the actual acceptance path.".into())
            } else {
                Ok(())
            }
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    let runtime = Fixture {
        dir: tempfile::tempdir().map_err(|e| e.to_string())?,
        requests: Mutex::new(Vec::new()),
    };
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: "Will I get married in the next year?".into(),
    });
    if device_available {
        session.candidates.push(LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        });
    }
    let previous = session.clone();
    run(
        &mut session,
        &runtime,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )?;
    if !device_available {
        let previous = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "Woodbridge, Virginia, United States.".into(),
        });
        run(
            &mut session,
            &runtime,
            &GeocodeState::default(),
            1789387800000.,
            None,
            &previous,
        )?;
    }
    let mut requests = runtime.requests.into_inner().map_err(|e| e.to_string())?;
    requests.sort_by_key(|r| {
        Stage::ALL
            .iter()
            .position(|s| serde_json::to_value(s).unwrap() == r["stage"])
    });
    Ok(
        json!({"authorship":"Synthetic inputs and authored worksheet outputs, captured from the actual runtime scheduler; no model invoked.","examples":requests,"visibleProposedAnswer":session.sections.last().map(|s|&s.body),"chartMomentMs":session.chart.as_ref().map(|c|&c["timestampMs"]),"canonicalQuestion":session.question,"horizon":session.method.brief.horizon,"nativeDefaults":{"place":if device_available{"device coordinates"}else{"geocoded stated city"},"moment":"1789387200000, understood question receipt instant","noPlaceOrMomentModelCall":true}}),
    )
}
