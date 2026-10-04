//! The model chooses bounded tools; Rust owns places, charts, revisions and disk.
#![forbid(unsafe_code)]
use crate::reading_method::{self, BookRule, Fact, Role, RoleChoice, Step};
use crate::review_progress::{self, Progress};
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceContext {
    pub timezone: String,
    pub locale: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy_meters: Option<f64>,
}

fn device_place(context: &DeviceContext) -> Result<Option<LocationCandidate>, String> {
    horary_ai_core::chart_input::resolve_chart_time("2000-01-01T12:00", &context.timezone, "")?;
    if context.locale.len() > 80 {
        return Err("The device language is invalid.".into());
    }
    let (Some(latitude), Some(longitude)) = (context.latitude, context.longitude) else {
        return Ok(None);
    };
    if !latitude.is_finite()
        || latitude.abs() >= 90.
        || !longitude.is_finite()
        || longitude.abs() > 180.
    {
        return Err("The device location is invalid.".into());
    }
    if context
        .accuracy_meters
        .is_some_and(|v| !v.is_finite() || !(0.0..=10000.).contains(&v))
    {
        return Ok(None);
    }
    let near = crate::geocode::reverse_geocode_local_city(crate::geocode::ReverseGeocodeRequest {
        latitude,
        longitude,
        max_distance_km: Some(75.),
    })
    .map_err(|e| e.message)?;
    // The clock's zone alone is never used to guess a geographic position.
    Ok(near
        .filter(|near| near.timezone == context.timezone)
        .map(|near| LocationCandidate {
            id: "device-location".into(),
            label: format!("Near {}", near.label),
            name: near.name,
            country: near.country,
            latitude,
            longitude,
            timezone: context.timezone.clone(),
            provider: "device".into(),
        }))
}

#[tauri::command]
pub fn conversation_device_context(
    app: tauri::AppHandle,
    context: DeviceContext,
) -> Result<(), String> {
    let state = app.state::<ConversationState>();
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Wait for the current reply to finish.")?;
    let _lease = Lease(&state.busy);
    let place = device_place(&context)?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut session = state.load(&dir)?;
    let available = place.is_some();
    if session.chart.is_none() && session.place.is_none() {
        session.candidates.retain(|p| p.provider != "device");
        session.candidates.extend(place);
    }
    session.device_context = Some(context);
    note(
        &mut session,
        &dir,
        "device",
        if available {
            "The device supplied its clock and present location."
        } else {
            "The device clock is available; the place may need a short clarification."
        },
        0,
    );
    state.publish(&mut session, &dir)
}

fn note(session: &mut Session, dir: &Path, event: &str, detail: &str, elapsed: u64) {
    match review_progress::record(dir, event, detail, elapsed) {
        Ok(item) => {
            session.progress.push(item);
            if session.progress.len() > 160 {
                session.progress.remove(0);
            }
        }
        Err(e) => log::warn!("Could not write local progress journal: {e}"),
    }
}

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
    #[serde(default)]
    pub progress: Vec<Progress>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub device_context: Option<DeviceContext>,
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
    #[serde(default)]
    pub step: Option<Step>,
    #[serde(default)]
    pub rules: Vec<BookRule>,
    #[serde(default)]
    pub because: String,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub draft: String,
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
        step: Step,
        title: String,
        body: String,
        evidence: Vec<String>,
        rule_ids: Vec<String>,
        limitation: Option<String>,
        because: String,
        roles: Vec<RoleChoice>,
    },
    RestoreReading {
        revision: u64,
    },
    NewQuestion {
        question: String,
    },
}

fn schema(session: &Session) -> String {
    let turn_calls: Vec<&str> = session
        .audit
        .iter()
        .rev()
        .take_while(|entry| entry["event"] != "user_turn")
        .filter_map(|entry| entry["call"]["action"].as_str())
        .collect();
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
    let mut actions = vec![variant("say", vec![("message", text(1200))])];
    if turn_calls.iter().filter(|a| **a == "find_place").count() < 2
        && !turn_calls.contains(&"cast_chart")
    {
        actions.push(variant("find_place", vec![("query", text(100))]));
    }
    // Calculated evidence is already supplied with every decision. A tool
    // that only returns the same data wastes another complete model prefill.
    // A conversational turn should leave room for the person's next thought.
    // A clear question can complete all three method stages in one turn.
    // Writing calls remain bounded even when the model tries to revise itself.
    if session
        .sections
        .iter()
        .filter(|s| s.after_message == session.messages.len())
        .count()
        >= 3
        || turn_calls
            .iter()
            .filter(|action| **action == "write_scroll")
            .count()
            >= 4
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
        && !turn_calls.contains(&"cast_chart")
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
        let ids: Vec<String> = available_facts(session).into_iter().map(|f| f.id).collect();
        let mut steps = vec![Step::Significators];
        if session
            .sections
            .iter()
            .any(|s| s.step == Some(Step::Significators))
        {
            steps.push(Step::Testimony);
        }
        if session
            .sections
            .iter()
            .any(|s| s.step == Some(Step::Testimony))
        {
            steps.push(Step::Judgment);
        }
        steps.retain(|step| {
            !session
                .audit
                .iter()
                .rev()
                .take_while(|entry| entry["event"] != "user_turn")
                .any(|r| r["result"]["written"] == true && r["call"]["step"] == json!(step))
        });
        let role = |basis: &str, value: Value| {
            json!({"type":"object","properties":{
            "label":text(80),basis:value,"reason":{"type":"string","minLength":12,"maxLength":240}
        },"required":["label",basis,"reason"],"additionalProperties":false})
        };
        for step in steps {
            let roles = if step == Step::Significators {
                json!({"type":"array","minItems":1,"maxItems":5,"items":{"oneOf":[
                    role("house",json!({"enum":[1,2,3,4,5,6,7,8,9,10,11,12]})),
                    role("natural",json!({"enum":["Moon","Sun","Venus"]}))
                ]}})
            } else {
                json!({"type":"array","maxItems":0,"items":{"type":"object"}})
            };
            actions.push(variant("write_scroll",vec![
                ("step",json!({"const":step})),("roles",roles),
                ("evidence",json!({"type":"array","items":{"enum":ids},"minItems":1,"maxItems":8})),
                ("rule_ids",json!({"type":"array","items":{"enum":reading_method::rules().iter().filter(|r|reading_method::rule_allowed(step,&r.id)).map(|r|r.id.clone()).collect::<Vec<_>>()},"minItems":1,"maxItems":2})),
                ("limitation",json!({"const":if step==Step::Judgment {evidence(session).into_iter().find(|f|f.kind=="boundary").map(|f|f.id)}else{None}})),
                ("because",json!({"type":"string","minLength":20,"maxLength":360})),("body",text(650)),("title",text(80)),
            ]));
        }
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
        session.facts = reading_method::facts(session.chart.as_ref());
        session.busy = self.busy.load(Ordering::Acquire);
        Ok(session)
    }
    fn publish(&self, session: &mut Session, dir: &Path) -> Result<(), String> {
        session.facts = reading_method::facts(session.chart.as_ref());
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
fn evidence(session: &Session) -> Vec<Fact> {
    reading_method::facts(session.chart.as_ref())
}
fn available_facts(session: &Session) -> Vec<Fact> {
    let roles: Vec<&Role> = session
        .sections
        .iter()
        .flat_map(|s| s.roles.iter())
        .collect();
    evidence(session)
        .into_iter()
        .filter(|fact| {
            if roles.is_empty() {
                return matches!(fact.kind.as_str(), "position" | "house" | "boundary");
            }
            if fact.kind == "house" {
                return roles.iter().any(|r| {
                    r.house
                        .is_some_and(|number| fact.label == format!("House {number}"))
                });
            }
            fact.kind == "position"
                || fact.planets.is_empty()
                || fact
                    .planets
                    .iter()
                    .any(|p| roles.iter().any(|r| r.planet == *p))
        })
        .collect()
}
fn evidence_index(session: &Session) -> Value {
    json!(available_facts(session))
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

fn finish_working_reading(session: &mut Session) -> bool {
    if ![Step::Significators, Step::Testimony, Step::Judgment]
        .into_iter()
        .all(|step| session.sections.iter().any(|s| s.step == Some(step)))
    {
        return false;
    }
    session.messages.push(Message {role:"assistant".into(),text:"Your chart and its testimony are here. The answer remains open while we check how the pieces fit. You can explore the margins, or tell me anything that needs correcting.".into()});
    session.audit.push(json!({"event":"working_reading_ready","narration":"native","reason":"calculation_limits","revision":session.revision}));
    true
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
            step,
            title,
            body,
            evidence: mut ids,
            rule_ids,
            limitation,
            because,
            roles,
        } => {
            if session.chart.is_none() {
                return Err("Calculate the chart before writing a judgment.".into());
            }
            let facts = evidence(session);
            if ids.is_empty()
                || ids.len() > 8
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
            if step == Step::Judgment {
                let boundary = facts
                    .iter()
                    .find(|f| f.kind == "boundary")
                    .ok_or("The calculation boundary is unavailable.")?;
                if limitation.as_deref() != Some(boundary.id.as_str()) {
                    return Err(
                        "Use the supplied calculation-boundary reference for this judgment.".into(),
                    );
                }
                if !ids.contains(&boundary.id) {
                    ids.push(boundary.id.clone());
                }
            } else if limitation.is_some() {
                return Err("Only the judgment carries the calculation-boundary reference.".into());
            }
            if title.chars().count() > 80
                || body.chars().count() > 900
                || !(20..=360).contains(&because.trim().chars().count())
            {
                return Err(
                    "Keep the passage short and explain how its facts support the interpretation."
                        .into(),
                );
            }
            let has_significators = session
                .sections
                .iter()
                .any(|s| s.step == Some(Step::Significators));
            let has_testimony = session
                .sections
                .iter()
                .any(|s| s.step == Some(Step::Testimony));
            if (step != Step::Significators && !has_significators)
                || (step == Step::Judgment && !has_testimony)
            {
                return Err("Identify the significators, then weigh testimony, before drawing the judgment.".into());
            }
            let catalog = reading_method::rules();
            let rules: Vec<BookRule> = rule_ids
                .iter()
                .map(|id| {
                    catalog
                        .iter()
                        .find(|r| r.id == *id && reading_method::rule_allowed(step, id))
                        .cloned()
                        .ok_or("Choose a supplied book rule relevant to this step.")
                })
                .collect::<Result<_, _>>()?;
            if rules.is_empty() || rules.len() > 2 {
                return Err("Give one or two relevant book rules.".into());
            }
            let roles = if step == Step::Significators {
                reading_method::assign(
                    session.chart.as_ref().ok_or("A chart is required.")?,
                    roles,
                )?
            } else {
                if !roles.is_empty() {
                    return Err(
                        "Change role assignments in the significators passage first.".into(),
                    );
                }
                Vec::new()
            };
            let cited: Vec<Fact> = ids
                .iter()
                .filter_map(|id| facts.iter().find(|f| f.id == *id).cloned())
                .collect();
            if cited.iter().all(|f| f.kind == "boundary") {
                return Err("Use a calculated fact as well as any limitations.".into());
            }
            if rules.iter().any(|r| r.id == "reception")
                && !cited.iter().any(|f| f.kind == "reception")
            {
                return Err("A reception inference must cite a directed reception fact, not a planet's own dignity.".into());
            }
            if step == Step::Judgment && !cited.iter().any(|f| f.kind == "boundary") {
                return Err("A provisional judgment must cite the calculation boundary and explain what it leaves unresolved.".into());
            }
            let prose = section_prose(&title, &body);
            if prose.trim().is_empty() {
                return Err("The passage needs prose after its heading.".into());
            }
            let (title, body) = reading_method::passage(step);
            let section = Section {
                body: body.into(),
                title: title.into(),
                draft: prose,
                evidence: ids,
                revision: session.revision,
                after_message: session.messages.len(),
                step: Some(step),
                rules,
                because,
                roles,
                facts: cited,
            };
            // Downstream conclusions belong to their premises. Audit receipts
            // retain the replaced prose; the visible document must not keep an
            // answer based on an assignment or testimony that just changed.
            session.sections.retain(|s| match step {
                Step::Significators => !matches!(s.step, Some(Step::Testimony | Step::Judgment)),
                Step::Testimony => s.step != Some(Step::Judgment),
                Step::Judgment => true,
            });
            if let Some(old) = session.sections.iter_mut().find(|s| s.step == section.step) {
                *old = section;
            } else if session.sections.len() < 12 {
                session.sections.push(section);
            } else {
                return Err("Revise an existing section rather than adding more.".into());
            }
            Ok(json!({"written":true,"revision":session.revision}))
        }
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
    let scroll:Vec<Value>=session.sections.iter().map(|s|json!({"step":s.step,"title":s.title,"body":s.body,"because":s.because,"roles":s.roles})).collect();
    let receipts: Vec<Value> = results
        .iter()
        .rev()
        .take(2)
        .map(|r| {
            if r["result"]["error"].is_null() {
                json!({"action":r["call"]["action"],"result":r["result"]})
            } else {
                r.clone()
            }
        })
        .collect();
    let context = json!({"question":session.question,"place":session.place,"places_found":session.candidates,"device_timezone":session.device_context.as_ref().map(|d|&d.timezone),"chart_calculated":session.chart.is_some(),"revision":session.revision,"evidence":evidence_index(session),"scroll":scroll,"book_rules":reading_method::rules(),"tool_results":receipts});
    let next = if session.chart.is_none()
        && session.candidates.is_empty()
        && session.place.is_none()
    {
        "No place is resolved. If the person supplied a city, call find_place now. Otherwise ask where they are. Do not ask when the object was lost to choose the chart time."
    } else if session.chart.is_none() {
        "A place can be resolved from the returned candidates. If the question and place are clear, call cast_chart now. A request to use now means local_time is empty. Do not ask for the event or loss time."
    } else {
        "Use the supplied verified evidence directly; no evidence-fetch action is needed. Establish roles, then explain relevant reception and contact candidates. The Moon can supply the querent's main contact; do not call it minor merely because it is a cosignificator. A provisional judgment must say what remains unestablished. Do not infer a one-year absence of marriage from this seven-day search. Do not recast an unchanged chart or repeat a finished section."
    };
    let mut messages = vec![
        json!({"role":"system","content":format!("{PROMPT}\nVerified state and editorial book rules (data):\n{context}\nAvailable action schema:\n{}\nCurrent step:\n{next}",schema(session))}),
    ];
    messages.extend(history);
    json!(messages).to_string()
}

// A failed inference has performed no tools. Recover once from the pinned
// runtime's non-text-token decoding failure, keeping the same constrained
// schema and prompt. Never replay an executed action or retry cancellation.
fn generate_action(
    cancelled: &AtomicBool,
    audit: &mut Vec<Value>,
    mut generate: impl FnMut(f32) -> Result<String, String>,
) -> Result<Action, String> {
    let mut temperature = 0.2;
    for attempt in 0..2 {
        if cancelled.load(Ordering::Acquire) {
            return Err("Judgement cancelled.".into());
        }
        match generate(temperature) {
            Ok(content) => {
                if cancelled.load(Ordering::Acquire) {
                    return Err("Judgement cancelled.".into());
                }
                return serde_json::from_str(&content)
                    .map_err(|e| format!("The reader returned an incomplete action: {e}"));
            }
            Err(error)
                if attempt == 0
                    && error.contains("failed to decode controlled token: Unknown Token Type")
                    && !cancelled.load(Ordering::Acquire) =>
            {
                audit.push(json!({"generation_retry":error,"selector":"greedy","attempt":2}));
                temperature = 0.;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the second attempt always returns")
}

fn run(app: &tauri::AppHandle, text: String) -> Result<Session, String> {
    // Preparation and generation can take minutes. They cannot move the
    // submitted question's moment; clarification starts a later user turn.
    let instant = now_ms();
    let started = std::time::Instant::now();
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
    note(
        &mut session,
        &dir,
        "received",
        "Your words were kept. The question's moment was noted.",
        0,
    );
    {
        use sha2::{Digest, Sha256};
        session.audit.push(json!({"event":"user_turn","build":env!("HORARY_BUILD_GIT_SHA"),"model":"gemma-4-12b-qat","policySha256":format!("{:x}",Sha256::digest(format!("{PROMPT}\n{BOOK}"))),"modelManifest":crate::model_manifest::bundled_model_manifest().map_err(|e|e.message)?}));
    }
    session.status = "Preparing the local reader…".into();
    state.publish(&mut session, &dir)?;
    let result: Result<(), String> = (|| {
        let acquisition = app.state::<AcquisitionState>();
        if !acquisition.0.status(&dir).map_err(|e| e.message)?.ready {
            note(
                &mut session,
                &dir,
                "preparing",
                "The reader is quietly preparing.",
                started.elapsed().as_millis() as u64,
            );
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
        note(
            &mut session,
            &dir,
            "ready",
            "The reader is here, with the book's method at hand.",
            started.elapsed().as_millis() as u64,
        );
        let mut results = Vec::<Value>::new();
        for _ in 0..8 {
            state.check()?;
            session.status = "Considering your question…".into();
            state.publish(&mut session, &dir)?;
            let prompt = conversation_prompt(&session, &results);
            let prompt_hash = {
                use sha2::{Digest, Sha256};
                format!("{:x}", Sha256::digest(&prompt))
            };
            let action_schema = schema(&session);
            let inference_start = std::time::Instant::now();
            let mut measurements = Vec::new();
            let generated = generate_action(&state.cancelled, &mut session.audit, |temperature| {
                generate_native(
                    &native,
                    prompt.clone(),
                    NativeGenerateOptions {
                        max_tokens: 800,
                        temperature,
                        response_schema: Some(action_schema.clone()),
                        cancel: Some(state.cancelled.clone()),
                        ..Default::default()
                    },
                )
                .map(|answer| {
                    log::info!("reader inference: prompt_tokens={} output_tokens={} duration_ms={} tokens_per_second={:.2}",answer.prompt_tokens,answer.generated_tokens,answer.elapsed_ms,answer.tokens_per_second);
                    measurements.push(review_progress::Inference {
                        prompt_tokens: answer.prompt_tokens,
                        output_tokens: answer.generated_tokens,
                        elapsed_ms: u64::try_from(answer.elapsed_ms).unwrap_or(u64::MAX),
                        tokens_per_second: answer.tokens_per_second,
                    });
                    answer.content
                })
                .map_err(|e| e.message)
            });
            for measurement in measurements {
                session
                    .audit
                    .push(json!({"generation":measurement,"promptSha256":prompt_hash}));
                if let Ok(item) = review_progress::record_with_inference(
                    &dir,
                    "inference",
                    "The reader considered this step.",
                    measurement.elapsed_ms,
                    Some(measurement),
                ) {
                    session.progress.push(item);
                }
            }
            let action = generated?;
            note(
                &mut session,
                &dir,
                "considered",
                "A piece of the question was considered.",
                inference_start.elapsed().as_millis() as u64,
            );
            state.check()?;
            if let Action::Say { message } = action {
                if message.trim().is_empty() {
                    return Err("The reader returned an empty reply. Please try again.".into());
                }
                session.messages.push(Message {
                    role: "assistant".into(),
                    text: message,
                });
                note(
                    &mut session,
                    &dir,
                    "reply",
                    "The conversation returns to you.",
                    started.elapsed().as_millis() as u64,
                );
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
            let detail = match call["action"].as_str() {
                Some("find_place") => "A place was sought.",
                Some("cast_chart") => "The chart was calculated for the question's moment.",
                Some("write_scroll") => "A passage was connected to chart facts and a book rule.",
                Some("restore_reading") => "An earlier reading was brought back.",
                Some("new_question") => "A new question opened a new leaf.",
                _ => "The evidence was consulted.",
            };
            note(
                &mut session,
                &dir,
                if result.is_ok() {
                    "completed"
                } else {
                    "needs_attention"
                },
                if result.is_ok() {
                    detail
                } else {
                    "This step needs a correction before it can continue."
                },
                started.elapsed().as_millis() as u64,
            );
            let receipt = json!({"call":call,"result":match result {Ok(value)=>value,Err(error)=>json!({"error":error})},"revision":session.revision});
            let judgment_written =
                receipt["result"]["written"] == true && receipt["call"]["step"] == "judgment";
            session.audit.push(receipt.clone());
            results.push(receipt);
            state.publish(&mut session, &dir)?;
            if judgment_written && finish_working_reading(&mut session) {
                note(
                    &mut session,
                    &dir,
                    "reply",
                    "The chart and book method are ready to examine; the answer remains open.",
                    started.elapsed().as_millis() as u64,
                );
                return Ok(());
            }
        }
        Err("I’ve paused here to keep the reading focused. Tell me what you’d like to explore next.".into())
    })();
    session.status.clear();
    session.busy = false;
    if let Err(error) = result {
        note(
            &mut session,
            &dir,
            "paused",
            "The reading paused; your words and completed passages were kept.",
            started.elapsed().as_millis() as u64,
        );
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
    fn device_location_keeps_coordinates_and_requires_credible_zone_and_accuracy() {
        let mut context = DeviceContext {
            timezone: "America/New_York".into(),
            locale: "en-US".into(),
            latitude: Some(38.657),
            longitude: Some(-77.249),
            accuracy_meters: Some(800.),
        };
        let place = device_place(&context).unwrap().unwrap();
        assert_eq!(place.provider, "device");
        assert_eq!(place.latitude, 38.657);
        assert_eq!(place.longitude, -77.249);
        assert_eq!(place.timezone, "America/New_York");
        context.timezone = "Europe/London".into();
        assert!(device_place(&context).unwrap().is_none());
        context.timezone = "America/New_York".into();
        context.accuracy_meters = Some(50000.);
        assert!(device_place(&context).unwrap().is_none());
        context.latitude = None;
        assert!(device_place(&context).unwrap().is_none());
    }

    fn passage(step: Step) -> Action {
        Action::WriteScroll {
            step,
            limitation: if step == Step::Judgment {
                let chart =
                    horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
                reading_method::facts(Some(&chart))
                    .into_iter()
                    .find(|f| f.kind == "boundary")
                    .map(|f| f.id)
            } else {
                None
            },
            title: format!("{step:?}"),
            body: "The passage draws on the chart and explains the method.".into(),
            evidence: if step == Step::Judgment {
                let chart =
                    horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
                vec![
                    "e7".into(),
                    reading_method::facts(Some(&chart))
                        .into_iter()
                        .find(|f| f.kind == "boundary")
                        .unwrap()
                        .id,
                ]
            } else {
                vec!["e7".into()]
            },
            rule_ids: vec![if step == Step::Significators {
                "significators"
            } else {
                "perfection"
            }
            .into()],
            because: "The selected house supplies the person's traditional significator.".into(),
            roles: if step == Step::Significators {
                vec![RoleChoice {
                    label: "You".into(),
                    house: Some(1),
                    natural: None,
                    reason: "The querent is the person asking this question.".into(),
                }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn reading_steps_require_source_rules_and_clear_dependent_conclusions() {
        let mut session = Session {
            chart: Some(horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap()),
            ..Default::default()
        };
        let geo = GeocodeState::default();
        assert!(execute(&mut session, passage(Step::Judgment), &geo, 0.).is_err());
        let mut bad = passage(Step::Significators);
        if let Action::WriteScroll {
            ref mut rule_ids, ..
        } = bad
        {
            *rule_ids = vec!["invented_reference".into()];
        }
        assert!(execute(&mut session, bad, &geo, 0.).is_err());
        execute(&mut session, passage(Step::Significators), &geo, 0.).unwrap();
        assert_eq!(session.sections[0].facts[0].label, "House 1");
        let planet = reading_method::ruler(
            session.chart.as_ref().unwrap()["houses"][0]["sign"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(session.sections[0].roles[0].planet, planet);
        execute(&mut session, passage(Step::Testimony), &geo, 0.).unwrap();
        execute(&mut session, passage(Step::Judgment), &geo, 0.).unwrap();
        assert_eq!(session.sections.len(), 3);
        execute(&mut session, passage(Step::Testimony), &geo, 0.).unwrap();
        assert_eq!(session.sections.len(), 2);
        execute(&mut session, passage(Step::Significators), &geo, 0.).unwrap();
        assert_eq!(session.sections.len(), 1);
        assert_eq!(session.sections[0].rules[0].pages, "15–38");
        let mut mistaken = passage(Step::Significators);
        if let Action::WriteScroll { ref mut body, .. } = mistaken {
            *body = "Venus in Scorpio is in the detriment of Mars.".into();
        }
        execute(&mut session, mistaken, &geo, 0.).unwrap();
        assert!(!session.sections[0].body.contains("detriment of Mars"));
        assert!(session.sections[0].draft.contains("detriment of Mars"));
    }

    #[test]
    fn place_search_cannot_repeat_indefinitely_and_schema_constrains_role_bases() {
        let mut session = Session::default();
        for _ in 0..2 {
            session.audit.push(json!({"call":{"action":"find_place"}}));
        }
        assert!(!schema(&session).contains("find_place"));
        session.chart =
            Some(horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap());
        let parsed: Value = serde_json::from_str(&schema(&session)).unwrap();
        let write = parsed["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["properties"]["action"]["const"] == "write_scroll")
            .unwrap();
        assert_eq!(write["properties"]["step"]["const"], "significators");
        assert_eq!(write["properties"]["roles"]["minItems"], 1);
        assert!(
            write["properties"]["roles"]["items"]["oneOf"][0]["properties"]["house"].is_object()
        );
        let fields = write["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            vec![
                "action",
                "step",
                "roles",
                "evidence",
                "rule_ids",
                "limitation",
                "because",
                "body",
                "title"
            ]
        );
    }

    #[test]
    #[ignore = "Real Gemma hardware qualification; run with explicit local weights."]
    fn native_device_marriage_conversation() {
        use crate::native_llama_worker::{start_native_llama_from_path, stop_native_llama};
        use sha2::{Digest, Sha256};
        let native = NativeLlamaState::default();
        let model = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("local model path");
        start_native_llama_from_path(
            &native,
            "device-reading-eval".into(),
            String::new(),
            model.into(),
            std::env::temp_dir(),
            serde_json::from_value(
                json!({"modelId":"device-reading-eval","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let context = DeviceContext {
            timezone: "America/New_York".into(),
            locale: "en-US".into(),
            latitude: Some(38.657),
            longitude: Some(-77.249),
            accuracy_meters: Some(800.),
        };
        let mut session = Session {
            candidates: vec![device_place(&context).unwrap().unwrap()],
            device_context: Some(context),
            ..Default::default()
        };
        let geo = GeocodeState::default();
        for text in ["Will I get married in the next year? I mean a future partner; I am not seeing anyone. Use where this device is now and begin the reading."] {
            session.messages.push(Message {role:"user".into(),text:text.into()});
            session.audit.push(json!({"event":"user_turn","build":env!("HORARY_BUILD_GIT_SHA")}));
            let mut results=Vec::new();
            for _ in 0..8 {
                let prompt=conversation_prompt(&session,&results);
                let hash=format!("{:x}",Sha256::digest(&prompt));
                let action_schema=schema(&session);
                let mut measurements=Vec::new();
                let action=generate_action(&Arc::new(AtomicBool::new(false)),&mut session.audit,|temperature| {
                    let generated=generate_native(&native,prompt.clone(),NativeGenerateOptions {max_tokens:800,temperature,response_schema:Some(action_schema.clone()),..Default::default()}).map_err(|e|e.message)?;
                    eprintln!("READING INFERENCE: {} tokens / {}ms ({:.2} tok/s)",generated.generated_tokens,generated.elapsed_ms,generated.tokens_per_second);
                    measurements.push(json!({"promptSha256":hash,"promptTokens":generated.prompt_tokens,"outputTokens":generated.generated_tokens,"elapsedMs":generated.elapsed_ms,"tokensPerSecond":generated.tokens_per_second}));
                    Ok(generated.content)
                }).unwrap();
                session.audit.extend(measurements.into_iter().map(|m|json!({"generation":m})));
                eprintln!("DEVICE READING ACTION: {}",serde_json::to_string(&action).unwrap());
                if let Action::Say {message}=action {session.messages.push(Message {role:"assistant".into(),text:message});break;}
                let call=serde_json::to_value(&action).unwrap();
                let result=execute(&mut session,action,&geo,1789387200000.);
                let receipt=json!({"call":call,"result":match result {Ok(value)=>value,Err(error)=>json!({"error":error})},"promptSha256":hash});
                session.audit.push(receipt.clone());
                results.push(receipt);
                if session.sections.iter().any(|s|s.step==Some(Step::Judgment)) && finish_working_reading(&mut session) {break;}
            }
        }
        stop_native_llama(&native).unwrap();
        session.facts = evidence(&session);
        if let Some(path) = std::env::var_os("HORARY_CONVERSATION_REPORT") {
            std::fs::write(path, serde_json::to_vec_pretty(&session).unwrap()).unwrap();
        }
        assert!(
            session.chart.is_some(),
            "Device context should enable casting without typing a place"
        );
        assert_eq!(session.place.as_ref().unwrap().provider, "device");
        assert!(
            !session
                .audit
                .iter()
                .any(|r| r["call"]["action"] == "find_place"),
            "A supplied device location needs no search loop"
        );
        for step in [Step::Significators, Step::Testimony, Step::Judgment] {
            assert!(
                session.sections.iter().any(|s| s.step == Some(step)),
                "Missing method step {step:?}"
            );
        }
        let judgment = session
            .sections
            .iter()
            .find(|s| s.step == Some(Step::Judgment))
            .unwrap();
        assert!(judgment
            .body
            .contains("do not establish a complete judgment"));
        assert!(!judgment.draft.is_empty());
        assert_ne!(judgment.body, judgment.draft);
    }
    #[test]
    fn token_decode_recovery_is_bounded_and_never_retries_cancellation() {
        let failure = "decode_failed: failed to decode controlled token: Unknown Token Type";
        let cancelled = AtomicBool::new(false);
        let mut audit = Vec::new();
        let mut temperatures = Vec::new();
        let action = generate_action(&cancelled, &mut audit, |temperature| {
            temperatures.push(temperature);
            if temperatures.len() == 1 {
                Err(failure.into())
            } else {
                Ok(r#"{"action":"say","message":"A short passage."}"#.into())
            }
        })
        .unwrap();
        assert!(matches!(action, Action::Say { .. }));
        assert_eq!(temperatures, [0.2, 0.]);
        assert_eq!(audit.len(), 1);
        let mut calls = 0;
        assert!(generate_action(&cancelled, &mut audit, |_| {
            calls += 1;
            Err(failure.into())
        })
        .is_err());
        assert_eq!(calls, 2);
        calls = 0;
        assert!(generate_action(&cancelled, &mut audit, |_| {
            calls += 1;
            cancelled.store(true, Ordering::Release);
            Err(failure.into())
        })
        .is_err());
        assert_eq!(calls, 1);
        cancelled.store(false, Ordering::Release);
        calls = 0;
        assert!(generate_action(&cancelled, &mut audit, |_| {
            calls += 1;
            Err("The model is unavailable.".into())
        })
        .is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn revising_the_same_passage_still_returns_control_to_the_person() {
        let mut session = Session::default();
        session.audit.push(json!({"event":"user_turn"}));
        session
            .audit
            .push(json!({"call":{"action":"write_scroll"},"result":{"written":true}}));
        session
            .audit
            .push(json!({"call":{"action":"write_scroll"},"result":{"written":true}}));
        session
            .audit
            .push(json!({"call":{"action":"write_scroll"},"result":{"written":true}}));
        session
            .audit
            .push(json!({"call":{"action":"write_scroll"}}));
        let actions: Value = serde_json::from_str(&schema(&session)).unwrap();
        assert_eq!(actions["oneOf"].as_array().unwrap().len(), 1);
        assert_eq!(actions["oneOf"][0]["properties"]["action"]["const"], "say");
        session.audit.push(json!({"event":"user_turn"}));
        let next: Value = serde_json::from_str(&schema(&session)).unwrap();
        assert!(next["oneOf"].as_array().unwrap().len() > 1);
        session
            .audit
            .push(json!({"call":{"action":"read_evidence"}}));
        let read: Value = serde_json::from_str(&schema(&session)).unwrap();
        assert!(!read["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["properties"]["action"]["const"] == "read_evidence"));
    }

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
    fn completed_chart_is_not_recast_and_writing_attempts_are_bounded() {
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
                    step: Step::Significators,
                    rule_ids: vec!["significators".into()],
                    limitation: None,
                    because: "The chosen house represents the person asking this question.".into(),
                    roles: vec![RoleChoice {
                        label: "You".into(),
                        house: Some(1),
                        natural: None,
                        reason: "The querent is represented by the first house.".into(),
                    }],
                    title: title.into(),
                    body: "A passage.".into(),
                    evidence: vec!["e0".into()],
                },
                &g,
                0.,
            )
            .unwrap();
        }
        s.audit.push(json!({"call":{"action":"write_scroll"}}));
        s.audit.push(json!({"call":{"action":"write_scroll"}}));
        s.audit.push(json!({"call":{"action":"write_scroll"}}));
        s.audit.push(json!({"call":{"action":"write_scroll"}}));
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
                step: Step::Significators,
                rule_ids: vec!["significators".into()],
                limitation: None,
                because: "The chosen house represents the person asking this question.".into(),
                roles: vec![RoleChoice {
                    label: "You".into(),
                    house: Some(1),
                    natural: None,
                    reason: "The querent is represented by the first house.".into()
                }],
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
            let receipt = json!({"call":call,"result":result});
            session.audit.push(receipt.clone());
            results.push(receipt);
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
                step: Step::Significators,
                rule_ids: vec!["significators".into()],
                limitation: None,
                because: "The chosen house represents the person asking this question.".into(),
                roles: vec![RoleChoice {
                    label: "You".into(),
                    house: Some(1),
                    natural: None,
                    reason: "The querent is represented by the first house.".into(),
                }],
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
                step: Step::Significators,
                rule_ids: vec!["significators".into()],
                limitation: None,
                because: "The chosen house represents the person asking this question.".into(),
                roles: vec![RoleChoice {
                    label: "You".into(),
                    house: Some(1),
                    natural: None,
                    reason: "The querent is represented by the first house.".into()
                }],
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
