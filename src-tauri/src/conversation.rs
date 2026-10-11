//! Rust schedules explicit teaching tasks and owns places, charts and storage.
#![forbid(unsafe_code)]
use crate::reading_method::{self, BookRule, Fact, Role, Step};
use crate::review_progress::{self, Progress};
use crate::{
    geocode::{GeocodeState, LocationCandidate},
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
const TRANSCRIPTION_PROMPT: &str = "Transcribe the spoken words in this audio faithfully. Output only the transcript, without commentary, interpretation, or answers. If no intelligible speech is present, output [inaudible].";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceContext {
    pub timezone: String,
    pub locale: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy_meters: Option<f64>,
}

pub(crate) fn device_place(context: &DeviceContext) -> Result<Option<LocationCandidate>, String> {
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
    Ok(Some(LocationCandidate {
        id: "device-location".into(),
        label: near
            .as_ref()
            .map_or_else(|| "Here".into(), |near| format!("Near {}", near.label)),
        name: near
            .as_ref()
            .map_or_else(|| "Here".into(), |near| near.name.clone()),
        country: near
            .as_ref()
            .map_or_else(String::new, |near| near.country.clone()),
        latitude,
        longitude,
        timezone: near.map_or_else(|| context.timezone.clone(), |near| near.timezone),
        provider: "device".into(),
    }))
}

#[tauri::command]
pub fn conversation_device_context(
    app: tauri::AppHandle,
    context: DeviceContext,
    reading_id: Option<String>,
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
    ensure_scope(&session, reading_id.as_deref())?;
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
    #[serde(default)]
    pub reading_id: String,
    #[serde(default)]
    pub saved_readings: Vec<crate::reading_store::SavedReading>,
    #[serde(default)]
    pub method: crate::horary_pipeline::MethodState,
    #[serde(default)]
    pub candidate_moment_ms: Option<f64>,
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
    pub(crate) candidates: Vec<LocationCandidate>,
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
    #[serde(default)]
    pub worksheet: Value,
    #[serde(default)]
    pub method_stage: Option<crate::horary_lessons::Stage>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Revision {
    pub number: u64,
    pub question: String,
    pub chart: Option<Value>,
    pub sections: Vec<Section>,
    pub place: Option<LocationCandidate>,
    #[serde(default)]
    pub brief: crate::horary_pipeline::Brief,
    #[serde(default)]
    pub consultation: Option<crate::reading_contracts::Consultation>,
}

#[derive(Default)]
pub struct ConversationState {
    session: Mutex<Option<Session>>,
    pub cancelled: Arc<AtomicBool>,
    busy: AtomicBool,
    opened: Mutex<bool>,
}
struct Lease<'a>(&'a AtomicBool);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
                crate::reading_store::read(&path)?
            } else {
                Session::default()
            };
            *slot = Some(session);
        }
        let mut session = slot.as_ref().ok_or("Conversation unavailable")?.clone();
        session.facts = reading_method::facts(session.chart.as_ref());
        Ok(session)
    }
    fn snapshot(&self, dir: &Path) -> Result<Session, String> {
        let mut session = self.load(dir)?;
        // The operation lease describes the live process, not its predecessor
        // reading. Overlay it only for UI snapshots, never for archival data.
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
            json!({"modelId":"gemma-4-12b-qat","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}),
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
    let prompt = json!([{"role":"user","content":TRANSCRIPTION_PROMPT}]).to_string();
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
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if let Err(error) = review_progress::record_with_inference(
        &dir,
        "voice_comparison",
        "Your spoken words were transcribed for comparison.",
        u64::try_from(answer.elapsed_ms).unwrap_or(u64::MAX),
        Some(inference_measurement(&answer)),
    ) {
        log::warn!("Could not record voice timing: {error}");
    }
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
pub(crate) fn inference_measurement(
    answer: &crate::native_llama_worker::NativeGenerationResult,
) -> review_progress::Inference {
    review_progress::Inference {
        prompt_tokens: answer.prompt_tokens,
        output_tokens: answer.generated_tokens,
        elapsed_ms: u64::try_from(answer.elapsed_ms).unwrap_or(u64::MAX),
        tokens_per_second: answer.tokens_per_second,
        cached_prompt_tokens: answer.cached_prompt_tokens,
        prefilled_prompt_tokens: answer.prefilled_prompt_tokens,
        first_token_ms: answer.first_token_ms.and_then(|v| u64::try_from(v).ok()),
    }
}
enum TurnInput {
    Text(String),
    Voice(u64),
}

struct PipelineRuntime<'a> {
    app: &'a tauri::AppHandle,
    dir: &'a Path,
}
impl crate::horary_pipeline::Runtime for PipelineRuntime<'_> {
    fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
        let location =
            tauri::async_runtime::block_on(crate::native_location::get_current_location_native(
                crate::native_location::CurrentLocationRequest {
                    timeout_ms: Some(6000),
                },
            ))
            .map_err(|e| e.message)?;
        device_place(&DeviceContext {
            timezone: "UTC".into(),
            locale: String::new(),
            latitude: Some(location.latitude),
            longitude: Some(location.longitude),
            accuracy_meters: location.accuracy_meters,
        })
    }
    fn generate_batch(
        &self,
        tasks: &[(
            crate::horary_lessons::Stage,
            crate::horary_lessons::Matter,
            Value,
            Value,
        )],
    ) -> Result<Vec<crate::native_llama_worker::NativeGenerationResult>, String> {
        let prompts = tasks
            .iter()
            .map(|(stage, matter, input, contract)| {
                crate::horary_pipeline::prompt(*stage, *matter, input, contract).map(|p| (p, 1000))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let wall = std::time::Instant::now();
        let result = crate::native_llama_worker::generate_native_batch(
            &self.app.state::<NativeLlamaState>(),
            prompts,
            NativeGenerateOptions {
                max_tokens: 1000,
                temperature: 0.,
                cache_lesson: true,
                cancel: Some(self.app.state::<ConversationState>().cancelled.clone()),
                ..Default::default()
            },
        )
        .map_err(|e| e.message);
        let receipt = json!({"kind":"independent_stage_batch","tasks":tasks,"wallMs":wall.elapsed().as_millis(),"result":result.as_ref().map_err(|e|e.as_str())});
        let receipts = self.dir.join("method-receipts");
        std::fs::create_dir_all(&receipts).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&receipts).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&receipt).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        let name = format!(
            "{}-batch.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        file.persist_noclobber(receipts.join(name))
            .map_err(|e| e.to_string())?;
        result
    }
    fn generate(
        &self,
        stage: crate::horary_lessons::Stage,
        matter: crate::horary_lessons::Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<crate::native_llama_worker::NativeGenerationResult, String> {
        let prompt = crate::horary_pipeline::prompt(stage, matter, input, schema)?;
        let result = generate_native(
            &self.app.state::<NativeLlamaState>(),
            prompt,
            NativeGenerateOptions {
                max_tokens: if stage == crate::horary_lessons::Stage::Judgment {
                    1400
                } else {
                    1000
                },
                temperature: 0.,
                response_schema: audio.is_none().then(|| schema.to_string()),
                audio: audio.map(<[u8]>::to_vec),
                cache_lesson: audio.is_none(),
                cancel: Some(self.app.state::<ConversationState>().cancelled.clone()),
                ..Default::default()
            },
        )
        .map_err(|e| e.message);
        // Write the original output before parsing. Failed worksheets remain
        // inspectable; neither private words nor coordinates enter the log.
        let receipt = json!({"stage":stage,"guideSha256":crate::horary_lessons::digest(&crate::horary_contract::guide_for(stage,matter,input)?),"input":input,"schema":schema,"result":result.as_ref().map_err(|e|e.as_str())});
        let receipts = self.dir.join("method-receipts");
        std::fs::create_dir_all(&receipts).map_err(|e| e.to_string())?;
        let name = format!(
            "{}-{}-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            std::process::id(),
            stage.name()
        );
        let mut file = tempfile::NamedTempFile::new_in(&receipts).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&receipt).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist_noclobber(receipts.join(name))
            .map_err(|e| e.to_string())?;
        result
    }
    fn publish(&self, session: &mut Session) -> Result<(), String> {
        self.app
            .state::<ConversationState>()
            .publish(session, self.dir)
    }
    fn check(&self) -> Result<(), String> {
        self.app.state::<ConversationState>().check()
    }
    fn directory(&self) -> &Path {
        self.dir
    }
}

fn replace_leaf(app: &tauri::AppHandle, saved: Option<&str>) -> Result<Session, String> {
    let state = app.state::<ConversationState>();
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Pause the current reading before opening another leaf.")?;
    let _lease = Lease(&state.busy);
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let current = state.load(&dir)?;
    let mut next = if let Some(id) = saved {
        crate::reading_store::reopen(&dir, &current, id)?
    } else {
        crate::reading_store::fresh(&dir, &current, None)?
    };
    state.publish(&mut next, &dir)?;
    Ok(next)
}

#[tauri::command]
pub fn conversation_open(app: tauri::AppHandle) -> Result<Session, String> {
    let state = app.state::<ConversationState>();
    let mut opened = state
        .opened
        .lock()
        .map_err(|_| "Reading startup unavailable")?;
    if *opened {
        return state.snapshot(&app.path().app_data_dir().map_err(|e| e.to_string())?);
    }
    let next = replace_leaf(&app, None)?;
    *opened = true;
    Ok(next)
}
#[tauri::command]
pub fn conversation_fresh(app: tauri::AppHandle) -> Result<Session, String> {
    replace_leaf(&app, None)
}
#[tauri::command]
pub fn conversation_reopen(app: tauri::AppHandle, id: String) -> Result<Session, String> {
    replace_leaf(&app, Some(&id))
}

fn ensure_scope(session: &Session, expected: Option<&str>) -> Result<(), String> {
    if expected.is_some_and(|id| id != session.reading_id) {
        Err("Those words belong to an earlier leaf. Your current reading is unchanged.".into())
    } else {
        Ok(())
    }
}

fn run(
    app: &tauri::AppHandle,
    input: TurnInput,
    reading_id: Option<String>,
) -> Result<Session, String> {
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
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut session = state.load(&dir)?;
    ensure_scope(&session, reading_id.as_deref())?;
    // Acquire the conversation before consuming the one-use voice receipt.
    // A competing turn must leave the pending words intact.
    let (text, voice) = match input {
        TurnInput::Text(text) => (text, None),
        TurnInput::Voice(id) => {
            let voice = app.state::<crate::voice::VoiceState>().take(id)?;
            let text = match &voice.payload {
                crate::voice::VoicePayload::Text(text) => text.clone(),
                crate::voice::VoicePayload::Audio(_) => "Your spoken question…".into(),
            };
            (text, Some(voice))
        }
    };
    // Preparation and generation cannot move the question's submitted moment.
    let instant = voice.as_ref().map_or_else(now_ms, |v| v.received_at_ms);
    state.cancelled.store(false, Ordering::Release);
    if text.trim().is_empty() || text.len() > 8000 {
        return Err("Please send a message of 1–8000 bytes.".into());
    }
    let previous = session.clone();
    session.busy = true;
    session.messages.push(Message {
        role: "user".into(),
        text,
    });
    let mut audio = None;
    if let Some(voice) = voice {
        session.audit.push(json!({"event":"voice_input","route":voice.mode,"preparationMs":voice.preparation_ms,"audioPersisted":false}));
        note(
            &mut session,
            &dir,
            "voice",
            match voice.mode {
                crate::voice::VoiceMode::Native => "Your spoken words became writing here.",
                crate::voice::VoiceMode::GemmaTranscription => {
                    "Your spoken words were transcribed for comparison."
                }
                _ => "The reader is hearing your question directly.",
            },
            voice.preparation_ms,
        );
        if let crate::voice::VoicePayload::Audio(bytes) = voice.payload {
            audio = Some(bytes);
        }
    }
    note(
        &mut session,
        &dir,
        "received",
        "Your words were kept. The question's moment was noted.",
        0,
    );
    {
        use sha2::{Digest, Sha256};
        session.audit.push(json!({"event":"user_turn","build":env!("HORARY_BUILD_GIT_SHA"),"model":"gemma-4-12b-qat","policySha256":format!("{:x}",Sha256::digest(crate::horary_lessons::guide(crate::horary_lessons::Stage::Intake,crate::horary_lessons::Matter::Other)?)),"modelManifest":crate::model_manifest::bundled_model_manifest().map_err(|e|e.message)?}));
    }
    session.status = "The reading is gathering…".into();
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
                json!({"modelId":"gemma-4-12b-qat","ctxSize":crate::native_llama_worker::READING_CONTEXT_TOKENS,"nGpuLayers":"auto"}),
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
        let runtime = PipelineRuntime { app, dir: &dir };
        crate::horary_pipeline::run(
            &mut session,
            &runtime,
            &app.state::<GeocodeState>(),
            instant,
            audio.as_deref(),
            &previous,
        )
    })();
    session.status.clear();
    session.busy = false;
    if let Err(error) = result {
        session.method.flow.pause(error.clone());
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
            text: if state.cancelled.load(Ordering::Acquire) {
                "We can pause here. Your chart and completed passages are kept; say ‘continue’ when you’re ready."
            } else if session.chart.is_some() {
                "The chart is kept, but the reading hasn't finished. Say ‘continue’ and I'll pick up the unfinished step."
            } else {
                "Your question is kept. I couldn't finish this step; say ‘continue’ to try it again."
            }.into(),
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
    state.snapshot(&app.path().app_data_dir().map_err(|e| e.to_string())?)
}
#[tauri::command]
pub async fn conversation_send(
    app: tauri::AppHandle,
    text: String,
    reading_id: Option<String>,
) -> Result<Session, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app, TurnInput::Text(text), reading_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn conversation_voice(
    app: tauri::AppHandle,
    id: u64,
    reading_id: Option<String>,
) -> Result<Session, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app, TurnInput::Voice(id), reading_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn conversation_cancel(
    state: tauri::State<'_, ConversationState>,
    acquisition: tauri::State<'_, AcquisitionState>,
    voice: tauri::State<'_, crate::voice::VoiceState>,
) {
    state.cancelled.store(true, Ordering::Release);
    acquisition.0.cancel();
    voice.discard_pending();
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
        let travel = device_place(&context).unwrap().unwrap();
        assert_eq!(travel.latitude, 38.657);
        assert_eq!(travel.timezone, "America/New_York");
        context.timezone = "America/New_York".into();
        context.accuracy_meters = Some(50000.);
        assert!(device_place(&context).unwrap().is_none());
        context.latitude = None;
        assert!(device_place(&context).unwrap().is_none());
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
    #[test]
    fn operation_lease_does_not_modify_the_archived_reading() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConversationState::default();
        let mut original = Session {
            messages: vec![Message {
                role: "user".into(),
                text: "An unfinished question, kept exactly.".into(),
            }],
            ..Default::default()
        };
        state.publish(&mut original, dir.path()).unwrap();
        let expected = std::fs::read(dir.path().join(FILE)).unwrap();
        state.busy.store(true, Ordering::Release);
        let _lease = Lease(&state.busy);
        let predecessor = state.load(dir.path()).unwrap();
        assert!(state.snapshot(dir.path()).unwrap().busy);
        assert!(!predecessor.busy);
        let next = crate::reading_store::fresh(dir.path(), &predecessor, None).unwrap();
        let path = dir
            .path()
            .join("readings")
            .join(format!("{}.json", next.saved_readings[0].id));
        assert_eq!(std::fs::read(path).unwrap(), expected);
    }
    #[test]
    fn stale_reading_scope_cannot_consume_a_followup() {
        let session = Session {
            reading_id: "new-leaf".into(),
            ..Default::default()
        };
        assert!(ensure_scope(&session, Some("old-leaf")).is_err());
        assert!(ensure_scope(&session, Some("new-leaf")).is_ok());
    }
}
