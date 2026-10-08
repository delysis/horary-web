//! The model speaks; the scaffold owns facts, readiness and pending reminders.
#![forbid(unsafe_code)]
use crate::{
    conversation::{Message, Session},
    horary_executor::task,
    horary_lessons::Stage,
    horary_pipeline::Runtime,
    reading_contracts::{Anchor, Field, InformationNeed, Need, ReadingResult, RequirementKey},
};
use serde_json::{json, Value};

pub(crate) fn clipboard(session: &Session) -> Value {
    let anchor = session
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
        });
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
    // A failed native anchor check can be more specific than the catalogue's
    // generic question. Preserve that reason, including DST folds and gaps.
    if let Some(ReadingResult::NeedsInformation { need }) = &session.method.result {
        if let Some(existing) = needs.iter_mut().find(|n| n.key == need.key) {
            existing.reason = need.reason.clone();
            if let Some(question) = &need.question {
                existing.question = question.clone();
            }
        } else {
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
    json!({"latest_words":session.messages.iter().rev().find(|m|m.role=="user").map(|m|&m.text),
        "dialogue":dialogue,"canonical_question":session.question,
        "consultation":session.method.consultation.as_ref().map(|c|c.recognition_snapshot()),"reminders":reminders,
        "method_limit":plan.as_ref().and_then(|p|p.limitation.as_ref()),
        "result":session.method.result,"chart_context":anchor,
        "event_context":event_context,
        "anchor_policy":"For an ordinary consultation, use the reader's location and when the question became clear. Device place is this app's default reader location, not the event venue. An explicit earlier consultation or corrected reader location requires its own verified anchor.",
        "chart_state":if anchor.is_some(){"cast"}else{"not_cast"},
        "state_reminder":if anchor.is_some(){"A chart exists; distinguish its facts from unfinished judgment."}else{"No chart has been cast. Discuss the question or the book's method, never findings from this chart."},
        "reader_place":session.place,"device_place_available":device,
        "specialist_findings":specialists,"unfinished_requests":session.method.flow.pending,
        "authority":"The clipboard owns accepted facts and readiness. This response speaks to the person; it cannot authorize a chart or unsupported judgment."})
}

pub(crate) fn schema(input: &Value) -> Value {
    let mut choices = vec![json!("")];
    choices.extend(
        input["reminders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.get("id"))
            .cloned(),
    );
    json!({"type":"object","properties":{
        "reply":{"type":"string","maxLength":1600},
        "ask":{"type":"string","enum":choices}
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
