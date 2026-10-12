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
    /// The guide identity used by dispatch, records and native data reuse.
    /// An evaluator's explicit teaching must resolve through this same boundary.
    fn guide_for(&self, stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
        crate::horary_contract::guide_for(stage, matter, input)
    }
    /// An offline/hosted evaluator may stop at native acceptance of the initial
    /// private extractor. This hook is absent from production builds.
    #[cfg(test)]
    fn initial_extractor_target(&self) -> Option<crate::reading_contracts::Method> {
        None
    }
    #[cfg(test)]
    fn initial_extractor_sequence(&self) -> Option<u64> {
        None
    }
    #[cfg(test)]
    fn supplying_extractor_target(&self) -> Option<crate::reading_contracts::Method> {
        None
    }
    #[cfg(test)]
    fn supplying_extractor_sequence(&self) -> Option<u64> {
        None
    }
    /// Measurement-only stop at a real accepted-input reading stage. No
    /// production caller or full-journey campaign enables this test hook.
    #[cfg(test)]
    fn selected_stage_target(&self) -> Option<Stage> {
        None
    }
    #[cfg(test)]
    fn selected_stage_sequence(&self) -> Option<u64> {
        None
    }
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
    if is_chart_request(words) {
        return true;
    }
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

/// An explicit calculation request is separate from permission to interpret.
/// Match whole commands so questions about casting, refusals and quoted plans
/// cannot start a calculation accidentally.
fn is_chart_request(words: &str) -> bool {
    let normalized = words
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_lowercase();
    let mut command = normalized.as_str();
    loop {
        let next = [
            "ok ",
            "okay ",
            "please ",
            "can you ",
            "could you ",
            "would you ",
        ]
        .iter()
        .find_map(|prefix| command.strip_prefix(prefix));
        match next {
            Some(next) => command = next,
            None => break,
        }
    }
    command = command.strip_suffix(" now").unwrap_or(command);
    command = command.strip_suffix(" please").unwrap_or(command);
    matches!(
        command,
        "cast the chart" | "cast a chart" | "run the chart" | "run a chart"
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

fn record_event_dates(session: &mut Session, normalizations: &[Value]) -> Result<(), String> {
    if normalizations.is_empty() {
        return Ok(());
    }
    let source_sha256 = normalizations[0]["source_input_sha256"]
        .as_str()
        .ok_or("A native event date lacks its original task binding.")?;
    let worksheet_sha256 = normalizations[0]["worksheet_sha256"]
        .as_str()
        .ok_or("A native event date lacks its raw proposal binding.")?;
    let (record_index, record) = session
        .method
        .records
        .iter()
        .enumerate()
        .rev()
        .find(|(_, record)| {
            record.stage == Stage::Intake
                && record.revision == session.revision
                && record.validation_error.is_none()
                && lessons::digest(&step::original_input(&record.input).to_string())
                    == source_sha256
                && lessons::digest(&record.worksheet.to_string()) == worksheet_sha256
        })
        .ok_or("A native event date lacks its accepted recognition record.")?;
    session.audit.push(
        json!({"event":"native_event_date_normalization","version":1,
        "turn":session.messages.len(),"record_index":record_index,
        "revision":record.revision,"record_input_sha256":record.input_sha256,
        "normalizations":normalizations}),
    );
    Ok(())
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
        facts.push(Fact { id:"chart.moment".into(),kind:"chart_context".into(),label:"The chart's actual moment".into(),detail:format!("The existing chart uses {} in {} (instant {}). This is the question's recorded moment, not the reported event date.",native_context["local_civil_time"],native_context["timezone"],native_context["timestamp_ms"]),planets:Vec::new(),condition_facet:None,event:None,reception:None });
        facts.push(Fact { id:"chart.place".into(),kind:"chart_context".into(),label:"The chart's actual place".into(),detail:format!("The existing chart was cast for {}. A place mentioned as the event venue is not confirmation that the reader is there.",native_context["reader_place"]["label"]),planets:Vec::new(),condition_facet:None,event:None,reception:None });
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

/// Intervening planets need evidence without becoming mandatory subject roles.
/// Preserve the supplied contact graph, including two intermediary contacts,
/// so prohibition, translation and collection can be judged in event order.
/// Moon seeds contextual evidence; it is not thereby assigned to the principal.
fn contact_evidence(facts: &[Fact], roles: &[Role]) -> Vec<Fact> {
    let mut planets: std::collections::BTreeSet<&str> =
        roles.iter().map(|role| role.planet.as_str()).collect();
    planets.insert("Moon");
    loop {
        let before = planets.len();
        for fact in facts.iter().filter(|fact| fact.kind == "event") {
            if fact
                .planets
                .iter()
                .any(|planet| planets.contains(planet.as_str()))
            {
                planets.extend(fact.planets.iter().map(String::as_str));
            }
        }
        if planets.len() == before {
            break;
        }
    }
    facts
        .iter()
        .filter(|fact| {
            matches!(fact.kind.as_str(), "house" | "boundary")
                || matches!(
                    fact.kind.as_str(),
                    "event" | "moon" | "position" | "condition" | "reception"
                ) && fact
                    .planets
                    .iter()
                    .all(|planet| planets.contains(planet.as_str()))
        })
        .cloned()
        .collect()
}

/// Contacts selects possible occasions independently of motive and ability.
/// Keep its complete mechanical graph, but leave dignity and reception to
/// their specialists. Judgment still receives the full contextual graph.
fn contact_mechanics(context: &[Fact]) -> Vec<Fact> {
    context
        .iter()
        .filter(|fact| {
            matches!(
                fact.kind.as_str(),
                "event" | "moon" | "position" | "house" | "boundary"
            )
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
    let source_passages = lessons::source_passages(
        stage,
        session.method.brief.matter,
        session
            .method
            .consultation
            .as_ref()
            .and_then(crate::reading_contracts::Consultation::method),
    );
    let rules = lessons::passages()
        .ok()
        .into_iter()
        .flatten()
        .filter(|p| source_passages.contains(&p.id.as_str()))
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
    let result =
        run_inner(session, runtime, geocode, instant, audio, previous).and_then(|answer| {
            let speak = session.method.brief.intent != "pause";
            #[cfg(test)]
            let speak = speak
                && !(runtime.selected_stage_target().is_some()
                    && session.audit.iter().any(|event| {
                        event["event"] == "selected_stage_boundary"
                            || event["event"] == "selected_stage_uninvoked"
                    }));
            if speak {
                if let Some(record_index) = answer {
                    crate::horary_conversation::deliver_fresh_answer(
                        session,
                        runtime,
                        record_index,
                    )?;
                } else {
                    crate::horary_conversation::respond(session, runtime)?;
                }
            }
            Ok(())
        });
    if let Err(error) = &result {
        crate::horary_executor::pause_after_error(session, error.clone());
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
) -> Result<Option<usize>, String> {
    if let Some(ready) = prepare_reading(session, runtime, geocode, instant, audio, previous)? {
        return generate_reading(session, runtime, ready);
    }
    Ok(None)
}

/// Exercise production input preparation and actual inquiries. At ReadyReading,
/// stop explicitly: production continues into interpretation before the guru.
/// A skipped interpretation must not elicit a fabricated closing conversation.
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
        prepare_reading(session, runtime, geocode, instant, audio, previous).and_then(|ready| {
            if let Some(ready) = ready {
                session
                    .audit
                    .push(json!({"event":"input_evaluation_boundary",
                    "boundary":"ready_reading","binding":ready.binding(),
                    "after_message":session.messages.len(),
                    "reading_executed":false,"conversation_executed":false}));
                runtime.publish(session)?;
            } else if runtime.initial_extractor_target().is_none()
                && !session
                    .audit
                    .iter()
                    .any(|event| event["event"] == "supplying_extractor_boundary")
                && session.method.brief.intent != "pause"
            {
                crate::horary_conversation::respond(session, runtime)?;
            }
            Ok(())
        });
    if let Err(error) = &result {
        crate::horary_executor::pause_after_error(session, error.clone());
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
    let migration = session
        .method
        .consultation
        .as_mut()
        .and_then(contracts::upgrade_catalogue);
    if let Some(receipt) = &migration {
        session
            .audit
            .push(json!({"event":"catalogue_migration","receipt":receipt}));
    }
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
        && migration.is_none()
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
            Ok(Some((contracts::control(Intent::Resume), Vec::new())))
        } else {
            execute(session, runtime, Stage::Intake, input, &[], audio).and_then(|data| {
                data.map(|data| {
                    data.turn()
                        .cloned()
                        .ok_or_else(|| "Recognition completion lacks its checked fact patch".into())
                        .map(|turn| (turn, data.calendar_normalizations().to_vec()))
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
    let Some((mut turn, calendar_normalizations)) = recognition? else {
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
    record_event_dates(session, &calendar_normalizations)?;
    #[cfg(test)]
    if let Some(target) = runtime.supplying_extractor_target() {
        let record_index = session.method.records.len().checked_sub(1);
        if let Some(record_index) = record_index {
            let record = &session.method.records[record_index];
            let original = crate::horary_step::original_input(&record.input);
            let signature = horary_prompt_program::signature("intake", original);
            if signature.recognition_phase == Some("update_selected_program")
                && signature.method == Some(target.name())
                && previous
                    .method
                    .consultation
                    .as_ref()
                    .is_some_and(|c| c.requested.is_some())
            {
                let job = session.method.flow.jobs.iter().find(|job| {
                    matches!(job.phase(), step::Phase::Complete { record_index: index } if *index == record_index)
                }).ok_or("Supplying extractor acceptance lacks its completed native job")?;
                session.audit.push(json!({"event":"supplying_extractor_boundary","version":1,
                    "target_method":target,"recognition_phase":"update_selected_program",
                    "after_message":session.messages.len(),"record_index":record_index,
                    "last_sequence":runtime.supplying_extractor_sequence().ok_or("Supplying extractor observation lacks its actual sequence")?,
                    "revision":session.revision,"record_input_sha256":record.input_sha256,
                    "native_job":job,"before_consultation":previous.method.consultation,
                    "bound_requirement":previous.method.consultation.as_ref().and_then(|c|c.requested.as_ref()),
                    "accepted_consultation":session.method.consultation,
                    "later_chart_executed":false,"later_conversation_executed":false}));
                runtime.publish(session)?;
                return Ok(None);
            }
        }
    }
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
    #[cfg(test)]
    if runtime
        .initial_extractor_target()
        .is_some_and(|target| selected != Some(target))
    {
        // A different/unresolved classification is not permission to force the
        // target lesson, fabricate a negative, or pay for unrelated actors.
        return Ok(None);
    }
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
        record_event_dates(session, data.calendar_normalizations())?;
        #[cfg(test)]
        if let Some(target) = runtime.initial_extractor_target() {
            let record_index = session
                .method
                .records
                .len()
                .checked_sub(1)
                .ok_or("Initial extractor acceptance lacks its native record")?;
            let record = &session.method.records[record_index];
            let job = session.method.flow.jobs.iter().find(|job| {
                matches!(job.phase(), step::Phase::Complete { record_index: index } if *index == record_index)
            }).ok_or("Initial extractor acceptance lacks its completed native job")?;
            session.audit.push(json!({"event":"initial_extractor_boundary","version":1,
                "target_method":target,"recognition_phase":"complete_selected_program",
                "after_message":session.messages.len(),"record_index":record_index,
                "last_sequence":runtime.initial_extractor_sequence().ok_or("Initial extractor observation lacks its actual call sequence")?,
                "revision":session.revision,"record_input_sha256":record.input_sha256,
                "native_job":job,"accepted_consultation":session.method.consultation,
                "chart_executed":false,"conversation_executed":false,"supplying_executed":false}));
            runtime.publish(session)?;
            return Ok(None);
        }
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
    let chart_only_limit = is_chart_request(&words)
        && session
            .method
            .consultation
            .as_ref()
            .is_some_and(|case| case.understood())
        && plan.limitation.is_some();
    if !chart_only_limit
        && plan
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
        (!chart_only_limit
            || matches!(
                need.key,
                RequirementKey::ChartPlace
                    | RequirementKey::ChartMoment
                    | RequirementKey::Field(
                        contracts::Field::ReaderPlace | contracts::Field::QuestionTime
                    )
            ))
            && (need.key != RequirementKey::ChartPlace
                || need.state != "native_acquisition_pending")
    }) {
        contract_question(session, need);
        return Ok(None);
    }
    if let Some(limitation) = plan.limitation.as_ref().filter(|_| !chart_only_limit) {
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
    if chart_only_limit {
        let limitation = plan.limitation.as_ref().expect("Explicit chart-only limit");
        session.audit.push(json!({"event":"explicit_chart_calculation",
            "chart_exists":true,"interpretation_executed":false,"limitation":limitation,
            "basis":"The person explicitly requested the chart. Calculation does not authorize an unsupported interpretation."}));
        contract_limitation(session, limitation);
        return Ok(None);
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

#[derive(Serialize)]
struct AcceptedReceptionRole {
    role_index: usize,
    selected_id: String,
    label: String,
    house: Option<u8>,
    planet: String,
}

#[derive(Serialize)]
struct ReceptionRoleEdge<'a> {
    fact_id: &'a str,
    guest_role_indices: Vec<usize>,
    host_role_indices: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guest_roles: Option<Vec<&'a AcceptedReceptionRole>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_roles: Option<Vec<&'a AcceptedReceptionRole>>,
}

/// Selection IDs and role order come from the same native acceptance permit.
/// Labels never become IDs, and a shared planet never loses one of its roles.
fn accepted_reception_roles(data: &CheckedData) -> Result<Vec<AcceptedReceptionRole>, String> {
    let selections = data.worksheet()["selections"]
        .as_array()
        .ok_or("Accepted roles require their native selection worksheet")?;
    if selections.len() != data.roles().len() {
        return Err("Accepted native selections and derived roles must remain paired".into());
    }
    selections
        .iter()
        .zip(data.roles())
        .enumerate()
        .map(|(role_index, (selection, role))| {
            Ok(AcceptedReceptionRole {
                role_index,
                selected_id: selection["id"]
                    .as_str()
                    .ok_or("An accepted role requires its native selection ID")?
                    .into(),
                label: role.label.clone(),
                house: role.house,
                planet: role.planet.clone(),
            })
        })
        .collect()
}

fn reception_clipboard(roles: &[AcceptedReceptionRole], facts: &[Fact]) -> Value {
    reception_clipboard_view(roles, facts, true)
}

fn judgment_reception_clipboard(roles: &[AcceptedReceptionRole], facts: &[Fact]) -> Value {
    reception_clipboard_view(roles, facts, false)
}

fn native_contact_role_reference(
    case: &crate::reading_contracts::Consultation,
    request: &Value,
    roles: &[AcceptedReceptionRole],
    facts: &[Fact],
) -> Option<Value> {
    use crate::reading_contracts::{Evidence, Field, Method};
    if case.method() != Some(Method::Contact) {
        return None;
    }
    let mut accepted = Vec::new();
    for field in [Field::PrincipalMode, Field::PrincipalId] {
        if let Some(crate::reading_contracts::Slot::Resolved { observation }) =
            case.facts.get(&field)
        {
            let quote = match &observation.evidence {
                Evidence::User { quote, .. } | Evidence::RetainedQuestion { quote } => {
                    quote.as_str()
                }
                _ => "",
            };
            accepted.push(json!({"field":field.name(),"value":observation.value,
                "quote":quote,"evidence":observation.evidence}));
        }
    }
    accepted.push(
        json!({"field":"subject.owner_id","value":request["subject"]["owner_id"],
        "quote":request["subject"]["source_quote"]}),
    );
    for (id, person) in request["people"].as_object().into_iter().flatten() {
        for (name, key) in [("label", "label"), ("relationship", "relationship")] {
            accepted.push(
                json!({"field":format!("people.{id}.{name}"),"value":person[key],
                "quote":person["source_quote"]}),
            );
        }
    }
    let selections: Vec<_> = roles
        .iter()
        .map(|role| reading_method::ContactAcceptedRole {
            selected_id: role.selected_id.clone(),
            label: role.label.clone(),
            house: role.house,
            planet: role.planet.clone(),
        })
        .collect();
    let projection = json!({"facts":accepted,"unknown":[],
        "source_identity":{"reading_input_sha256":request["binding"]["input_sha256"],
            "role_selections_sha256":crate::horary_lessons::digest(&serde_json::to_string(&selections).expect("Finite native role selections"))},
        "source_scope":"native_accepted_consultation_and_roles"});
    Some(reading_method::contact_role_reference(
        &projection,
        facts,
        Some(&selections),
    ))
}

/// Keep native limitations beside the proposed contact interpretation. A
/// checked worksheet can still overstate an event or its calendar deadline.
fn with_contact_judgment_limits(mut input: Value, facts: &[Fact]) -> Result<Value, String> {
    if input["reading_request"]["binding"]["frame"]["method"] != "contact" {
        return Ok(input);
    }
    let timing_status = input["timing"]["timing_status"]
        .as_str()
        .ok_or("Contact judgment requires the native timing status")?
        .to_owned();
    let candidates = input["contacts"]["candidate_ids"]
        .as_array()
        .ok_or("Contact judgment requires the checked candidate IDs")?
        .iter()
        .map(|id| {
            let id = id
                .as_str()
                .ok_or("A contact candidate requires its fact ID")?;
            let event = facts
                .iter()
                .find(|fact| fact.id == id)
                .and_then(|fact| fact.event.as_ref())
                .ok_or("A checked contact candidate lacks its native event fact")?;
            Ok(json!({"fact_id":id,"within_current_signs":event.within_current_signs}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    input["native_contact_limits"] = json!({
        "worksheet_authority":"checked_model_proposals",
        "earthly_timing_status":timing_status,
        "selected_candidates":candidates,
        "post_ingress_condition_and_reception":"not_supplied",
        "event_verdict":"requires_contextual_judgment"
    });
    // Records and sections keep the original worksheets. The final reader
    // receives their proposed statuses/IDs and uncertainties, not a story to
    // paraphrase in preference to the native facts already supplied once.
    for name in ["condition", "reception", "contacts", "timing"] {
        let Some(worksheet) = input.get_mut(name).and_then(Value::as_object_mut) else {
            continue;
        };
        for key in ["summary", "finding", "because", "reason"] {
            worksheet.remove(key);
        }
        if let Some(checks) = worksheet.get_mut("checks").and_then(Value::as_object_mut) {
            for check in checks.values_mut().filter_map(Value::as_object_mut) {
                for key in ["finding", "because"] {
                    check.remove(key);
                }
            }
        }
        if let Some(rows) = worksheet
            .get_mut("edge_interpretations")
            .filter(|_| name == "reception")
            .and_then(Value::as_array_mut)
        {
            for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                // Only the five mechanically checked identity/quality fields
                // reach final Contact synthesis. Original prose stays recorded.
                row.remove("interpretation");
                row.remove("limitations");
            }
        }
    }
    Ok(input)
}

fn reception_clipboard_view(
    roles: &[AcceptedReceptionRole],
    facts: &[Fact],
    direct_roles: bool,
) -> Value {
    let edges: Vec<_> = facts
        .iter()
        .filter(|fact| fact.kind == "reception")
        .filter_map(|fact| {
            let reception = fact.reception.as_ref()?;
            let matching_roles = |planet: &str| {
                roles
                    .iter()
                    .filter(|role| role.planet == planet)
                    .collect::<Vec<_>>()
            };
            let guest_roles = matching_roles(&reception.guest_planet);
            let host_roles = matching_roles(&reception.host_planet);
            Some(ReceptionRoleEdge {
                fact_id: &fact.id,
                guest_role_indices: guest_roles.iter().map(|role| role.role_index).collect(),
                host_role_indices: host_roles.iter().map(|role| role.role_index).collect(),
                guest_roles: direct_roles.then_some(guest_roles),
                host_roles: direct_roles.then_some(host_roles),
            })
        })
        .collect();
    json!({"accepted_roles":roles,"edges":edges})
}

#[cfg(test)]
fn reception_receipt_fixture(input: &Value) -> Value {
    let input = step::original_input(input);
    let roles = input["native_reception"]["accepted_roles"].as_array();
    let rows = input["facts"].as_array().into_iter().flatten().filter_map(|fact| {
        if fact["kind"] != "reception" || !fact["reception"].is_object() {return None;}
        let ids = |endpoint| roles.into_iter().flatten().filter(|role|role["planet"]==fact["reception"][endpoint])
            .map(|role|role["selected_id"].clone()).collect::<Vec<_>>();
        let qualities = |polarity| fact["reception"]["qualities"].as_array().into_iter().flatten()
            .filter(|quality|quality["polarity"]==polarity).map(|quality|quality["dignity"].clone()).collect::<Vec<_>>();
        Some(json!({"edge_id":fact["id"],"regarding_actor_ids":ids("guestPlanet"),"regarded_actor_ids":ids("hostPlanet"),
            "positive_qualities":qualities("positive"),"negative_qualities":qualities("negative"),
            "interpretation":"Receipt-only offline fixture; contextual truth is unobserved.",
            "limitations":"No event, reverse regard or timing is established."}))
    }).collect::<Vec<_>>();
    json!(rows)
}

fn generate_reading(
    session: &mut Session,
    runtime: &impl Runtime,
    ready: crate::reading_contracts::ReadyReading,
) -> Result<Option<usize>, String> {
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
        return Ok(None);
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
    #[cfg(test)]
    if runtime.selected_stage_target() == Some(Stage::Significators) {
        return selected_stage_boundary(session, runtime, &ready, Stage::Significators)
            .map(|()| None);
    }
    let reception_roles = accepted_reception_roles(&assignment)?;
    let condition = relevant(&all, &roles, &["condition", "position", "boundary"]);
    let reception = relevant(&all, &roles, &["reception", "boundary"]);
    let contact_context = contact_evidence(&all, &roles);
    let events = contact_mechanics(&contact_context);
    let native_arrival =
        crate::message_programs::clipboard(&ready, assignment.worksheet(), &roles, &events)?;
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
            json!({"reading_request":reading_request,"question":common,"facts":reception,
                "native_reception":reception_clipboard(&reception_roles,&reception)}),
            reception.clone(),
        ),
        (
            Stage::Contacts,
            crate::message_programs::with_clipboard(
                json!({"reading_request":reading_request,"brief":reading_brief,"roles":roles,"facts":events}),
                &native_arrival,
            ),
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
    #[cfg(test)]
    if let Some(target) = runtime.selected_stage_target().filter(|stage| {
        matches!(
            stage,
            Stage::Condition | Stage::Reception | Stage::Contacts | Stage::Location
        )
    }) {
        if let Some((stage, input, facts)) = tasks.iter().find(|(stage, _, _)| *stage == target) {
            // Use the existing production tuple and guard. Unrun siblings are
            // absent, never represented by fabricated successful worksheets.
            if let Some(data) = task(session, runtime, *stage, input.clone(), facts)? {
                section(session, *stage, data.worksheet(), facts, Vec::new());
                return selected_stage_boundary(session, runtime, &ready, *stage).map(|()| None);
            }
            return Ok(None);
        }
        // This method does not invoke the selected stage (e.g. Location on a
        // non-lost question). Do not manufacture authority or run other actors.
        session.audit.push(
            json!({"event":"selected_stage_uninvoked","target_stage":target,
            "reason":"The real method does not dispatch this independent stage"}),
        );
        return runtime.publish(session).map(|()| None);
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
        return Ok(None);
    }
    let c = values[0].as_ref().map(CheckedData::worksheet);
    let r = values[1].as_ref().map(CheckedData::worksheet);
    let contact = values[2].as_ref().map(CheckedData::worksheet);
    let location = values
        .get(3)
        .and_then(Option::as_ref)
        .map(CheckedData::worksheet);
    // Approximate per-body candidate motion is not a completed traditional
    // symbolic timing judgment. No actor or calendar unit is chosen here.
    let timing = json!({"timing_status":"unestablished","reason":"The native calculation supplies approximate candidate mechanics, not an established symbolic timing judgment. Astronomical hours are not symbolic calendar timing."});
    session
        .audit
        .push(json!({"event":"native_stage","stage":"timing","result":timing}));
    let mut facts = relevant(
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
    for fact in &contact_context {
        if !facts.iter().any(|existing| existing.id == fact.id) {
            facts.push(fact.clone());
        }
    }
    // Reference IDs belong to this judgment's actual evidence contract, not
    // to calculations omitted from its final input.
    let contact_roles = native_contact_role_reference(
        session
            .method
            .consultation
            .as_ref()
            .expect("Consultation installed"),
        &reading_request,
        &reception_roles,
        &facts,
    );
    let mut judgment_input = json!({"reading_request":reading_request,"brief":reading_brief,"roles":roles,"condition":c,"reception":r,"contacts":contact,"location":location,"timing":timing,"facts":facts,
        "native_reception":judgment_reception_clipboard(&reception_roles,&facts)});
    if let Some(reference) = contact_roles {
        judgment_input["contact_role_reference"] = reference;
    }
    let record_start = session.method.records.len();
    let Some(judgment) = task(
        session,
        runtime,
        Stage::Judgment,
        with_contact_judgment_limits(
            crate::message_programs::with_clipboard(judgment_input, &native_arrival),
            &facts,
        )?,
        &facts,
    )?
    else {
        return Ok(None);
    };
    ready.validate_current(
        session
            .method
            .consultation
            .as_ref()
            .expect("Consultation installed"),
    )?;
    #[cfg(test)]
    if runtime.selected_stage_target() == Some(Stage::Judgment) {
        return selected_stage_boundary(session, runtime, &ready, Stage::Judgment).map(|()| None);
    }
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
    // A cached earlier Judgment is still valid native data, but cannot replace
    // the response to a new follow-up. Only this execution's accepted record
    // supplies the immediate model-authored reply.
    Ok(session
        .method
        .records
        .len()
        .checked_sub(1)
        .filter(|index| *index >= record_start))
}

/// Acceptance is emitted only after the production guard completed its native
/// job, with the same current ReadyReading. The evaluator cannot create this
/// permit from a captured specialist request.
#[cfg(test)]
fn selected_stage_boundary(
    session: &mut Session,
    runtime: &impl Runtime,
    ready: &crate::reading_contracts::ReadyReading,
    stage: Stage,
) -> Result<(), String> {
    ready.validate_current(
        session
            .method
            .consultation
            .as_ref()
            .ok_or("No current consultation")?,
    )?;
    let index = session
        .method
        .records
        .len()
        .checked_sub(1)
        .ok_or("No accepted stage record")?;
    let record = &session.method.records[index];
    if record.stage != stage
        || record.revision != session.revision
        || record.validation_error.is_some()
    {
        return Err("Selected stage boundary is not its actual accepted native record".into());
    }
    let jobs = session.method.flow.jobs.iter().filter(|job| {
        job.stage == stage && job.revision == session.revision
            && matches!(job.phase(), step::Phase::Complete { record_index } if *record_index == index)
    }).collect::<Vec<_>>();
    if jobs.len() != 1 {
        return Err("Selected stage boundary lacks its unique completed native job".into());
    }
    session.audit.push(json!({"event":"selected_stage_boundary","version":1,
        "target_stage":stage,"binding":ready.binding(),"reading_request":ready.input(),
        "revision":session.revision,"after_message":session.messages.len(),"record_index":index,
        "record_input_sha256":record.input_sha256,"native_job":jobs[0],"work_key":jobs[0].key,
        "last_sequence":runtime.selected_stage_sequence().ok_or("No actual selected-stage call sequence")?,
        "reading_delivered":false,"post_boundary_conversation_executed":false}));
    runtime.publish(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_final_reference_reuses_actual_native_role_permit_and_input_binding() {
        use crate::reading_contracts::{self as contracts, Frame, Intent, Method};
        let words = "Will my mother Alma contact me?";
        let mut case = contracts::Consultation::default();
        let mut patch = contracts::control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame {
            method: Method::Contact,
            facet: contracts::Facet::Event,
        });
        patch.people = vec![crate::horary_role_options::Person {
            id: "alma".into(),
            label: "Alma".into(),
            relationship: "mother".into(),
            source_quote: "my mother Alma".into(),
        }];
        patch.subject = Some(crate::horary_role_options::Subject {
            name: "Alma".into(),
            kind: "person".into(),
            owner_id: "alma".into(),
            source_quote: "Alma".into(),
        });
        case.apply(&patch, 1, words, false).unwrap();
        let anchor = contracts::Anchor {
            timestamp_ms: 1791478980000.,
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
        };
        let ready = contracts::ReadyReading::prepare(&case, anchor.clone()).unwrap();
        let native = reading_method::facts(Some(
            &horary_ai_core::astronomy::chart(
                anchor.timestamp_ms,
                anchor.latitude,
                anchor.longitude,
            )
            .unwrap(),
        ));
        let brief = ready.brief();
        let options = crate::horary_role_options::build_for(
            &case,
            brief.matter,
            &brief.people,
            &brief.subject,
        );
        let worksheet = json!({"selections":options.required_groups.iter().map(|group| {
            assert_eq!(group.len(),1);
            let choice = options.choices.iter().find(|choice| choice.id == group[0]).unwrap();
            json!({"id":choice.id,"reason":choice.basis})
        }).collect::<Vec<_>>(),"summary":"Actual bound role calculation only.","unknowns":[]});
        let step::Checked::Data(data) = step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &json!({"native_role_options":options}),
            &native,
        )
        .unwrap() else {
            panic!("Authored selections must cross the real native acceptance boundary");
        };
        let selected = accepted_reception_roles(&data).unwrap();
        let request = ready.input();
        let reference = native_contact_role_reference(&case, &request, &selected, &native).unwrap();
        assert_eq!(reference["alias_authority"], "native_accepted_selections");
        assert_eq!(
            reference["source_identity"]["reading_input_sha256"],
            request["binding"]["input_sha256"]
        );
        for (alias, role) in reference["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&selected)
        {
            assert_eq!(alias["role_id"], role.selected_id);
            assert_eq!(alias["label"], role.label);
            assert_eq!(alias["planet"], role.planet);
            assert_eq!(alias["significator_house"], json!(role.house));
        }
        let input = json!({"reading_request":request,"brief":{"question":words},"roles":data.roles(),
            "facts":native,"contact_role_reference":reference,"timing":{"timing_status":"unestablished"},
            "contacts":{"candidate_ids":[],"unknowns":["No model contact assessment executed in this handoff regression."]}});
        let final_input = with_contact_judgment_limits(input.clone(), &native).unwrap();
        assert_eq!(
            final_input["contact_role_reference"],
            input["contact_role_reference"]
        );
        assert_eq!(final_input["facts"], input["facts"]);
        let final_facts: Vec<_> = native
            .iter()
            .filter(|fact| {
                matches!(
                    fact.kind.as_str(),
                    "condition" | "reception" | "event" | "position" | "moon" | "boundary"
                )
            })
            .cloned()
            .collect();
        let limited = native_contact_role_reference(
            &case,
            &input["reading_request"],
            &selected,
            &final_facts,
        )
        .unwrap();
        for row in limited["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["optional"] == false)
        {
            let role = selected
                .iter()
                .find(|role| row["selected_id"] == role.selected_id)
                .expect("Final role rows retain actual native accepted selections");
            assert_eq!(row["label"], role.label);
            assert_eq!(row["significator_house"], json!(role.house));
            assert_eq!(row["ruler_planet"], role.planet);
            assert!(row["cusp_fact_id"].is_null());
        }
        assert_eq!(
            limited["moon"]["available_as_contextual_reference"],
            selected
                .iter()
                .any(|role| role.selected_id == "moon.contextual")
                && !selected
                    .iter()
                    .any(|role| role.planet == "Moon" && role.house.is_some())
        );
        for id in limited["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row["position_fact_ids"].as_array().unwrap())
        {
            assert!(final_facts.iter().any(|fact| id == &fact.id));
        }
        for edge in limited["directed_reception"]
            .as_array()
            .unwrap()
            .iter()
            .chain(limited["event_endpoints"].as_array().unwrap())
        {
            assert!(final_facts.iter().any(|fact| edge["fact_id"] == fact.id));
        }
        let mut other = case;
        if let contracts::Slot::Resolved { observation } = &mut other.frame {
            observation.value.method = Method::NewJob;
        }
        assert!(native_contact_role_reference(
            &other,
            &input["reading_request"],
            &selected,
            &native
        )
        .is_none());
    }

    #[test]
    fn contact_final_handoff_keeps_native_limits_apart_from_proposed_prose() {
        let facts = reading_method::facts(Some(&json!({"derived":{"eventSearch":{"events":[
            {"planet1":"Moon","planet2":"Mars","aspectName":"Sextile",
             "withinCurrentSigns":false,"estimatedPerfectsWithinHours":25.0},
            {"planet1":"Saturn","planet2":"Mars","aspectName":"Trine",
             "estimatedPerfectsWithinHours":30.0}
        ]}}})));
        let events = facts
            .iter()
            .filter(|f| f.event.is_some())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        let input = json!({
            "reading_request":{"binding":{"frame":{"method":"contact","facet":"event"}}},
            "brief":{"question":"Will my friend contact me by Sunday?"},
            "roles":[{"label":"You","house":1,"planet":"Saturn"},
                {"label":"The friend","house":11,"planet":"Mars"}],
            "facts":facts,
            "condition":{"checks":{"ability_to_act":{"state":"supported",
                "evidence":[],"finding":"MISLEADING_CAPACITY_PROSE"}},
                "summary":"MISLEADING_CONDITION_SUMMARY","unknowns":["A declared capacity uncertainty."]},
            "timing":{"timing_status":"unestablished","reason":"MISLEADING_TIMING_REASON"},
            "contacts":{"candidate_ids":[events[0].id,events[1].id],
                "candidate_signs":[{"id":events[0].id,"within_current_signs":true}],
                "basis":"direct_candidate","checks":{"event_order":{"state":"unestablished",
                    "evidence":[events[0].id],"finding":"MISLEADING_CONTACT_FINDING",
                    "because":"MISLEADING_CONTACT_BECAUSE"}},
                "summary":"MISLEADING_CONTACT_SUMMARY","unknowns":["A declared event-order uncertainty."]},
            "reception":{"checks":{"contextual_motive":{"state":"unestablished",
                "evidence":[],"finding":"MISLEADING_FEELINGS_FINDING"}},
                "summary":"MISLEADING_RECEPTION_SUMMARY","unknowns":[]}
        });
        let original = with_contact_judgment_limits(input.clone(), &facts).unwrap();
        assert!(!original.to_string().contains("MISLEADING_"));
        assert!(
            input.to_string().contains("MISLEADING_"),
            "The original worksheets are unchanged"
        );
        for key in ["reading_request", "brief", "roles", "facts"] {
            assert_eq!(original[key], input[key]);
        }
        for name in ["condition", "reception", "contacts"] {
            assert_eq!(original[name]["unknowns"], input[name]["unknowns"]);
            for (key, check) in input[name]["checks"].as_object().unwrap() {
                assert_eq!(original[name]["checks"][key]["state"], check["state"]);
                assert_eq!(original[name]["checks"][key]["evidence"], check["evidence"]);
            }
        }
        for key in ["basis", "candidate_ids", "candidate_signs"] {
            assert_eq!(original["contacts"][key], input["contacts"][key]);
        }
        assert_eq!(original["timing"], json!({"timing_status":"unestablished"}));
        assert_eq!(original["brief"], input["brief"]);
        assert_eq!(
            original["native_contact_limits"],
            json!({
                "worksheet_authority":"checked_model_proposals",
                "earthly_timing_status":"unestablished",
                "selected_candidates":[
                    {"fact_id":events[0].id,"within_current_signs":false},
                    {"fact_id":events[1].id,"within_current_signs":null}
                ],
                "post_ingress_condition_and_reception":"not_supplied",
                "event_verdict":"requires_contextual_judgment"
            })
        );
        assert!(original.get("verdict").is_none());
        let repair = json!({"original_input":original,"previous_worksheet":{
            "native_contact_limits":{"earthly_timing_status":"established"}
        }});
        let guide =
            crate::horary_contract::guide_for(Stage::Judgment, Matter::Other, &repair).unwrap();
        assert!(guide.contains("<contact_answer>"));
        for id in [
            "timing_distance",
            "contact_with_person",
            "reception_action_context",
        ] {
            let card = lessons::passages()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .unwrap();
            assert!(guide.contains(&card.quote));
        }
        assert_eq!(
            step::original_input(&repair)["native_contact_limits"],
            original["native_contact_limits"]
        );
        let mut foreign = input.clone();
        foreign["contacts"]["candidate_ids"] = json!(["foreign"]);
        assert!(with_contact_judgment_limits(foreign, &facts).is_err());
        let mut unrelated = input;
        unrelated["reading_request"]["binding"]["frame"]["method"] = json!("movable_deal");
        assert_eq!(
            with_contact_judgment_limits(unrelated.clone(), &facts).unwrap(),
            unrelated
        );
        let other_guide =
            crate::horary_contract::guide_for(Stage::Judgment, Matter::Other, &unrelated).unwrap();
        assert!(!other_guide.contains("<contact_answer>"));
    }

    fn expand_indexed_reception_clipboard(mut clipboard: Value) -> Value {
        let roles = clipboard["accepted_roles"].as_array().unwrap().clone();
        for edge in clipboard["edges"].as_array_mut().unwrap() {
            for (direct, indices) in [
                ("guest_roles", "guest_role_indices"),
                ("host_roles", "host_role_indices"),
            ] {
                let rows = edge[indices]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|index| roles[index.as_u64().unwrap() as usize].clone())
                    .collect::<Vec<_>>();
                edge[direct] = json!(rows);
            }
        }
        clipboard
    }

    fn reception_binding_fixture() -> (Value, Value, Vec<Fact>) {
        let facts = reading_method::facts(Some(&json!({
            "houses":[
                {"number":1,"degree":0.,"sign":"Sagittarius"},
                {"number":2,"degree":0.,"sign":"Aquarius"},
                {"number":7,"degree":0.,"sign":"Gemini"},
                {"number":8,"degree":0.,"sign":"Cancer"}
            ],
            "derived":{"receptions":[
                {"guestPlanet":"Moon","hostPlanet":"Saturn","dignity":"domicile","polarity":"positive"},
                {"guestPlanet":"Saturn","hostPlanet":"Jupiter","dignity":"termRuler","polarity":"positive"},
                {"guestPlanet":"Venus","hostPlanet":"Moon","dignity":"fall","polarity":"negative"}
            ]}
        })));
        let choices = json!([
            {"id":"querent.self","label":"You","house":1,"natural":null,"basis":"Synthetic principal"},
            {"id":"goods.actual","label":"Your goods","house":2,"natural":null,"basis":"Synthetic movable goods"},
            {"id":"party.buyer","label":"Buyer","house":7,"natural":null,"basis":"Synthetic counterparty"},
            {"id":"recipient.pocket","label":"Your pocket","house":2,"natural":null,"basis":"Synthetic recipient money"},
            {"id":"buyer.money","label":"Buyer money","house":8,"natural":null,"basis":"Synthetic buyer second house"},
            {"id":"moon.contextual","label":"Contextual Moon","house":null,"natural":"Moon","basis":"Only if no house claims Moon"}
        ]);
        let selections = choices
            .as_array()
            .unwrap()
            .iter()
            .take(5)
            .map(|choice| json!({"id":choice["id"],"reason":choice["basis"]}))
            .collect::<Vec<_>>();
        (
            json!({"native_role_options":{"choices":choices,"required_groups":[],
                "conditional_roles":[],"missing":[],"compare":[]}}),
            json!({"selections":selections,"summary":"Synthetic bindings, no judgment.","unknowns":[]}),
            facts,
        )
    }

    #[test]
    fn reception_clipboard_keeps_shared_roles_and_house_claimed_moon() {
        let (input, worksheet, facts) = reception_binding_fixture();
        let step::Checked::Data(data) = step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &input,
            &facts,
        )
        .unwrap() else {
            panic!("Synthetic role data must pass the actual native boundary");
        };
        let roles = accepted_reception_roles(&data).unwrap();
        let clipboard = reception_clipboard(&roles, &facts);
        assert_eq!(clipboard["accepted_roles"].as_array().unwrap().len(), 5);
        assert_eq!(
            clipboard["accepted_roles"][4],
            json!({
                "role_index":4,"selected_id":"buyer.money","label":"Buyer money","house":8,"planet":"Moon"
            })
        );
        assert!(clipboard["accepted_roles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|role| role["house"].is_number()));
        let moon = facts
            .iter()
            .find(|fact| fact.label == "Moon → Saturn")
            .unwrap();
        let edge = clipboard["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|edge| edge["fact_id"] == moon.id)
            .unwrap();
        assert_eq!(edge["guest_role_indices"], json!([4]));
        assert_eq!(edge["host_role_indices"], json!([1, 3]));
        assert_eq!(
            edge["guest_roles"],
            json!([{
                "role_index":4,"selected_id":"buyer.money","label":"Buyer money","house":8,"planet":"Moon"
            }])
        );
        assert_eq!(
            edge["host_roles"],
            json!([
                {"role_index":1,"selected_id":"goods.actual","label":"Your goods","house":2,"planet":"Saturn"},
                {"role_index":3,"selected_id":"recipient.pocket","label":"Your pocket","house":2,"planet":"Saturn"}
            ])
        );
        assert_eq!(data.roles()[1].planet, data.roles()[3].planet);
        let unassigned = facts
            .iter()
            .find(|fact| fact.label == "Venus → Moon")
            .unwrap();
        let edge = clipboard["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|edge| edge["fact_id"] == unassigned.id)
            .unwrap();
        assert_eq!(edge["guest_role_indices"], json!([]));
        assert_eq!(edge["host_role_indices"], json!([4]));
        assert_eq!(edge["guest_roles"], json!([]));
        assert_eq!(edge["host_roles"], json!([clipboard["accepted_roles"][4]]));
        assert_eq!(
            roles.len(),
            5,
            "Evidence does not assign an intermediary role"
        );
    }

    #[test]
    fn reception_edges_resolve_every_side_to_exact_accepted_rows() {
        let (input, worksheet, facts) = reception_binding_fixture();
        let step::Checked::Data(data) = step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &input,
            &facts,
        )
        .unwrap() else {
            panic!("Expected actual native role acceptance");
        };
        let roles = accepted_reception_roles(&data).unwrap();
        let clipboard = reception_clipboard(&roles, &facts);
        let rows = clipboard["accepted_roles"].as_array().unwrap();
        let edges = clipboard["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 3);
        for edge in edges {
            let fact = facts
                .iter()
                .find(|fact| edge["fact_id"] == fact.id)
                .unwrap();
            let reception = fact.reception.as_ref().unwrap();
            for (direct, indices, planet) in [
                ("guest_roles", "guest_role_indices", &reception.guest_planet),
                ("host_roles", "host_role_indices", &reception.host_planet),
            ] {
                let expected = rows
                    .iter()
                    .filter(|role| role["planet"] == *planet)
                    .collect::<Vec<_>>();
                assert_eq!(edge[direct], json!(expected));
                let expected_indices = expected
                    .iter()
                    .map(|role| role["role_index"].clone())
                    .collect::<Vec<_>>();
                assert_eq!(edge[indices], json!(expected_indices));
                for row in edge[direct].as_array().unwrap() {
                    let index = row["role_index"].as_u64().unwrap() as usize;
                    assert_eq!(row, &rows[index]);
                    assert_eq!(row["selected_id"], worksheet["selections"][index]["id"]);
                    assert_eq!(row["label"], data.roles()[index].label);
                    assert_eq!(row["house"], json!(data.roles()[index].house));
                    assert_eq!(row["planet"], data.roles()[index].planet);
                }
            }
        }
        let unassigned = reading_method::facts(Some(&json!({
            "derived":{"receptions":[
                {"guestPlanet":"Moon","hostPlanet":"Venus","dignity":"fall","polarity":"negative"},
                {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"domicile","polarity":"positive"}
            ]}
        })));
        let clipboard = reception_clipboard(&roles, &unassigned);
        let edges = clipboard["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0]["guest_roles"], json!([rows[4]]));
        assert_eq!(edges[0]["guest_role_indices"], json!([4]));
        for edge in edges {
            assert_eq!(edge["host_roles"], json!([]));
            assert_eq!(edge["host_role_indices"], json!([]));
        }
        assert_eq!(edges[1]["guest_roles"], json!([]));
        assert_eq!(edges[1]["guest_role_indices"], json!([]));
    }

    #[test]
    fn reception_clipboard_cannot_bypass_native_role_ids_or_moon_priority() {
        let (input, worksheet, facts) = reception_binding_fixture();
        for id in ["Buyer money", "querent.self"] {
            let mut rejected = worksheet.clone();
            rejected["selections"][4]["id"] = json!(id);
            assert!(step::check(
                Stage::Significators,
                Matter::Other,
                &rejected,
                &input,
                &facts
            )
            .is_err());
        }
        let mut competing_moon = worksheet;
        competing_moon["selections"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id":"moon.contextual","reason":"An attempted competing natural role"
            }));
        let error = step::check(
            Stage::Significators,
            Matter::Other,
            &competing_moon,
            &input,
            &facts,
        )
        .err()
        .unwrap();
        assert!(error.contains("house ruler has first claim"));
    }

    #[test]
    fn reception_rows_cross_real_clipboard_repair_and_contact_final_boundary() {
        let (role_input, role_worksheet, facts) = reception_binding_fixture();
        let step::Checked::Data(permit) = step::check(
            Stage::Significators,
            Matter::Other,
            &role_worksheet,
            &role_input,
            &facts,
        )
        .unwrap() else {
            panic!("Actual native role permit required")
        };
        let roles = accepted_reception_roles(&permit).unwrap();
        let clipboard = reception_clipboard(&roles, &facts);
        let rows = facts
            .iter()
            .filter_map(|fact| {
                let edge = fact
                    .reception
                    .as_ref()
                    .filter(|_| fact.kind == "reception")?;
                let ids = |planet: &str| {
                    roles
                        .iter()
                        .filter(|role| role.planet == planet)
                        .map(|role| role.selected_id.clone())
                        .collect::<Vec<_>>()
                };
                let qualities = |polarity| {
                    edge.qualities
                        .iter()
                        .filter(|quality| quality.polarity == polarity)
                        .map(|quality| quality.dignity.native_name().to_owned())
                        .collect::<Vec<_>>()
                };
                Some(reading_method::ReceptionInterpretation {
                    edge_id: fact.id.clone(),
                    regarding_actor_ids: ids(&edge.guest_planet),
                    regarded_actor_ids: ids(&edge.host_planet),
                    positive_qualities: qualities(reading_method::ReceptionPolarity::Positive),
                    negative_qualities: qualities(reading_method::ReceptionPolarity::Negative),
                    interpretation: "UNQUALIFIED_RECEPTION_STORY".into(),
                    limitations: "UNQUALIFIED_RECEPTION_LIMIT_PROSE".into(),
                })
            })
            .collect::<Vec<_>>();
        let input = json!({"roles":permit.roles(),"facts":facts,"native_reception":clipboard});
        let mut checks = serde_json::Map::new();
        for key in Stage::Reception.checks() {
            checks.insert((*key).into(),json!({"state":"unestablished","evidence":[],"finding":"Offline native receipt test only."}));
        }
        let worksheet = json!({"checks":checks,"summary":"No semantic reading qualification.","unknowns":[],"edge_interpretations":rows});
        let before = worksheet.clone();
        let mut rejected = worksheet.clone();
        rejected["edge_interpretations"][0]["regarding_actor_ids"] = json!([]);
        let error = step::check(Stage::Reception, Matter::Other, &rejected, &input, &facts)
            .err()
            .unwrap();
        assert!(error.contains("regarding_actor_ids"));
        let repair = json!({"original_input":input,"previous_worksheet":rejected,"native_validation_error":error});
        let step::Checked::Data(accepted) =
            step::check(Stage::Reception, Matter::Other, &worksheet, &repair, &facts).unwrap()
        else {
            panic!("Corrected same-stage row should complete")
        };
        assert_eq!(accepted.worksheet(), &before);
        let final_input = json!({"reading_request":{"binding":{"frame":{"method":"contact"}}},
            "reception":accepted.worksheet(),"timing":{"timing_status":"unestablished"},
            "contacts":{"candidate_ids":[]},"facts":facts,"roles":permit.roles()});
        let final_view = with_contact_judgment_limits(final_input.clone(), &facts).unwrap();
        assert!(!final_view.to_string().contains("UNQUALIFIED_RECEPTION_"));
        assert!(final_input
            .to_string()
            .contains("UNQUALIFIED_RECEPTION_STORY"));
        for (projected, original) in final_view["reception"]["edge_interpretations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&rows)
        {
            let original = serde_json::to_value(original).unwrap();
            assert_eq!(projected.as_object().unwrap().len(), 5);
            for key in [
                "edge_id",
                "regarding_actor_ids",
                "regarded_actor_ids",
                "positive_qualities",
                "negative_qualities",
            ] {
                assert_eq!(projected[key], original[key]);
            }
        }
        assert_eq!(final_view["facts"], final_input["facts"]);
        assert_eq!(final_view["roles"], final_input["roles"]);
        assert_eq!(worksheet, before);
    }

    #[test]
    fn judgment_reception_clipboard_reconstructs_exact_shared_and_unassigned_roles() {
        let (input, worksheet, facts) = reception_binding_fixture();
        let step::Checked::Data(data) = step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &input,
            &facts,
        )
        .unwrap() else {
            panic!("Expected native role acceptance");
        };
        let roles = accepted_reception_roles(&data).unwrap();
        let before_roles = serde_json::to_value(data.roles()).unwrap();
        let unassigned = reading_method::facts(Some(&json!({
            "derived":{"receptions":[
                {"guestPlanet":"Moon","hostPlanet":"Venus","dignity":"fall","polarity":"negative"},
                {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"domicile","polarity":"positive"}
            ]}
        })));
        for facts in [facts, unassigned] {
            let before_facts = serde_json::to_value(&facts).unwrap();
            let expanded = reception_clipboard(&roles, &facts);
            let indexed = judgment_reception_clipboard(&roles, &facts);
            assert_eq!(indexed["accepted_roles"], expanded["accepted_roles"]);
            for edge in indexed["edges"].as_array().unwrap() {
                assert!(edge.get("guest_roles").is_none());
                assert!(edge.get("host_roles").is_none());
                assert!(edge["guest_role_indices"].is_array());
                assert!(edge["host_role_indices"].is_array());
            }
            assert!(indexed.to_string().len() < expanded.to_string().len());
            assert_eq!(expand_indexed_reception_clipboard(indexed), expanded);
            assert_eq!(serde_json::to_value(&facts).unwrap(), before_facts);
        }
        assert_eq!(serde_json::to_value(data.roles()).unwrap(), before_roles);
    }

    #[test]
    fn historical_prose_and_non_reception_facts_do_not_create_clipboard_edges() {
        let (input, worksheet, mut facts) = reception_binding_fixture();
        let step::Checked::Data(data) = step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &input,
            &facts,
        )
        .unwrap() else {
            panic!("Expected actual native role acceptance");
        };
        let roles = accepted_reception_roles(&data).unwrap();
        for fact in &mut facts {
            if fact.kind == "reception" {
                // Retain the readable prose while simulating historical JSON.
                fact.reception = None;
            }
        }
        assert_eq!(reception_clipboard(&roles, &facts)["edges"], json!([]));
        facts[0].reception = Some(reading_method::DirectedReception {
            guest_planet: "Jupiter".into(),
            host_planet: "Saturn".into(),
            qualities: Vec::new(),
        });
        assert_eq!(facts[0].kind, "house");
        assert_eq!(reception_clipboard(&roles, &facts)["edges"], json!([]));
    }

    #[test]
    fn contact_graph_keeps_unassigned_prohibition_and_two_intermediary_witnesses() {
        let make = |id: &str, kind: &str, planets: &[&str], hours: Option<f64>| Fact {
            id: id.into(),
            kind: kind.into(),
            label: id.into(),
            detail: "Authored native evidence".into(),
            planets: planets.iter().map(|planet| (*planet).into()).collect(),
            condition_facet: None,
            event: hours.map(|astronomical_hours| reading_method::EventCandidate {
                within_current_signs: Some(true),
                astronomical_hours,
                perfection_motion: None,
            }),
            reception: None,
        };
        let facts = vec![
            make("party-contact", "event", &["Mars", "Venus"], Some(20.)),
            make(
                "buyer-money-prohibition",
                "event",
                &["Mars", "Saturn"],
                Some(5.),
            ),
            make(
                "intermediary-contact",
                "event",
                &["Saturn", "Jupiter"],
                Some(8.),
            ),
            make(
                "second-intermediary-contact",
                "event",
                &["Jupiter", "Venus"],
                Some(12.),
            ),
            make("money-ruler", "house", &["Saturn"], None),
            make("intermediary-capacity", "condition", &["Jupiter"], None),
            make(
                "intermediary-reception",
                "reception",
                &["Saturn", "Jupiter"],
                None,
            ),
            make("moon-context", "moon", &["Moon"], None),
            make("uncertainty", "boundary", &[], None),
            make("unconnected-position", "position", &["Uranus"], None),
        ];
        let roles = vec![
            Role {
                label: "Seller".into(),
                house: Some(1),
                planet: "Mars".into(),
                reason: "Authored principal".into(),
            },
            Role {
                label: "Buyer".into(),
                house: Some(7),
                planet: "Venus".into(),
                reason: "Authored counterparty".into(),
            },
        ];
        let ordinary = relevant(&facts, &roles, &["event"]);
        assert_eq!(
            ordinary.len(),
            1,
            "The old endpoint filter would discard intervening evidence"
        );
        let supplied = contact_evidence(&facts, &roles);
        assert_eq!(supplied.len(), 9);
        assert!(supplied
            .iter()
            .any(|fact| fact.id == "buyer-money-prohibition"
                && fact.event.as_ref().unwrap().astronomical_hours == 5.));
        assert!(supplied
            .iter()
            .any(|fact| fact.id == "intermediary-contact"));
        assert!(supplied
            .iter()
            .any(|fact| fact.id == "intermediary-reception"));
        assert!(supplied.iter().any(|fact| fact.id == "money-ruler"));
        assert!(supplied.iter().any(|fact| fact.id == "moon-context"));
        assert!(!supplied
            .iter()
            .any(|fact| fact.id == "unconnected-position"));
        assert_eq!(
            roles.len(),
            2,
            "Evidence availability does not assign intermediary or Moon roles"
        );
        let mechanics = contact_mechanics(&supplied);
        assert_eq!(
            mechanics
                .iter()
                .map(|fact| fact.id.as_str())
                .collect::<Vec<_>>(),
            [
                "party-contact",
                "buyer-money-prohibition",
                "intermediary-contact",
                "second-intermediary-contact",
                "money-ruler",
                "moon-context",
                "uncertainty",
            ],
            "Independent contact selection must keep intermediary order, Moon and coverage"
        );
        for fact in mechanics {
            let original = facts
                .iter()
                .find(|original| original.id == fact.id)
                .unwrap();
            assert_eq!(
                serde_json::to_value(fact).unwrap(),
                serde_json::to_value(original).unwrap()
            );
        }
    }
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
    fn actual_scheduler_supplies_reception_clipboard_only_to_reception_and_judgment() {
        let fixture = process_examples().unwrap();
        let examples = fixture["examples"].as_array().unwrap();
        let mut accepted_roles = None;
        let mut current_roles = None;
        let mut reception_facts: Option<Vec<Value>> = None;
        for example in examples {
            let envelope: Value =
                serde_json::from_str(example["messages"][1]["content"].as_str().unwrap()).unwrap();
            let input = &envelope["input"];
            if matches!(example["stage"].as_str(), Some("reception" | "judgment")) {
                let clipboard = &input["native_reception"];
                let roles = clipboard["accepted_roles"].as_array().unwrap();
                assert!(!roles.is_empty());
                assert!(roles.iter().enumerate().all(|(index, role)| {
                    role["role_index"] == json!(index) && role["selected_id"].as_str().is_some()
                }));
                if let Some(previous) = &accepted_roles {
                    assert_eq!(previous, &clipboard["accepted_roles"]);
                } else {
                    accepted_roles = Some(clipboard["accepted_roles"].clone());
                }
                let facts = input["facts"].as_array().unwrap();
                let roles = input.get("roles").unwrap_or(&input["question"]["roles"]);
                if let Some(previous) = &current_roles {
                    assert_eq!(previous, roles);
                } else {
                    current_roles = Some(roles.clone());
                }
                let raw_reception_facts = facts
                    .iter()
                    .filter(|fact| fact["kind"] == "reception" && fact["reception"].is_object())
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(previous) = &reception_facts {
                    // Judgment also receives unselected native edges. The
                    // Reception subset must survive with identical facts.
                    for fact in previous {
                        assert!(raw_reception_facts.contains(fact));
                    }
                } else {
                    reception_facts = Some(raw_reception_facts);
                }
                let typed_ids = facts
                    .iter()
                    .filter(|fact| fact["kind"] == "reception" && fact["reception"].is_object())
                    .map(|fact| fact["id"].clone())
                    .collect::<Vec<_>>();
                let bound_ids = clipboard["edges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|edge| edge["fact_id"].clone())
                    .collect::<Vec<_>>();
                assert_eq!(bound_ids, typed_ids);
                let expanded = expand_indexed_reception_clipboard(clipboard.clone());
                for edge in clipboard["edges"].as_array().unwrap() {
                    for (direct, indices) in [
                        ("guest_roles", "guest_role_indices"),
                        ("host_roles", "host_role_indices"),
                    ] {
                        let reconstructed = expanded["edges"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|row| row["fact_id"] == edge["fact_id"])
                            .unwrap();
                        assert_eq!(edge[indices], reconstructed[indices]);
                        if example["stage"] == "judgment" {
                            assert!(edge.get(direct).is_none());
                        } else {
                            assert_eq!(edge[direct], reconstructed[direct]);
                        }
                    }
                }
            } else {
                assert!(input.get("native_reception").is_none());
            }
        }
        assert!(accepted_roles.is_some());
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
    #[test]
    fn input_evaluation_stops_at_real_handoff_without_inventing_a_closing_reply() {
        for device_available in [true, false] {
            let fixture = process_examples_at_boundary(device_available, true).unwrap();
            let calls = fixture["examples"].as_array().unwrap();
            assert!(!calls
                .iter()
                .any(|c| matches!(c["stage"].as_str(), Some("judgment" | "significators"))));
            let conversations = calls
                .iter()
                .filter(|c| c["stage"] == "conversation")
                .count();
            assert_eq!(
                conversations,
                usize::from(!device_available),
                "Only a genuinely missing place needs an inquiry"
            );
            assert_eq!(
                fixture["inputEvaluationBoundary"]["boundary"],
                "ready_reading"
            );
            assert_eq!(
                fixture["inputEvaluationBoundary"]["conversation_executed"],
                false
            );
            assert_eq!(
                fixture["inputEvaluationBoundary"]["reading_executed"],
                false
            );
            assert!(fixture["inputEvaluationBoundary"]["binding"].is_object());
            assert_eq!(
                fixture["canonicalQuestion"],
                "Will I get married in the next year?"
            );
            assert_eq!(fixture["chartMomentMs"], 1789387200000.);
        }
    }
}

#[cfg(test)]
mod selected_stage_execution_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    #[derive(Clone, Copy)]
    enum JudgmentReply {
        Answer,
        MissingContext,
        Invalid,
    }
    struct Reader {
        dir: tempfile::TempDir,
        target: Option<Stage>,
        calls: Mutex<Vec<(Stage, Value, Value)>>,
        reject_first: AtomicBool,
        judgment: JudgmentReply,
    }
    impl Runtime for Reader {
        fn selected_stage_target(&self) -> Option<Stage> {
            self.target
        }
        fn selected_stage_sequence(&self) -> Option<u64> {
            Some(self.calls.lock().unwrap().len() as u64)
        }
        fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
            Ok(Some(place()))
        }
        fn generate(
            &self,
            stage: Stage,
            _matter: Matter,
            input: &Value,
            schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            self.calls
                .lock()
                .unwrap()
                .push((stage, input.clone(), schema.clone()));
            let original = step::original_input(input);
            let answer = match stage {
                Stage::Conversation => {
                    return Err("Unchanged conversation callback intentionally fails".into())
                }
                Stage::Judgment if matches!(self.judgment, JudgmentReply::MissingContext) => {
                    json!({"request_input":{"field":"context",
                        "question":"Which circumstances are you concerned about?",
                        "reason":"The synthetic reader requires additional event context"}})
                }
                Stage::Intake if original["recognition_phase"] == "complete_selected_program" => {
                    json!({
                    "intent":"clarify","question":null,"frame":null,"people":[],
                    "subject":{"name":"Prospective partner","kind":"person","owner_id":"","source_quote":"Will I get married in the next year?"},
                    "updates":[{"field":"baseline","value":"hoped_for","quote":"Will I get married in the next year?","mode":"supply"},
                        {"field":"horizon","value":"within the next year","quote":"in the next year","mode":"supply"}],
                    "heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
                }
                Stage::Intake => {
                    json!({"intent":"read","question":"Will I get married in the next year?",
                    "frame":{"method":"relationship","facet":"event"},"people":[],"subject":null,"updates":[],
                    "heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
                }
                Stage::Significators => {
                    json!({"selections":[{"id":"querent.self","reason":"Synthetic person asking"},
                    {"id":"subject.primary","reason":"Synthetic prospective partner"},
                    {"id":"moon.contextual","reason":"Synthetic contextual Moon role"}],
                    "summary":"Offline execution fixture, no interpretation claim","unknowns":[]})
                }
                Stage::Place => {
                    json!({"mode":"select","place_id":"device","query":"","clarification":"","basis":"Actual synthetic device candidate"})
                }
                _ => {
                    let mut checks = serde_json::Map::new();
                    for key in stage.checks() {
                        checks.insert((*key).into(),json!({"state":"unestablished","evidence":[],"finding":"Offline mechanics only"}));
                    }
                    let mut value = json!({"checks":checks,"summary":"Offline mechanics only; no source-correct reading claimed","unknowns":[]});
                    if stage == Stage::Reception {
                        value["edge_interpretations"] = reception_receipt_fixture(original);
                    }
                    if stage == Stage::Condition {
                        for (facet, key) in [
                            ("essential", "own_dignity"),
                            ("house_capacity", "ability_to_act"),
                        ] {
                            let ids = original["facts"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter(|fact| fact["condition_facet"] == facet)
                                .map(|fact| fact["id"].clone())
                                .collect::<Vec<_>>();
                            value["checks"][key]["evidence"] = json!(ids);
                        }
                        value["checks"]["context_exceptions"]["evidence"] = json!(original
                            ["facts"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|fact| matches!(
                                fact["condition_facet"].as_str(),
                                Some("retrograde" | "solar_opposition")
                            ))
                            .map(|fact| fact["id"].clone())
                            .collect::<Vec<_>>());
                        if self.reject_first.swap(false, Ordering::AcqRel) {
                            value["checks"]["ability_to_act"]["evidence"] = json!([]);
                        }
                    }
                    if stage == Stage::Contacts {
                        value["basis"] = json!("no_candidate_covered");
                        value["candidate_ids"] = json!([]);
                        value["candidate_signs"] = json!([]);
                    }
                    if stage == Stage::Judgment {
                        value["verdict"] = json!("unresolved");
                        value["answer"] =
                            json!(if matches!(self.judgment, JudgmentReply::Invalid) {
                                ""
                            } else {
                                "  Offline mechanics fixture only; no interpretation claim.\n"
                            });
                        value["evidence"] = json!([]);
                        let boundary = original["facts"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|f| f["kind"] == "boundary")
                            .unwrap();
                        value["checks"]["scope_of_answer"]["evidence"] = json!([boundary["id"]]);
                    }
                    value
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
            if self.calls.lock().unwrap().len() > 24 {
                Err("Offline fixture exceeded native guard attempts".into())
            } else {
                Ok(())
            }
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    fn place() -> LocationCandidate {
        LocationCandidate {
            id: "device".into(),
            label: "Synthetic device place".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        }
    }
    fn execute(target: Option<Stage>, reject_first: bool) -> (Session, Reader, Result<(), String>) {
        execute_with(target, reject_first, JudgmentReply::Answer)
    }

    #[test]
    fn explicit_chart_request_calculates_despite_interpretation_limit_and_keeps_anchor() {
        use crate::reading_contracts as contracts;
        use contracts::{Consultation, Evidence, Facet, Frame, Method, Observation, Slot};
        fn resolved<T>(value: T) -> Slot<T> {
            Slot::Resolved {
                observation: Observation {
                    value,
                    evidence: Evidence::Migration {
                        detail: "Synthetic chart execution regression".into(),
                    },
                },
            }
        }
        for facet in [Facet::Event, Facet::Situation] {
            let reader = Reader {
                dir: tempfile::tempdir().unwrap(),
                target: None,
                calls: Mutex::new(vec![]),
                reject_first: AtomicBool::new(false),
                judgment: JudgmentReply::Answer,
            };
            let question = "Will my application launch successfully?";
            let mut session = Session {
                question: question.into(),
                ..Default::default()
            };
            session.method.brief.question = question.into();
            session.method.consultation = Some(Consultation {
                question: resolved(question.into()),
                frame: resolved(Frame {
                    method: Method::Undertaking,
                    facet,
                }),
                subject: resolved(crate::horary_role_options::Subject {
                    name: "My application launch".into(),
                    kind: "other".into(),
                    owner_id: "querent".into(),
                    source_quote: "my application".into(),
                }),
                ..Default::default()
            });
            session.candidates.push(place());
            session.candidate_moment_ms = Some(1789387200000.);
            session.messages.push(Message {
                role: "user".into(),
                text: "continue".into(),
            });
            let previous = session.clone();
            assert!(prepare_reading(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387300000.,
                None,
                &previous
            )
            .unwrap()
            .is_none());
            assert!(
                session.chart.is_none(),
                "An ordinary resume must retain the interpretation limit"
            );
            session.messages.push(Message {
                role: "user".into(),
                text: "OK can you run the chart now".into(),
            });
            let previous = session.clone();
            assert!(prepare_reading(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387400000.,
                None,
                &previous
            )
            .unwrap()
            .is_none());
            assert_eq!(
                session.chart.as_ref().unwrap()["timestampMs"],
                1789387200000.
            );
            assert!(matches!(
                session.method.result,
                Some(contracts::ReadingResult::Limited { .. })
            ));
            assert!(session.sections.is_empty());
            assert!(
                reader.calls.lock().unwrap().is_empty(),
                "No specialist or judgment bypass"
            );
            let chart = session.chart.clone();
            let previous = session.clone();
            prepare_reading(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387500000.,
                None,
                &previous,
            )
            .unwrap();
            assert_eq!(
                session.chart, chart,
                "Repeated request keeps the understood question's sky"
            );
        }
    }

    #[test]
    fn chart_commands_exclude_negation_explanation_and_other_matter() {
        for words in [
            "cast the chart",
            "Please cast the chart",
            "OK can you run the chart now?",
        ] {
            assert!(is_chart_request(words));
        }
        for words in [
            "Don't cast the chart",
            "Why didn't you cast the chart?",
            "Can you explain the chart now",
            "Will I cast the chart successfully?",
        ] {
            assert!(!is_chart_request(words));
        }
    }
    fn execute_with(
        target: Option<Stage>,
        reject_first: bool,
        judgment: JudgmentReply,
    ) -> (Session, Reader, Result<(), String>) {
        let reader = Reader {
            dir: tempfile::tempdir().unwrap(),
            target,
            calls: Mutex::new(vec![]),
            reject_first: AtomicBool::new(reject_first),
            judgment,
        };
        let mut session = Session::default();
        session.messages.push(Message {
            role: "user".into(),
            text: "Will I get married in the next year?".into(),
        });
        session.candidates.push(place());
        session.device_context = Some(crate::conversation::DeviceContext {
            timezone: "America/New_York".into(),
            locale: "en-US".into(),
            latitude: Some(38.657),
            longitude: Some(-77.249),
            accuracy_meters: Some(30.),
        });
        let previous = session.clone();
        let result = run(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1791388800000.,
            None,
            &previous,
        );
        (session, reader, result)
    }
    #[test]
    fn actual_original_question_stops_before_failing_guru_and_independent_siblings() {
        let (session, reader, result) = execute(Some(Stage::Condition), false);
        result.unwrap();
        let calls = reader.calls.lock().unwrap();
        assert!(session.chart.is_some());
        assert!(calls.iter().any(|(s, _, _)| *s == Stage::Intake));
        assert!(calls.iter().any(|(s, _, _)| *s == Stage::Significators));
        assert_eq!(
            calls
                .iter()
                .filter(|(s, _, _)| *s == Stage::Condition)
                .count(),
            1
        );
        assert!(!calls.iter().any(|(s, _, _)| matches!(
            s,
            Stage::Reception | Stage::Contacts | Stage::Judgment | Stage::Conversation
        )));
        assert!(session.method.result.is_none());
        assert!(session
            .audit
            .iter()
            .any(|e| e["event"] == "selected_stage_boundary"));
    }
    #[test]
    fn fresh_current_judgment_is_exact_immediate_reply_without_redundant_conversation() {
        let (session, reader, result) = execute(None, false);
        result.unwrap();
        assert!(!reader
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(stage, _, _)| *stage == Stage::Conversation));
        let crate::reading_contracts::ReadingResult::Judgment {
            binding,
            answer,
            worksheet,
            ..
        } = session.method.result.as_ref().unwrap()
        else {
            panic!("The production judgment must be retained");
        };
        let reply = session.messages.last().unwrap();
        assert_eq!(reply.role, "assistant");
        assert_eq!(
            &reply.text, answer,
            "No trimming, paraphrase or scaffold prose"
        );
        assert!(answer.starts_with("  ") && answer.ends_with('\n'));
        assert_eq!(binding.question, session.question);
        let receipt = session
            .audit
            .iter()
            .find(|event| event["event"] == "judgment_answer_reply")
            .unwrap();
        let index = receipt["record_index"].as_u64().unwrap() as usize;
        let record = &session.method.records[index];
        assert_eq!(record.stage, Stage::Judgment);
        assert!(record.validation_error.is_none());
        assert_eq!(&record.worksheet, worksheet);
        assert_eq!(receipt["binding"], serde_json::to_value(binding).unwrap());
        assert_eq!(receipt["answer_sha256"], lessons::digest(answer));
        assert_eq!(receipt["native_job"]["phase"]["record_index"], index);
        let section = session
            .sections
            .iter()
            .find(|section| section.method_stage == Some(Stage::Judgment))
            .unwrap();
        assert_eq!(&section.body, answer);
        assert_eq!(&section.worksheet, worksheet);
        assert_eq!(section.after_message + 1, session.messages.len());
    }
    #[test]
    fn cached_judgment_on_a_later_turn_keeps_the_conversation_route() {
        let (mut session, reader, result) = execute(None, false);
        result.unwrap();
        let previous = session.clone();
        let judgments = reader
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(stage, _, _)| *stage == Stage::Judgment)
            .count();
        let replies = session
            .audit
            .iter()
            .filter(|event| event["event"] == "judgment_answer_reply")
            .count();
        session.messages.push(Message {
            role: "user".into(),
            text: "continue".into(),
        });
        let result = run(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1791388801000.,
            None,
            &previous,
        );
        assert_eq!(
            result.unwrap_err(),
            "Unchanged conversation callback intentionally fails"
        );
        let calls = reader.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(stage, _, _)| *stage == Stage::Judgment)
                .count(),
            judgments
        );
        assert!(calls
            .iter()
            .any(|(stage, _, _)| *stage == Stage::Conversation));
        assert_eq!(
            session
                .audit
                .iter()
                .filter(|event| event["event"] == "judgment_answer_reply")
                .count(),
            replies
        );
        assert_eq!(session.messages.last().unwrap().text, "continue");
    }
    #[test]
    fn stale_rejected_or_already_delivered_judgment_cannot_supply_a_new_reply() {
        let (completed, reader, result) = execute(None, false);
        result.unwrap();
        let index = completed.method.records.len() - 1;
        let mut before_delivery = completed.clone();
        before_delivery.messages.pop().unwrap();
        for change in 0..8 {
            let mut session = before_delivery.clone();
            match change {
                0 => session.method.consultation.as_mut().unwrap().revision += 1,
                1 => {
                    session
                        .sections
                        .iter_mut()
                        .find(|s| s.method_stage == Some(Stage::Judgment))
                        .unwrap()
                        .revision += 1
                }
                2 => {
                    session.method.records[index].validation_error =
                        Some("Synthetic native rejection".into())
                }
                3 => {
                    session.method.records[index].input["reading_request"]["binding"]
                        ["case_revision"] = json!(999)
                }
                4 => session.messages.push(Message {
                    role: "user".into(),
                    text: "A new follow-up needs a new reply".into(),
                }),
                5 => session.method.result = None,
                6 => session.question = "A different original question".into(),
                7 => session.method.records[index].raw.push_str("changed"),
                _ => unreachable!(),
            }
            let snapshot = serde_json::to_value(&session).unwrap();
            assert!(
                crate::horary_conversation::deliver_fresh_answer(&mut session, &reader, index)
                    .is_err(),
                "Invalid current answer {change}"
            );
            assert_eq!(
                serde_json::to_value(&session).unwrap(),
                snapshot,
                "A rejected delivery must not mutate the native state"
            );
        }
        let mut twice = completed;
        let messages = twice.messages.len();
        assert!(
            crate::horary_conversation::deliver_fresh_answer(&mut twice, &reader, index).is_err()
        );
        assert_eq!(twice.messages.len(), messages);
    }
    #[test]
    fn incomplete_judgment_keeps_its_real_inquiry_and_conversation_route() {
        let (session, reader, result) = execute_with(None, false, JudgmentReply::MissingContext);
        assert_eq!(
            result.unwrap_err(),
            "Unchanged conversation callback intentionally fails"
        );
        assert!(matches!(
            session.method.result,
            Some(crate::reading_contracts::ReadingResult::NeedsInformation { .. })
        ));
        assert!(session
            .method
            .flow
            .pending
            .iter()
            .any(|pending| pending.stage == Stage::Judgment));
        assert!(reader
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(stage, _, _)| *stage == Stage::Conversation));
        assert!(!session
            .audit
            .iter()
            .any(|event| event["event"] == "judgment_answer_reply"));
        assert!(!session
            .sections
            .iter()
            .any(|section| section.method_stage == Some(Stage::Judgment)));
    }
    #[test]
    fn failed_judgment_preserves_native_repair_and_never_delivers_invalid_prose() {
        let (session, reader, result) = execute_with(None, false, JudgmentReply::Invalid);
        assert!(result.is_err());
        let records = session
            .method
            .records
            .iter()
            .filter(|record| record.stage == Stage::Judgment)
            .collect::<Vec<_>>();
        assert!(
            records.len() > 1,
            "The original native repair route remains in use"
        );
        assert!(records
            .iter()
            .all(|record| record.validation_error.is_some()));
        assert!(session.method.result.is_none());
        assert!(!session
            .audit
            .iter()
            .any(|event| event["event"] == "judgment_answer_reply"));
        assert!(!reader
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(stage, _, _)| *stage == Stage::Conversation));
    }
    #[test]
    fn actual_rejected_then_repaired_condition_has_original_job_and_effective_guide() {
        let (session, reader, result) = execute(Some(Stage::Condition), true);
        result.unwrap();
        let calls = reader.calls.lock().unwrap();
        let attempts = calls
            .iter()
            .filter(|(s, _, _)| *s == Stage::Condition)
            .collect::<Vec<_>>();
        assert_eq!(attempts.len(), 2);
        assert_eq!(step::original_input(&attempts[1].1), &attempts[0].1);
        let boundary = session
            .audit
            .iter()
            .find(|e| e["event"] == "selected_stage_boundary")
            .unwrap();
        let records = session
            .method
            .records
            .iter()
            .filter(|r| r.stage == Stage::Condition)
            .collect::<Vec<_>>();
        assert!(records[0].validation_error.is_some());
        assert!(records[1].validation_error.is_none());
        assert_eq!(
            attempts[1].1["native_validation_error"],
            json!(records[0].validation_error.as_ref().unwrap())
        );
        assert_eq!(
            boundary["native_job"]["inputSha256"],
            records[0].input_sha256
        );
        assert_eq!(boundary["native_job"]["phase"]["state"], "complete");
        assert_eq!(boundary["record_input_sha256"], records[1].input_sha256);
        assert_eq!(
            records[1].guide_sha256,
            crate::horary_lessons::digest(
                &reader
                    .guide_for(Stage::Condition, Matter::Relationship, &attempts[1].1)
                    .unwrap()
            )
        );
    }
    #[test]
    fn non_applicable_location_does_not_invent_execution_or_call_guru() {
        let (session, reader, result) = execute(Some(Stage::Location), false);
        result.unwrap();
        assert!(!session
            .audit
            .iter()
            .any(|e| e["event"] == "selected_stage_boundary"));
        assert!(session
            .audit
            .iter()
            .any(|e| e["event"] == "selected_stage_uninvoked"));
        assert!(!reader
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(s, _, _)| matches!(s, Stage::Location | Stage::Conversation | Stage::Judgment)));
    }
    #[test]
    fn significators_and_judgment_stop_only_after_their_actual_native_acceptance() {
        for target in [Stage::Significators, Stage::Judgment] {
            let (session, reader, result) = execute(Some(target), false);
            result.unwrap();
            let boundary = session
                .audit
                .iter()
                .find(|e| e["event"] == "selected_stage_boundary")
                .unwrap();
            assert_eq!(boundary["target_stage"], json!(target));
            assert_eq!(session.method.records.last().unwrap().stage, target);
            assert!(session
                .method
                .records
                .last()
                .unwrap()
                .validation_error
                .is_none());
            assert!(session.method.result.is_none());
            let calls = reader.calls.lock().unwrap();
            assert!(!calls
                .iter()
                .any(|(stage, _, _)| *stage == Stage::Conversation));
            if target == Stage::Significators {
                assert!(!calls.iter().any(|(stage, _, _)| *stage == Stage::Condition));
            } else {
                for stage in [Stage::Condition, Stage::Reception, Stage::Contacts] {
                    assert!(calls.iter().any(|(actual, _, _)| *actual == stage));
                }
            }
        }
    }
    #[test]
    fn actual_stage_inputs_keep_contacts_mechanical_and_judgment_context_complete() {
        let (session, reader, result) = execute(Some(Stage::Judgment), false);
        result.unwrap();
        let calls = reader.calls.lock().unwrap();
        let contacts = &calls
            .iter()
            .find(|(stage, _, _)| *stage == Stage::Contacts)
            .unwrap()
            .1;
        let judgment = &calls
            .iter()
            .find(|(stage, _, _)| *stage == Stage::Judgment)
            .unwrap()
            .1;
        let roles: Vec<Role> = serde_json::from_value(contacts["roles"].clone()).unwrap();
        let all = reading_method::facts(session.chart.as_ref());
        let context = contact_evidence(&all, &roles);
        let mechanics = contacts["facts"].as_array().unwrap();
        assert!(context.iter().any(|fact| fact.kind == "condition"));
        assert!(context.iter().any(|fact| fact.kind == "reception"));
        assert!(mechanics
            .iter()
            .all(|fact| fact["kind"] != "condition" && fact["kind"] != "reception"));
        for fact in &context {
            let expected = serde_json::to_value(fact).unwrap();
            assert!(
                judgment["facts"].as_array().unwrap().contains(&expected),
                "Final judgment lost {}",
                fact.id
            );
            if matches!(
                fact.kind.as_str(),
                "event" | "moon" | "position" | "house" | "boundary"
            ) {
                assert!(mechanics.contains(&expected), "Contacts lost {}", fact.id);
            }
        }
        assert!(mechanics.len() < context.len());
    }

    #[test]
    fn reading_handoff_reaches_roles_and_every_subsequent_job_stage_with_source_context() {
        use crate::reading_contracts::{
            self as contracts, Field, Frame, Intent, Method, Update, UpdateMode,
        };
        let words = "I applied for a laboratory job through a recruiter. There is no offer yet. Will I get this job?";
        let mut case = contracts::Consultation::default();
        let mut patch = contracts::control(Intent::Read);
        patch.question = Some("Will I get this job?".into());
        patch.frame = Some(Frame {
            method: Method::NewJob,
            facet: contracts::Facet::Event,
        });
        patch.subject = Some(crate::horary_role_options::Subject {
            name: "laboratory job".into(),
            kind: "job".into(),
            owner_id: "querent".into(),
            source_quote: "laboratory job".into(),
        });
        patch.updates = vec![Update {
            field: Field::JobContext,
            value: "laboratory job".into(),
            quote: "laboratory job".into(),
            mode: UpdateMode::Supply,
        }];
        case.apply(&patch, 1, words, false).unwrap();
        let anchor = contracts::Anchor {
            timestamp_ms: 1791478980000.,
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
        };
        let ready = contracts::ReadyReading::prepare(&case, anchor.clone()).unwrap();
        let mut session = Session {
            question: ready.binding().question.clone(),
            chart: Some(
                horary_ai_core::astronomy::chart(
                    anchor.timestamp_ms,
                    anchor.latitude,
                    anchor.longitude,
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        session.method.brief = ready.brief();
        session.method.consultation = Some(case);
        let reader = Reader {
            dir: tempfile::tempdir().unwrap(),
            target: Some(Stage::Judgment),
            calls: Mutex::new(vec![]),
            reject_first: AtomicBool::new(false),
            judgment: JudgmentReply::Answer,
        };
        generate_reading(&mut session, &reader, ready).unwrap();
        let calls = reader.calls.lock().unwrap();
        for stage in [
            Stage::Significators,
            Stage::Condition,
            Stage::Reception,
            Stage::Contacts,
            Stage::Judgment,
        ] {
            let input = &calls
                .iter()
                .find(|(actual, _, _)| *actual == stage)
                .unwrap()
                .1;
            assert_eq!(
                input["reading_request"]["contextual_words"]["original_question"]["quote"],
                words
            );
            assert_eq!(
                input["reading_request"]["binding"]["question"],
                "Will I get this job?"
            );
            assert_eq!(
                input["reading_request"]["inputs"]["job_context"],
                "laboratory job"
            );
        }
        assert!(!calls
            .iter()
            .any(|(stage, _, _)| *stage == Stage::Intake || *stage == Stage::Conversation));
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
                    max_tokens: step::generation_token_budget(stage, input),
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
                    prompt(*stage, *matter, input, contract)
                        .map(|p| (p, step::generation_token_budget(*stage, input)))
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
    process_examples_at_boundary(device_available, false)
}

#[cfg(test)]
pub(crate) fn process_examples_at_boundary(
    device_available: bool,
    input_only: bool,
) -> Result<Value, String> {
    process_examples_with_chart(
        device_available,
        input_only,
        !input_only && device_available,
    )
}

/// Explicit generator entry: captures the same authored scheduler's native
/// chart before the documentation-only fixture substitution is enabled.
#[cfg(test)]
pub(crate) fn capture_process_reference_chart() -> Result<Value, String> {
    let mut captured = process_examples_with_chart(true, false, false)?;
    if captured["canonicalQuestion"] != "Will I get married in the next year?"
        || captured["chartMomentMs"] != json!(1789387200000.)
        || captured["nativeDefaults"]["place"] != "device coordinates"
    {
        return Err("Authored documentation question or anchor changed; do not mislabel its chart provenance".into());
    }
    captured
        .as_object_mut()
        .and_then(|object| object.remove("capturedNativeChart"))
        .filter(Value::is_object)
        .ok_or_else(|| "Authored scheduler did not produce its native chart".into())
}

#[cfg(test)]
fn process_examples_with_chart(
    device_available: bool,
    input_only: bool,
    freeze_documentation_chart: bool,
) -> Result<Value, String> {
    use std::sync::Mutex;
    struct Fixture {
        dir: tempfile::TempDir,
        requests: Mutex<Vec<Value>>,
        chart_fixture: Option<crate::process_reference::DocumentationChart>,
        chart_installed: std::sync::atomic::AtomicBool,
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
                    if stage == Stage::Reception {
                        answer["edge_interpretations"] = reception_receipt_fixture(input);
                    }
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
                        answer["checks"]["context_exceptions"]["evidence"] = json!(original
                            ["facts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|fact| matches!(
                                fact["condition_facet"].as_str(),
                                Some("retrograde" | "solar_opposition")
                            ))
                            .map(|fact| fact["id"].clone())
                            .collect::<Vec<_>>());
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
        fn publish(&self, session: &mut Session) -> Result<(), String> {
            // This Runtime is an authored documentation fixture, not the app
            // or model evaluator. Fix only its supplied astronomical chart,
            // before the real scheduler derives specialist facts and prompts.
            // No prompt, contract, accepted worksheet or output is normalized.
            if session.chart.is_some()
                && !self
                    .chart_installed
                    .load(std::sync::atomic::Ordering::Relaxed)
            {
                if let Some(chart) = &self.chart_fixture {
                    chart.install(session)?;
                    self.chart_installed
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
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
        chart_fixture: freeze_documentation_chart
            .then(crate::process_reference::documentation_chart)
            .transpose()?,
        chart_installed: std::sync::atomic::AtomicBool::new(false),
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
    let execute = if input_only { run_elicitation } else { run };
    execute(
        &mut session,
        &runtime,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )?;
    let first_session = input_only.then(|| session.clone());
    if !device_available {
        let previous = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "Woodbridge, Virginia, United States.".into(),
        });
        execute(
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
    let mut fixture = json!({"authorship":"Synthetic inputs and authored worksheet outputs, captured from the actual runtime scheduler; no model invoked.","examples":requests,"visibleProposedAnswer":session.sections.last().map(|s|&s.body),"chartMomentMs":session.chart.as_ref().map(|c|&c["timestampMs"]),"canonicalQuestion":session.question,"horizon":session.method.brief.horizon,"inputEvaluationBoundary":session.audit.iter().rev().find(|e|e["event"]=="input_evaluation_boundary"),"nativeDefaults":{"place":if device_available{"device coordinates"}else{"geocoded stated city"},"moment":"1789387200000, understood question receipt instant","noPlaceOrMomentModelCall":true}});
    if let Some(chart) = &runtime.chart_fixture {
        fixture["astronomicalFixture"] = chart.provenance();
    } else if !input_only && device_available {
        // Returned only by the explicit chart-capture generator. Normal
        // documentation and input-only/missing-device fixtures omit this.
        fixture["capturedNativeChart"] = session
            .chart
            .clone()
            .ok_or("Missing native fixture chart")?;
    }
    if input_only {
        fixture["first_session"] = json!(first_session);
        fixture["accepted_session"] = json!(session);
    }
    Ok(fixture)
}

#[cfg(test)]
mod initial_extractor_tests {
    use super::*;
    use crate::reading_contracts::Method;
    use std::sync::Mutex;
    struct Reader {
        dir: tempfile::TempDir,
        calls: Mutex<Vec<Value>>,
        target: Method,
        refine: bool,
    }
    impl Runtime for Reader {
        fn initial_extractor_target(&self) -> Option<Method> {
            Some(self.target)
        }
        fn initial_extractor_sequence(&self) -> Option<u64> {
            Some(self.calls.lock().unwrap().len() as u64)
        }
        fn generate(
            &self,
            stage: Stage,
            _matter: Matter,
            input: &Value,
            _schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            assert_eq!(
                stage,
                Stage::Intake,
                "Fixed actors must not execute in a component evaluation"
            );
            self.calls.lock().unwrap().push(input.clone());
            let original = crate::horary_step::original_input(input);
            let focused = original["recognition_phase"] == "complete_selected_program";
            let words = original["latest_words"].as_str().unwrap();
            let answer = if !focused {
                json!({"intent":"read","question":words,"frame":{"method":"relationship","facet":"event"},"people":[],"subject":null,"updates":[],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
            } else if self.refine {
                json!({"intent":"clarify","question":null,"frame":{"method":"contact","facet":"event"},"people":[],"subject":null,"updates":[],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
            } else {
                json!({"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"Prospective partner","kind":"person","owner_id":"","source_quote":words},
                    "updates":[{"field":"baseline","value":"hoped_for","quote":words,"mode":"supply"},{"field":"horizon","value":"within the next year","quote":"in the next year","mode":"supply"}],
                    "heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null})
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
        fn generate_batch(
            &self,
            _tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            panic!("No fixed batch actor may execute")
        }
        fn publish(&self, _session: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn check(&self) -> Result<(), String> {
            if self.calls.lock().unwrap().len() < 4 {
                Ok(())
            } else {
                Err("Authored fixture exceeded its initial boundary".into())
            }
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    fn run_fixture(target: Method, refine: bool) -> (Session, Vec<Value>) {
        let reader = Reader {
            dir: tempfile::tempdir().unwrap(),
            calls: Mutex::new(vec![]),
            target,
            refine,
        };
        let mut session = Session::default();
        let previous = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: if refine {
                "Will Sam contact me?"
            } else {
                "Will I get married in the next year?"
            }
            .into(),
        });
        run_elicitation(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1789387200000.,
            None,
            &previous,
        )
        .unwrap();
        (session, reader.calls.into_inner().unwrap())
    }
    #[test]
    fn initial_boundary_is_after_native_apply_and_before_any_fixed_actor() {
        let (session, calls) = run_fixture(Method::Relationship, false);
        assert_eq!(calls.len(), 2);
        assert_eq!(session.messages.len(), 1);
        assert!(session.chart.is_none());
        let event = session
            .audit
            .iter()
            .find(|e| e["event"] == "initial_extractor_boundary")
            .unwrap();
        assert_eq!(event["last_sequence"], 2);
        assert_eq!(event["after_message"], 1);
        assert_eq!(
            event["accepted_consultation"],
            json!(session.method.consultation)
        );
        assert_eq!(
            session
                .method
                .consultation
                .as_ref()
                .unwrap()
                .text(crate::reading_contracts::Field::Horizon),
            Some("within the next year")
        );
        let index = event["record_index"].as_u64().unwrap() as usize;
        assert!(session.method.records[index].validation_error.is_none());
        assert_eq!(event["native_job"]["phase"]["record_index"], index);
    }
    #[test]
    fn alternate_upstream_method_stays_uninvoked_without_gold_or_guru() {
        let (session, calls) = run_fixture(Method::Contact, false);
        assert_eq!(calls.len(), 1);
        assert_eq!(session.messages.len(), 1);
        assert!(!session
            .audit
            .iter()
            .any(|e| e["event"] == "initial_extractor_boundary"));
        assert_eq!(
            session.method.consultation.as_ref().unwrap().method(),
            Some(Method::Relationship)
        );
    }
    #[test]
    fn initial_extractor_owns_sourced_correction_of_provisional_frame() {
        let (session, calls) = run_fixture(Method::Relationship, true);
        assert_eq!(calls.len(), 2);
        assert_eq!(
            session.method.consultation.as_ref().unwrap().method(),
            Some(Method::Contact)
        );
        let event = session
            .audit
            .iter()
            .find(|e| e["event"] == "initial_extractor_boundary")
            .unwrap();
        assert_eq!(event["target_method"], "relationship");
        assert_eq!(
            event["accepted_consultation"]["frame"]["observation"]["value"]["method"],
            "contact"
        );
    }
}

#[cfg(test)]
mod supplying_extractor_tests {
    use super::*;
    use crate::reading_contracts::{Method, RequirementKey};
    use std::sync::Mutex;
    struct Reader {
        dir: tempfile::TempDir,
        calls: Mutex<Vec<(Stage, Value)>>,
        repair: bool,
    }
    impl Runtime for Reader {
        fn supplying_extractor_target(&self) -> Option<Method> {
            Some(Method::MovableDeal)
        }
        fn supplying_extractor_sequence(&self) -> Option<u64> {
            Some(self.calls.lock().unwrap().len() as u64)
        }
        fn generate(
            &self,
            stage: Stage,
            _matter: Matter,
            input: &Value,
            _schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            let mut calls = self.calls.lock().unwrap();
            let updates = calls
                .iter()
                .filter(|(stage, input)| {
                    *stage == Stage::Intake
                        && crate::horary_step::original_input(input)["recognition_phase"]
                            == "update_selected_program"
                })
                .count();
            calls.push((stage, input.clone()));
            let original = crate::horary_step::original_input(input);
            let phase = original["recognition_phase"].as_str();
            if input.get("native_validation_error").is_some() {
                println!(
                    "Offline synthetic repair: {}",
                    input["native_validation_error"]
                );
            }
            let mut answer = json!({"intent":"clarify","question":null,"frame":null,"people":[],"subject":null,"updates":[],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null});
            match (stage, phase) {
                (Stage::Intake, Some("classify_question")) => {
                    answer["intent"] = json!("read");
                    answer["question"] = original["latest_words"].clone();
                    answer["frame"] = json!({"method":"movable_deal","facet":"profit"});
                }
                (Stage::Intake, Some("complete_selected_program")) => {
                    answer["subject"] = json!({"name":"books","kind":"movable","owner_id":"","source_quote":"these books"});
                    answer["updates"] = json!([{"field":"deal_capacity","value":"sell","quote":"selling","mode":"supply"},{"field":"deal_actor","value":"querent","quote":"I am selling","mode":"supply"},{"field":"deal_beneficiary","value":"querent","quote":"me","mode":"supply"}]);
                }
                (Stage::Intake, Some("update_selected_program")) => {
                    assert_eq!(
                        original["pending_requirement"],
                        json!(RequirementKey::Owner)
                    );
                    if self.repair && updates == 0 {
                        answer["updates"] = json!([{"field":"subject","value":"querent","quote":"mine","mode":"supply"}]);
                    } else {
                        answer["subject"] = json!({"name":"books","kind":"movable","owner_id":"querent","source_quote":"These books are mine"});
                    }
                }
                (Stage::Conversation, _) => {
                    assert_eq!(
                        updates, 0,
                        "No later guru may execute after supplying acceptance"
                    );
                    let reminder = input["reminders"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|r| r["key"] == json!(RequirementKey::Owner))
                        .unwrap();
                    answer = json!({"reply":"Who owns these books?","ask":reminder["id"]});
                }
                _ => panic!("No chart or later fixed actor belongs to this component: {stage:?}"),
            }
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
        fn generate_batch(
            &self,
            _tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            panic!("No fixed batch should execute")
        }
        fn publish(&self, _s: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn check(&self) -> Result<(), String> {
            if self.calls.lock().unwrap().len() < 8 {
                Ok(())
            } else {
                Err("Authored offline call cap".into())
            }
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    #[test]
    fn supplying_original_question_reaches_real_inquiry_then_repairs_and_stops_before_later_guru() {
        let reader = Reader {
            dir: tempfile::tempdir().unwrap(),
            calls: Mutex::new(vec![]),
            repair: true,
        };
        let mut session = Session::default();
        let initial = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "I am selling these books. Would the sale be profitable for me?".into(),
        });
        run_elicitation(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1789387200000.,
            None,
            &initial,
        )
        .unwrap();
        assert_eq!(
            session.method.consultation.as_ref().unwrap().requested,
            Some(RequirementKey::Owner)
        );
        assert!(!session
            .audit
            .iter()
            .any(|e| e["event"] == "supplying_extractor_boundary"));
        let before = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "These books are mine".into(),
        });
        run_elicitation(
            &mut session,
            &reader,
            &GeocodeState::default(),
            1789387260000.,
            None,
            &before,
        )
        .unwrap();
        assert!(session.chart.is_none());
        assert_eq!(session.question, before.question);
        assert_eq!(session.candidate_moment_ms, before.candidate_moment_ms);
        assert_eq!(
            session
                .method
                .consultation
                .as_ref()
                .unwrap()
                .subject
                .resolved()
                .unwrap()
                .owner_id,
            "querent"
        );
        let event = session
            .audit
            .iter()
            .find(|e| e["event"] == "supplying_extractor_boundary")
            .unwrap();
        let index = event["record_index"].as_u64().unwrap() as usize;
        assert_eq!(event["native_job"]["phase"]["record_index"], index);
        assert_eq!(
            event["before_consultation"],
            json!(before.method.consultation)
        );
        assert!(session.method.records[index - 1].validation_error.is_some());
        assert!(session.method.records[index].validation_error.is_none());
        let calls = reader.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(stage, _)| *stage == Stage::Conversation)
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(stage, input)| *stage == Stage::Intake
                    && crate::horary_step::original_input(input)["recognition_phase"]
                        == "update_selected_program")
                .count(),
            2
        );
    }
}
