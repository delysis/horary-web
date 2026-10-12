//! Teacher routing only: actual listed text models, then the authorized Codex
//! fallback. A completed bad answer is never relabeled as a quota failure.
use crate::{
    codex::Codex,
    google::{self, CallBudget, Disposition, Google, ModelInfo, RateLimiter},
};
use anyhow::{Context, Result, ensure};
use dsrust::lm::{
    Capabilities, ChatModel,
    api::{LmRequest, LmResponse},
};
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
        let mut last = None;
        while *current < self.models.len() {
            let model = &self.models[*current];
            match model.forward(&reqwest::Client::new(), request).await {
                Ok(response) => {
                    self.record(request,json!({"model":response.model,"api":"generateContent","outcome":"completed","cache_hit":response.cache_hit,"response_id":response.response_id}))?;
                    return Ok(response);
                }
                Err(error) => {
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
        let listing = json!({"models":[info("gemini-3.8-flash"),info("gemini-3.6-flash")],"listing_complete":true});
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
