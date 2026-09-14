//! The model chooses bounded tools; Rust owns places, charts, revisions and disk.
#![forbid(unsafe_code)]
use crate::{
    geocode::{geocode_with_cache, GeocodeRequest, GeocodeState, LocationCandidate},
    hf_cache::AcquisitionState,
    native_llama_worker::{
        generate_native, start_native_llama_in_dir, NativeGenerateOptions, NativeLlamaState,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const FILE: &str = "conversation.json";
const PROMPT: &str = include_str!("conversation_prompt.txt");
const BOOK: &str = include_str!("conversation_method.txt");

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub messages: Vec<Message>,
    pub question: String,
    pub chart: Option<Value>,
    pub place: Option<LocationCandidate>,
    pub sections: Vec<Section>,
    pub revisions: Vec<Revision>,
    pub audit: Vec<Value>,
    pub revision: u64,
    #[serde(default)]
    pub snapshot_id: u64,
    #[serde(default)]
    pub chart_after_message: usize,
    #[serde(skip_deserializing)]
    pub status: String,
    #[serde(skip_deserializing)]
    pub busy: bool,
    #[serde(skip)]
    candidates: Vec<LocationCandidate>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub body: String,
    pub evidence: Vec<String>,
    pub revision: u64,
    #[serde(default)]
    pub after_message: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Revision {
    pub number: u64,
    pub question: String,
    pub chart: Option<Value>,
    pub sections: Vec<Section>,
    pub place: Option<LocationCandidate>,
}

#[derive(Default)]
pub struct ConversationState {
    session: Mutex<Option<Session>>,
    pub cancelled: Arc<AtomicBool>,
    busy: AtomicBool,
}
struct Lease<'a>(&'a AtomicBool);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Say {
        message: String,
    },
    FindPlace {
        query: String,
    },
    CastChart {
        question: String,
        place_id: String,
        local_time: String,
        occurrence: String,
    },
    WriteScroll {
        title: String,
        body: String,
        evidence: Vec<String>,
    },
    ReadEvidence,
    RestoreReading {
        revision: u64,
    },
    NewQuestion {
        question: String,
    },
}

fn schema(session: &Session) -> String {
    let text = |max| json!({"type":"string","maxLength":max});
    let variant = |name: &str, fields: Vec<(&str, Value)>| {
        let mut props = serde_json::Map::new();
        props.insert("action".into(), json!({"const":name}));
        let mut required = vec!["action"];
        for (key, value) in fields {
            props.insert(key.into(), value);
            required.push(key);
        }
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    let mut actions = vec![
        variant("say", vec![("message", text(1200))]),
        variant("find_place", vec![("query", text(100))]),
        variant("read_evidence", vec![]),
    ];
    // A conversational turn should leave room for the person's next thought.
    // The model cannot wander through more tools after two new passages.
    if session
        .sections
        .iter()
        .filter(|s| s.after_message == session.messages.len())
        .count()
        >= 2
    {
        return json!({"oneOf":[variant("say",vec![("message",text(700))])]}).to_string();
    }
    let ids: Vec<&str> = session
        .candidates
        .iter()
        .chain(session.place.iter())
        .map(|p| p.id.as_str())
        .collect();
    if !ids.is_empty()
        && (session.chart.is_none() || session.chart_after_message != session.messages.len())
    {
        actions.push(variant(
            "cast_chart",
            vec![
                ("question", text(500)),
                ("place_id", json!({"enum":ids})),
                ("local_time", text(30)),
                ("occurrence", json!({"enum":["","earlier","later"]})),
            ],
        ));
    }
    if session.chart.is_some() {
        let ids: Vec<String> = (0..evidence(session).len())
            .map(|i| format!("e{i}"))
            .collect();
        actions.push(variant(
            "write_scroll",
            vec![
                ("title", text(80)),
                ("body", text(900)),
                (
                    "evidence",
                    json!({"type":"array","items":{"enum":ids},"minItems":1,"maxItems":8}),
                ),
            ],
        ));
        if session.chart_after_message != session.messages.len() {
            actions.push(variant("new_question", vec![("question", text(500))]));
        }
    }
    if !session.revisions.is_empty() {
        actions.push(variant(
            "restore_reading",
            vec![(
                "revision",
                json!({"enum":session.revisions.iter().map(|r|r.number).collect::<Vec<_>>()}),
            )],
        ));
    }
    json!({"oneOf":actions}).to_string()
}

impl ConversationState {
    fn load(&self, dir: &Path) -> Result<Session, String> {
        let mut slot = self
            .session
            .lock()
            .map_err(|_| "Conversation unavailable")?;
        if slot.is_none() {
            let path = dir.join(FILE);
            let session = if path.exists() {
                if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
                    return Err("Conversation file is too large to open safely.".into());
                }
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Could not read the saved conversation: {e}"))?
            } else {
                Session::default()
            };
            *slot = Some(session);
        }
        let mut session = slot.as_ref().ok_or("Conversation unavailable")?.clone();
        session.busy = self.busy.load(Ordering::Acquire);
        Ok(session)
    }
    fn publish(&self, session: &mut Session, dir: &Path) -> Result<(), String> {
        session.snapshot_id = session
            .snapshot_id
            .checked_add(1)
            .ok_or("Conversation sequence exhausted")?;
        let bytes = serde_json::to_vec(&*session).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("This conversation is full. Your existing reading is saved.".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(dir.join(FILE)).map_err(|e| e.to_string())?;
        *self
            .session
            .lock()
            .map_err(|_| "Conversation unavailable")? = Some(session.clone());
        Ok(())
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Stopped. Tell me what you’d like to change.".into())
        } else {
            Ok(())
        }
    }
}

pub fn prepare_reader(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let acquisition = app.state::<AcquisitionState>();
    if !acquisition.0.status(&dir).map_err(|e| e.message)?.ready {
        acquisition.0.begin().map_err(|e| e.message)?;
        acquisition.0.run(&dir).map_err(|e| e.message)?;
    }
    let state = app.state::<ConversationState>();
    state.check()?;
    start_native_llama_in_dir(
        &dir,
        &app.state::<NativeLlamaState>(),
        serde_json::from_value(
            json!({"modelId":"gemma-4-12b-qat","ctxSize":16384,"nGpuLayers":"auto"}),
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.message)?;
    Ok(())
}

pub fn transcribe(app: &tauri::AppHandle, audio: Vec<u8>) -> Result<String, String> {
    let state = app.state::<ConversationState>();
    if state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("Wait for the current reply to stop.".into());
    }
    let _lease = Lease(&state.busy);
    state.cancelled.store(false, Ordering::Release);
    prepare_reader(app)?;
    let prompt=json!([{"role":"user","content":"Transcribe the spoken words in this audio faithfully. Output only the transcript, without commentary, interpretation, or answers. If no intelligible speech is present, output [inaudible]."}]).to_string();
    let answer = generate_native(
        &app.state::<NativeLlamaState>(),
        prompt,
        NativeGenerateOptions {
            audio: Some(audio),
            max_tokens: 1200,
            temperature: 0.,
            cancel: Some(state.cancelled.clone()),
            ..Default::default()
        },
    )
    .map_err(|e| e.message)?;
    state.check()?;
    let text = answer.content.trim();
    if text.is_empty() || text == "[inaudible]" {
        return Err(
            "I couldn’t make out the words. Please try again, or type your question.".into(),
        );
    }
    Ok(text.into())
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.
}
fn evidence(session: &Session) -> Vec<String> {
    let mut facts = vec!["John Frawley, The Horary Textbook (2005); editorial method notes supplied in the system prompt.".to_string()];
    if let Some(chart) = &session.chart {
        facts.extend(horary_ai_core::book_method::evidence(chart));
        for house in chart["houses"].as_array().into_iter().flatten() {
            facts.push(format!("cusp: {house}"));
        }
        for body in chart["bodies"].as_array().into_iter().flatten() {
            facts.push(format!("position: {body}"));
        }
        facts.push(format!("events: {}", chart["derived"]["eventSearch"]));
        facts.push(format!("limits: {}", chart["calculationNote"]));
    }
    facts
}
fn evidence_index(session: &Session) -> Value {
    json!(evidence(session)
        .iter()
        .enumerate()
        .map(|(i, text)| json!({"id":format!("e{i}"),"text":text}))
        .collect::<Vec<_>>())
}

fn section_prose(title: &str, body: &str) -> String {
    let body = body.trim();
    let prose = body
        .split_once('\n')
        .filter(|(first, _)| {
            first.starts_with('#')
                && first
                    .trim_start_matches('#')
                    .trim()
                    .eq_ignore_ascii_case(title.trim())
        })
        .map_or(body, |(_, rest)| rest.trim());
    prose.replace("**", "")
}

fn execute(
    session: &mut Session,
    action: Action,
    geocode: &GeocodeState,
    instant: f64,
) -> Result<Value, String> {
    match action {
        Action::FindPlace { query } => {
            session.candidates = geocode_with_cache(
                geocode,
                GeocodeRequest {
                    query,
                    limit: Some(5),
                },
            )
            .map_err(|e| e.message)?;
            Ok(json!(session.candidates))
        }
        Action::CastChart {
            question,
            place_id,
            local_time,
            occurrence,
        } => {
            if question.trim().is_empty() {
                return Err("Clarify the question before casting.".into());
            }
            let place = session
                .candidates
                .iter()
                .chain(session.place.iter())
                .find(|p| p.id == place_id)
                .cloned()
                .ok_or("Resolve the place with find_place first; never invent an ID.")?;
            let moment = if local_time.is_empty() {
                session
                    .chart
                    .as_ref()
                    .and_then(|c| c["timestampMs"].as_f64())
                    .unwrap_or(instant)
            } else {
                horary_ai_core::chart_input::resolve_chart_time(
                    &local_time,
                    &place.timezone,
                    &occurrence,
                )?
            };
            let chart = horary_ai_core::astronomy::chart(moment, place.latitude, place.longitude)?;
            if session.chart.as_ref() == Some(&chart) && session.question == question {
                return Ok(json!({"unchanged":true,"revision":session.revision}));
            }
            if session.chart.is_some() {
                session.revisions.push(Revision {
                    number: session.revision,
                    question: session.question.clone(),
                    chart: session.chart.clone(),
                    sections: session.sections.clone(),
                    place: session.place.clone(),
                });
            }
            session.revision += 1;
            session.chart_after_message = session.messages.len();
            session.question = question;
            session.chart = Some(chart);
            session.place = Some(place);
            session.sections.clear();
            Ok(
                json!({"revision":session.revision,"calculated":true,"evidence":"Current evidence is included in verified state."}),
            )
        }
        Action::WriteScroll {
            title,
            body,
            evidence: ids,
        } => {
            if session.chart.is_none() {
                return Err("Calculate the chart before writing a judgment.".into());
            }
            let facts = evidence(session);
            if ids.is_empty()
                || ids.iter().any(|id| {
                    id.strip_prefix('e')
                        .and_then(|s| s.parse::<usize>().ok())
                        .is_none_or(|i| i >= facts.len())
                })
            {
                return Err("Use evidence IDs from the current chart only.".into());
            }
            if title.trim().is_empty() || body.trim().is_empty() {
                return Err("A scroll section needs a title and explanation.".into());
            }
            let section = Section {
                body: section_prose(&title, &body),
                title,
                evidence: ids,
                revision: session.revision,
                after_message: session.messages.len(),
            };
            if let Some(old) = session
                .sections
                .iter_mut()
                .find(|s| s.title == section.title)
            {
                *old = section;
            } else if session.sections.len() < 12 {
                session.sections.push(section);
            } else {
                return Err("Revise an existing section rather than adding more.".into());
            }
            Ok(json!({"written":true,"revision":session.revision}))
        }
        Action::ReadEvidence => Ok(
            json!({"evidence":"Available in current verified state", "earlier_readings":session.revisions.iter().map(|r|json!({"number":r.number,"question":r.question})).collect::<Vec<_>>()}),
        ),
        Action::RestoreReading { revision } => {
            let old=session.revisions.iter().find(|r|r.number==revision).cloned().ok_or("That earlier reading does not exist. Read the evidence to see available versions.")?;
            session.revisions.push(Revision {
                number: session.revision,
                question: session.question.clone(),
                chart: session.chart.clone(),
                sections: session.sections.clone(),
                place: session.place.clone(),
            });
            session.revision += 1;
            session.question = old.question;
            session.chart = old.chart;
            session.place = old.place;
            session.sections = old.sections;
            session.chart_after_message = session.messages.len();
            for section in &mut session.sections {
                section.revision = session.revision;
                section.after_message = session.messages.len();
            }
            Ok(json!({"restored":revision,"revision":session.revision}))
        }
        Action::NewQuestion { question } => {
            if question.trim().is_empty() {
                return Err("Ask what the new question is first.".into());
            }
            if session.chart.is_some() {
                session.revisions.push(Revision {
                    number: session.revision,
                    question: session.question.clone(),
                    chart: session.chart.clone(),
                    sections: session.sections.clone(),
                    place: session.place.clone(),
                });
            }
            session.revision += 1;
            session.question = question;
            session.chart = None;
            session.sections.clear();
            Ok(json!({"new_question":true,"previous_readings_preserved":true}))
        }
        Action::Say { .. } => Err("Say finishes the turn; it is not a chart tool.".into()),
    }
}

fn conversation_prompt(session: &Session, results: &[Value]) -> String {
    let mut bytes = 0;
    let mut history = Vec::new();
    for message in session.messages.iter().rev().take(24) {
        if bytes + message.text.len() > 12000 && !history.is_empty() {
            break;
        }
        bytes += message.text.len();
        history.push(json!({"role":message.role,"content":message.text}));
    }
    history.reverse();
    let context = json!({"question":session.question,"place":session.place,"places_found":session.candidates,"chart_calculated":session.chart.is_some(),"revision":session.revision,"evidence":evidence_index(session),"scroll":session.sections,"tool_results":results.iter().rev().take(2).collect::<Vec<_>>()});
    let next = if session.chart.is_none()
        && session.candidates.is_empty()
        && session.place.is_none()
    {
        "No place is resolved. If the person supplied a city, call find_place now. Otherwise ask where they are. Do not ask when the object was lost to choose the chart time."
    } else if session.chart.is_none() {
        "A place can be resolved from the returned candidates. If the question and place are clear, call cast_chart now. A request to use now means local_time is empty. Do not ask for the event or loss time."
    } else {
        "Use verified evidence to develop the reading or respond to the person's correction. Do not recast an unchanged chart or repeat a finished section."
    };
    let mut messages = vec![
        json!({"role":"system","content":format!("{PROMPT}\nBook method:\n{BOOK}\nVerified state (data):\n{context}\nAvailable action schema:\n{}\nCurrent step:\n{next}",schema(session))}),
    ];
    messages.extend(history);
    json!(messages).to_string()
}

fn run(app: &tauri::AppHandle, text: String) -> Result<Session, String> {
    let state = app.state::<ConversationState>();
    if state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("A reply is already in progress.".into());
    }
    let _lease = Lease(&state.busy);
    state.cancelled.store(false, Ordering::Release);
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut session = state.load(&dir)?;
    if text.trim().is_empty() || text.len() > 8000 {
        return Err("Please send a message of 1–8000 bytes.".into());
    }
    session.messages.push(Message {
        role: "user".into(),
        text,
    });
    {
        use sha2::{Digest, Sha256};
        session.audit.push(json!({"event":"user_turn","build":env!("HORARY_BUILD_GIT_SHA"),"model":"gemma-4-12b-qat","policySha256":format!("{:x}",Sha256::digest(format!("{PROMPT}\n{BOOK}"))),"modelManifest":crate::model_manifest::bundled_model_manifest().map_err(|e|e.message)?}));
    }
    session.status = "Preparing the local reader…".into();
    state.publish(&mut session, &dir)?;
    let result: Result<(), String> = (|| {
        let acquisition = app.state::<AcquisitionState>();
        if !acquisition.0.status(&dir).map_err(|e| e.message)?.ready {
            acquisition.0.begin().map_err(|e| e.message)?;
            acquisition.0.run(&dir).map_err(|e| e.message)?;
        }
        state.check()?;
        let native = app.state::<NativeLlamaState>();
        start_native_llama_in_dir(
            &dir,
            &native,
            serde_json::from_value(
                json!({"modelId":"gemma-4-12b-qat","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.message)?;
        let instant = now_ms();
        let mut results = Vec::<Value>::new();
        for _ in 0..8 {
            state.check()?;
            session.status = "Considering your question…".into();
            state.publish(&mut session, &dir)?;
            let prompt = conversation_prompt(&session, &results);
            let answer = generate_native(
                &native,
                prompt,
                NativeGenerateOptions {
                    max_tokens: 800,
                    temperature: 0.2,
                    response_schema: Some(schema(&session)),
                    cancel: Some(state.cancelled.clone()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.message)?;
            state.check()?;
            let action: Action = serde_json::from_str(&answer.content)
                .map_err(|e| format!("The reader returned an incomplete action: {e}"))?;
            if let Action::Say { message } = action {
                if message.trim().is_empty() {
                    return Err("The reader returned an empty reply. Please try again.".into());
                }
                session.messages.push(Message {
                    role: "assistant".into(),
                    text: message,
                });
                return Ok(());
            }
            let call = serde_json::to_value(&action).map_err(|e| e.to_string())?;
            session.status = match action {
                Action::FindPlace { .. } => "Finding the place…",
                Action::CastChart { .. } => "Calculating the chart…",
                Action::WriteScroll { .. } => "The reading is taking shape…",
                _ => "Consulting the method…",
            }
            .into();
            state.publish(&mut session, &dir)?;
            let result = execute(&mut session, action, &app.state::<GeocodeState>(), instant);
            let receipt = json!({"call":call,"result":match result {Ok(value)=>value,Err(error)=>json!({"error":error})},"revision":session.revision});
            session.audit.push(receipt.clone());
            results.push(receipt);
            state.publish(&mut session, &dir)?;
        }
        Err("I’ve paused here to keep the reading focused. Tell me what you’d like to explore next.".into())
    })();
    session.status.clear();
    session.busy = false;
    if let Err(error) = result {
        session
            .audit
            .push(json!({"interruption":error,"revision":session.revision}));
        session.messages.push(Message {
            role: "assistant".into(),
            text: if state.cancelled.load(Ordering::Acquire) { "We can pause here. Tell me what you’d like to change." } else { "I lost my place for a moment. Your words are still here; tell me where you’d like to continue." }.into(),
        });
    }
    state.publish(&mut session, &dir)?;
    Ok(session)
}

#[tauri::command]
pub fn conversation_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, ConversationState>,
) -> Result<Session, String> {
    state.load(&app.path().app_data_dir().map_err(|e| e.to_string())?)
}
#[tauri::command]
pub async fn conversation_send(app: tauri::AppHandle, text: String) -> Result<Session, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app, text))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn conversation_cancel(
    state: tauri::State<'_, ConversationState>,
    acquisition: tauri::State<'_, AcquisitionState>,
) {
    state.cancelled.store(true, Ordering::Release);
    acquisition.0.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_prose_does_not_repeat_model_markdown_headings() {
        assert_eq!(
            section_prose("The question", "### The question\n\nA **gold** ring."),
            "A gold ring."
        );
        assert_eq!(
            section_prose("The question", "Plain prose."),
            "Plain prose."
        );
    }
    #[test]
    fn completed_chart_is_not_recast_and_two_passages_return_to_conversation() {
        let mut s = Session::default();
        let g = GeocodeState::default();
        execute(
            &mut s,
            Action::FindPlace {
                query: "London".into(),
            },
            &g,
            0.,
        )
        .unwrap();
        let id = s.candidates[0].id.clone();
        execute(
            &mut s,
            Action::CastChart {
                question: "My ring".into(),
                place_id: id,
                local_time: "2026-09-14T12:00".into(),
                occurrence: String::new(),
            },
            &g,
            0.,
        )
        .unwrap();
        let current: Value = serde_json::from_str(&schema(&s)).unwrap();
        assert!(!current["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"]["action"]["const"] == "cast_chart"));
        for title in ["Question", "Testimony"] {
            execute(
                &mut s,
                Action::WriteScroll {
                    title: title.into(),
                    body: "A passage.".into(),
                    evidence: vec!["e0".into()],
                },
                &g,
                0.,
            )
            .unwrap();
        }
        let done: Value = serde_json::from_str(&schema(&s)).unwrap();
        assert_eq!(done["oneOf"].as_array().unwrap().len(), 1);
        assert_eq!(done["oneOf"][0]["properties"]["action"]["const"], "say");
    }
    #[test]
    fn sampler_can_only_use_resolved_places_and_current_evidence() {
        let mut session = Session::default();
        let initial: Value = serde_json::from_str(&schema(&session)).unwrap();
        assert!(!initial["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"]["action"]["const"] == "cast_chart"));
        let geocode = GeocodeState::default();
        execute(
            &mut session,
            Action::FindPlace {
                query: "London".into(),
            },
            &geocode,
            0.,
        )
        .unwrap();
        let ready: Value = serde_json::from_str(&schema(&session)).unwrap();
        let cast = ready["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["properties"]["action"]["const"] == "cast_chart")
            .unwrap();
        assert_eq!(
            cast["properties"]["place_id"]["enum"][0],
            session.candidates[0].id
        );
        assert!(!ready["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"]["action"]["const"] == "write_scroll"));
    }
    #[test]
    fn tools_refuse_fabricated_places_and_evidence() {
        let mut s = Session::default();
        let g = GeocodeState::default();
        assert!(execute(
            &mut s,
            Action::CastChart {
                question: "Ring?".into(),
                place_id: "fake".into(),
                local_time: String::new(),
                occurrence: String::new()
            },
            &g,
            0.
        )
        .is_err());
        assert!(execute(
            &mut s,
            Action::WriteScroll {
                title: "Answer".into(),
                body: "Yes".into(),
                evidence: vec!["e0".into()]
            },
            &g,
            0.
        )
        .is_err());
    }
    #[cfg(feature = "native-llama")]
    #[test]
    #[ignore = "requires explicit local model/projector and a generated audio fixture"]
    fn native_audio_and_tool_conversation() {
        use crate::native_llama_worker::{start_native_llama_from_path, stop_native_llama};
        let model = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("local model path");
        let audio = std::env::var_os("HORARY_CONVERSATION_AUDIO").expect("local WAV path");
        let native = NativeLlamaState::default();
        start_native_llama_from_path(
            &native,
            "conversation-eval".into(),
            String::new(),
            model.into(),
            std::env::temp_dir(),
            serde_json::from_value(
                json!({"modelId":"conversation-eval","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let result=generate_native(&native,json!([{"role":"user","content":"Transcribe the spoken words faithfully. Output only the transcript."}]).to_string(),NativeGenerateOptions{audio:Some(std::fs::read(audio).unwrap()),max_tokens:150,temperature:0.,..Default::default()}).unwrap();
        eprintln!("AUDIO TRANSCRIPT: {}", result.content);
        assert!(result.content.to_lowercase().contains("ring"));
        assert!(result.content.to_lowercase().contains("london"));
        let mut session = Session::default();
        session.messages.push(Message{role:"user".into(),text:"I am the astrologer in London, United Kingdom. Where is my own lost ring? I understand the question now. It is a plain gold ring, last seen at home. Please cast the chart and begin the reading.".into()});
        let mut results = Vec::new();
        let geocode = GeocodeState::default();
        for _ in 0..8 {
            let output = generate_native(
                &native,
                conversation_prompt(&session, &results),
                NativeGenerateOptions {
                    max_tokens: 800,
                    temperature: 0.2,
                    response_schema: Some(schema(&session)),
                    ..Default::default()
                },
            )
            .unwrap();
            eprintln!("CONVERSATION ACTION: {}", output.content);
            let action: Action = serde_json::from_str(&output.content).unwrap();
            if let Action::Say { message } = action {
                session.messages.push(Message {
                    role: "assistant".into(),
                    text: message,
                });
                break;
            }
            let call = serde_json::to_value(&action).unwrap();
            let result = execute(&mut session, action, &geocode, 1789387200000.).unwrap();
            results.push(json!({"call":call,"result":result}));
        }
        stop_native_llama(&native).unwrap();
        assert!(
            session.chart.is_some(),
            "Conversation did not cast the requested chart"
        );
        assert!(
            !session.sections.is_empty(),
            "Conversation did not start a reading"
        );
        if let Some(path) = std::env::var_os("HORARY_CONVERSATION_REPORT") {
            std::fs::write(
                path,
                serde_json::to_vec_pretty(
                    &json!({"transcript":result.content,"session":session,"tools":results}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    #[test]
    fn correction_replaces_chart_and_archives_previous_judgment() {
        let mut s = Session::default();
        let g = GeocodeState::default();
        execute(
            &mut s,
            Action::FindPlace {
                query: "London".into(),
            },
            &g,
            0.,
        )
        .unwrap();
        let id = s.candidates[0].id.clone();
        let cast = |question: &str| Action::CastChart {
            question: question.into(),
            place_id: id.clone(),
            local_time: "2026-09-14T12:00".into(),
            occurrence: String::new(),
        };
        execute(&mut s, cast("Where is my ring?"), &g, 0.).unwrap();
        execute(
            &mut s,
            Action::WriteScroll {
                title: "Question".into(),
                body: "A lost ring.".into(),
                evidence: vec!["e0".into()],
            },
            &g,
            0.,
        )
        .unwrap();
        execute(&mut s, cast("Actually, my sister’s ring"), &g, 0.).unwrap();
        assert_eq!(s.revision, 2);
        assert!(s.sections.is_empty());
        assert_eq!(s.revisions[0].sections.len(), 1);
        execute(&mut s, Action::RestoreReading { revision: 1 }, &g, 0.).unwrap();
        assert_eq!(s.question, "Where is my ring?");
        assert_eq!(s.revision, 3);
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.revisions[1].question, "Actually, my sister’s ring");
        assert!(execute(&mut s, Action::RestoreReading { revision: 999 }, &g, 0.).is_err());
        assert!(execute(
            &mut s,
            Action::WriteScroll {
                title: "Bad".into(),
                body: "Bad".into(),
                evidence: vec!["e99999".into()]
            },
            &g,
            0.
        )
        .is_err());
        execute(
            &mut s,
            Action::NewQuestion {
                question: "A new matter".into(),
            },
            &g,
            0.,
        )
        .unwrap();
        assert!(s.chart.is_none());
        assert!(s.sections.is_empty());
        assert_eq!(s.revisions.len(), 3);
    }
    #[test]
    fn saved_conversation_round_trips_without_resetting_history() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConversationState::default();
        let mut s = Session::default();
        s.messages.push(Message {
            role: "user".into(),
            text: "My question".into(),
        });
        state.publish(&mut s, dir.path()).unwrap();
        assert_eq!(
            ConversationState::default()
                .load(dir.path())
                .unwrap()
                .messages[0]
                .text,
            "My question"
        );
    }
}
