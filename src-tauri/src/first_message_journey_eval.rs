//! Fresh application journeys from the full DSPy first-message case bank.
//! The oracle receives only the original question and the authored device.
//! Expected routes, facts, chart anchors and rubrics stay in private evidence.
#![forbid(unsafe_code)]

use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    file: PathBuf,
    sha256: String,
}

impl Pin {
    fn bytes(&self) -> Result<Vec<u8>, String> {
        if !self.file.is_absolute()
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("A first-message source needs an absolute file and SHA256".into());
        }
        let metadata = fs::symlink_metadata(&self.file).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
            return Err("A first-message source must be a bounded ordinary file".into());
        }
        let bytes = fs::read(&self.file).map_err(|error| error.to_string())?;
        if !horary_prompt_program::digest(&bytes).eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "First-message source hash changed: {}",
                self.file.display()
            ));
        }
        Ok(bytes)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Task {
    version: u32,
    synthetic_only: bool,
    fixture_jsonl: Pin,
    case_ids: Vec<String>,
    #[serde(default)]
    prompt_program: Option<Pin>,
    max_calls: u64,
    case_seconds: u64,
}

impl Task {
    fn validate(&self) -> Result<(), String> {
        let ids: BTreeSet<_> = self.case_ids.iter().collect();
        if self.version != 1
            || !self.synthetic_only
            || ids.is_empty()
            || ids.len() != self.case_ids.len()
            || self.case_ids.len() > 20
            || self.case_ids.iter().any(|id| !safe_id(id))
            || !(1..=100).contains(&self.max_calls)
            || !(1..=3600).contains(&self.case_seconds)
        {
            return Err("First-message tasks need version 1, synthetic cases, one to twenty exact unique IDs, and bounded call/deadline limits".into());
        }
        Ok(())
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

// This schema matches tools/horary-dspy/src/catalogue.rs. Do not import that
// evaluation crate: its label-aware helpers are not application input authority.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Turn {
    role: String,
    content: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Device {
    observed_at: String,
    time_zone: String,
    place: Place,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fact {
    field: String,
    value: String,
    quote: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    facet: String,
    facts: Vec<Fact>,
    missing: Vec<String>,
    reading_status: String,
    must_explain: Vec<String>,
    must_not: Vec<String>,
    source_pages: Vec<usize>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Chart {
    utc: String,
    latitude: f64,
    longitude: f64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FirstMessageCase {
    id: String,
    method: String,
    split: String,
    variant: String,
    question: String,
    history: Vec<Turn>,
    device: Device,
    expected: Expected,
    chart: Chart,
}

impl Device {
    fn instant(&self) -> Result<f64, String> {
        let instant = chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .map_err(|error| format!("Invalid fixture device instant: {error}"))?
            .timestamp_millis() as f64;
        horary_ai_core::chart_input::local_clock(instant, &self.time_zone)?;
        Ok(instant)
    }

    fn candidate(&self) -> Result<LocationCandidate, String> {
        if self.place.name.trim().is_empty()
            || !self.place.latitude.is_finite()
            || self.place.latitude.abs() >= 90.
            || !self.place.longitude.is_finite()
            || self.place.longitude.abs() > 180.
        {
            return Err("Invalid fixture device place".into());
        }
        self.instant()?;
        Ok(LocationCandidate {
            id: "device-location".into(),
            label: self.place.name.clone(),
            name: self.place.name.clone(),
            country: String::new(),
            latitude: self.place.latitude,
            longitude: self.place.longitude,
            timezone: self.time_zone.clone(),
            provider: "device".into(),
        })
    }
}

impl FirstMessageCase {
    fn validate(&self) -> Result<(), String> {
        if !safe_id(&self.id)
            || !matches!(self.split.as_str(), "train" | "development" | "test")
            || !matches!(self.variant.as_str(), "explicit" | "implicit")
            || self.question.trim().is_empty()
            || !self.history.is_empty()
            || !self.expected.missing.is_empty()
            || self.expected.reading_status != "answer"
            || self.expected.must_explain.is_empty()
            || self.expected.source_pages.is_empty()
            || self.expected.source_pages.contains(&0)
        {
            return Err(format!(
                "{} is not a complete first-message fixture",
                self.id
            ));
        }
        serde_json::from_value::<Method>(json!(self.method)).map_err(|error| error.to_string())?;
        serde_json::from_value::<Facet>(json!(self.expected.facet))
            .map_err(|error| error.to_string())?;
        for fact in &self.expected.facts {
            if fact.field.is_empty()
                || fact.value.is_empty()
                || !fact.quote.is_empty() && !self.question.contains(&fact.quote)
            {
                return Err(format!("{} has an invalid authored fact witness", self.id));
            }
        }
        self.device.candidate()?;
        chrono::DateTime::parse_from_rfc3339(&self.chart.utc)
            .map_err(|error| format!("Invalid private expected chart instant: {error}"))?;
        if !self.chart.latitude.is_finite()
            || self.chart.latitude.abs() >= 90.
            || !self.chart.longitude.is_finite()
            || self.chart.longitude.abs() > 180.
        {
            return Err("Invalid private expected chart coordinates".into());
        }
        Ok(())
    }
}

fn selected_cases(task: &Task, bytes: &[u8]) -> Result<Vec<FirstMessageCase>, String> {
    task.validate()?;
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut all = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let case: FirstMessageCase = serde_json::from_str(line)
            .map_err(|error| format!("First-message JSONL line {}: {error}", index + 1))?;
        case.validate()?;
        let id = case.id.clone();
        if all.insert(id.clone(), case).is_some() {
            return Err(format!("Duplicate first-message fixture {id}"));
        }
    }
    task.case_ids
        .iter()
        .map(|id| {
            all.remove(id)
                .ok_or_else(|| format!("Unknown exact first-message ID {id}"))
        })
        .collect()
}

/// Gold is deliberately absent from this constructor. The device clock is the
/// receipt instant; user-declared historical anchors are still resolved by the
/// existing native pipeline from the original utterance.
fn initial(case: &FirstMessageCase) -> Result<(Session, Session, LocationCandidate, f64), String> {
    let instant = case.device.instant()?;
    let device = case.device.candidate()?;
    let mut session = Session::default();
    session.candidates.push(device.clone());
    session.device_context = Some(crate::conversation::DeviceContext {
        timezone: case.device.time_zone.clone(),
        locale: "und".into(),
        latitude: Some(case.device.place.latitude),
        longitude: Some(case.device.place.longitude),
        accuracy_meters: None,
    });
    let previous = session.clone();
    session.messages.push(Message {
        role: "user".into(),
        text: case.question.clone(),
    });
    Ok((session, previous, device, instant))
}

/// Delegate all model work and persistence to the existing evaluator. Only the
/// acquisition fixture varies; this does not activate any component stop hook.
struct FixtureReader<'a> {
    reader: Reader<'a>,
    device: LocationCandidate,
}

impl Runtime for FixtureReader<'_> {
    fn guide_for(&self, stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
        self.reader.guide_for(stage, matter, input)
    }
    fn device_location(&self) -> Result<Option<LocationCandidate>, String> {
        let sequence = self.reader.device_calls.fetch_add(1, Ordering::AcqRel) + 1;
        let result = Some(self.device.clone());
        write_new(
            &self
                .reader
                .dir
                .join(format!("synthetic-device-acquisition-{sequence:04}.json")),
            &result,
        )?;
        Ok(result)
    }
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        self.reader.generate(stage, matter, input, schema, audio)
    }
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String> {
        if tasks.len() > 4 {
            return Err("Fresh journey batches are bounded to four native branches".into());
        }
        self.reader.generate_batch(tasks)
    }
    fn publish(&self, session: &mut Session) -> Result<(), String> {
        self.reader.publish(session)
    }
    fn check(&self) -> Result<(), String> {
        self.reader.check()
    }
    fn directory(&self) -> &Path {
        self.reader.directory()
    }
}

/// Find the accepted classifier record, not the later consultation frame. The
/// focused extractor is permitted to refine that frame after this completion.
fn accepted_classifier(session: &Session) -> Result<Option<Value>, String> {
    let mut receipts = Vec::new();
    for event in &session.audit {
        if event["event"] != "stage_transition"
            || event["stage"] != "intake"
            || event["to"] != "complete"
        {
            continue;
        }
        let index = event["recordIndex"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("Intake acceptance has no native record index")?;
        let record = session
            .method
            .records
            .get(index)
            .ok_or("Intake acceptance references an absent native record")?;
        let original = crate::horary_step::original_input(&record.input);
        if original["recognition_phase"] != "classify_question" {
            continue;
        }
        let jobs = session.method.flow.jobs.iter().filter(|job| {
            job.key == event["key"].as_str().unwrap_or("")
                && job.stage == Stage::Intake
                && job.revision == record.revision
                && matches!(job.phase(), crate::horary_step::Phase::Complete { record_index } if *record_index == index)
        }).collect::<Vec<_>>();
        if record.stage != Stage::Intake
            || record.validation_error.is_some()
            || record.input_sha256 != horary_lessons::digest(&record.input.to_string())
            || jobs.len() != 1
            || jobs[0].input_sha256 != horary_lessons::digest(&original.to_string())
            || record.raw != record.generation.content
            || crate::horary_contract::decode_json(&record.raw)? != record.worksheet
        {
            return Err(
                "Classifier acceptance does not bind its complete native job and record".into(),
            );
        }
        // Recheck the recorded permit with the same native authority. This
        // does not apply a patch, run a model or reconstruct a completed state.
        let checked = crate::horary_step::check(
            Stage::Intake,
            Matter::Other,
            &record.worksheet,
            &record.input,
            &[],
        )?;
        let crate::horary_step::Checked::Data(data) = checked else {
            return Err("Classifier completion has no native data permit".into());
        };
        let turn = data.turn().ok_or("Classifier permit has no checked Turn")?;
        receipts.push(json!({"record_index":index,"revision":record.revision,
            "record_sha256":horary_prompt_program::digest(serde_json::to_vec(record).map_err(|e|e.to_string())?),
            "record_input_sha256":record.input_sha256,"original_input_sha256":jobs[0].input_sha256,
            "guide_sha256":record.guide_sha256,"schema_sha256":record.schema_sha256,
            "raw_sha256":horary_prompt_program::digest(&record.raw),"job_key":jobs[0].key,
            "recognition_phase":"classify_question","actual_route":turn.frame,
            "accepted_question":turn.question,"accepted_question_sha256":turn.question.as_ref().map(horary_prompt_program::digest),
            "source_words":original["latest_words"],"spoken_input":original["spoken_input"],
            "first_user_words":session.messages.iter().find(|message|message.role=="user").map(|message|&message.text)}));
    }
    match receipts.len() {
        0 => Ok(None),
        1 => Ok(receipts.pop()),
        _ => Err(
            "More than one accepted classifier record makes this first-message boundary ambiguous"
                .into(),
        ),
    }
}

/// This private-gold subgrade observes only accepted method/facet and original
/// input continuity. It is independent of execution/reading success, and does
/// not judge principal, branch or semantic preservation of a concise question.
fn classification_grade(case: &FirstMessageCase, dir: &Path) -> Value {
    let mut grade = json!({"version":1,"scope":"accepted_first_message_classifier_method_facet",
        "state":"unobserved","passed":null,"accepted_classifier_phase_observed":false,
        "expected_route":{"method":case.method,"facet":case.expected.facet},
        "actual_route":null,"original_input_continuity":null,"acceptance_receipt":null,
        "source_correct_classification":"unobserved","semantic_question_preservation":"unobserved",
        "limits":"Mechanical private-gold route comparison only. Principal, branch, semantic question preservation and source-correct reading require independent review. Downstream failure does not change this grade."});
    let observation = (|| -> Result<Option<Value>, String> {
        let mut accepted = BTreeMap::new();
        let mut files = fs::read_dir(dir.join("checkpoints"))
            .map_err(|error| error.to_string())?
            .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for path in files {
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            let checkpoint: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let saved: Session = serde_json::from_value(checkpoint["session"].clone())
                .map_err(|error| error.to_string())?;
            if let Some(receipt) = accepted_classifier(&saved)? {
                // Repeated publishes retain the same receipt. Keep its first
                // checkpoint, including across a later new-question reset.
                let identity = horary_prompt_program::digest(receipt.to_string());
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("Non-UTF8 classifier checkpoint")?;
                accepted.entry(identity).or_insert(json!({"record":receipt,
                    "checkpoint":{"file":format!("checkpoints/{name}"),
                        "sha256":horary_prompt_program::digest(&bytes)}}));
                if accepted.len() > 1 {
                    return Err("Distinct accepted classifier receipts make this first-message boundary ambiguous".into());
                }
            }
        }
        Ok(accepted.into_values().next())
    })();
    match observation {
        Ok(None) => grade["reason"] = json!("No completed native classifier phase was observed"),
        Err(error) => {
            grade["state"] = json!("evidence_error");
            grade["reason"] = json!(error);
        }
        Ok(Some(receipt)) => {
            let record = &receipt["record"];
            let continuity = record["source_words"] == case.question
                && record["first_user_words"] == case.question
                && record["spoken_input"] == false;
            grade["accepted_classifier_phase_observed"] = json!(true);
            grade["original_input_continuity"] = json!(continuity);
            grade["actual_route"] = record["actual_route"].clone();
            if !continuity {
                grade["state"] = json!("evidence_error");
                grade["reason"] = json!(
                    "Accepted classifier input is not this exact original typed first message"
                );
            } else if record["actual_route"].is_null() {
                grade["reason"] =
                    json!("Native phase completed without an observed method/facet route");
            } else {
                let pass = record["actual_route"] == grade["expected_route"];
                grade["state"] = json!(if pass { "pass" } else { "mismatch" });
                grade["passed"] = json!(pass);
            }
            grade["original_question_sha256"] =
                json!(horary_prompt_program::digest(&case.question));
            grade["acceptance_receipt"] = receipt;
        }
    }
    grade
}

fn execute(
    state: &NativeLlamaState,
    hosted: Arc<crate::hosted_gemma_eval::Client>,
    directory: &Path,
    case: &FirstMessageCase,
    task: &Task,
    program: Option<Arc<horary_prompt_program::Program>>,
) -> Result<Value, String> {
    let dir = directory.join("cases").join(&case.id);
    fs::create_dir(&dir).map_err(|error| error.to_string())?;
    fs::create_dir(dir.join("calls")).map_err(|error| error.to_string())?;
    fs::create_dir(dir.join("checkpoints")).map_err(|error| error.to_string())?;
    write_new(&dir.join("fixture.json"), case)?;
    let (mut session, previous, device, instant) = initial(case)?;
    write_new(
        &dir.join("initial.json"),
        &json!({"session":session,"candidates":session.candidates,"receipt_instant_ms":instant}),
    )?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let deadline = Deadline::start(task.case_seconds, cancelled.clone());
    let reader = FixtureReader {
        reader: Reader {
            state,
            hosted: Some(hosted),
            response_memo: None,
            dir: dir.clone(),
            cancelled,
            calls: Mutex::new(Vec::new()),
            sequence: AtomicU64::new(0),
            checkpoints: AtomicU64::new(0),
            max_calls: task.max_calls,
            device_available: false, // Never fall back to the old Oct 7 fixture.
            device_calls: AtomicU64::new(0),
            dispatcher: None,
            group: None,
            program,
            initial_extractor_target: None,
            supplying_extractor_target: None,
            selected_stage_target: None,
        },
        device,
    };
    let start = Instant::now();
    println!(
        "{}",
        json!({"event":"first_message_case_started","case_id":case.id})
    );
    let result = crate::horary_pipeline::run(
        &mut session,
        &reader,
        &GeocodeState::default(),
        instant,
        None,
        &previous,
    );
    drop(deadline);
    // This invokes only the existing native structure checker, not a semantic
    // input or interpretation judge. Its flag means inputs reached native
    // execution, not that expected facts or method obligations were verified.
    let native_structure = crate::reading_eval::reading_gate(
        &session,
        true,
        true,
        result.as_ref().err().map(String::as_str),
    );
    let native_classification = classification_grade(case, &dir);
    let calls = reader
        .reader
        .calls
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let outcome = json!({"id":case.id,"scope":"fresh_application_first_message_journey",
        "execution_completed":result.is_ok(),"result":result,"elapsed_ms":start.elapsed().as_millis(),
        "native_structure":native_structure,"native_classification":native_classification,"semantic_input_review":"unobserved",
        "source_correct_interpretation":"unobserved",
        "semantic_review_requirement":"Independent input and book/rubric interpretation review required",
        "qualified":false,"original_question":case.question,"receipt_instant_ms":instant,
        "original_question_sha256":horary_prompt_program::digest(&case.question),
        "deadline_cancelled":reader.reader.cancelled.load(Ordering::Acquire),
        "model_call_groups":calls.len(),"generation_attempts":input_journey_generation_attempts(&calls),
        "declared_route":{"method":case.method,"facet":case.expected.facet},
        "actual_consultation":session.method.consultation,"native_result":session.method.result,
        "trace":format!("cases/{}/trace.json",case.id)});
    write_new(
        &dir.join("first-turn.json"),
        &json!({"result":result,"session":session,"candidates":session.candidates}),
    )?;
    write_new(
        &dir.join("final.json"),
        &json!({"session":session,"candidates":session.candidates,"outcome":outcome}),
    )?;
    write_new(
        &dir.join("trace.json"),
        &json!({"fixture":case,"initial_receipt_ms":instant,"result":result,"session":session,"calls":calls,"outcome":outcome}),
    )?;
    write_new(&dir.join("outcome.json"), &outcome)?;
    println!(
        "{}",
        json!({"event":"first_message_case_completed","case_id":case.id,"execution_completed":result.is_ok(),"native_structure":native_structure,"native_classification":native_classification})
    );
    Ok(outcome)
}

#[test]
#[ignore = "Explicit fresh model journey: pinned HORARY_FIRST_MESSAGE_TASK, fresh HORARY_EVAL_EVIDENCE, and local endpoint or Google credential locator required"]
fn real_model_first_message_application_journey() -> Result<(), String> {
    let task_file = PathBuf::from(
        std::env::var_os("HORARY_FIRST_MESSAGE_TASK").ok_or("Set HORARY_FIRST_MESSAGE_TASK")?,
    );
    let task_bytes = fs::read(&task_file).map_err(|error| error.to_string())?;
    let task: Task = serde_json::from_slice(&task_bytes).map_err(|error| error.to_string())?;
    task.validate()?;
    let cases = selected_cases(&task, &task.fixture_jsonl.bytes()?)?;
    let program_bytes = task.prompt_program.as_ref().map(Pin::bytes).transpose()?;
    let program = program_bytes
        .as_deref()
        .map(horary_prompt_program::Program::parse)
        .transpose()?
        .map(Arc::new);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Repository root unavailable")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let dir = PathBuf::from(
        std::env::var_os("HORARY_EVAL_EVIDENCE").ok_or("Set fresh HORARY_EVAL_EVIDENCE")?,
    );
    // A fresh directory is the durable no-replay guard, as in the existing
    // campaigns. Never import checkpoints, cached responses or prior sessions.
    fs::create_dir(&dir).map_err(|error| error.to_string())?;
    fs::create_dir(dir.join("cases")).map_err(|error| error.to_string())?;
    let dir = dir.canonicalize().map_err(|error| error.to_string())?;
    let local = std::env::var("HORARY_FIRST_MESSAGE_LOCAL_URL").ok();
    let hosted = Arc::new(if let Some(endpoint) = &local {
        crate::hosted_gemma_eval::Client::from_local_endpoint(endpoint, 4)?
    } else {
        let keyfile = PathBuf::from(
            std::env::var_os("HORARY_GOOGLE_KEY_FILE")
                .ok_or("Set private HORARY_GOOGLE_KEY_FILE or HORARY_FIRST_MESSAGE_LOCAL_URL")?,
        );
        let credential_target = keyfile
            .canonicalize()
            .map_err(|_| "Hosted credential unavailable")?;
        if credential_target.starts_with(&root) || credential_target.starts_with(&dir) {
            return Err("Keep hosted credentials outside source and evidence".into());
        }
        crate::hosted_gemma_eval::Client::from_file(&keyfile, 4)?
    });
    let attempts_per_branch = if local.is_some() { 1 } else { 3 };
    let sources = source_hashes(&root)?;
    let manifest = json!({"version":1,"scope":"fresh_application_first_message_journey",
        "task_sha256":horary_prompt_program::digest(&task_bytes),"task":task,
        "sources":sources,"sources_sha256":horary_prompt_program::digest(serde_json::to_vec(&sources).map_err(|error|error.to_string())?),
        "git_head":git(&root,&["rev-parse","HEAD"])? ,
        "working_diff_sha256":horary_prompt_program::digest(git(&root,&["diff","--binary","HEAD"])?),
        "executable_sha256":model_digest(&std::env::current_exe().map_err(|error|error.to_string())?)?,
        "model":hosted.metadata(),"program_sha256":program_bytes.as_ref().map(horary_prompt_program::digest),
        "oracle_inputs":"Original question plus the selected fixture's own device clock, place and timezone only",
        "private_expected_data":"No expected method/facts/roles/chart or completed state is injected",
        "qualification":"Native structure plus separately checkpoint-bound classifier method/facet comparison; independent input and source-correct interpretation review remains required",
        "limits":{"native_call_groups_per_case":task.max_calls,"maximum_branches_per_group":4,
            "completed_http_attempts_per_branch":attempts_per_branch,"case_seconds":task.case_seconds,
            "physical_attempt_reservation":task.max_calls * 4 * attempts_per_branch * cases.len() as u64,
            "reservation_basis":"Reader group cap, four-branch guard and selected transport attempt bound; observed attempts are reported separately"},
        "provider_limits":if local.is_some(){"Local four-slot generation; no hosted quota pacing or automatic HTTP retries"}else{"Per-case call-group budget and deadline, four parallel branches and bounded completed-HTTP retries, token-paced and retained"}});
    atomic_evidence_file(&dir.join("task.json"), true, |file| {
        file.write_all(&task_bytes)
            .map_err(|error| error.to_string())
    })?;
    write_new(&dir.join("manifest.json"), &manifest)?;
    if let Some(bytes) = program_bytes {
        atomic_evidence_file(&dir.join("prompt-program.json"), true, |file| {
            file.write_all(&bytes).map_err(|error| error.to_string())
        })?;
    }
    let state = NativeLlamaState::default();
    let mut outcomes = Vec::new();
    for case in &cases {
        if source_hashes(&root)? != sources {
            let error = "Application source changed during the fresh journey gate; no further case submitted";
            write_new(
                &dir.join("interruption.json"),
                &json!({"case_id":case.id,"error":error,"completed_case_ids":outcomes.iter().map(|value:&Value|&value["id"]).collect::<Vec<_>>()}),
            )?;
            return Err(error.into());
        }
        let outcome = match execute(&state, hosted.clone(), &dir, case, &task, program.clone()) {
            Ok(outcome) => outcome,
            Err(error) => {
                write_new(
                    &dir.join("interruption.json"),
                    &json!({"case_id":case.id,"error":error,"completed_case_ids":outcomes.iter().map(|value:&Value|&value["id"]).collect::<Vec<_>>()}),
                )?;
                return Err(error);
            }
        };
        let completed = outcome["execution_completed"] == true;
        outcomes.push(outcome);
        write_index_json(
            &dir.join("summary.json"),
            &json!({"status":if completed {"running"} else {"stopped_after_execution_error"},"outcomes":outcomes,"source_correct_interpretation":"unobserved","qualified":false}),
        )?;
        if !completed {
            return Err("Fresh native journey stopped after its recorded execution error; remaining cases were not submitted".into());
        }
    }
    write_index_json(
        &dir.join("summary.json"),
        &json!({"status":"closed","outcomes":outcomes,"source_correct_interpretation":"unobserved","qualified":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FirstMessageCase {
        FirstMessageCase {
            id: "offline-first-message".into(),
            method: "movable_deal".into(),
            split: "train".into(),
            variant: "explicit".into(),
            question: "Will I sell my violin to Ada at the fair tomorrow?".into(),
            history: vec![],
            device: Device {
                observed_at: "2026-10-11T20:14:27.123+02:00".into(),
                time_zone: "Europe/Berlin".into(),
                place: Place {
                    name: "Berlin, Germany".into(),
                    latitude: 52.52,
                    longitude: 13.405,
                },
            },
            expected: Expected {
                facet: "event".into(),
                facts: vec![Fact {
                    field: "subject.name".into(),
                    value: "PRIVATE_EXPECTED_ONLY_93".into(),
                    quote: String::new(),
                }],
                missing: vec![],
                reading_status: "answer".into(),
                must_explain: vec!["PRIVATE_RUBRIC_ONLY_93".into()],
                must_not: vec!["PRIVATE_FORBIDDEN_ONLY_93".into()],
                source_pages: vec![133],
            },
            chart: Chart {
                utc: "2025-04-21T12:00:00Z".into(),
                latitude: -35.29,
                longitude: 149.13,
            },
        }
    }

    fn task(case: &FirstMessageCase) -> Task {
        Task {
            version: 1,
            synthetic_only: true,
            fixture_jsonl: Pin {
                file: PathBuf::from("/offline/fixtures.jsonl"),
                sha256: horary_prompt_program::digest("offline"),
            },
            case_ids: vec![case.id.clone()],
            prompt_program: None,
            max_calls: 28,
            case_seconds: 600,
        }
    }

    struct Capture {
        dir: PathBuf,
        requests: Mutex<Vec<Value>>,
        responses: Mutex<std::collections::VecDeque<String>>,
        checkpoints: AtomicU64,
    }
    impl Capture {
        fn new(dir: &Path, responses: Vec<String>) -> Self {
            Self {
                dir: dir.to_owned(),
                requests: Mutex::new(vec![]),
                responses: Mutex::new(responses.into()),
                checkpoints: AtomicU64::new(0),
            }
        }
    }
    impl Runtime for Capture {
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            schema: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            self.requests.lock().unwrap().push(json!({"stage":stage,"input":input,"prompt":trial_prompt(stage,matter,input,schema,None)?.0}));
            let content = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or("Offline witness stop before inference")?;
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
            _tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            Err("Offline witness does not infer".into())
        }
        fn publish(&self, session: &mut Session) -> Result<(), String> {
            // Mirror the existing Reader checkpoint boundary; the native
            // executor, not this mock, supplies records and completion events.
            let sequence = self.checkpoints.fetch_add(1, Ordering::AcqRel) + 1;
            let path = self.dir.join("checkpoints");
            fs::create_dir_all(&path).map_err(|error| error.to_string())?;
            write_new(
                &path.join(format!("{sequence:04}.json")),
                &json!({"session":session,"candidates":session.candidates}),
            )
        }
        fn check(&self) -> Result<(), String> {
            Ok(())
        }
        fn directory(&self) -> &Path {
            &self.dir
        }
    }

    #[test]
    fn fixture_clock_and_place_override_only_the_old_evaluation_defaults() {
        let case = fixture();
        case.validate().unwrap();
        let (session, previous, candidate, instant) = initial(&case).unwrap();
        let expected = chrono::DateTime::parse_from_rfc3339("2026-10-11T18:14:27.123Z")
            .unwrap()
            .timestamp_millis() as f64;
        assert_eq!(instant, expected);
        assert_ne!(instant, frozen_moment());
        assert_eq!(candidate.latitude, 52.52);
        assert_eq!(candidate.longitude, 13.405);
        assert_eq!(candidate.timezone, "Europe/Berlin");
        assert_ne!(candidate.latitude, device().latitude);
        assert_eq!(
            horary_ai_core::chart_input::local_clock(instant, &candidate.timezone).unwrap(),
            "2026-10-11T20:14"
        );
        assert!(previous.messages.is_empty());
        assert!(session.chart.is_none());
        assert!(session.method.consultation.is_none());
        assert!(session.method.records.is_empty());
        assert!(session.candidate_moment_ms.is_none());
        assert!(session.place.is_none());
    }

    #[test]
    fn actual_native_first_request_keeps_original_words_and_cannot_see_expected_data() {
        let case = fixture();
        let dir = tempfile::tempdir().unwrap();
        let capture = Capture::new(dir.path(), vec![]);
        let (mut session, previous, _, instant) = initial(&case).unwrap();
        let result = crate::horary_pipeline::run(
            &mut session,
            &capture,
            &GeocodeState::default(),
            instant,
            None,
            &previous,
        );
        assert!(result.is_err());
        let requests = capture.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["stage"], "intake");
        let wire = requests[0].to_string();
        assert!(wire.contains(&case.question));
        assert!(wire.contains("Berlin, Germany"));
        assert!(wire.contains("2026-10-11T20:14"));
        for private in [
            "PRIVATE_EXPECTED_ONLY_93",
            "PRIVATE_RUBRIC_ONLY_93",
            "PRIVATE_FORBIDDEN_ONLY_93",
            "2025-04-21T12:00:00Z",
        ] {
            assert!(!wire.contains(private));
        }
        assert_eq!(session.messages[0].text, case.question);
        let mut other_gold = case.clone();
        other_gold.chart.utc = "2024-03-03T08:00:00Z".into();
        other_gold.chart.latitude = 1.;
        other_gold.expected.facts.clear();
        other_gold.method = "new_job".into();
        other_gold.expected.facet = "profit".into();
        assert_eq!(
            serde_json::to_value(initial(&case).unwrap().0).unwrap(),
            serde_json::to_value(initial(&other_gold).unwrap().0).unwrap()
        );
    }

    #[test]
    fn strict_selection_rejects_missing_duplicate_or_non_first_message_cases() {
        let case = fixture();
        let mut selection = task(&case);
        let bytes = serde_json::to_vec(&case).unwrap();
        assert_eq!(selected_cases(&selection, &bytes).unwrap().len(), 1);
        selection.case_ids.push(case.id.clone());
        assert!(selected_cases(&selection, &bytes).is_err());
        selection.case_ids = vec!["not-a-real-id".into()];
        assert!(selected_cases(&selection, &bytes).is_err());
        selection.case_ids = vec![case.id.clone()];
        let mut followup = case.clone();
        followup.history.push(Turn {
            role: "user".into(),
            content: "An earlier question".into(),
        });
        assert!(selected_cases(&selection, &serde_json::to_vec(&followup).unwrap()).is_err());
        let mut unknown = serde_json::to_value(&case).unwrap();
        unknown["accepted_checkpoint"] = json!({});
        assert!(selected_cases(&selection, &serde_json::to_vec(&unknown).unwrap()).is_err());
    }

    #[test]
    fn changed_source_and_replay_fields_are_rejected_before_inference() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("fixture.jsonl");
        fs::write(&file, "first bytes").unwrap();
        let pin = Pin {
            file,
            sha256: horary_prompt_program::digest("first bytes"),
        };
        assert_eq!(pin.bytes().unwrap(), b"first bytes");
        fs::write(&pin.file, "changed bytes").unwrap();
        assert!(pin.bytes().is_err());
        let mut value = serde_json::to_value(task(&fixture())).unwrap();
        value["checkpoint"] = json!("old-state.json");
        assert!(serde_json::from_value::<Task>(value).is_err());
    }

    fn classifier_reply(case: &FirstMessageCase, method: Method, facet: Facet) -> String {
        let mut turn = reading_contracts::control(reading_contracts::Intent::Read);
        turn.question = Some(case.question.clone());
        turn.frame = Some(reading_contracts::Frame { method, facet });
        serde_json::to_string(&turn).unwrap()
    }

    fn offline_journey(
        replies: Vec<String>,
    ) -> (
        FirstMessageCase,
        tempfile::TempDir,
        Session,
        Result<(), String>,
    ) {
        let case = fixture();
        let dir = tempfile::tempdir().unwrap();
        let capture = Capture::new(dir.path(), replies);
        let (mut session, previous, _, instant) = initial(&case).unwrap();
        let result = crate::horary_pipeline::run(
            &mut session,
            &capture,
            &GeocodeState::default(),
            instant,
            None,
            &previous,
        );
        (case, dir, session, result)
    }

    #[test]
    fn accepted_classifier_pass_is_separate_from_downstream_failure() {
        let (case, dir, session, result) = offline_journey(vec![classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        )]);
        assert!(result.is_err());
        assert!(session.method.records[0].validation_error.is_none());
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "pass");
        assert_eq!(grade["passed"], true);
        assert_eq!(grade["original_input_continuity"], true);
        assert_eq!(grade["semantic_question_preservation"], "unobserved");
        assert_eq!(grade["source_correct_classification"], "unobserved");
        let checkpoint = &grade["acceptance_receipt"]["checkpoint"];
        let bytes = fs::read(dir.path().join(checkpoint["file"].as_str().unwrap())).unwrap();
        assert_eq!(checkpoint["sha256"], horary_prompt_program::digest(bytes));
        let saved: Value = serde_json::from_slice(
            &fs::read(dir.path().join(checkpoint["file"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        // This is before the classifier patch was applied to the consultation,
        // and before any focused extraction or reading result existed.
        assert!(saved["session"]["method"]["consultation"]["frame"]["state"] == "missing");
        assert!(saved["session"]["method"]["result"].is_null());
    }

    #[test]
    fn focused_frame_refinement_does_not_regrade_the_accepted_classifier() {
        let mut refinement = reading_contracts::control(reading_contracts::Intent::Clarify);
        refinement.frame = Some(reading_contracts::Frame {
            method: Method::Property,
            facet: Facet::Event,
        });
        let (case, dir, session, result) = offline_journey(vec![
            classifier_reply(&fixture(), Method::MovableDeal, Facet::Event),
            serde_json::to_string(&refinement).unwrap(),
        ]);
        assert!(result.is_err());
        assert_eq!(
            session.method.consultation.as_ref().unwrap().method(),
            Some(Method::Property)
        );
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["passed"], true);
        assert_eq!(grade["actual_route"]["method"], "movable_deal");
        assert_eq!(grade["acceptance_receipt"]["record"]["record_index"], 0);
    }

    #[test]
    fn accepted_method_and_facet_mismatches_are_observed_independently() {
        for (method, facet) in [
            (Method::Relationship, Facet::Event),
            (Method::MovableDeal, Facet::Timing),
        ] {
            let (case, dir, _session, result) =
                offline_journey(vec![classifier_reply(&fixture(), method, facet)]);
            assert!(result.is_err());
            let grade = classification_grade(&case, dir.path());
            assert_eq!(grade["state"], "mismatch");
            assert_eq!(grade["passed"], false);
            assert_eq!(grade["accepted_classifier_phase_observed"], true);
            assert_eq!(
                grade["actual_route"],
                json!({"method":method,"facet":facet})
            );
        }
    }

    #[test]
    fn repair_receipt_uses_original_input_and_does_not_require_a_literal_full_message_question() {
        let mut rejected: Value = serde_json::from_str(&classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        ))
        .unwrap();
        rejected["question"] = Value::Null;
        let mut accepted = rejected.clone();
        accepted["question"] = json!("Will I sell my violin?");
        let (case, dir, session, result) =
            offline_journey(vec![rejected.to_string(), accepted.to_string()]);
        assert!(result.is_err());
        assert!(session.method.records[0].validation_error.is_some());
        assert!(session.method.records[1].input["original_input"].is_object());
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["passed"], true);
        assert_eq!(grade["acceptance_receipt"]["record"]["record_index"], 1);
        assert_eq!(
            grade["acceptance_receipt"]["record"]["accepted_question"],
            "Will I sell my violin?"
        );
        assert_eq!(grade["semantic_question_preservation"], "unobserved");
    }

    #[test]
    fn absent_acceptance_cannot_pass_from_a_later_frame_or_private_expected_route() {
        let (case, dir, mut session, result) = offline_journey(vec![]);
        assert!(result.is_err());
        let turn: reading_contracts::Turn =
            serde_json::from_str(&classifier_reply(&case, Method::MovableDeal, Facet::Event))
                .unwrap();
        session
            .method
            .consultation
            .as_mut()
            .unwrap()
            .apply(&turn, 1, &case.question, false)
            .unwrap();
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "unobserved");
        assert!(grade["passed"].is_null());
        assert_eq!(grade["accepted_classifier_phase_observed"], false);
        assert!(grade["actual_route"].is_null());
    }

    #[test]
    fn a_completed_classifier_phase_without_a_route_remains_unobserved() {
        let mut reply: Value = serde_json::from_str(&classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        ))
        .unwrap();
        reply["frame"] = Value::Null;
        let (case, dir, _session, result) = offline_journey(vec![reply.to_string()]);
        assert!(result.is_err());
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "unobserved");
        assert_eq!(grade["accepted_classifier_phase_observed"], true);
        assert!(grade["passed"].is_null());
        assert!(grade["actual_route"].is_null());
        assert!(grade["acceptance_receipt"].is_object());
    }

    #[test]
    fn incomplete_or_ambiguous_receipts_remain_unknown_not_negative_or_pass() {
        let (case, dir, session, _) = offline_journey(vec![classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        )]);
        let mut duplicate = session.clone();
        let complete = duplicate
            .audit
            .iter()
            .find(|event| event["to"] == "complete")
            .unwrap()
            .clone();
        duplicate.audit.push(complete);
        let mut wrong_job = session.clone();
        wrong_job.method.flow.jobs[0].input_sha256 = horary_prompt_program::digest("another input");
        let extra = dir.path().join("checkpoints/9999.json");
        for changed in [duplicate, wrong_job] {
            fs::write(
                &extra,
                serde_json::to_vec(&json!({"session":changed})).unwrap(),
            )
            .unwrap();
            let grade = classification_grade(&case, dir.path());
            assert_eq!(grade["state"], "evidence_error");
            assert!(grade["passed"].is_null());
        }
        let (_, _, other, _) = offline_journey(vec![classifier_reply(
            &fixture(),
            Method::Relationship,
            Facet::Event,
        )]);
        fs::write(
            &extra,
            serde_json::to_vec(&json!({"session":other})).unwrap(),
        )
        .unwrap();
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "evidence_error");
        assert!(grade["passed"].is_null());
        fs::remove_dir_all(dir.path().join("checkpoints")).unwrap();
        fs::create_dir(dir.path().join("checkpoints")).unwrap();
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "unobserved");
        assert!(grade["passed"].is_null());
    }

    #[test]
    fn a_different_original_message_is_an_evidence_error_not_a_route_score() {
        let (mut case, dir, _session, _) = offline_journey(vec![classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        )]);
        case.question = "Will I sell someone else's piano?".into();
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["original_input_continuity"], false);
        assert_eq!(grade["state"], "evidence_error");
        assert!(grade["passed"].is_null());
    }

    #[test]
    fn a_new_question_reset_does_not_erase_the_observed_classifier_acceptance() {
        let mut reply: Value = serde_json::from_str(&classifier_reply(
            &fixture(),
            Method::MovableDeal,
            Facet::Event,
        ))
        .unwrap();
        reply["intent"] = json!("new_question");
        let (case, dir, session, result) = offline_journey(vec![reply.to_string()]);
        assert!(result.is_err());
        assert!(accepted_classifier(&session).unwrap().is_none());
        assert!(!session.method.records.is_empty());
        let grade = classification_grade(&case, dir.path());
        assert_eq!(grade["state"], "pass");
        assert_eq!(grade["passed"], true);
        assert_eq!(grade["original_input_continuity"], true);
        assert!(grade["acceptance_receipt"]["checkpoint"]["sha256"].is_string());
    }
}
