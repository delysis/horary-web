//! The model speaks; the scaffold owns facts, readiness and pending reminders.
#![forbid(unsafe_code)]
use crate::{
    conversation::{Message, Session},
    horary_executor::task,
    horary_lessons::Stage,
    horary_pipeline::Runtime,
    reading_contracts::{
        Anchor, Field, InformationNeed, Need, ReadingResult, ReadyReading, RequirementKey,
    },
};
use serde_json::{json, Value};

fn chart_anchor(session: &Session) -> Option<Anchor> {
    session
        .chart
        .as_ref()
        .zip(session.place.as_ref())
        .and_then(|(chart, place)| {
            Some(Anchor {
                timestamp_ms: chart["timestampMs"].as_f64()?,
                latitude: place.latitude,
                longitude: place.longitude,
                timezone: place.timezone.clone(),
            })
        })
}

/// A chart alone is not a completed answer. The retained interpretation must
/// still belong to the current consultation and its visible main passage.
pub(crate) fn current_answer(session: &Session) -> Option<&str> {
    let ReadingResult::Judgment {
        binding,
        answer,
        worksheet,
        ..
    } = session.method.result.as_ref()?
    else {
        return None;
    };
    let ready = ReadyReading::prepare(
        session.method.consultation.as_ref()?,
        chart_anchor(session)?,
    )
    .ok()?;
    if ready.binding() != binding
        || answer.trim().is_empty()
        || !session.sections.iter().any(|section| {
            section.method_stage == Some(Stage::Judgment)
                && section.revision == session.revision
                && section.body == *answer
                && section.worksheet == *worksheet
                && section.after_message.max(1) <= session.messages.len()
        })
    {
        return None;
    }
    Some(answer)
}

/// The newly accepted Judgment is itself model-authored human-facing prose.
/// Publish that exact answer without a second model rewriting or withholding
/// it. The caller supplies only a record accepted during this execution.
pub(crate) fn deliver_fresh_answer(
    session: &mut Session,
    runtime: &impl Runtime,
    record_index: usize,
) -> Result<(), String> {
    let answer = current_answer(session)
        .ok_or("No current accepted interpretation can be delivered")?
        .to_owned();
    let Some(ReadingResult::Judgment {
        binding, worksheet, ..
    }) = &session.method.result
    else {
        return Err("An unfinished reading cannot supply an immediate answer".into());
    };
    let record = session
        .method
        .records
        .get(record_index)
        .ok_or("The immediate answer lacks its accepted native record")?;
    let jobs = session.method.flow.jobs.iter().filter(|job| {
        job.stage == Stage::Judgment && job.revision == session.revision
            && matches!(job.phase(), crate::horary_step::Phase::Complete { record_index: index } if *index == record_index)
    }).collect::<Vec<_>>();
    if record.stage != Stage::Judgment
        || record.revision != session.revision
        || binding.question != session.question
        || record.validation_error.is_some()
        || record.worksheet != *worksheet
        || record.raw != record.generation.content
        || crate::horary_step::original_input(&record.input)["reading_request"]["binding"]
            != serde_json::to_value(binding).map_err(|e| e.to_string())?
        || jobs.len() != 1
        || session.messages.last().is_none_or(|m| m.role != "user")
        || !session.sections.iter().any(|section| {
            section.method_stage == Some(Stage::Judgment)
                && section.revision == session.revision
                && section.after_message == session.messages.len()
                && section.body == answer
                && section.worksheet == *worksheet
        })
    {
        return Err(
            "The immediate answer is stale or differs from its current native acceptance".into(),
        );
    }
    runtime.check()?;
    session.audit.push(json!({"event":"judgment_answer_reply","record_index":record_index,
        "revision":session.revision,"binding":binding,"native_job":jobs[0],
        "record_input_sha256":record.input_sha256,"guide_sha256":record.guide_sha256,
        "schema_sha256":record.schema_sha256,"generation_sha256":crate::horary_lessons::digest(&record.raw),
        "answer_sha256":crate::horary_lessons::digest(&answer),"after_message":session.messages.len()+1,
        "authorship":"Exact newly accepted model-authored Judgment.answer; no later Conversation generated",
        "speech":"Existing voice handoff; spoken delivery not asserted by this receipt"}));
    session.messages.push(Message {
        role: "assistant".into(),
        text: answer,
    });
    runtime.publish(session)
}

pub(crate) fn interruption_reply(session: &Session, cancelled: bool) -> &'static str {
    if cancelled {
        "We can pause here. Your chart and completed passages are kept; say ‘continue’ when you’re ready."
    } else if current_answer(session).is_some() {
        "Your completed answer is kept. I couldn't finish this reply."
    } else if session.chart.is_some() {
        "The chart is kept, but the reading hasn't finished. Say ‘continue’ and I'll pick up the unfinished step."
    } else {
        "Your question is kept. I couldn't finish this step; say ‘continue’ to try it again."
    }
}

pub(crate) fn clipboard(session: &Session) -> Value {
    let anchor = chart_anchor(session);
    let device = session.candidates.iter().any(|p| p.provider == "device");
    let plan = session
        .method
        .consultation
        .as_ref()
        .map(|case| case.plan(anchor.as_ref()));
    let mut needs: Vec<Need> = plan
        .as_ref()
        .map(|plan| plan.needs.clone())
        .unwrap_or_default();
    needs.retain(|need| {
        !(need.key == RequirementKey::ChartPlace
            && (anchor.is_some()
                || session.place.is_some()
                || device && need.state == "native_acquisition_pending"))
    });
    // Native place and moment work can wait independently. Keep both typed
    // requests selectable; a single prior result must not erase its sibling.
    // Ordinary fact requests continue to be governed by the current plan.
    for pending in &session.method.flow.pending {
        let key = match (pending.stage, pending.request.field.as_str()) {
            (Stage::Place, "chart_place") => RequirementKey::ChartPlace,
            (Stage::Moment, "chart_moment") => RequirementKey::ChartMoment,
            _ => continue,
        };
        if let Some(existing) = needs.iter_mut().find(|need| need.key == key) {
            existing.reason.clone_from(&pending.request.reason);
            existing.question.clone_from(&pending.request.question);
        } else {
            needs.push(Need {
                key,
                state: "native_stage_awaiting_input".into(),
                reason: pending.request.reason.clone(),
                question: pending.request.question.clone(),
            });
        }
    }
    // A failed native anchor check can be more specific than the catalogue's
    // generic question. Preserve that reason, including DST folds and gaps.
    if let Some(ReadingResult::NeedsInformation { need }) = &session.method.result {
        if let Some(existing) = needs.iter_mut().find(|n| n.key == need.key) {
            existing.reason = need.reason.clone();
            if let Some(question) = &need.question {
                existing.question = question.clone();
            }
        } else if matches!(
            need.key,
            RequirementKey::ChartPlace | RequirementKey::ChartMoment
        ) {
            // These may reflect a more precise native check than the
            // catalogue, e.g. an invalid explicit time despite an old chart.
            // Ordinary fact needs must still occur in the current plan. Their
            // previous-turn result cannot resurrect a resolved field.
            needs.push(Need {
                key: need.key.clone(),
                state: "requested_by_reading".into(),
                reason: need.reason.clone(),
                question: need.question.clone().unwrap_or_default(),
            });
        }
    }
    if let Some(requested) = session
        .method
        .consultation
        .as_ref()
        .and_then(|c| c.requested.as_ref())
    {
        needs.sort_by_key(|need| need.key != *requested);
    }
    let reminders: Vec<_> = needs
        .iter()
        .enumerate()
        .map(|(i, need)| {
            json!({
                "id":format!("need_{i}"),"key":need.key,"state":need.state,
                "reason":need.reason,"example_question":need.question
            })
        })
        .collect();
    let dialogue: Vec<_> = session.messages.iter().rev().take(12).rev().collect();
    let event_context = json!({
        "place":session.method.consultation.as_ref().and_then(|c|c.facts.get(&Field::EventPlace)),
        "time":session.method.consultation.as_ref().and_then(|c|c.facts.get(&Field::EventTime)),
        "use":"These describe the event being judged, not the chart anchor. Ask about them when they matter to understanding or judging the question; device coordinates do not resolve them."
    });
    let specialists: Vec<_> = session
        .method
        .records
        .iter()
        .filter(|record| {
            let binding =
                &crate::horary_step::original_input(&record.input)["reading_request"]["binding"];
            let current_binding = !binding.is_object()
                || session.method.consultation.as_ref().is_some_and(|case| {
                    binding["catalogue_version"] == case.catalogue_version
                        && binding["case_revision"].as_u64() == Some(case.revision)
                });
            record.revision == session.revision
                && current_binding
                && record.validation_error.is_none()
                && !matches!(record.stage, Stage::Intake | Stage::Conversation)
                && record.worksheet.get("request_input").is_none()
        })
        .map(|r| json!({"stage":r.stage,"finding":r.worksheet}))
        .collect();
    let result_kind = session.method.result.as_ref().map(|result| match result {
        ReadingResult::Judgment { .. } => "judgment",
        ReadingResult::NeedsInformation { .. } => "needs_information",
        ReadingResult::Limited { .. } => "limited",
    });
    json!({"latest_words":session.messages.iter().rev().find(|m|m.role=="user").map(|m|&m.text),
        "dialogue":dialogue,"canonical_question":session.question,
        "consultation":session.method.consultation.as_ref().map(|c|c.recognition_snapshot()),"reminders":reminders,
        "method_limit":plan.as_ref().and_then(|p|p.limitation.as_ref()),
        "result":session.method.result,"chart_context":anchor,
        "event_context":event_context,
        "anchor_policy":"For an ordinary consultation, use the reader's location and when the question became clear. Device place is this app's default reader location, not the event venue. An explicit earlier consultation or corrected reader location requires its own verified anchor.",
        "chart_state":if anchor.is_some(){"cast"}else{"not_cast"},
        "state_reminder":if anchor.is_some(){"A chart exists; distinguish its facts from unfinished judgment."}else{"No chart has been cast. Discuss the question or the book's method, never findings from this chart."},
        // respond() is the end of this turn, after native preparation has
        // stopped. A user request or an old job is not scheduled future work.
        "action_state":{"after_reply":"wait_for_user","scheduled_actions":[],
            "chart_available":anchor.is_some(),"answer_available":current_answer(session).is_some(),
            "result":result_kind,
            "authority":"This reply does not schedule chart casting, preparation or a reading. Report completed work only when available; explain an actual limit or obtain a genuine missing answer."},
        "reader_place":session.place,"device_place_available":device,
        "specialist_findings":specialists,"unfinished_requests":session.method.flow.pending,
        "authority":"The clipboard owns accepted facts and readiness. This response speaks to the person; it cannot authorize a chart or unsupported judgment."})
}

/// An unresolved native request requires an inquiry, while the reader may
/// choose among independent current gaps. Binding does not certify the prose.
fn required_ask(input: &Value) -> Option<&str> {
    if input["result"]["result"] != "needs_information" || !input["method_limit"].is_null() {
        return None;
    }
    let key = &input["result"]["need"]["key"];
    if !key.is_object() {
        return None;
    }
    input["reminders"]
        .as_array()?
        .iter()
        .find(|reminder| reminder["key"] == *key && is_actionable(input, reminder))?["id"]
        .as_str()
        .filter(|id| !id.is_empty())
}

pub(crate) fn validate_ask(input: &Value, value: &Value) -> Result<(), String> {
    if required_ask(input).is_some() {
        let allowed = actionable_asks(input);
        if !value["ask"]
            .as_str()
            .is_some_and(|ask| allowed.contains(&ask))
        {
            return Err(format!(
                "The current native request is still answerable and unresolved. Bind ask to one of these current actionable reminders: {allowed:?}, and obtain its answer in your own words. You may choose the order of genuine gaps. No sourced unavailability or method limit permits abandoning them. The binding alone is not evidence that your prose asks the right question."
            ));
        }
    }
    Ok(())
}

/// A narrow backstop for explicit first-person work claims. The native action
/// boundary and state-selected instruction are primary; this does not certify
/// all conversational meaning or treat a phrase match as a scheduled action.
pub(crate) fn validate_work_claim(input: &Value, value: &Value) -> Result<(), String> {
    let Some(reply) = value["reply"].as_str() else {
        return Ok(()); // The response schema owns its type.
    };
    for clause in reply.split(['.', '!', '?', ';', ':', '\n']) {
        let normalized = clause.to_lowercase().replace('’', "'");
        let words: Vec<_> = normalized
            .split(|c: char| !c.is_alphabetic() && c != '\'')
            .filter(|word| !word.is_empty())
            .collect();
        for (start, word) in words.iter().enumerate() {
            let (offset, completed) = match (*word, words.get(start + 1).copied()) {
                ("i", Some("will" | "am")) | ("we", Some("will" | "are")) => (2, false),
                ("i", Some("have")) | ("we", Some("have")) => (2, true),
                ("i" | "we", Some("cast" | "calculated" | "drew" | "prepared")) => (1, true),
                ("i'll" | "i'm" | "we'll" | "we're", _) => (1, false),
                ("i've" | "we've", _) => (1, true),
                ("let", Some("me" | "us")) => (2, false),
                ("let's", _) => (1, false),
                _ => continue,
            };
            // Conditional capability and explicit negation do not announce
            // execution. Keep them usable for honest explanations of a gap.
            if words[..start]
                .iter()
                .any(|w| matches!(*w, "if" | "once" | "when" | "unless"))
            {
                continue;
            }
            let work = &words[start + offset..];
            let Some(action) = work.iter().take(5).position(|w| {
                matches!(
                    *w,
                    "cast"
                        | "casting"
                        | "calculate"
                        | "calculating"
                        | "calculated"
                        | "draw"
                        | "drawing"
                        | "drew"
                        | "prepare"
                        | "preparing"
                        | "prepared"
                        | "begin"
                        | "beginning"
                        | "start"
                        | "starting"
                )
            }) else {
                continue;
            };
            let Some(object) = work
                .iter()
                .skip(action + 1)
                .take(8)
                .position(|w| matches!(*w, "chart" | "reading"))
            else {
                continue;
            };
            let end = action + 1 + object;
            if work[..=end]
                .iter()
                .any(|w| matches!(*w, "not" | "never" | "no"))
                || work[end + 1..]
                    .iter()
                    .any(|w| matches!(*w, "if" | "once" | "when" | "unless"))
            {
                continue;
            }
            let chart = work[end] == "chart";
            if completed && chart && input["chart_state"] == "cast" {
                continue;
            }
            if completed && !chart && input["action_state"]["answer_available"] == true {
                continue;
            }
            return Err(format!(
                "The reply claims chart casting or reading preparation without its native authority. chart_state={}; this conversation ends the turn and schedules no further calculation. Report only actual completed work, explain the current method limit if present, or ask for a genuine bound missing answer. Do not promise casting/preparation or imply that work continues after this reply.",
                input["chart_state"].as_str().unwrap_or("unknown")
            ));
        }
    }
    Ok(())
}

fn is_actionable(input: &Value, reminder: &Value) -> bool {
    matches!(
        reminder["state"].as_str(),
        Some(
            "missing"
                | "proposed"
                | "conflicting"
                | "conflicting_parties"
                | "conflicting_actor_roles"
                | "unclassified"
                | "explicit_historical_anchor_missing"
                | "requested_by_reading"
                | "native_stage_awaiting_input"
                | "native_acquisition_pending"
                | "invalid_anchor"
        )
    ) && reminder["key"].is_object()
        && !input["consultation"]["unavailable"]
            .as_array()
            .is_some_and(|notes| notes.iter().any(|note| note["key"] == reminder["key"]))
}

fn actionable_asks(input: &Value) -> Vec<&str> {
    input["reminders"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|reminder| is_actionable(input, reminder))
        .filter_map(|reminder| reminder["id"].as_str().filter(|id| !id.is_empty()))
        .collect()
}

pub(crate) fn schema(input: &Value) -> Value {
    let choices = if required_ask(input).is_some() {
        actionable_asks(input)
            .into_iter()
            .map(|id| json!(id))
            .collect()
    } else {
        let mut choices = vec![json!("")];
        choices.extend(
            input["reminders"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.get("id"))
                .cloned(),
        );
        choices
    };
    json!({"type":"object","properties":{
        "ask":{"type":"string","enum":choices,"description":"Choose the exact missing-answer reminder before composing reply. Empty means no fact is being elicited. The spoken question must obtain this reminder's answer, not a different detail."},
        "reply":{"type":"string","maxLength":1600,"description":"Speak naturally to the person within action_state: no chart casting or reading preparation follows this reply. Explain an actual limit without a preparation promise. If ask is nonempty, obtain that exact missing answer in one contextual question, preserving known names and circumstances."}
    },"required":["reply","ask"],"additionalProperties":false})
}

pub(crate) fn respond(session: &mut Session, runtime: &impl Runtime) -> Result<(), String> {
    let input = clipboard(session);
    let Some(data) = task(session, runtime, Stage::Conversation, input.clone(), &[])? else {
        return Err("The conversational reader has not delivered a reply.".into());
    };
    let value = data.worksheet();
    let selected = input["reminders"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|need| need["id"] == value["ask"]);
    if let Some(case) = session.method.consultation.as_mut() {
        case.requested = selected
            .map(|n| serde_json::from_value(n["key"].clone()))
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(need) = selected {
            if !matches!(session.method.result, Some(ReadingResult::Limited { .. })) {
                session.method.result = Some(ReadingResult::NeedsInformation {
                    need: InformationNeed {
                        key: serde_json::from_value(need["key"].clone())
                            .map_err(|e| e.to_string())?,
                        reason: need["reason"].as_str().unwrap_or("").into(),
                        question: Some(value["reply"].as_str().unwrap_or("").into()),
                    },
                });
            }
        }
    }
    session.audit.push(json!({"event":"conversation_reminder_selected","ask":value["ask"],
        "key":selected.map(|n|&n["key"]),"authority":"model reply; scaffold fact ownership retained"}));
    session.messages.push(Message {
        role: "assistant".into(),
        text: value["reply"]
            .as_str()
            .ok_or("The reader omitted its reply")?
            .trim()
            .into(),
    });
    runtime.publish(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geocode::LocationCandidate, horary_lessons::Matter, horary_step, reading_contracts,
    };

    fn resolved<T>(value: T) -> reading_contracts::Slot<T> {
        reading_contracts::Slot::Resolved {
            observation: reading_contracts::Observation {
                value,
                evidence: reading_contracts::Evidence::Migration {
                    detail: "Authored native response-contract regression".into(),
                },
            },
        }
    }

    fn complete_case_session() -> Session {
        let mut case = reading_contracts::Consultation {
            question: resolved("Will I sell these books?".into()),
            frame: resolved(reading_contracts::Frame {
                method: reading_contracts::Method::MovableDeal,
                facet: reading_contracts::Facet::Event,
            }),
            subject: resolved(crate::horary_role_options::Subject {
                name: "books".into(),
                kind: "movable".into(),
                owner_id: String::new(),
                source_quote: "these books".into(),
            }),
            ..reading_contracts::Consultation::default()
        };
        case.facts
            .insert(Field::DealActor, resolved("querent".into()));
        case.facts
            .insert(Field::DealCapacity, resolved("sell".into()));
        let mut session = Session {
            question: "Will I sell these books?".into(),
            chart: Some(json!({"timestampMs":1789387200000_f64})),
            place: Some(LocationCandidate {
                id: "regression-reader-place".into(),
                label: "Woodbridge".into(),
                name: "Woodbridge".into(),
                country: "US".into(),
                latitude: 38.657,
                longitude: -77.249,
                timezone: "America/New_York".into(),
                provider: "device".into(),
            }),
            ..Session::default()
        };
        ReadyReading::prepare(&case, chart_anchor(&session).unwrap()).unwrap();
        session.method.consultation = Some(case);
        session
    }

    fn pending_owner_input() -> Value {
        let mut session = complete_case_session();
        let anchor = chart_anchor(&session);
        let question = "Would selling these books be profitable for me?".to_string();
        session.question = question.clone();
        let case = session.method.consultation.as_mut().unwrap();
        case.question = resolved(question);
        case.frame = resolved(reading_contracts::Frame {
            method: reading_contracts::Method::MovableDeal,
            facet: reading_contracts::Facet::Profit,
        });
        case.facts
            .insert(Field::DealBeneficiary, resolved("querent".into()));
        let need = case
            .plan(anchor.as_ref())
            .needs
            .into_iter()
            .find(|need| need.key == RequirementKey::Owner)
            .unwrap();
        session.method.result = Some(ReadingResult::NeedsInformation {
            need: InformationNeed {
                key: need.key,
                reason: need.reason,
                question: Some(need.question),
            },
        });
        clipboard(&session)
    }

    #[test]
    fn limited_question_has_no_casting_authority_despite_available_device_place() {
        let mut session = complete_case_session();
        session.chart = None;
        session.candidates.push(session.place.clone().unwrap());
        let case = session.method.consultation.as_mut().unwrap();
        case.frame = resolved(reading_contracts::Frame {
            method: reading_contracts::Method::Undertaking,
            facet: reading_contracts::Facet::Event,
        });
        let plan = case.plan(None);
        let limitation = plan.limitation.as_ref().unwrap();
        session.method.result = Some(ReadingResult::Limited {
            limitation: limitation.into(),
        });
        let input = clipboard(&session);
        assert_eq!(input["chart_state"], "not_cast");
        assert_eq!(input["device_place_available"], true);
        assert_eq!(input["action_state"]["scheduled_actions"], json!([]));
        assert_eq!(input["action_state"]["result"], "limited");
        assert_eq!(input["action_state"]["chart_available"], false);
        assert_eq!(required_ask(&input), None);
        assert!(horary_step::check(Stage::Conversation, Matter::Other,
            &json!({"reply":"I haven't cast a chart, and I haven't established an answer to that undertaking.","ask":""}),
            &input, &[]).is_ok());
        for reply in [
            "I'm casting the chart now.",
            "I'll prepare your reading.",
            "I have cast your chart.",
            "I cast the chart.",
            "Let me calculate the chart for you.",
        ] {
            let error = horary_step::check(
                Stage::Conversation,
                Matter::Other,
                &json!({"reply":reply,"ask":""}),
                &input,
                &[],
            )
            .err()
            .unwrap();
            assert!(
                error.contains("schedules no further calculation"),
                "{reply}: {error}"
            );
        }
        assert!(session.chart.is_none());
        assert!(matches!(
            session.method.result,
            Some(ReadingResult::Limited { .. })
        ));
    }

    #[test]
    fn completed_chart_allows_factual_report_but_not_unscheduled_preparation() {
        let input = clipboard(&complete_case_session());
        assert_eq!(input["chart_state"], "cast");
        assert_eq!(input["action_state"]["answer_available"], false);
        validate_work_claim(
            &input,
            &json!({"reply":"I have cast the chart, but haven't established an answer."}),
        )
        .unwrap();
        for reply in [
            "I'll calculate another chart now.",
            "I'm preparing your reading.",
            "I have prepared your reading.",
        ] {
            assert!(validate_work_claim(&input, &json!({"reply":reply})).is_err());
        }
        let mut completed = input;
        completed["action_state"]["answer_available"] = json!(true);
        validate_work_claim(&completed, &json!({"reply":"I have prepared the reading."})).unwrap();
    }

    #[test]
    fn work_claim_repair_retains_original_authority_and_actual_ask_binding() {
        let mut input = pending_owner_input();
        input["chart_state"] = json!("not_cast");
        input["action_state"]["chart_available"] = json!(false);
        let rejected =
            json!({"reply":"I'll cast the chart now. Who owns the books?","ask":"need_0"});
        assert!(
            horary_step::check(Stage::Conversation, Matter::Other, &rejected, &input, &[]).is_err()
        );
        let repair =
            json!({"original_input":input,"previous_worksheet":rejected,"chart_state":"cast"});
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"I have cast the chart. Who owns the books?","ask":"need_0"}),
            &repair,
            &[]
        )
        .is_err());
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Before I weigh this sale, who owns the books?","ask":"need_0"}),
            &repair,
            &[]
        )
        .is_ok());
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Who owns the books?","ask":""}),
            &repair,
            &[]
        )
        .is_err());
    }

    #[test]
    fn negative_conditional_and_method_explanations_do_not_announce_work() {
        let input = json!({"chart_state":"not_cast","action_state":{"scheduled_actions":[]}});
        for reply in [
            "I'm not casting a chart yet.",
            "I have not prepared a reading.",
            "If the question is clear, I will cast the chart using the reader's place.",
            "I will prepare a reading when those necessary facts are available.",
            "I cast charts for the moment a question becomes clear.",
            "I can't judge that outcome with the available procedure.",
        ] {
            validate_work_claim(&input, &json!({"reply":reply})).unwrap();
        }
    }

    #[test]
    fn answerable_owner_request_cannot_be_abandoned_or_retargeted() {
        let mut input = pending_owner_input();
        assert_eq!(required_ask(&input), Some("need_0"));
        assert_eq!(
            schema(&input)["properties"]["ask"]["enum"],
            json!(["need_0"])
        );
        input["reminders"].as_array_mut().unwrap().push(json!({
            "id":"need_1","key":{"kind":"field","id":"context"},"state":"resolved"}));
        for ask in ["", "need_1"] {
            let reply = json!({"reply":"I will leave that ownership open for now.","ask":ask});
            assert!(
                horary_step::check(Stage::Conversation, Matter::Other, &reply, &input, &[])
                    .err()
                    .unwrap()
                    .contains("current native request is still answerable")
            );
        }
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Who owns the books being sold?","ask":"need_0"}),
            &input,
            &[],
        )
        .is_ok());
    }

    #[test]
    fn captured_training_owner_gap_requires_binding_without_scripting_the_question() {
        let input: Value = serde_json::from_str(include_str!(
            "../test-fixtures/training-conversation-owner-reminder-20261011.json"
        ))
        .unwrap();
        assert_eq!(required_ask(&input), Some("need_0"));
        let abandoned = json!({"reply":"That's all right. Ownership changes whose possessions we're looking at, so I'll leave that part open for now.","ask":""});
        assert!(
            horary_step::check(Stage::Conversation, Matter::Other, &abandoned, &input, &[])
                .is_err()
        );
        for reply in [
            "Who owns the goods being offered?",
            "Before I weigh the sale for you, whose stock is being sold?",
        ] {
            assert!(horary_step::check(
                Stage::Conversation,
                Matter::Other,
                &json!({"reply":reply,"ask":"need_0"}),
                &input,
                &[],
            )
            .is_ok());
        }
        // Fixing the binding alone cannot qualify the unchanged acknowledgement.
        let mut merely_bound = abandoned;
        merely_bound["ask"] = json!("need_0");
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &merely_bound,
            &input,
            &[],
        )
        .is_ok());
    }

    #[test]
    fn reader_can_choose_the_order_of_independent_current_gaps() {
        let mut input = pending_owner_input();
        input["reminders"].as_array_mut().unwrap().push(json!({
            "id":"need_1","key":{"kind":"chart_place"},"state":"missing"}));
        assert_eq!(
            schema(&input)["properties"]["ask"]["enum"],
            json!(["need_0", "need_1"])
        );
        for ask in ["need_0", "need_1"] {
            validate_ask(&input, &json!({"ask":ask})).unwrap();
        }
        assert!(validate_ask(&input, &json!({"ask":""})).is_err());
        input["consultation"]["unavailable"] = json!([{"key":{"kind":"chart_place"}}]);
        assert!(validate_ask(&input, &json!({"ask":"need_1"})).is_err());
        assert_eq!(
            schema(&input)["properties"]["ask"]["enum"],
            json!(["need_0"])
        );
    }

    #[test]
    fn unavailable_limits_completed_and_stale_needs_do_not_force_inquiry() {
        let input = pending_owner_input();
        let mut unavailable = input.clone();
        unavailable["reminders"][0]["state"] = json!("unavailable");
        let mut recorded_unavailable = input.clone();
        recorded_unavailable["consultation"]["unavailable"] = json!([{"key":{"kind":"owner"}}]);
        let mut limited = input.clone();
        limited["method_limit"] = json!({"code":"judgment_program_needs_review"});
        let mut alternative = input.clone();
        alternative["result"] =
            json!({"result":"limited","limitation":{"code":"answer_facet_not_implemented"}});
        let mut completed = input.clone();
        completed["result"] = json!({"result":"judgment"});
        completed["reminders"] = json!([]);
        let mut resolved = input.clone();
        resolved["reminders"][0]["state"] = json!("resolved");
        let mut wrong_key = input.clone();
        wrong_key["reminders"][0]["key"] = json!({"kind":"field","id":"context"});
        let mut internal_binding = input;
        internal_binding["reminders"][0]["state"] = json!("unbound_person");
        for input in [
            unavailable,
            recorded_unavailable,
            limited,
            alternative,
            completed,
            resolved,
            wrong_key,
            internal_binding,
        ] {
            assert_eq!(required_ask(&input), None);
            assert!(horary_step::check(
                Stage::Conversation,
                Matter::Other,
                &json!({"reply":"I can discuss what remains possible.","ask":""}),
                &input,
                &[],
            )
            .is_ok());
        }
    }

    #[test]
    fn proposed_and_conflicting_inputs_bind_clarification_to_the_actual_need() {
        for state in [
            "proposed",
            "conflicting",
            "conflicting_parties",
            "conflicting_actor_roles",
        ] {
            let mut input = pending_owner_input();
            input["reminders"][0]["state"] = json!(state);
            assert_eq!(required_ask(&input), Some("need_0"));
            assert!(validate_ask(&input, &json!({"ask":""})).is_err());
            validate_ask(&input, &json!({"ask":"need_0"})).unwrap();
        }
    }

    #[test]
    fn ask_binding_is_not_a_certificate_that_the_prose_obtains_the_answer() {
        let input = pending_owner_input();
        // Semantic review must still reject this acknowledgement as elicitation.
        // The native guard proves the binding only; it makes no inquiry grade.
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"I will leave that part open for now.","ask":"need_0"}),
            &input,
            &[],
        )
        .is_ok());
    }

    fn completed_answer_session() -> Session {
        let mut session = complete_case_session();
        session.messages.push(crate::conversation::Message {
            role: "user".into(),
            text: session.question.clone(),
        });
        let ready = ReadyReading::prepare(
            session.method.consultation.as_ref().unwrap(),
            chart_anchor(&session).unwrap(),
        )
        .unwrap();
        let answer =
            "An authored accepted-answer storage fixture; no horary inference is asserted.";
        let worksheet = json!({"answer":answer});
        session.method.result = Some(ReadingResult::Judgment {
            binding: ready.binding().clone(),
            verdict: "unresolved".into(),
            answer: answer.into(),
            evidence: vec![],
            worksheet: worksheet.clone(),
        });
        session.sections.push(
            serde_json::from_value(json!({
            "title":"Fixture answer","body":answer,"evidence":[],"revision":session.revision,
            "method_stage":"judgment","worksheet":worksheet}))
            .unwrap(),
        );
        session
    }

    #[test]
    fn completed_answer_interruption_is_distinct_from_unfinished_chart_or_reply() {
        let mut session = completed_answer_session();
        let answer = current_answer(&session).unwrap().to_string();
        let reply = interruption_reply(&session, false);
        assert!(reply.contains("completed answer is kept"));
        assert!(!reply.contains("reading hasn't finished"));
        assert!(
            !reply.contains(answer.as_str()),
            "Do not fabricate a new reply from an older answer"
        );
        assert_eq!(current_answer(&session), Some(answer.as_str()));
        assert!(interruption_reply(&session, true).contains("We can pause here"));
        session.method.result = None;
        assert!(interruption_reply(&session, false).contains("reading hasn't finished"));
    }

    #[test]
    fn retained_answer_must_have_a_renderable_message_position() {
        let mut session = completed_answer_session();
        assert!(current_answer(&session).is_some());
        session.sections[0].after_message = session.messages.len() + 1;
        assert!(current_answer(&session).is_none());
        session.sections[0].after_message = 0; // The renderer uses the first message.
        assert!(current_answer(&session).is_some());
        session.messages.clear();
        assert!(current_answer(&session).is_none());
    }

    #[test]
    fn stale_binding_or_mismatched_main_passage_is_not_a_completed_answer() {
        let mut changed = completed_answer_session();
        changed.method.consultation.as_mut().unwrap().facts.insert(
            Field::Context,
            resolved("New accepted circumstances require another judgment".into()),
        );
        assert!(current_answer(&changed).is_none());
        let mut stale = completed_answer_session();
        stale.method.consultation.as_mut().unwrap().revision += 1;
        assert!(current_answer(&stale).is_none());
        let mut passage = completed_answer_session();
        passage.sections[0].revision += 1;
        assert!(current_answer(&passage).is_none());
        passage = completed_answer_session();
        passage.sections[0].body = "Different prose".into();
        assert!(current_answer(&passage).is_none());
        passage = completed_answer_session();
        passage.sections[0].worksheet = json!({});
        assert!(current_answer(&passage).is_none());
    }

    #[test]
    fn reader_anchor_and_event_context_remain_distinct_in_the_gurus_clipboard() {
        let mut session = Session {
            question: "Will Bob sell his fish at the market?".into(),
            place: Some(LocationCandidate {
                id: "device-location".into(),
                label: "Woodbridge".into(),
                name: "Woodbridge".into(),
                country: "US".into(),
                latitude: 38.657,
                longitude: -77.249,
                timezone: "America/New_York".into(),
                provider: "device".into(),
            }),
            chart: Some(json!({"timestampMs":1789387200000_f64})),
            ..Session::default()
        };
        session.candidates.push(session.place.clone().unwrap());
        let mut case = reading_contracts::Consultation::default();
        for (field, value, quote) in [
            (
                Field::EventPlace,
                "Bozeman, Montana",
                "The market is in Bozeman, Montana",
            ),
            (Field::EventTime, "Friday at three", "Friday at three"),
        ] {
            case.facts.insert(
                field,
                reading_contracts::Slot::Resolved {
                    observation: reading_contracts::Observation {
                        value: value.into(),
                        evidence: reading_contracts::Evidence::User {
                            turn: 1,
                            quote: quote.into(),
                        },
                    },
                },
            );
        }
        session.method.consultation = Some(case);
        let input = clipboard(&session);
        assert_eq!(input["chart_state"], "cast");
        assert_eq!(input["chart_context"]["latitude"], 38.657);
        assert_eq!(input["chart_context"]["timezone"], "America/New_York");
        assert_eq!(input["chart_context"]["timestamp_ms"], 1789387200000_f64);
        assert_eq!(
            input["event_context"]["place"]["observation"]["value"],
            "Bozeman, Montana"
        );
        assert_eq!(
            input["event_context"]["time"]["observation"]["value"],
            "Friday at three"
        );
        assert_eq!(
            input["event_context"]["place"]["observation"]["evidence"]["source"],
            "user"
        );
        session.method.consultation.as_mut().unwrap().facts.clear();
        session.chart = None;
        session.place = None;
        let pending = clipboard(&session);
        assert_eq!(pending["chart_state"], "not_cast");
        assert_eq!(pending["device_place_available"], true);
        assert!(pending["event_context"]["place"].is_null());
        assert!(pending["event_context"]["time"].is_null());
        assert!(pending["chart_context"].is_null());
    }
    #[test]
    fn reader_cannot_request_a_nonexistent_fact_or_change_clipboard_data() {
        let input = json!({"reminders":[{"id":"need_0","key":{"field":"reader_place"}}]});
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Where are you as we talk?","ask":"need_0"}),
            &input,
            &[]
        )
        .is_ok());
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Tell me the chart data.","ask":"invented"}),
            &input,
            &[]
        )
        .is_err());
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":"Done.","ask":"","question":"Will Bob sell fish?"}),
            &input,
            &[]
        )
        .is_err());
        assert!(horary_step::check(
            Stage::Conversation,
            Matter::Other,
            &json!({"reply":" ","ask":""}),
            &input,
            &[]
        )
        .is_err());
    }
}
