//! Teacher routing only: actual listed text models, then the authorized Codex
//! fallback. A completed bad answer is never relabeled as a quota failure.
use crate::{
    codex::Codex,
    google::{self, CallBudget, CompletedTruncation, Disposition, Google, ModelInfo, RateLimiter},
};
use anyhow::{Context, Result, ensure};
use dsrust::lm::{
    Capabilities, ChatModel,
    api::{LmPart, LmRequest, LmResponse},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

const SPECIALIZED_ROUTES: &[&str] = &[
    "embedding",
    "-tts",
    "image",
    "live",
    "audio",
    "robotics",
    "deep-research",
    "transcribe",
    "nano-banana",
    "computer-use",
];
const FORMAT_REPAIR_REMINDER: &str = "Follow this request's exact output format. Return only its requested OUTPUT fields or rewritten instruction, beginning directly with that output. Do not echo input fields, examples, book passages, rubric, or prompt. Do not add a preamble or a reasoning transcript. Use the original supplied evidence and complete every requested output field.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatRepair {
    version: usize,
    original_pool_request_sha256: String,
    witness: CompletedTruncation,
    repaired_request: LmRequest,
    repaired_google_key: String,
}
impl FormatRepair {
    fn prepare(request: &LmRequest, witness: CompletedTruncation) -> Result<Self> {
        let limit = 8192.min(witness.model_output_token_limit);
        ensure!(
            limit > witness.original_max_output_tokens,
            "Listed teacher output limit does not permit the bounded larger format repair"
        );
        let mut repaired = request.clone();
        let system = repaired
            .messages
            .iter_mut()
            .find(|m| m.role == "system")
            .context("Teacher format repair needs the original text system instruction")?;
        ensure!(
            system.parts.len() == 1,
            "Teacher format repair cannot translate a multipart system instruction"
        );
        let original = system.parts[0]
            .as_text()
            .context("Teacher format repair needs ordinary text")?;
        system.parts[0] = LmPart::text(format!("{original}\n\n{FORMAT_REPAIR_REMINDER}"));
        repaired.config.max_tokens = Some(limit as u32);
        let key = repaired.cache_key(&witness.model);
        ensure!(
            key != witness.original_key,
            "Teacher format repair must use a distinct request key"
        );
        Ok(Self {
            version: 1,
            original_pool_request_sha256: google::digest(serde_json::to_vec(request)?),
            repaired_request: repaired,
            repaired_google_key: key,
            witness,
        })
    }
}

pub fn is_text_teacher(info: &ModelInfo) -> bool {
    let id = info.name.strip_prefix("models/").unwrap_or_default();
    id.starts_with("gemini-")
        && info
            .supported_generation_methods
            .iter()
            .any(|m| m == "generateContent")
        && !SPECIALIZED_ROUTES.iter().any(|kind| id.contains(kind))
}
fn version(info: &ModelInfo) -> (usize, usize) {
    let id = info.name.strip_prefix("models/gemini-").unwrap_or_default();
    let number = id.split('-').next().unwrap_or_default();
    let mut parts = number.split('.');
    (
        parts.next().and_then(|n| n.parse().ok()).unwrap_or(0),
        parts.next().and_then(|n| n.parse().ok()).unwrap_or(0),
    )
}
pub fn available(listing: &Value) -> Result<Vec<ModelInfo>> {
    ensure!(
        listing["listing_complete"] == true,
        "Teacher pool needs a complete actual model listing"
    );
    let rows = listing["models"]
        .as_array()
        .context("No actual models list")?;
    let mut models = rows
        .iter()
        .filter(|row| {
            let name = row["name"].as_str().unwrap_or_default();
            name.starts_with("models/gemini-")
                && row["supportedGenerationMethods"]
                    .as_array()
                    .is_some_and(|methods| methods.iter().any(|m| m == "generateContent"))
                && !SPECIALIZED_ROUTES.iter().any(|kind| name.contains(kind))
        })
        .map(|row| serde_json::from_value::<ModelInfo>(row.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for model in &models {
        model.id()?;
    }
    models.sort_by(|a, b| {
        let preferred = |m: &ModelInfo| m.name == format!("models/{}", google::TEACHER);
        preferred(b)
            .cmp(&preferred(a))
            .then_with(|| version(b).cmp(&version(a)))
            .then_with(|| a.name.cmp(&b.name))
    });
    models.dedup_by(|a, b| a.name == b.name);
    Ok(models)
}

pub struct TeacherPool {
    models: Vec<Google>,
    fallback: Option<Arc<Codex>>,
    directory: PathBuf,
    // A known exhausted route is not bought again for every new function.
    // This bounded pool proceeds one way; exact settled successes remain reusable.
    serial: Mutex<usize>,
}
impl TeacherPool {
    pub fn new(
        http: reqwest::Client,
        listing: &Value,
        directory: PathBuf,
        budget: Arc<CallBudget>,
        tokens_per_minute_hint: usize,
        fallback: Option<Codex>,
    ) -> Result<Self> {
        let choices = available(listing)?;
        ensure!(
            !choices.is_empty() || fallback.is_some(),
            "No listed text teacher or authorized fallback"
        );
        fs::create_dir_all(&directory)?;
        let models = choices
            .into_iter()
            .map(|info| {
                let path = directory.join(info.id()?);
                Google::from_listing(
                    info,
                    http.clone(),
                    path,
                    budget.clone(),
                    RateLimiter::new(tokens_per_minute_hint)?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            models,
            fallback: fallback.map(|c| Arc::new(c.with_budget(budget))),
            directory,
            serial: Mutex::new(0),
        })
    }
    /// A sealed continuation may restore its last witnessed route without
    /// discarding successful caches belonging to earlier teacher models.
    pub fn with_start_route(mut self, index: usize) -> Result<Self> {
        ensure!(
            index <= self.models.len(),
            "Teacher start route is outside the actual listed pool"
        );
        *self.serial.get_mut() = index;
        Ok(self)
    }
    fn repair_path(&self, request: &LmRequest) -> Result<PathBuf> {
        Ok(self.directory.join(format!(
            "{}.format-repair.json",
            google::digest(serde_json::to_vec(request)?)
        )))
    }
    async fn saved_repair(&self, request: &LmRequest) -> Result<Option<LmResponse>> {
        let path = self.repair_path(request)?;
        if !path.exists() {
            return Ok(None);
        }
        let binding: FormatRepair = serde_json::from_slice(&fs::read(&path)?)?;
        ensure!(
            binding.version == 1,
            "Unknown teacher format-repair binding version"
        );
        let model = self
            .models
            .iter()
            .find(|m| m.model_id() == binding.witness.model)
            .context("Bound format-repair teacher is absent from actual listing")?;
        let actual = model
            .completed_truncation(request)
            .await?
            .context("Bound teacher truncation witness is absent")?;
        ensure!(
            actual == binding.witness,
            "Bound teacher truncation witness changed"
        );
        let expected = FormatRepair::prepare(request, actual)?;
        ensure!(
            binding.original_pool_request_sha256 == expected.original_pool_request_sha256
                && binding.repaired_request == expected.repaired_request
                && binding.repaired_google_key == expected.repaired_google_key,
            "Bound teacher repair request identity differs"
        );
        if let Some(response) = model.cached_response(&binding.repaired_request)? {
            self.record(request, json!({"model":response.model,"api":"generateContent","outcome":"format_repair_reused","cache_hit":true,"original_logical_key":binding.witness.original_key,"original_raw_sha256":binding.witness.original_raw_sha256,"repaired_request_key":binding.repaired_google_key,"response_id":response.response_id,"response_observation_sha256":google::digest(serde_json::to_vec(&response)?)}))?;
            return Ok(Some(response));
        }
        Err(google::ProviderFailure { disposition:Disposition::PriorUnresolved, model:binding.witness.model, http_status:None, retry_after:None, generation_submitted:false, detail:"The one bounded teacher format repair is already prepared, failed or unsettled; no repeated submission is permitted".into() }.into())
    }
    async fn repair(
        &self,
        request: &LmRequest,
        model: &Google,
        witness: CompletedTruncation,
    ) -> Result<LmResponse> {
        let binding = FormatRepair::prepare(request, witness)?;
        let bytes = serde_json::to_vec_pretty(&binding)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.repair_path(request)?)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        self.record(request,json!({"model":binding.witness.model,"api":"generateContent","outcome":"format_repair_prepared","original_logical_key":binding.witness.original_key,"original_raw_sha256":binding.witness.original_raw_sha256,"repaired_request_key":binding.repaired_google_key,"binding_sha256":google::digest(&bytes),"max_output_tokens":binding.repaired_request.config.max_tokens,"generation_submitted":false,"bound":"One distinct same-route format request; no semantic-answer repair or teacher substitution"}))?;
        match model
            .forward(&reqwest::Client::new(), &binding.repaired_request)
            .await
        {
            Ok(response) => {
                self.record(request,json!({"model":response.model,"api":"generateContent","outcome":"format_repair_completed","cache_hit":response.cache_hit,"original_logical_key":binding.witness.original_key,"original_raw_sha256":binding.witness.original_raw_sha256,"repaired_request_key":binding.repaired_google_key,"response_id":response.response_id,"response_observation_sha256":google::digest(serde_json::to_vec(&response)?)}))?;
                Ok(response)
            }
            Err(error) => {
                self.record(request,json!({"model":binding.witness.model,"api":"generateContent","outcome":"format_repair_failed","original_logical_key":binding.witness.original_key,"original_raw_sha256":binding.witness.original_raw_sha256,"repaired_request_key":binding.repaired_google_key,"disposition":google::disposition(&error).map(|f|f.disposition),"error":error.to_string(),"next_route_allowed":false,"repair_submissions_remaining":0}))?;
                Err(error)
            }
        }
    }
    fn record(&self, request: &LmRequest, event: Value) -> Result<()> {
        let key = google::digest(serde_json::to_vec(request)?);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join(format!("{key}.routes.jsonl")))?;
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
        file.sync_data()?;
        Ok(())
    }
    async fn request(&self, request: &LmRequest) -> Result<LmResponse> {
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "Teacher pool permits ordinary unconstrained text without tools"
        );
        let mut current = self.serial.lock().await;
        // Changing the active route must not repurchase an identical completed
        // answer from an earlier Gemini route or the saved-login fallback.
        for model in &self.models {
            if let Some(response) = model.cached_response(request)? {
                self.record(request,json!({"model":response.model,"api":"generateContent","outcome":"completed","cache_hit":true,"response_id":response.response_id,"reuse":"Exact settled response before active-route submission"}))?;
                return Ok(response);
            }
        }
        if let Some(fallback) = &self.fallback
            && let Some(response) = fallback.cached_response(request)?
        {
            self.record(request,json!({"model":response.model,"api":"tool-free saved-login CLI","outcome":"completed","cache_hit":true,"reuse":"Exact settled response before active-route submission"}))?;
            return Ok(response);
        }
        if let Some(response) = self.saved_repair(request).await? {
            return Ok(response);
        }
        // Imported completed truncations are inspected before touching the old
        // failed key or selecting a new route. They are not uncertain requests.
        let mut witnessed = None;
        for model in &self.models {
            if let Some(witness) = model.completed_truncation(request).await? {
                ensure!(
                    witnessed.is_none(),
                    "Multiple teacher truncation witnesses need explicit provenance review"
                );
                witnessed = Some((model, witness));
            }
        }
        if let Some((model, witness)) = witnessed {
            return self.repair(request, model, witness).await;
        }
        let mut last = None;
        while *current < self.models.len() {
            let model = &self.models[*current];
            match model.forward(&reqwest::Client::new(), request).await {
                Ok(response) => {
                    self.record(request,json!({"model":response.model,"api":"generateContent","outcome":"completed","cache_hit":response.cache_hit,"response_id":response.response_id}))?;
                    return Ok(response);
                }
                Err(error) => {
                    if google::disposition(&error).is_some_and(|f| {
                        f.disposition == Disposition::CompletedOutput
                            && f.http_status == Some(200)
                            && f.generation_submitted
                    }) && let Some(witness) = model.completed_truncation(request).await?
                    {
                        self.record(request,json!({"model":witness.model,"api":"generateContent","outcome":"complete_output_truncated","original_logical_key":witness.original_key,"original_raw_sha256":witness.original_raw_sha256,"next_route_allowed":false}))?;
                        return self.repair(request, model, witness).await;
                    }
                    let failure = google::disposition(&error);
                    let rotate = failure.is_some_and(|f| {
                        matches!(
                            f.disposition,
                            Disposition::QuotaOrUnavailable
                                | Disposition::QuotaDeferred
                                | Disposition::ContextLimit
                        )
                    });
                    self.record(request,json!({"model":failure.map(|f|f.model.as_str()),"api":"generateContent","outcome":"failed","disposition":failure.map(|f|f.disposition),"http_status":failure.and_then(|f|f.http_status),"generation_submitted":failure.map(|f|f.generation_submitted),"retry_after_ms":failure.and_then(|f|f.retry_after).map(|d|d.as_millis()),"next_route_allowed":rotate,"error":error.to_string()}))?;
                    if !rotate {
                        return Err(error);
                    }
                    *current += 1;
                    last = Some(error);
                }
            }
        }
        if let Some(fallback) = &self.fallback {
            self.record(request,json!({"model":"codex-saved-login-temporary-teacher","api":"tool-free saved-login CLI","outcome":"fallback_started","reason":"All actual listed Gemini text routes failed with known unavailability/quota/context dispositions."}))?;
            let response = fallback.forward(&reqwest::Client::new(), request).await?;
            self.record(request,json!({"model":response.model,"api":"tool-free saved-login CLI","outcome":"completed","cache_hit":response.cache_hit}))?;
            return Ok(response);
        }
        Err(last.unwrap_or_else(|| {
            google::ProviderFailure {
                disposition: Disposition::QuotaOrUnavailable,
                model: "exhausted-listed-Gemini-teacher-pool".into(),
                http_status: None,
                retry_after: None,
                generation_submitted: false,
                detail: "All actual listed text routes were exhausted in this bounded pool; no fresh calls or authorized fallback remain".into(),
            }.into()
        }))
    }
}
impl ChatModel for TeacherPool {
    async fn forward(&self, _http: &reqwest::Client, request: &LmRequest) -> Result<LmResponse> {
        self.request(request).await
    }
    async fn capabilities(&self, _http: &reqwest::Client) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::tests::{Mock, Reply};
    use dsrust::lm::api::{LmMessage, LmPart};
    fn info(id: &str) -> ModelInfo {
        ModelInfo {
            name: format!("models/{id}"),
            supported_generation_methods: vec!["generateContent".into()],
            input_token_limit: 100_000,
            output_token_limit: 8192,
        }
    }
    #[test]
    fn selects_only_listed_text_teachers_and_orders_requested_then_current() {
        let rows = [
            "gemini-2.5-flash",
            "gemini-3.10-flash",
            "gemini-3.8-flash",
            "gemini-3.6-flash",
            "gemini-3.1-pro-image-preview",
            "gemini-embedding-001",
            "gemini-2.5-flash-tts",
            "gemini-2.5-flash-native-audio-preview",
            "gemini-nano-banana-2.1",
            "gemini-3.5-transcribe",
            "gemini-2.5-computer-use-preview-10-2025",
            "gemma-4-26b-a4b-it",
        ]
        .map(info);
        let listing = json!({"models":rows,"listing_complete":true});
        let order = available(&listing)
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec![
                "models/gemini-3.8-flash",
                "models/gemini-3.10-flash",
                "models/gemini-3.6-flash",
                "models/gemini-2.5-flash"
            ]
        );
        assert!(available(&json!({"models":rows,"listing_complete":false})).is_err());
    }
    #[test]
    fn metadata_context_is_separate_from_rate_hint() {
        let listing = json!({"models":[info(google::STUDENT)],"listing_complete":true});
        assert_eq!(
            google::model_context_limit(&listing, google::STUDENT).unwrap(),
            100_000
        );
        assert!(google::model_context_limit(&listing, "absent").is_err());
    }
    fn pool(mock: &Mock, directory: &std::path::Path, budget: Arc<CallBudget>) -> TeacherPool {
        pool_with_output_limit(mock, directory, budget, 8192)
    }
    fn pool_with_output_limit(
        mock: &Mock,
        directory: &std::path::Path,
        budget: Arc<CallBudget>,
        limit: usize,
    ) -> TeacherPool {
        let mut first = info("gemini-3.8-flash");
        first.output_token_limit = limit;
        let listing = json!({"models":[first,info("gemini-3.6-flash")],"listing_complete":true});
        let http = reqwest::Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let mut pool =
            TeacherPool::new(http, &listing, directory.into(), budget, 120_000, None).unwrap();
        for model in &mut pool.models {
            model.test_api(&mock.api);
        }
        pool
    }
    fn request(text: &str) -> LmRequest {
        LmRequest::new(
            "gepa-reflection",
            vec![
                LmMessage::system(vec![LmPart::text(
                    "Salient synthetic book witness, preserved exactly.",
                )]),
                LmMessage::user(vec![LmPart::text(text)]),
            ],
        )
    }
    fn counts(directory: &std::path::Path, request: &LmRequest) {
        for id in ["gemini-3.8-flash", "gemini-3.6-flash"] {
            let mut routed = request.clone();
            routed.model = id.into();
            fs::write(
                directory
                    .join(id)
                    .join(format!("{}.tokens.json", routed.cache_key(id))),
                json!({"totalTokens":100}).to_string(),
            )
            .unwrap();
        }
    }
    const SUCCESS: &str = r#"{"candidates":[{"content":{"parts":[{"text":"A completed synthetic teacher answer"}]},"finishReason":"STOP"}],"responseId":"synthetic-response-id"}"#;
    const TRUNCATED: &str = r#"{"candidates":[{"content":{"parts":[{"text":"Synthetic input echo, cut off before outputs"}]},"finishReason":"MAX_TOKENS"}],"usageMetadata":{"candidatesTokenCount":4092}}"#;
    fn repair_input(request: &LmRequest, limit: usize) -> LmRequest {
        FormatRepair::prepare(
            request,
            CompletedTruncation {
                model: google::TEACHER.into(),
                original_key: request.cache_key(google::TEACHER),
                original_request_sha256: String::new(),
                original_raw_sha256: String::new(),
                parsed_raw_sha256: String::new(),
                original_max_output_tokens: 4096,
                model_output_token_limit: limit,
            },
        )
        .unwrap()
        .repaired_request
    }
    #[tokio::test]
    async fn complete_truncation_repairs_once_same_route_and_reuses_the_binding_before_active_state()
     {
        let mock = Mock::new(vec![
            Reply::Complete(200, TRUNCATED),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic complete teacher truncation");
        let repaired = repair_input(&input, 8192);
        counts(dir.path(), &input);
        counts(dir.path(), &repaired);
        let response = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert_eq!(response.model.as_deref(), Some(google::TEACHER));
        assert_eq!(budget.spent(), 2);
        let binding: FormatRepair =
            serde_json::from_slice(&fs::read(pool.repair_path(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(
            binding.witness.original_raw_sha256,
            google::digest(TRUNCATED)
        );
        assert_eq!(
            binding.repaired_google_key,
            repaired.cache_key(google::TEACHER)
        );
        let teacher_dir = dir.path().join(google::TEACHER);
        let original_request =
            teacher_dir.join(format!("{}.request.json", binding.witness.original_key));
        assert_eq!(
            google::digest(fs::read(&original_request).unwrap()),
            binding.witness.original_request_sha256
        );
        assert!(
            !teacher_dir
                .join(format!("{}.response.json", binding.witness.original_key))
                .exists()
        );
        let pool = pool.with_start_route(1).unwrap();
        let cached = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert!(cached.cache_hit);
        assert_eq!(cached.model, response.model);
        assert_eq!(budget.spent(), 2);
        assert_eq!(
            google::digest(fs::read(original_request).unwrap()),
            binding.witness.original_request_sha256
        );
        let actual = mock.requests.lock().unwrap();
        assert_eq!(actual.len(), 2);
        assert!(actual.iter().all(|r| {
            r["path"]
                .as_str()
                .unwrap()
                .contains("gemini-3.8-flash:generateContent")
        }));
        assert_eq!(
            actual[0]["body"]["generationConfig"]["maxOutputTokens"],
            4096
        );
        assert_eq!(
            actual[1]["body"]["generationConfig"]["maxOutputTokens"],
            8192
        );
        assert_eq!(actual[0]["body"]["contents"], actual[1]["body"]["contents"]);
        assert!(
            actual[1]["body"]["systemInstruction"]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with(input.system())
        );
        let journal = fs::read_to_string(dir.path().join(format!(
            "{}.routes.jsonl",
            google::digest(serde_json::to_vec(&input).unwrap())
        )))
        .unwrap();
        assert!(
            journal.contains("format_repair_completed") && journal.contains("format_repair_reused")
        );
        assert!(
            journal.contains(&binding.witness.original_raw_sha256)
                && journal.contains(&binding.repaired_google_key)
        );
    }
    #[tokio::test]
    async fn imported_completed_truncation_is_repaired_without_resending_the_old_key_or_new_active_route()
     {
        let mock = Mock::new(vec![
            Reply::Complete(200, TRUNCATED),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic imported closed truncation");
        counts(dir.path(), &input);
        counts(dir.path(), &repair_input(&input, 8192));
        let error = pool.models[0]
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        assert_eq!(
            google::disposition(&error).unwrap().disposition,
            Disposition::CompletedOutput
        );
        let pool = pool.with_start_route(1).unwrap();
        let response = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert_eq!(response.model.as_deref(), Some(google::TEACHER));
        assert_eq!(budget.spent(), 2);
        assert!(mock.requests.lock().unwrap().iter().all(|r| {
            r["path"]
                .as_str()
                .unwrap()
                .contains("gemini-3.8-flash:generateContent")
        }));
    }
    #[tokio::test]
    async fn truncated_repair_stops_bounded_and_never_becomes_another_teacher_or_zero_label() {
        let mock = Mock::new(vec![
            Reply::Complete(200, TRUNCATED),
            Reply::Complete(200, TRUNCATED),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(3);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic twice truncated teacher");
        counts(dir.path(), &input);
        counts(dir.path(), &repair_input(&input, 8192));
        let error = pool
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        assert_eq!(
            google::disposition(&error).unwrap().disposition,
            Disposition::CompletedOutput
        );
        let error = pool
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        assert_eq!(
            google::disposition(&error).unwrap().disposition,
            Disposition::PriorUnresolved
        );
        assert_eq!(budget.spent(), 2);
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
        let binding: FormatRepair =
            serde_json::from_slice(&fs::read(pool.repair_path(&input).unwrap()).unwrap()).unwrap();
        for key in [binding.witness.original_key, binding.repaired_google_key] {
            assert!(
                dir.path()
                    .join(google::TEACHER)
                    .join(format!("{key}.raw.json"))
                    .is_file()
            );
            assert!(
                !dir.path()
                    .join(google::TEACHER)
                    .join(format!("{key}.response.json"))
                    .exists()
            );
        }
    }
    #[tokio::test]
    async fn invalid_truncation_witness_stops_before_any_repair_submission() {
        for corruption in ["partial", "body-hash", "request-identity"] {
            let mock = Mock::new(vec![Reply::Complete(200, TRUNCATED)]);
            let dir = tempfile::tempdir().unwrap();
            let budget = CallBudget::new(2);
            let pool = pool(&mock, dir.path(), budget.clone());
            let input = request("Synthetic invalid truncation witness");
            counts(dir.path(), &input);
            pool.models[0]
                .forward(&reqwest::Client::new(), &input)
                .await
                .unwrap_err();
            let key = input.cache_key(google::TEACHER);
            let teacher_dir = dir.path().join(google::TEACHER);
            match corruption {
                "partial" => {
                    let path = teacher_dir.join(format!("{key}.attempt-1.result.json"));
                    let mut result: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    result["response_complete"] = json!(false);
                    fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
                }
                "body-hash" => fs::write(
                    teacher_dir.join(format!("{key}.attempt-1.body")),
                    format!("{TRUNCATED} "),
                )
                .unwrap(),
                _ => {
                    let path = teacher_dir.join(format!("{key}.attempt-1.request.json"));
                    let mut saved: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    saved["model"] = json!("gemini-3.6-flash");
                    fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
                }
            }
            assert!(
                pool.forward(&reqwest::Client::new(), &input).await.is_err(),
                "{corruption}"
            );
            assert!(!pool.repair_path(&input).unwrap().exists());
            assert_eq!(budget.spent(), 1);
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn semantic_bad_complete_answer_is_not_repaired_and_original_cache_precedes_any_binding()
    {
        const BAD: &str = r#"{"candidates":[{"content":{"parts":[{"text":"Wrong output field; semantic/schema rejection remains for the caller"}]},"finishReason":"STOP"}]}"#;
        let mock = Mock::new(vec![Reply::Complete(200, BAD)]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic completed bad format without truncation");
        counts(dir.path(), &input);
        let response = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert!(response.first_text().starts_with("Wrong output field"));
        assert!(!pool.repair_path(&input).unwrap().exists());
        fs::write(
            pool.repair_path(&input).unwrap(),
            b"Invalid binding must not displace an original settled success",
        )
        .unwrap();
        let reused = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert!(reused.cache_hit);
        assert_eq!(reused.outputs, response.outputs);
        assert_eq!(budget.spent(), 1);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn format_repair_budget_uses_actual_model_limit_and_restored_route_is_range_checked() {
        let mock = Mock::new(vec![
            Reply::Complete(200, TRUNCATED),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool_with_output_limit(&mock, dir.path(), budget.clone(), 6144);
        let input = request("Synthetic limited teacher output budget");
        counts(dir.path(), &input);
        counts(dir.path(), &repair_input(&input, 6144));
        pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert_eq!(
            mock.requests.lock().unwrap()[1]["body"]["generationConfig"]["maxOutputTokens"],
            6144
        );
        assert!(pool.with_start_route(3).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(
            pool_with_output_limit(&Mock::new(vec![]), empty.path(), CallBudget::new(0), 8192)
                .with_start_route(2)
                .is_ok()
        );
    }
    #[tokio::test]
    async fn uncertain_or_authentication_failure_during_repair_is_never_retried_or_rerouted() {
        for (reply, kind) in [
            (
                Reply::Partial(200, "Synthetic partial repair output"),
                Disposition::Uncertain,
            ),
            (
                Reply::Complete(403, r#"{"error":{"code":403}}"#),
                Disposition::Authentication,
            ),
        ] {
            let mock = Mock::new(vec![Reply::Complete(200, TRUNCATED), reply]);
            let dir = tempfile::tempdir().unwrap();
            let budget = CallBudget::new(4);
            let pool = pool(&mock, dir.path(), budget.clone());
            let input = request("Synthetic failed repair transport");
            counts(dir.path(), &input);
            counts(dir.path(), &repair_input(&input, 8192));
            let error = pool
                .forward(&reqwest::Client::new(), &input)
                .await
                .unwrap_err();
            assert_eq!(google::disposition(&error).unwrap().disposition, kind);
            let error = pool
                .forward(&reqwest::Client::new(), &input)
                .await
                .unwrap_err();
            assert_eq!(
                google::disposition(&error).unwrap().disposition,
                Disposition::PriorUnresolved
            );
            assert_eq!(budget.spent(), 2);
            assert_eq!(mock.requests.lock().unwrap().len(), 2);
        }
    }
    #[tokio::test]
    async fn prepared_repair_without_a_settled_response_blocks_any_repeat_submission() {
        let mock = Mock::new(vec![Reply::Complete(200, TRUNCATED)]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic crash after durable repair preparation");
        counts(dir.path(), &input);
        pool.models[0]
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        let witness = pool.models[0]
            .completed_truncation(&input)
            .await
            .unwrap()
            .unwrap();
        let binding = FormatRepair::prepare(&input, witness).unwrap();
        fs::write(
            pool.repair_path(&input).unwrap(),
            serde_json::to_vec(&binding).unwrap(),
        )
        .unwrap();
        let error = pool
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        assert_eq!(
            google::disposition(&error).unwrap().disposition,
            Disposition::PriorUnresolved
        );
        assert_eq!(budget.spent(), 1);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn known_unavailability_rotates_models_with_one_combined_cap_and_exact_source() {
        let mock = Mock::new(vec![
            Reply::Complete(503, r#"{"error":{"code":503}}"#),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic teacher function");
        counts(dir.path(), &input);
        let response = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert_eq!(response.model.as_deref(), Some("gemini-3.6-flash"));
        assert_eq!(budget.spent(), 2);
        let cached = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert!(cached.cache_hit);
        assert_eq!(budget.spent(), 2);
        let actual = mock.requests.lock().unwrap();
        assert_eq!(actual.len(), 2);
        assert!(
            actual[0]["path"]
                .as_str()
                .unwrap()
                .contains("gemini-3.8-flash:generateContent")
        );
        assert!(
            actual[1]["path"]
                .as_str()
                .unwrap()
                .contains("gemini-3.6-flash:generateContent")
        );
        assert_eq!(actual[0]["body"], actual[1]["body"]);
        assert_eq!(
            actual[1]["body"]["systemInstruction"]["parts"][0]["text"],
            "Salient synthetic book witness, preserved exactly."
        );
        assert!(
            actual[1]["body"]["generationConfig"]
                .get("responseSchema")
                .is_none()
        );
    }
    #[tokio::test]
    async fn shared_cap_cannot_be_reset_by_switching_teacher_models() {
        let mock = Mock::new(vec![Reply::Complete(503, r#"{"error":{"code":503}}"#)]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(1);
        let pool = pool(&mock, dir.path(), budget.clone());
        let input = request("Synthetic cap exhaustion");
        counts(dir.path(), &input);
        let error = pool
            .forward(&reqwest::Client::new(), &input)
            .await
            .unwrap_err();
        assert_eq!(
            google::disposition(&error).unwrap().disposition,
            Disposition::BudgetExhausted
        );
        assert_eq!(budget.spent(), 1);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn uncertain_authentication_or_completed_bad_answer_never_tries_another_teacher() {
        for (reply, expected) in [
            (
                Reply::Partial(500, "Partial response"),
                Disposition::Uncertain,
            ),
            (
                Reply::Complete(403, r#"{"error":{"code":403}}"#),
                Disposition::Authentication,
            ),
            (
                Reply::Complete(200, "Invalid complete JSON"),
                Disposition::CompletedOutput,
            ),
            (
                Reply::Complete(200, r#"{"candidates":[]}"#),
                Disposition::CompletedOutput,
            ),
        ] {
            let mock = Mock::new(vec![reply]);
            let dir = tempfile::tempdir().unwrap();
            let budget = CallBudget::new(2);
            let pool = pool(&mock, dir.path(), budget.clone());
            let input = request("Synthetic halt condition");
            counts(dir.path(), &input);
            let error = pool
                .forward(&reqwest::Client::new(), &input)
                .await
                .unwrap_err();
            assert_eq!(google::disposition(&error).unwrap().disposition, expected);
            assert_eq!(budget.spent(), 1);
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn teacher_cooldown_skips_that_route_instead_of_blocking_the_pool() {
        const LIMITED: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"120s"}]}}"#;
        let mock = Mock::new(vec![
            Reply::Complete(429, LIMITED),
            Reply::Complete(200, SUCCESS),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(3);
        let pool = pool(&mock, dir.path(), budget.clone());
        let first = request("Synthetic first function");
        let second = request("Synthetic second function");
        counts(dir.path(), &first);
        counts(dir.path(), &second);
        pool.forward(&reqwest::Client::new(), &first).await.unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            pool.forward(&reqwest::Client::new(), &second),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.model.as_deref(), Some("gemini-3.6-flash"));
        assert_eq!(budget.spent(), 3);
        assert_eq!(mock.requests.lock().unwrap().len(), 3);
    }
    #[tokio::test]
    async fn exhausted_route_is_not_rebought_and_its_earlier_success_is_still_reused() {
        let mock = Mock::new(vec![
            Reply::Complete(200, SUCCESS),
            Reply::Complete(503, r#"{"error":{"code":503}}"#),
            Reply::Complete(200, SUCCESS),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(4);
        let pool = pool(&mock, dir.path(), budget.clone());
        let first = request("Synthetic first successful function");
        let second = request("Synthetic function that advances the route");
        let third = request("Synthetic new function after advancement");
        for input in [&first, &second, &third] {
            counts(dir.path(), input);
        }
        let old = pool.forward(&reqwest::Client::new(), &first).await.unwrap();
        assert_eq!(old.model.as_deref(), Some("gemini-3.8-flash"));
        let switched = pool
            .forward(&reqwest::Client::new(), &second)
            .await
            .unwrap();
        assert_eq!(switched.model.as_deref(), Some("gemini-3.6-flash"));
        let reused = pool.forward(&reqwest::Client::new(), &first).await.unwrap();
        assert!(reused.cache_hit);
        assert_eq!(reused.model, old.model);
        assert_eq!(budget.spent(), 3);
        let fresh = pool.forward(&reqwest::Client::new(), &third).await.unwrap();
        assert_eq!(fresh.model.as_deref(), Some("gemini-3.6-flash"));
        assert_eq!(budget.spent(), 4);
        let actual = mock.requests.lock().unwrap();
        assert_eq!(actual.len(), 4);
        for (index, id) in [
            "gemini-3.8-flash",
            "gemini-3.8-flash",
            "gemini-3.6-flash",
            "gemini-3.6-flash",
        ]
        .into_iter()
        .enumerate()
        {
            assert!(actual[index]["path"].as_str().unwrap().contains(id));
        }
    }
    #[tokio::test]
    async fn old_codex_success_is_reused_before_a_fresh_gemini_route_and_charges_nothing() {
        let mock = Mock::new(vec![]);
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("synthetic-not-executable");
        fs::write(&executable, b"Synthetic executable identity; never run").unwrap();
        let fallback_dir = dir.path().join("fallback");
        let input = request("Synthetic previously settled fallback teacher function");
        let executable_sha = google::digest(fs::read(&executable).unwrap());
        let key = google::digest(format!(
            "codex-login-text-v1\0{executable_sha}\0{}",
            input.cache_key("codex-saved-login")
        ));
        let saved_dir = fallback_dir.join(key);
        fs::create_dir_all(&saved_dir).unwrap();
        let saved = LmResponse::text("A previously completed synthetic Codex answer")
            .with_model("codex-saved-login-temporary-teacher");
        fs::write(
            saved_dir.join("response.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let fallback = Codex::new(executable, fallback_dir, 0).unwrap();
        let budget = CallBudget::new(0);
        let listing = json!({"models":[info("gemini-3.8-flash")],"listing_complete":true});
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut pool = TeacherPool::new(
            http,
            &listing,
            dir.path().join("pool"),
            budget.clone(),
            120_000,
            Some(fallback),
        )
        .unwrap();
        pool.models[0].test_api(&mock.api);
        let response = pool.forward(&reqwest::Client::new(), &input).await.unwrap();
        assert!(response.cache_hit);
        assert_eq!(response.model, saved.model);
        assert_eq!(response.outputs, saved.outputs);
        assert_eq!(budget.spent(), 0);
        assert!(mock.requests.lock().unwrap().is_empty());
    }
}
