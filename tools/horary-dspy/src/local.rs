//! Local selection and receipts; the pinned port owns the provider wire and parser.
use crate::google;
use anyhow::{Context, Result, ensure};
use dsrust::lm::{
    Capabilities, ChatModel, LM, TokenLimitRule,
    api::{LmRequest, LmResponse},
};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const PORT_REVISION: &str = "09d17e07e18e9db2d12e33e2a4a29ca639da65de";
pub const MODEL: &str = "gemma-4-12b-it-qat-mtp";

pub fn validate_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            && url.port().is_some()
            && url.path().trim_end_matches('/') == "/v1"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Local student must name a loopback HTTP /v1 endpoint with an explicit port"
    );
    Ok(url)
}

pub struct LocalStudent {
    model: LM,
    http: reqwest::Client,
    directory: PathBuf,
    calls: AtomicUsize,
    max_calls: usize,
    failed: AtomicBool,
}
impl LocalStudent {
    pub fn new(url: &str, model: &str, directory: &Path) -> Result<Self> {
        validate_url(url)?;
        ensure!(
            !model.is_empty() && !model.chars().any(char::is_whitespace),
            "Local server model identity is empty or malformed"
        );
        fs::create_dir_all(directory)?;
        let mut model = LM::new(&format!("openai/{model}"))?
            .with_openai_base_url(url)
            .with_openai_key("local")
            .with_openai_token_limit_rule(TokenLimitRule::AlwaysMaxTokens)
            .with_capabilities(Capabilities::default())
            .without_cache();
        model.openai.request_timeout = std::time::Duration::from_secs(300);
        Ok(Self {
            model,
            // The teacher's Google client carries a private default header;
            // it must never be forwarded to a local server or its logs.
            http: reqwest::Client::builder()
                .no_proxy()
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            directory: directory.into(),
            calls: AtomicUsize::new(0),
            max_calls: usize::MAX,
            failed: AtomicBool::new(false),
        })
    }
    /// Admission cap shared by every predictor using this resident student.
    /// A blocked request consumes no slot and creates no provider submission.
    pub fn with_max_calls(mut self, max_calls: usize) -> Self {
        self.max_calls = max_calls;
        self
    }
    fn save(&self, ordinal: usize, suffix: &str, value: &serde_json::Value) -> Result<()> {
        let path = self
            .directory
            .join(format!("call-{ordinal:06}.{suffix}.json"));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        Ok(())
    }
}
impl ChatModel for LocalStudent {
    async fn forward(&self, _http: &reqwest::Client, request: &LmRequest) -> Result<LmResponse> {
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "Local run permits ordinary unconstrained text only"
        );
        ensure!(
            !self.failed.load(Ordering::SeqCst),
            "Prior local provider failure is retained; no automatic replay"
        );
        let ordinal = self
            .calls
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |calls| {
                (calls < self.max_calls).then_some(calls.saturating_add(1))
            })
            .map_err(|_| {
                anyhow::anyhow!("Local physical-call cap reached; no request submitted")
            })?
            + 1;
        self.save(ordinal, "request", &json!({"ordinal":ordinal,"provider_model":self.model.model.id,"base_url":self.model.openai.base_url,"request":request,"request_sha256":google::digest(serde_json::to_vec(request)?),"at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"provider_schema":false,"port_timeout_patch_revision":PORT_REVISION,"port_base_revision":"f24adde08c1d8850e4d7079d019643bb40f905cb","port_timeout_seconds":self.model.openai.request_timeout.as_secs(),"hosted_student":false,"transport":"Pinned port OpenAiCompatible with configurable timeout; no custom parser, retry or hidden cache"}))?;
        let begun = Instant::now();
        let result = self.model.forward(&self.http, request).await;
        match &result {
            Ok(response) => self.save(ordinal, "response", &json!({"ordinal":ordinal,"elapsed_ms":begun.elapsed().as_millis(),"response":response,"response_sha256":google::digest(serde_json::to_vec(response)?)}))?,
            Err(error) => {
                self.failed.store(true, Ordering::SeqCst);
                self.save(ordinal, "failure", &json!({"ordinal":ordinal,"elapsed_ms":begun.elapsed().as_millis(),"error":error.to_string(),"response_observed":false,"automatic_retry":false}))?;
            }
        }
        result.context("Local port request failed; its receipt remains available")
    }
    async fn capabilities(&self, _http: &reqwest::Client) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::tests::{Mock, Reply};
    use dsrust::lm::api::{LmConfig, LmMessage, LmPart};

    #[test]
    fn only_explicit_loopback_v1_urls_can_select_the_local_student() {
        for url in ["http://127.0.0.1:63924/v1", "http://[::1]:63924/v1/"] {
            validate_url(url).unwrap();
        }
        for url in [
            "https://127.0.0.1:63924/v1",
            "http://localhost:63924/v1",
            "http://192.168.1.2:63924/v1",
            "http://127.0.0.1/v1",
            "http://user@127.0.0.1:63924/v1",
            "http://127.0.0.1:63924/v1?key=value",
            "http://127.0.0.1:63924/v1#value",
            "http://127.0.0.1:63924/completion",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn port_local_wire_is_plain_text_and_receipts_preserve_both_halves() {
        let mock = Mock::new(vec![Reply::Complete(
            200,
            r#"{"id":"synthetic-local","model":"gemma-4-12b-it-qat-mtp","choices":[{"message":{"role":"assistant","content":"Observed local response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}"#,
        )]);
        let dir = tempfile::tempdir().unwrap();
        let model = LocalStudent::new(&format!("{}/v1", mock.api), MODEL, dir.path()).unwrap();
        let request = LmRequest::new(
            "Original request identity",
            vec![LmMessage::user(vec![LmPart::text(
                "Synthetic local question",
            )])],
        )
        .configured(LmConfig {
            max_tokens: Some(128),
            temperature: Some(0.),
            ..Default::default()
        });
        let reply = model
            .forward(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        assert_eq!(reply.first_text(), "Observed local response");
        assert!(!reply.cache_hit);
        let requests = mock.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["path"], "/v1/chat/completions");
        assert_eq!(requests[0]["body"]["model"], MODEL);
        assert_eq!(requests[0]["body"]["max_tokens"], 128);
        assert!(requests[0]["body"].get("response_format").is_none());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("call-000001.request.json")).unwrap())
                .unwrap();
        assert_eq!(saved["request"], serde_json::to_value(request).unwrap());
        let saved: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.path().join("call-000001.response.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["response"], serde_json::to_value(reply).unwrap());
    }

    #[tokio::test]
    async fn shared_local_cap_stops_before_another_wire_request() {
        let mock = Mock::new(vec![Reply::Complete(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"Synthetic observed answer"},"finish_reason":"stop"}]}"#,
        )]);
        let dir = tempfile::tempdir().unwrap();
        let model = LocalStudent::new(&format!("{}/v1", mock.api), MODEL, dir.path())
            .unwrap()
            .with_max_calls(1);
        let request = LmRequest::new(
            "Synthetic capped request",
            vec![LmMessage::user(vec![LmPart::text("Question")])],
        );
        model
            .forward(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        let error = model
            .forward(&reqwest::Client::new(), &request)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("physical-call cap"));
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
        assert!(dir.path().join("call-000001.response.json").exists());
        assert!(!dir.path().join("call-000002.request.json").exists());
    }

    #[tokio::test]
    async fn local_provider_failure_is_retained_without_repeated_submission() {
        let mock = Mock::new(vec![Reply::Disconnect]);
        let dir = tempfile::tempdir().unwrap();
        let model = LocalStudent::new(&format!("{}/v1", mock.api), MODEL, dir.path()).unwrap();
        let request = LmRequest::new(
            "Synthetic local request",
            vec![LmMessage::user(vec![LmPart::text("Question")])],
        );
        assert!(
            model
                .forward(&reqwest::Client::new(), &request)
                .await
                .is_err()
        );
        assert!(
            model
                .forward(&reqwest::Client::new(), &request)
                .await
                .is_err()
        );
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        assert!(dir.path().join("call-000001.failure.json").exists());
        assert!(!dir.path().join("call-000002.request.json").exists());
    }
}
