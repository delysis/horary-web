//! Provider transport only. DsRust owns prompt rendering and optimizer execution.
use anyhow::{Context, Result, bail, ensure};
use dsrust::lm::{
    Capabilities, ChatModel,
    api::{LmOutput, LmRequest, LmResponse},
};
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub const TEACHER: &str = "gemini-3.8-flash";
pub const STUDENT: &str = "gemma-4-26b-a4b-it";
pub const CREDENTIAL_FILE: &str = "/Users/george/.codex/credentials/horary-google-api.key";
const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const MAX_SUBMISSIONS: usize = 3;
const WINDOW: Duration = Duration::from_secs(61);
const MAX_AUTOMATIC_DELAY: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    QuotaOrUnavailable,
    QuotaDeferred,
    ContextLimit,
    BudgetExhausted,
    Uncertain,
    Authentication,
    CompletedOutput,
    InvalidRequest,
    PriorUnresolved,
}

#[derive(Debug)]
pub struct ProviderFailure {
    pub disposition: Disposition,
    pub model: String,
    pub http_status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub generation_submitted: bool,
    pub detail: String,
}
impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Google {} {:?}: {}",
            self.model, self.disposition, self.detail
        )
    }
}
impl std::error::Error for ProviderFailure {}
pub fn disposition(error: &anyhow::Error) -> Option<&ProviderFailure> {
    error.downcast_ref::<ProviderFailure>()
}
/// A read-only witness for one fully received teacher output truncation. This
/// does not authorize resending the original key or repairing semantic errors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedTruncation {
    pub model: String,
    pub original_key: String,
    pub original_request_sha256: String,
    pub original_raw_sha256: String,
    pub parsed_raw_sha256: String,
    pub original_max_output_tokens: usize,
    pub model_output_token_limit: usize,
}
fn provider_error(
    model: &str,
    disposition: Disposition,
    http_status: Option<u16>,
    retry_after: Option<Duration>,
    generation_submitted: bool,
    detail: impl Into<String>,
) -> anyhow::Error {
    ProviderFailure {
        disposition,
        model: model.into(),
        http_status,
        retry_after,
        generation_submitted,
        detail: detail.into(),
    }
    .into()
}
pub fn model_context_limit(listing: &Value, model: &str) -> Result<usize> {
    let row = listing["models"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["name"] == format!("models/{model}"))
        .or_else(|| {
            ["student", "teacher"]
                .iter()
                .map(|key| &listing[key])
                .find(|row| row["name"] == format!("models/{model}"))
        })
        .context("Requested model is absent from actual listing")?;
    let info: ModelInfo = serde_json::from_value(row.clone())?;
    info.id()?;
    Ok(info.input_token_limit)
}
pub fn quota_hints(provider: &Value, retry_after: Option<&str>, model: &str) -> Value {
    serde_json::to_value(quota_advice(
        &serde_json::to_vec(provider).expect("JSON serializable"),
        retry_after,
        model,
    ))
    .expect("Quota observation serializable")
}

/// One physical-generation bound shared by all routes in a teacher pool.
pub struct CallBudget {
    spent: AtomicUsize,
    maximum: usize,
}
impl CallBudget {
    pub fn new(maximum: usize) -> Arc<Self> {
        Arc::new(Self {
            spent: AtomicUsize::new(0),
            maximum,
        })
    }
    pub fn seeded(maximum: usize, inherited_spent: usize) -> Result<Arc<Self>> {
        ensure!(
            inherited_spent <= maximum,
            "Inherited physical work exceeds original call cap"
        );
        Ok(Arc::new(Self {
            spent: AtomicUsize::new(inherited_spent),
            maximum,
        }))
    }
    pub fn spent(&self) -> usize {
        self.spent.load(Ordering::Relaxed)
    }
    pub fn remaining(&self) -> usize {
        self.maximum.saturating_sub(self.spent())
    }
    pub(crate) fn reserve(&self) -> Option<usize> {
        self.spent
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.maximum).then(|| n + 1)
            })
            .ok()
            .map(|n| n + 1)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,
    pub supported_generation_methods: Vec<String>,
    pub input_token_limit: usize,
    pub output_token_limit: usize,
}
impl ModelInfo {
    pub fn id(&self) -> Result<&str> {
        let id = self
            .name
            .strip_prefix("models/")
            .context("Model listing lacks resource prefix")?;
        ensure!(
            !id.is_empty()
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_')),
            "Invalid listed model identity"
        );
        ensure!(
            self.input_token_limit > 0 && self.output_token_limit > 0,
            "Listed model has no positive context/output limit"
        );
        Ok(id)
    }
}

#[derive(Default)]
struct PaceState {
    window: VecDeque<(Instant, usize)>,
    requests: VecDeque<Instant>,
    rate_debt: Option<Instant>,
    provider_retry: Option<Instant>,
    probed_tpm: usize,
    reported_tpm: Option<usize>,
    reported_rpm: Option<usize>,
}
/// Share this object across student jobs. A rate hint is not a context limit.
pub struct RateLimiter {
    hint: usize,
    probe_ceiling: usize,
    state: Mutex<PaceState>,
}
impl RateLimiter {
    pub fn new(tokens_per_minute_hint: usize) -> Result<Arc<Self>> {
        ensure!(tokens_per_minute_hint > 0, "Rate hint must be positive");
        Self::with_probe_ceiling(
            tokens_per_minute_hint,
            tokens_per_minute_hint.saturating_mul(16),
        )
    }
    pub fn with_probe_ceiling(hint: usize, probe_ceiling: usize) -> Result<Arc<Self>> {
        ensure!(
            hint > 0 && probe_ceiling >= hint,
            "Positive rate hint and a ceiling at least that hint are required"
        );
        Ok(Arc::new(Self {
            hint,
            probe_ceiling,
            state: Mutex::new(PaceState {
                probed_tpm: hint,
                ..Default::default()
            }),
        }))
    }
    async fn pace(&self, tokens: usize) -> Duration {
        let begun = Instant::now();
        loop {
            let mut state = self.state.lock().await;
            while state
                .window
                .front()
                .is_some_and(|(at, _)| at.elapsed() >= WINDOW)
            {
                state.window.pop_front();
            }
            while state
                .requests
                .front()
                .is_some_and(|at| at.elapsed() >= WINDOW)
            {
                state.requests.pop_front();
            }
            let now = Instant::now();
            let rate = state.reported_tpm.unwrap_or(state.probed_tpm).max(1);
            let used = state.window.iter().map(|(_, n)| *n).sum::<usize>();
            let debt = state
                .rate_debt
                .into_iter()
                .chain(state.provider_retry)
                .map(|until| until.saturating_duration_since(now))
                .max()
                .unwrap_or_default();
            let token_ready = state.window.is_empty() || used.saturating_add(tokens) <= rate;
            let requests_ready = state
                .reported_rpm
                .is_none_or(|rpm| state.requests.len() < rpm);
            if debt.is_zero() && token_ready && requests_ready {
                state.window.push_back((now, tokens));
                state.requests.push_back(now);
                if tokens > rate {
                    // Admit one oversized exchange, then pay its rate debt before
                    // another. The provider's context limit is checked separately.
                    let periods = tokens.div_ceil(rate).min(u32::MAX as usize) as u32;
                    state.rate_debt = Some(now + WINDOW.saturating_mul(periods));
                }
                return begun.elapsed();
            }
            let delay = if !debt.is_zero() {
                debt
            } else if !token_ready {
                WINDOW.saturating_sub(
                    state
                        .window
                        .front()
                        .expect("Nonempty token window")
                        .0
                        .elapsed(),
                )
            } else {
                WINDOW.saturating_sub(
                    state
                        .requests
                        .front()
                        .expect("Nonempty request window")
                        .elapsed(),
                )
            };
            drop(state);
            tokio::time::sleep(delay).await;
        }
    }
    async fn observe(&self, advice: &QuotaAdvice) {
        let mut state = self.state.lock().await;
        if let Some(rate) = advice.tokens_per_minute.filter(|n| *n > 0) {
            state.reported_tpm = Some(rate);
            Self::recompute_debt(&mut state, rate);
        } else if advice.requests_per_minute.is_none() && !advice.daily_quota && !advice.zero_quota
        {
            // The error supplied no usable minute dimension. Back off the
            // probe rather than pretending the initial hint was a known limit.
            state.probed_tpm = (state.probed_tpm / 2).max(1);
            let rate = state.reported_tpm.unwrap_or(state.probed_tpm);
            Self::recompute_debt(&mut state, rate);
        }
        if let Some(rate) = advice.requests_per_minute.filter(|n| *n > 0) {
            state.reported_rpm = Some(rate);
        }
        if let Some(delay) = advice.retry_after {
            let until = Instant::now() + delay;
            state.provider_retry = Some(state.provider_retry.map_or(until, |old| old.max(until)));
        }
    }
    fn recompute_debt(state: &mut PaceState, rate: usize) {
        let used = state.window.iter().map(|(_, n)| *n).sum::<usize>();
        state.rate_debt = if used > rate {
            state.window.front().map(|(at, _)| {
                *at + WINDOW.saturating_mul(used.div_ceil(rate).min(u32::MAX as usize) as u32)
            })
        } else {
            None
        };
    }
    async fn success(&self, tokens: usize) {
        let mut state = self.state.lock().await;
        if state.reported_tpm.is_none() {
            // A bounded probe, explicitly not a provider-published quota. Every
            // successful exchange raises the hint by at most 25% (or its own
            // observed size); a 429 immediately replaces it with actual limits.
            state.probed_tpm = state
                .probed_tpm
                .saturating_add(state.probed_tpm.div_ceil(4))
                .max(tokens)
                .min(self.probe_ceiling);
            let rate = state.probed_tpm;
            Self::recompute_debt(&mut state, rate);
        }
    }
    pub async fn snapshot(&self) -> Value {
        let mut state = self.state.lock().await;
        while state
            .window
            .front()
            .is_some_and(|(at, _)| at.elapsed() >= WINDOW)
        {
            state.window.pop_front();
        }
        while state
            .requests
            .front()
            .is_some_and(|at| at.elapsed() >= WINDOW)
        {
            state.requests.pop_front();
        }
        json!({"configured_hint_tpm":self.hint,"probe_ceiling_tpm":self.probe_ceiling,"probed_hint_tpm":state.probed_tpm,"probe_policy":"success +25%, capped; published 429 quota overrides probing","provider_reported_tpm":state.reported_tpm,"provider_reported_rpm":state.reported_rpm,
            "charged_input_tokens":state.window.iter().map(|(_,n)|*n).sum::<usize>(),"charged_requests":state.requests.len(),
            "next_window_release_ms":state.window.front().map(|(at,_)|WINDOW.saturating_sub(at.elapsed()).as_millis()),
            "cooldown_remaining_ms":state.rate_debt.into_iter().chain(state.provider_retry).map(|at|at.saturating_duration_since(Instant::now()).as_millis()).max()})
    }
}

#[derive(Default, Debug, Serialize)]
struct QuotaAdvice {
    #[serde(skip)]
    retry_after: Option<Duration>,
    retry_after_ms: Option<u128>,
    retry_after_header: Option<String>,
    tokens_per_minute: Option<usize>,
    requests_per_minute: Option<usize>,
    zero_quota: bool,
    daily_quota: bool,
    violations: Vec<Value>,
}
struct BlockedModel {
    until: Option<Instant>,
    disposition: Disposition,
}
fn duration_seconds(text: &str) -> Option<Duration> {
    let value = text.trim().trim_end_matches('s').parse::<f64>().ok()?;
    (value.is_finite() && value >= 0.).then(|| Duration::from_secs_f64(value.min(7. * 24. * 3600.)))
}
fn quota_model_matches(model: &str, dimension: &str) -> bool {
    let dimension = dimension.strip_prefix("models/").unwrap_or(dimension);
    dimension == model
        // The fixed hosted student's settled 429 identifies this exact quota
        // group. This is not a general Gemma-prefix or model-family match.
        || (model == STUDENT && dimension == "gemma-4-26b")
}
fn quota_advice(bytes: &[u8], header: Option<&str>, model: &str) -> QuotaAdvice {
    let mut result = QuotaAdvice {
        retry_after_header: header.map(str::to_owned),
        ..Default::default()
    };
    if let Some(header) = header {
        result.retry_after = duration_seconds(header).or_else(|| {
            let at = chrono::DateTime::parse_from_rfc2822(header)
                .ok()?
                .timestamp();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
            Some(Duration::from_secs(at.saturating_sub(now).max(0) as u64))
        });
    }
    if let Ok(body) = serde_json::from_slice::<Value>(bytes) {
        for detail in body["error"]["details"].as_array().into_iter().flatten() {
            if detail["@type"]
                .as_str()
                .is_some_and(|t| t.ends_with("RetryInfo"))
                && let Some(delay) = detail["retryDelay"].as_str().and_then(duration_seconds)
            {
                result.retry_after = Some(result.retry_after.map_or(delay, |old| old.max(delay)));
            }
            for violation in detail["violations"].as_array().into_iter().flatten() {
                result.violations.push(violation.clone());
                let relevant = violation["quotaDimensions"]["model"]
                    .as_str()
                    .is_none_or(|id| quota_model_matches(model, id));
                if !relevant {
                    continue;
                }
                let number = violation["quotaValue"]
                    .as_u64()
                    .map(|n| n as usize)
                    .or_else(|| violation["quotaValue"].as_str()?.parse().ok());
                let metric = format!(
                    "{} {}",
                    violation["quotaMetric"].as_str().unwrap_or_default(),
                    violation["quotaId"].as_str().unwrap_or_default()
                )
                .to_ascii_lowercase();
                let minute = metric.contains("perminute") || metric.contains("per_minute");
                result.daily_quota |= metric.contains("perday") || metric.contains("per_day");
                if let Some(number) = number {
                    result.zero_quota |= number == 0;
                    if minute && (metric.contains("input_token") || metric.contains("inputtoken")) {
                        result.tokens_per_minute = Some(
                            result
                                .tokens_per_minute
                                .map_or(number, |old| old.min(number)),
                        );
                    }
                    if minute && metric.contains("request") {
                        result.requests_per_minute = Some(
                            result
                                .requests_per_minute
                                .map_or(number, |old| old.min(number)),
                        );
                    }
                }
            }
        }
    }
    result.retry_after_ms = result.retry_after.map(|d| d.as_millis());
    result
}

fn retry_delay(status: u16, complete: bool, attempt: usize, remaining: usize) -> Option<Duration> {
    (complete
        && matches!(status, 429 | 500 | 502 | 503 | 504)
        && (1..MAX_SUBMISSIONS).contains(&attempt)
        && remaining > 0)
        .then(|| Duration::from_secs(1 << (attempt - 1)))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

pub fn client(credential_file: &Path) -> Result<reqwest::Client> {
    let key = fs::read_to_string(credential_file).context("Cannot read Google credential file")?;
    let mut header = HeaderValue::from_str(key.trim())
        .map_err(|_| anyhow::anyhow!("Invalid credential header"))?;
    header.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert("x-goog-api-key", header);
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(240))
        .build()?)
}

/// Listing is read-only; only public identities and capabilities are returned.
pub async fn models(http: &reqwest::Client) -> Result<Value> {
    let mut rows = Vec::new();
    let mut token = None;
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let mut url = reqwest::Url::parse(&format!("{API}/models"))?;
        url.query_pairs_mut().append_pair("pageSize", "1000");
        if let Some(value) = token.as_deref() {
            url.query_pairs_mut().append_pair("pageToken", value);
        }
        let response = http.get(url).send().await?;
        ensure!(
            response.status().is_success(),
            "Google model listing HTTP {}",
            response.status()
        );
        let body: Value = response.json().await?;
        rows.extend(body["models"].as_array().context("No Google model list")?.iter().map(|m|json!({"name":m["name"],"supportedGenerationMethods":m["supportedGenerationMethods"],"inputTokenLimit":m["inputTokenLimit"],"outputTokenLimit":m["outputTokenLimit"]})));
        token = body["nextPageToken"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let Some(next) = token.as_ref() else {
            break;
        };
        ensure!(
            seen.insert(next.clone()) && seen.len() < 100,
            "Google model listing pagination repeated or exceeded its bound"
        );
    }
    let select = |id: &str| {
        rows.iter()
            .find(|m| m["name"] == format!("models/{id}"))
            .cloned()
    };
    Ok(
        json!({"teacher":select(TEACHER),"student":select(STUDENT),"models":rows,"listing_complete":true,"next_page":null}),
    )
}

/// Count a student exchange without generation or teacher access. The exact
/// port-rendered request and provider count are retained for queue admission.
pub async fn count_tokens(
    http: &reqwest::Client,
    request: &LmRequest,
    directory: &Path,
) -> Result<Value> {
    count_tokens_at(http, request, directory, API).await
}
fn validate_count(record: &Value) -> Result<()> {
    let status = record["http_status"]
        .as_u64()
        .context("No recorded count HTTP status")? as u16;
    if !(200..300).contains(&status) {
        let hints = &record["quota_hints"];
        let kind = match status {
            429 if hints["zero_quota"] == true || hints["daily_quota"] == true => {
                Disposition::QuotaDeferred
            }
            429 | 500 | 502 | 503 | 504 => Disposition::QuotaOrUnavailable,
            401 | 403 => Disposition::Authentication,
            _ => Disposition::InvalidRequest,
        };
        let delay = hints["retry_after_ms"].as_u64().map(Duration::from_millis);
        return Err(provider_error(
            STUDENT,
            kind,
            Some(status),
            delay,
            false,
            "Read-only token count failed; exact completed response retained, no generation submitted",
        ));
    }
    ensure!(
        record["input_tokens"].as_u64().is_some(),
        "Provider token count missing"
    );
    Ok(())
}
async fn count_tokens_at(
    http: &reqwest::Client,
    request: &LmRequest,
    directory: &Path,
    api: &str,
) -> Result<Value> {
    ensure!(
        request.model == STUDENT && request.output_schema().is_none() && request.tools.is_empty(),
        "Student budget preflight permits ordinary text only"
    );
    fs::create_dir_all(directory)?;
    let key = request.cache_key(STUDENT);
    let file = directory.join(format!("{key}.json"));
    if file.exists() {
        let record = serde_json::from_slice(&fs::read(file)?)?;
        validate_count(&record)?;
        return Ok(record);
    }
    let started = directory.join(format!("{key}.request.json"));
    if started.exists() {
        return Err(provider_error(
            STUDENT,
            Disposition::PriorUnresolved,
            None,
            None,
            false,
            "Previous read-only count is unsettled or invalid; preserved without replay",
        ));
    }
    let contents = request.wire_messages().iter().filter(|message| message["role"] != "system").map(|message| {
        let text = message["content"].as_str().context("Budget preflight requires text inputs")?;
        Ok(json!({"role":if message["role"]=="assistant"{"model"}else{"user"},"parts":[{"text":text}]}))
    }).collect::<Result<Vec<_>>>()?;
    let body = json!({"generateContentRequest":{"model":format!("models/{STUDENT}"),
        "contents":contents,"systemInstruction":{"parts":[{"text":request.system()}]}}});
    write_new(
        &started,
        &serde_json::to_vec_pretty(
            &json!({"request":request,"body":body,"generation_submitted":false,"teacher_called":false}),
        )?,
    )?;
    let failure_record = |kind: Disposition, status: Option<u16>, detail: &str| -> Result<()> {
        write_new(
            &directory.join(format!("{key}.failure.json")),
            &serde_json::to_vec_pretty(
                &json!({"disposition":kind,"http_status":status,"response_complete":kind==Disposition::CompletedOutput,"generation_submitted":false,"teacher_called":false,"error":detail}),
            )?,
        )
    };
    let mut response = match http
        .post(format!("{api}/models/{STUDENT}:countTokens"))
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            failure_record(Disposition::Uncertain, None, &e.to_string())?;
            return Err(provider_error(
                STUDENT,
                Disposition::Uncertain,
                None,
                None,
                false,
                format!("Read-only count transport unsettled: {e}"),
            ));
        }
    };
    let status = response.status();
    let header = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(e) => {
                write_new(&directory.join(format!("{key}.body")), &bytes)?;
                failure_record(
                    Disposition::Uncertain,
                    Some(status.as_u16()),
                    &e.to_string(),
                )?;
                return Err(provider_error(
                    STUDENT,
                    Disposition::Uncertain,
                    Some(status.as_u16()),
                    None,
                    false,
                    format!("Read-only count response incomplete: {e}"),
                ));
            }
        }
    }
    write_new(&directory.join(format!("{key}.body")), &bytes)?;
    let provider: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) if !status.is_success() => Value::Null,
        Err(e) => {
            failure_record(
                Disposition::CompletedOutput,
                Some(status.as_u16()),
                &e.to_string(),
            )?;
            return Err(provider_error(
                STUDENT,
                Disposition::CompletedOutput,
                Some(status.as_u16()),
                None,
                false,
                format!("Read-only count completed with invalid JSON: {e}"),
            ));
        }
    };
    let advice = quota_advice(&bytes, header.as_deref(), STUDENT);
    let record = json!({"request_key":key,"request":request,"count_body":body,"http_status":status.as_u16(),
        "provider_response":provider,"input_tokens":provider["totalTokens"],"quota_hints":advice,"response_complete":true,"eligible_retry_unix_ms":advice.retry_after.map(|d|SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()+d.as_millis()),
        "generation_submitted":false,"teacher_called":false});
    write_new(&file, &serde_json::to_vec_pretty(&record)?)?;
    validate_count(&record)?;
    Ok(record)
}

/// A shared rolling window, rather than an arbitrary pause between each function.
pub struct Google {
    model: String,
    http: reqwest::Client,
    directory: PathBuf,
    spent: AtomicUsize,
    budget: Arc<CallBudget>,
    failed: AtomicBool,
    limiter: Arc<RateLimiter>,
    info: Mutex<Option<ModelInfo>>,
    interactions: bool,
    max_submissions: usize,
    quota_block: Mutex<Option<BlockedModel>>,
    serial: Mutex<()>,
    student_recovery: Option<crate::continuation::StudentRecovery>,
    // Fixed in production; credential-free unit tests substitute a local server.
    api: String,
}

impl Google {
    pub fn new(
        model: impl Into<String>,
        http: reqwest::Client,
        directory: PathBuf,
        max_calls: usize,
        tokens_per_minute: usize,
    ) -> Result<Self> {
        let model = model.into();
        ensure!(
            matches!(model.as_str(), TEACHER | STUDENT),
            "Unapproved model identity"
        );
        fs::create_dir_all(&directory)?;
        let interactions = model == TEACHER;
        Ok(Self {
            model,
            http,
            directory,
            spent: AtomicUsize::new(0),
            budget: CallBudget::new(max_calls),
            failed: AtomicBool::new(false),
            limiter: RateLimiter::new(tokens_per_minute)?,
            info: Mutex::new(None),
            interactions,
            max_submissions: MAX_SUBMISSIONS,
            quota_block: Mutex::new(None),
            serial: Mutex::new(()),
            student_recovery: None,
            api: API.into(),
        })
    }
    /// Listed text models use ordinary generateContent. A pool shares its
    /// physical bound while each model owns its distinct pacing state/cache.
    pub fn from_listing(
        info: ModelInfo,
        http: reqwest::Client,
        directory: PathBuf,
        budget: Arc<CallBudget>,
        limiter: Arc<RateLimiter>,
    ) -> Result<Self> {
        let model = info.id()?.to_owned();
        ensure!(
            model == STUDENT || crate::teachers::is_text_teacher(&info),
            "Listed model is not an approved text teacher or fixed student"
        );
        ensure!(
            info.supported_generation_methods
                .iter()
                .any(|m| m == "generateContent"),
            "Listed model does not support ordinary generation"
        );
        fs::create_dir_all(&directory)?;
        let max_submissions = if model == STUDENT { MAX_SUBMISSIONS } else { 1 };
        Ok(Self {
            model,
            http,
            directory,
            spent: AtomicUsize::new(0),
            budget,
            failed: AtomicBool::new(false),
            limiter,
            info: Mutex::new(Some(info)),
            interactions: false,
            max_submissions,
            quota_block: Mutex::new(None),
            serial: Mutex::new(()),
            student_recovery: None,
            api: API.into(),
        })
    }
    pub fn limiter(&self) -> Arc<RateLimiter> {
        self.limiter.clone()
    }
    pub fn with_limiter(mut self, limiter: Arc<RateLimiter>) -> Self {
        self.limiter = limiter;
        self
    }
    pub fn model_id(&self) -> &str {
        &self.model
    }
    /// Only an explicitly closed, source-bound carrier continuation can install
    /// this slot. The request body stays identical; fresh artifacts never use
    /// the failed original key or overwrite any inherited attempt.
    pub fn with_student_recovery(
        mut self,
        slot: crate::continuation::StudentRecovery,
    ) -> Result<Self> {
        ensure!(
            self.model == STUDENT
                && !self.interactions
                && self.budget.maximum == slot.original_physical_cap
                && self.budget.spent() + 1 == slot.first_physical_ordinal,
            "Student recovery requires its original seeded cumulative budget"
        );
        slot.verify_witnesses(&self.directory)?;
        self.student_recovery = Some(slot);
        Ok(self)
    }
    fn routed_key(&self, request: &LmRequest) -> (LmRequest, String) {
        let mut routed = request.clone();
        routed.model = self.model.clone();
        let key = if self.interactions {
            digest(format!(
                "interactions-stateless-v1\0{}",
                routed.cache_key(&self.model)
            ))
        } else {
            routed.cache_key(&self.model)
        };
        (routed, key)
    }
    /// Exact settled success lookup only. No metadata/token requests, quota
    /// retries or generation occur, even when this route is now exhausted.
    pub fn cached_response(&self, request: &LmRequest) -> Result<Option<LmResponse>> {
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "This experiment permits unconstrained text only"
        );
        let (routed, key) = self.routed_key(request);
        if let Some(slot) = self
            .student_recovery
            .as_ref()
            .filter(|s| s.original_key == key)
        {
            slot.verify_witnesses(&self.directory)?;
        }
        if let Some(response) = self.cached_key(&routed, &key)? {
            return Ok(Some(response));
        }
        if let Some(recovery_key) = self.recovery_key(&routed, &key)? {
            let slot = self.student_recovery.as_ref().context("No recovery slot")?;
            ensure!(recovery_key == slot.recovery_key, "Recovery key drifted");
            return self.settled_recovery(slot);
        }
        Ok(None)
    }
    fn cached_key(&self, routed: &LmRequest, key: &str) -> Result<Option<LmResponse>> {
        let cached = self.directory.join(format!("{key}.response.json"));
        if !cached.exists() {
            return Ok(None);
        }
        let saved: Value = serde_json::from_slice(&fs::read(
            self.directory.join(format!("{key}.request.json")),
        )?)?;
        ensure!(
            saved["model"] == self.model && saved["request"] == serde_json::to_value(routed)?,
            "Cached Google response request identity differs"
        );
        let mut result: LmResponse = serde_json::from_slice(&fs::read(cached)?)?;
        ensure!(
            result.model.as_deref() == Some(self.model.as_str())
                && !result.first_text().trim().is_empty(),
            "Cached Google response is not a settled success for this model"
        );
        result.cache_hit = true;
        Ok(Some(result))
    }
    fn recovery_key(&self, routed: &LmRequest, key: &str) -> Result<Option<String>> {
        let Some(slot) = self
            .student_recovery
            .as_ref()
            .filter(|s| s.original_key == key)
        else {
            return Ok(None);
        };
        slot.verify_witnesses(&self.directory)?;
        let saved: Value = serde_json::from_slice(&fs::read(
            self.directory.join(format!("{key}.request.json")),
        )?)?;
        ensure!(
            saved["model"] == STUDENT && saved["request"] == serde_json::to_value(routed)?,
            "Explicit student recovery request differs from its completed failure"
        );
        let binding_path = self
            .directory
            .join(format!("{}.recovery.json", slot.recovery_key));
        if binding_path.exists() {
            ensure!(
                serde_json::from_slice::<Value>(&fs::read(&binding_path)?)?
                    == serde_json::to_value(slot)?,
                "Saved student recovery binding differs from its explicit ancestry"
            );
        } else {
            ensure!(
                !self
                    .directory
                    .join(format!("{}.response.json", slot.recovery_key))
                    .exists(),
                "Recovered response lacks its explicit closed-owner ancestry"
            );
        }
        Ok(Some(slot.recovery_key.clone()))
    }
    fn settled_recovery(
        &self,
        slot: &crate::continuation::StudentRecovery,
    ) -> Result<Option<LmResponse>> {
        slot.verify_witnesses(&self.directory)?;
        let original: Value = serde_json::from_slice(&fs::read(
            self.directory
                .join(format!("{}.request.json", slot.original_key)),
        )?)?;
        let request: LmRequest = serde_json::from_value(original["request"].clone())?;
        self.recovery_key(&request, &slot.original_key)?;
        let key = &slot.recovery_key;
        let Some(response) = self.cached_key(&request, key)? else {
            return Ok(None);
        };
        let read = |suffix: &str| -> Result<Value> {
            Ok(serde_json::from_slice(&fs::read(
                self.directory.join(format!("{key}.{suffix}")),
            )?)?)
        };
        let logical_bytes = fs::read(self.directory.join(format!("{key}.request.json")))?;
        let logical: Value = serde_json::from_slice(&logical_bytes)?;
        let receipt = read("receipt.json")?;
        let attempt = receipt["physical_submissions"]
            .as_u64()
            .context("No recovered attempt count")? as usize;
        ensure!(
            (1..=slot.maximum_new_attempts).contains(&attempt)
                && logical == original
                && receipt["model"] == STUDENT
                && receipt["request_sha256"] == digest(&logical_bytes),
            "Recovered success has a changed request/body or unbound receipt"
        );
        let physical = read(&format!("attempt-{attempt}.request.json"))?;
        let result = read(&format!("attempt-{attempt}.result.json"))?;
        let body = fs::read(self.directory.join(format!("{key}.attempt-{attempt}.body")))?;
        let raw = read("raw.json")?;
        let ordinal = slot.first_physical_ordinal + attempt - 1;
        ensure!(
            physical["logical_key"] == *key
                && physical["model"] == STUDENT
                && physical["attempt"] == attempt
                && physical["physical_call"] == ordinal
                && physical["request_sha256"] == digest(&logical_bytes)
                && physical["body"] == original["body"]
                && physical["body_sha256"] == digest(serde_json::to_vec(&original["body"])?),
            "Recovered success physical request is outside the exact authorized slot"
        );
        ensure!(
            receipt["shared_physical_calls"] == ordinal
                && result["logical_key"] == *key
                && result["attempt"] == attempt
                && result["physical_call"] == ordinal
                && result["response_complete"] == true
                && result["http_status"] == 200
                && result["automatic_retry"] == false
                && result["body_sha256"] == digest(&body)
                && raw == serde_json::from_slice::<Value>(&body)?,
            "Recovered success lacks a complete hash-bound physical response"
        );
        let candidates = raw["candidates"]
            .as_array()
            .context("Recovered response has no candidates")?;
        ensure!(
            candidates.len() == response.outputs.len()
                && candidates
                    .iter()
                    .zip(&response.outputs)
                    .all(|(raw, output)| {
                        let text = raw["content"]["parts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|part| part["thought"] != true)
                            .filter_map(|part| part["text"].as_str())
                            .collect::<String>();
                        raw["finishReason"] != "MAX_TOKENS"
                            && !text.trim().is_empty()
                            && output.as_text() == text
                            && !output.truncated
                            && output.finish_reason.as_deref() == raw["finishReason"].as_str()
                    }),
            "Recovered normalized answer differs from its complete native provider response"
        );
        Ok(Some(response))
    }
    /// Verify saved transport evidence without a network operation. Only listed
    /// ordinary text teachers with a complete HTTP200/MAX_TOKENS response qualify.
    pub async fn completed_truncation(
        &self,
        request: &LmRequest,
    ) -> Result<Option<CompletedTruncation>> {
        if self.interactions || self.model == STUDENT {
            return Ok(None);
        }
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "Teacher repair permits ordinary text only"
        );
        let (routed, key) = self.routed_key(request);
        let raw_path = self.directory.join(format!("{key}.raw.json"));
        if !raw_path.exists() || self.cached_response(request)?.is_some() {
            return Ok(None);
        }
        let raw_bytes = fs::read(raw_path)?;
        let raw: Value = serde_json::from_slice(&raw_bytes)?;
        let candidates = raw["candidates"]
            .as_array()
            .context("No saved teacher candidates")?;
        if candidates.len() != 1 || candidates[0]["finishReason"] != "MAX_TOKENS" {
            return Ok(None);
        }
        ensure!(
            candidates[0]["content"]["parts"]
                .as_array()
                .is_some_and(|parts| parts
                    .iter()
                    .any(|p| p["thought"] != true
                        && p["text"].as_str().is_some_and(|s| !s.is_empty()))),
            "Truncated teacher response has no observed text output"
        );
        let saved_bytes = fs::read(self.directory.join(format!("{key}.request.json")))?;
        let saved: Value = serde_json::from_slice(&saved_bytes)?;
        ensure!(
            saved["model"] == self.model && saved["request"] == serde_json::to_value(&routed)?,
            "Truncation witness request identity differs"
        );
        let original_max = routed.config.max_tokens.unwrap_or(4096) as usize;
        ensure!(
            saved["body"]["generationConfig"]["maxOutputTokens"] == original_max,
            "Truncation witness output bound differs"
        );
        let attempt: Value = serde_json::from_slice(&fs::read(
            self.directory.join(format!("{key}.attempt-1.request.json")),
        )?)?;
        let result: Value = serde_json::from_slice(&fs::read(
            self.directory.join(format!("{key}.attempt-1.result.json")),
        )?)?;
        let body_bytes = fs::read(self.directory.join(format!("{key}.attempt-1.body")))?;
        let body_sha = digest(&body_bytes);
        let saved_sha = digest(&saved_bytes);
        ensure!(
            result["logical_key"] == key
                && result["attempt"] == 1
                && result["http_status"] == 200
                && result["response_complete"] == true
                && result["automatic_retry"] == false
                && result["body_sha256"] == body_sha,
            "Truncation witness is not a complete hash-bound HTTP200 response"
        );
        ensure!(
            serde_json::from_slice::<Value>(&body_bytes)? == raw,
            "Saved parsed teacher output differs from complete response body"
        );
        ensure!(
            attempt["logical_key"] == key
                && attempt["attempt"] == 1
                && attempt["model"] == self.model
                && attempt["physical_call"].as_u64().is_some_and(|n| n > 0)
                && result["physical_call"] == attempt["physical_call"]
                && attempt["request_sha256"] == saved_sha
                && attempt["body"] == saved["body"]
                && attempt["body_sha256"] == digest(serde_json::to_vec(&saved["body"])?),
            "Truncation witness attempt identity differs"
        );
        let info = self
            .info
            .lock()
            .await
            .clone()
            .context("Teacher repair needs captured listed model metadata")?;
        ensure!(
            info.id()? == self.model && crate::teachers::is_text_teacher(&info),
            "Truncation witness is not a listed text teacher"
        );
        Ok(Some(CompletedTruncation {
            model: self.model.clone(),
            original_key: key,
            original_request_sha256: saved_sha,
            original_raw_sha256: body_sha,
            parsed_raw_sha256: digest(raw_bytes),
            original_max_output_tokens: original_max,
            model_output_token_limit: info.output_token_limit,
        }))
    }
    #[cfg(test)]
    pub(crate) fn test_api(&mut self, api: &str) {
        self.api = api.into();
    }
    fn failure(
        &self,
        disposition: Disposition,
        http_status: Option<u16>,
        retry_after: Option<Duration>,
        generation_submitted: bool,
        detail: impl Into<String>,
    ) -> anyhow::Error {
        provider_error(
            &self.model,
            disposition,
            http_status,
            retry_after,
            generation_submitted,
            detail,
        )
    }
    async fn model_info(&self) -> Result<ModelInfo> {
        let mut info = self.info.lock().await;
        if let Some(info) = info.as_ref() {
            return Ok(info.clone());
        }
        let file = self.directory.join("model-info.json");
        let value: Value = if file.exists() {
            serde_json::from_slice(&fs::read(&file)?)?
        } else {
            let response = self
                .http
                .get(format!("{}/models/{}", self.api, self.model))
                .send()
                .await
                .map_err(|e| {
                    self.failure(
                        Disposition::Uncertain,
                        None,
                        None,
                        false,
                        format!("Read-only model metadata transport unsettled: {e}"),
                    )
                })?;
            let status = response.status();
            let bytes = response.bytes().await.map_err(|e| {
                self.failure(
                    Disposition::Uncertain,
                    Some(status.as_u16()),
                    None,
                    false,
                    format!("Read-only model metadata incomplete: {e}"),
                )
            })?;
            write_new(&self.directory.join("model-info.body"), &bytes)?;
            let value: Value = serde_json::from_slice(&bytes)
                .or_else(|e| {
                    if !status.is_success() {
                        Ok(Value::Null)
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| {
                    self.failure(
                        Disposition::CompletedOutput,
                        Some(status.as_u16()),
                        None,
                        false,
                        format!("Model metadata is not JSON: {e}"),
                    )
                })?;
            if !status.is_success() {
                return Err(self.http_failure(
                    status.as_u16(),
                    None,
                    false,
                    "Read-only model metadata failed",
                ));
            }
            write_new(&file, &serde_json::to_vec_pretty(&value)?)?;
            value
        };
        let result: ModelInfo = serde_json::from_value(value)?;
        ensure!(
            result.id()? == self.model,
            "Provider model metadata identity differs"
        );
        *info = Some(result.clone());
        Ok(result)
    }
    fn http_failure(
        &self,
        status: u16,
        retry_after: Option<Duration>,
        submitted: bool,
        detail: impl Into<String>,
    ) -> anyhow::Error {
        let kind = match status {
            429 | 500 | 502 | 503 | 504 | 404 => Disposition::QuotaOrUnavailable,
            401 | 403 => Disposition::Authentication,
            _ => Disposition::InvalidRequest,
        };
        self.failure(kind, Some(status), retry_after, submitted, detail)
    }

    /// Only a fully received, known transient HTTP response permits another submission.
    /// Each attempt is independently paced, budgeted and retained under the logical key.
    async fn generate(
        &self,
        key: &str,
        body: &Value,
        tokens: usize,
        request_sha256: &str,
    ) -> Result<(Vec<u8>, usize)> {
        let recovery = self
            .student_recovery
            .as_ref()
            .filter(|slot| slot.recovery_key == key);
        let maximum_submissions =
            recovery.map_or(self.max_submissions, |slot| slot.maximum_new_attempts);
        let endpoint = if self.interactions {
            format!("{}/interactions", self.api)
        } else {
            format!("{}/models/{}:generateContent", self.api, self.model)
        };
        let mut delay = Duration::ZERO;
        for attempt in 1..=maximum_submissions {
            if let Some(slot) = recovery {
                ensure!(
                    self.budget.spent() + 1 == slot.first_physical_ordinal + attempt - 1
                        && self.budget.spent() < slot.last_physical_ordinal,
                    "Explicit student recovery ordinal span is stale or exhausted; no submission"
                );
            }
            if self.budget.remaining() == 0 {
                return Err(self.failure(
                    Disposition::BudgetExhausted,
                    None,
                    None,
                    false,
                    "Configured shared physical generation-call budget exhausted",
                ));
            }
            let file = |suffix: &str| {
                self.directory
                    .join(format!("{key}.attempt-{attempt}.{suffix}"))
            };
            write_new(
                &file("wait.json"),
                &serde_json::to_vec_pretty(
                    &json!({"model":self.model,"logical_key":key,"attempt":attempt,"input_tokens":tokens,"recorded_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"retry_backoff_ms":delay.as_millis(),"pacing_before":self.limiter.snapshot().await,"generation_submitted":false,"scope":"Client pacing forecast, not inference latency"}),
                )?,
            )?;
            let wait_started = Instant::now();
            tokio::time::sleep(delay).await;
            let pacing_delay = self.limiter.pace(tokens).await;
            write_new(
                &file("wait-complete.json"),
                &serde_json::to_vec_pretty(
                    &json!({"model":self.model,"attempt":attempt,"actual_wait_ms":wait_started.elapsed().as_millis(),"pacing_after":self.limiter.snapshot().await,"generation_submitted":false}),
                )?,
            )?;
            let physical_call = self.budget.reserve().ok_or_else(|| {
                self.failure(
                    Disposition::BudgetExhausted,
                    None,
                    None,
                    false,
                    "Configured shared physical generation-call budget exhausted",
                )
            })?;
            write_new(
                &file("request.json"),
                &serde_json::to_vec_pretty(&json!({"logical_key":key,"attempt":attempt,
                    "physical_call":physical_call,"model":self.model,"request_sha256":request_sha256,
                    "body":body,"body_sha256":digest(serde_json::to_vec(body)?),
                    "input_tokens":tokens,"delay_before_ms":delay.as_millis(),"pacing_delay_ms":pacing_delay.as_millis(),"pacing":self.limiter.snapshot().await}))?,
            )?;
            self.spent.fetch_add(1, Ordering::Relaxed);
            eprintln!(
                "Google {} generation {} {} attempt {}",
                self.model,
                physical_call,
                &key[..12],
                attempt
            );
            let begun = Instant::now();
            let mut response = match self.http.post(&endpoint).json(body).send().await {
                Ok(response) => response,
                Err(error) => {
                    write_new(
                        &file("result.json"),
                        &serde_json::to_vec_pretty(&json!({
                            "logical_key":key,"attempt":attempt,"physical_call":physical_call,
                            "http_status":null,"response_complete":false,"automatic_retry":false,
                            "error_class":"transport_unsettled","elapsed_ms":begun.elapsed().as_millis()
                        }))?,
                    )?;
                    return Err(self.failure(
                        Disposition::Uncertain,
                        None,
                        None,
                        true,
                        format!("Google transport is unsettled; no retry: {error}"),
                    ));
                }
            };
            let status = response.status();
            let retry_after_header = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut bytes = Vec::new();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
                    Ok(None) => break,
                    Err(error) => {
                        write_new(&file("body"), &bytes)?;
                        write_new(
                            &file("result.json"),
                            &serde_json::to_vec_pretty(&json!({
                                "logical_key":key,"attempt":attempt,"physical_call":physical_call,
                                "http_status":status.as_u16(),"response_complete":false,
                                "body_sha256":digest(&bytes),"automatic_retry":false,
                                "error_class":"response_unsettled","elapsed_ms":begun.elapsed().as_millis()
                            }))?,
                        )?;
                        return Err(self.failure(
                            Disposition::Uncertain,
                            Some(status.as_u16()),
                            None,
                            true,
                            format!("Google response is incomplete; no retry: {error}"),
                        ));
                    }
                }
            }
            write_new(&file("body"), &bytes)?;
            let advice = quota_advice(&bytes, retry_after_header.as_deref(), &self.model);
            if status.as_u16() == 429 {
                self.limiter.observe(&advice).await;
                if advice.zero_quota
                    || self.model != STUDENT && (advice.daily_quota || advice.retry_after.is_some())
                {
                    *self.quota_block.lock().await = Some(BlockedModel {
                        until: advice.retry_after.map(|d| Instant::now() + d),
                        disposition: if advice.zero_quota || advice.daily_quota {
                            Disposition::QuotaDeferred
                        } else {
                            Disposition::QuotaOrUnavailable
                        },
                    });
                }
            }
            let next_delay = retry_delay(status.as_u16(), true, attempt, self.budget.remaining())
                .filter(|_| {
                    attempt < maximum_submissions
                        && !advice.zero_quota
                        && (!advice.daily_quota
                            || self.model == STUDENT && advice.retry_after.is_some())
                })
                .map(|base| base.max(advice.retry_after.unwrap_or_default()))
                .filter(|d| {
                    *d <= MAX_AUTOMATIC_DELAY
                        || status.as_u16() == 429
                            && self.model == STUDENT
                            && advice.retry_after.is_some()
                });
            write_new(
                &file("result.json"),
                &serde_json::to_vec_pretty(&json!({
                    "logical_key":key,"attempt":attempt,"physical_call":physical_call,
                    "http_status":status.as_u16(),"response_complete":true,
                    "body_sha256":digest(&bytes),"automatic_retry":next_delay.is_some(),
                    "retry_delay_ms":next_delay.map(|value|value.as_millis()),"provider_quota_advice":advice,
                    "elapsed_ms":begun.elapsed().as_millis()
                }))?,
            )?;
            if status.is_success() {
                return Ok((bytes, attempt));
            }
            if let Some(next) = next_delay {
                delay = next;
                continue;
            }
            write_new(
                &self.directory.join(format!("{key}.failure.json")),
                &serde_json::to_vec_pretty(&json!({"http_status":status.as_u16(),
                    "elapsed_ms":begun.elapsed().as_millis(),"physical_submissions":attempt,
                    "automatic_retry":false,"response_complete":true,"disposition":if status.as_u16()==429 && (advice.zero_quota || advice.daily_quota || advice.retry_after.is_some_and(|d|d>MAX_AUTOMATIC_DELAY)){Disposition::QuotaDeferred}else{match status.as_u16(){429|500|502|503|504|404=>Disposition::QuotaOrUnavailable,401|403=>Disposition::Authentication,_=>Disposition::InvalidRequest}},"eligible_retry_unix_ms":advice.retry_after.map(|d|SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()+d.as_millis()),"provider_quota_advice":advice}))?,
            )?;
            if status.as_u16() == 429
                && (advice.zero_quota
                    || advice.daily_quota
                    || advice.retry_after.is_some_and(|d| d > MAX_AUTOMATIC_DELAY))
            {
                return Err(self.failure(Disposition::QuotaDeferred,Some(429),advice.retry_after,true,"Known daily/zero/long quota response; durable deferral retained, no automatic replay"));
            }
            return Err(self.http_failure(
                status.as_u16(),
                advice.retry_after,
                true,
                format!(
                    "HTTP {}; stopped after {attempt} physical submissions",
                    status
                ),
            ));
        }
        bail!("No Google response after bounded physical submissions")
    }
    async fn request(&self, request: &LmRequest) -> Result<LmResponse> {
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "This experiment permits unconstrained text only"
        );
        let (routed, canonical_key) = self.routed_key(request);
        let _guard = self.serial.lock().await;
        if let Some(response) = self.cached_response(request)? {
            return Ok(response);
        }
        let recovery_key = self.recovery_key(&routed, &canonical_key)?;
        let key = recovery_key.unwrap_or(canonical_key);
        if let Some(slot) = &self.student_recovery {
            ensure!(
                key == slot.recovery_key || self.settled_recovery(slot)?.is_some(),
                "The explicit student recovery is unresolved; unrelated fresh submissions are withheld"
            );
        }
        if self.failed.load(Ordering::Relaxed) {
            return Err(self.failure(
                Disposition::PriorUnresolved,
                None,
                None,
                false,
                "A prior uncertain/authentication transport stops new work",
            ));
        }
        let cached = self.directory.join(format!("{key}.response.json"));
        {
            let mut blocked = self.quota_block.lock().await;
            if let Some(block) = blocked.as_ref() {
                if block.until.is_none_or(|until| until > Instant::now()) {
                    return Err(self.failure(
                        block.disposition,
                        Some(429),
                        block
                            .until
                            .map(|until| until.saturating_duration_since(Instant::now())),
                        false,
                        "Retained provider quota blocks this model; no new generation submitted",
                    ));
                }
                *blocked = None;
            }
        }
        let started = self.directory.join(format!("{key}.request.json"));
        if started.exists() {
            let failure = self.directory.join(format!("{key}.failure.json"));
            if failure.exists() {
                let saved: Value = serde_json::from_slice(&fs::read(failure)?)?;
                let status = saved["http_status"].as_u64().map(|s| s as u16);
                if let Some(status) = status {
                    let deferred = saved["disposition"] == "quota_deferred";
                    return Err(if deferred {
                        self.failure(
                            Disposition::QuotaDeferred,
                            Some(status),
                            None,
                            true,
                            "A previous known quota deferral is retained and will not be replayed",
                        )
                    } else {
                        self.http_failure(status,None,true,"A previous completed HTTP failure is retained and will not be replayed")
                    });
                }
            }
            return Err(self.failure(Disposition::PriorUnresolved,None,None,true,"A previous Google request is unresolved or failed; it is preserved and will not be replayed"));
        }
        if self.budget.remaining() == 0 {
            return Err(self.failure(
                Disposition::BudgetExhausted,
                None,
                None,
                false,
                "Configured shared physical generation-call budget exhausted",
            ));
        }
        let info = self.model_info().await?;
        let wire = routed.wire_messages();
        let contents = wire.iter().filter(|m| m["role"] != "system").map(|m| {
            let text = m["content"].as_str().context("Nontext input not allowed in this curriculum")?;
            Ok(json!({"role":if m["role"] == "assistant" {"model"} else {"user"},"parts":[{"text":text}]}))
        }).collect::<Result<Vec<_>>>()?;
        let mut legacy_body = json!({"systemInstruction":{"parts":[{"text":routed.system()}]},"contents":contents,"generationConfig":{"temperature":routed.config.temperature.unwrap_or(0.0),"maxOutputTokens":routed.config.max_tokens.unwrap_or(4096)}});
        if self.model == STUDENT {
            legacy_body["generationConfig"]["thinkingConfig"] =
                json!({"thinkingLevel":"MINIMAL","includeThoughts":false});
        }
        let body = if self.interactions {
            json!({"model":self.model,"store":false,"system_instruction":routed.system(),"input":contents.iter().map(|m|json!({"type":if m["role"]=="model"{"model_output"}else{"user_input"},"content":[{"type":"text","text":m["parts"][0]["text"]}]})).collect::<Vec<_>>(),"generation_config":{"temperature":routed.config.temperature.unwrap_or(0.),"max_output_tokens":routed.config.max_tokens.unwrap_or(4096),"thinking_level":"low"}})
        } else {
            legacy_body.clone()
        };
        if let Some(slot) = self
            .student_recovery
            .as_ref()
            .filter(|slot| slot.recovery_key == key)
        {
            ensure!(
                self.budget.spent() + 1 == slot.first_physical_ordinal,
                "Another fresh student call consumed this explicit recovery span"
            );
            let original: Value = serde_json::from_slice(&fs::read(
                self.directory
                    .join(format!("{}.request.json", slot.original_key)),
            )?)?;
            ensure!(
                body == original["body"],
                "Student recovery may not change the prompt, generation configuration or provider body"
            );
            let binding_path = self.directory.join(format!("{key}.recovery.json"));
            let binding = serde_json::to_value(slot)?;
            if binding_path.exists() {
                ensure!(
                    serde_json::from_slice::<Value>(&fs::read(&binding_path)?)? == binding,
                    "Existing student recovery ancestry differs"
                );
            } else {
                write_new(&binding_path, &serde_json::to_vec_pretty(&binding)?)?;
            }
            // Restore actual provider limits rather than rediscovering them in
            // the fresh process. Waiting a witnessed retry interval again is
            // conservative; it cannot bypass the provider's recorded cooldown.
            for name in slot
                .witnesses
                .keys()
                .filter(|name| name.ends_with(".result.json"))
            {
                let result: Value = serde_json::from_slice(&fs::read(self.directory.join(name))?)?;
                if result["http_status"] == 429 {
                    let body_path = name
                        .strip_suffix("result.json")
                        .context("Invalid result path")?
                        .to_owned()
                        + "body";
                    let bytes = fs::read(self.directory.join(body_path))?;
                    let header = result["provider_quota_advice"]["retry_after_header"].as_str();
                    let advice = quota_advice(&bytes, header, &self.model);
                    ensure!(
                        !advice.zero_quota && !advice.daily_quota,
                        "Zero or daily quota cannot use this completed-transient recovery slot"
                    );
                    self.limiter.observe(&advice).await;
                }
            }
        }
        let counted = self.directory.join(format!("{key}.tokens.json"));
        let count: Value = if counted.exists() {
            serde_json::from_slice(&fs::read(&counted)?)?
        } else {
            let response=self.http.post(format!("{}/models/{}:countTokens",self.api,self.model)).json(&json!({"generateContentRequest":{"model":format!("models/{}",self.model),"contents":legacy_body["contents"],"systemInstruction":legacy_body["systemInstruction"]}})).send().await.map_err(|e|self.failure(Disposition::Uncertain,None,None,false,format!("Read-only count transport unsettled: {e}")))?;
            let status = response.status();
            let header = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned);
            let bytes = response.bytes().await.map_err(|e| {
                self.failure(
                    Disposition::Uncertain,
                    Some(status.as_u16()),
                    None,
                    false,
                    format!("Read-only count response incomplete: {e}"),
                )
            })?;
            write_new(&self.directory.join(format!("{key}.count-body")), &bytes)?;
            let value: Value = serde_json::from_slice(&bytes)
                .or_else(|e| {
                    if !status.is_success() {
                        Ok(Value::Null)
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| {
                    self.failure(
                        Disposition::CompletedOutput,
                        Some(status.as_u16()),
                        None,
                        false,
                        format!("Read-only count is not JSON: {e}"),
                    )
                })?;
            if !status.is_success() {
                let advice = quota_advice(&bytes, header.as_deref(), &self.model);
                write_new(
                    &self.directory.join(format!("{key}.count-failure.json")),
                    &serde_json::to_vec_pretty(
                        &json!({"http_status":status.as_u16(),"provider_response":value,"provider_quota_advice":advice,"generation_submitted":false,"response_complete":true}),
                    )?,
                )?;
                if status.as_u16() == 429 {
                    self.limiter.observe(&advice).await;
                }
                return Err(self.http_failure(
                    status.as_u16(),
                    advice.retry_after,
                    false,
                    "Read-only token count failed; no generation submitted",
                ));
            }
            write_new(&counted, &serde_json::to_vec_pretty(&value)?)?;
            value
        };
        let tokens = count["totalTokens"]
            .as_u64()
            .context("No counted input tokens")? as usize;
        let preflight = self.directory.join(format!("{key}.preflight.json"));
        if !preflight.exists() {
            write_new(
                &preflight,
                &serde_json::to_vec_pretty(&json!({
                    "model":self.model,"request_key":key,"request":routed,"body":body,
                    "input_tokens":tokens,"configured_input_tokens_per_minute":self.limiter.hint,"pacing":self.limiter.snapshot().await,
                    "model_input_token_limit":info.input_token_limit,"admitted":tokens<=info.input_token_limit,"generation_submitted":false,
                    "scope":"Read-only provider token count and client admission; not a model response or verified project quota"
                }))?,
            )?;
        }
        if tokens > info.input_token_limit {
            return Err(self.failure(Disposition::ContextLimit,None,None,false,format!("Prompt has {tokens} input tokens, above listed model context {}. Exact preflight retained at {}",info.input_token_limit,preflight.display())));
        }
        if routed
            .config
            .max_tokens
            .is_some_and(|n| n as usize > info.output_token_limit)
        {
            return Err(self.failure(
                Disposition::ContextLimit,
                None,
                None,
                false,
                "Requested output budget exceeds listed model outputTokenLimit",
            ));
        }
        let saved_request =
            serde_json::to_vec_pretty(&json!({"model":self.model,"request":routed,"body":body}))?;
        write_new(&started, &saved_request)?;
        let begun = Instant::now();
        let (bytes, attempts) = self
            .generate(&key, &body, tokens, &digest(saved_request))
            .await?;
        let raw: Value = serde_json::from_slice(&bytes).map_err(|e| {
            self.failure(
                Disposition::CompletedOutput,
                Some(200),
                None,
                true,
                format!("Completed provider output is invalid JSON: {e}"),
            )
        })?;
        write_new(
            &self.directory.join(format!("{key}.raw.json")),
            &serde_json::to_vec_pretty(&raw)?,
        )?;
        let parsed_outputs = || -> Result<Vec<LmOutput>> {
            let outputs = if self.interactions {
                ensure!(
                    raw["status"] == "completed",
                    "Teacher interaction did not complete; raw response is retained"
                );
                let result = raw["steps"]
                    .as_array()
                    .context("No interaction steps")?
                    .iter()
                    .filter(|s| s["type"] == "model_output")
                    .flat_map(|s| s["content"].as_array().into_iter().flatten())
                    .filter(|p| p["type"] == "text")
                    .filter_map(|p| p["text"].as_str())
                    .collect::<String>();
                vec![LmOutput::text(result)]
            } else {
                raw["candidates"]
                    .as_array()
                    .context("Google returned no candidates")?
                    .iter()
                    .map(|c| {
                        let text = c["content"]["parts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|p| p["thought"] != true)
                            .filter_map(|p| p["text"].as_str())
                            .collect::<String>();
                        let mut output = LmOutput::text(text);
                        output.finish_reason = c["finishReason"].as_str().map(str::to_owned);
                        output.truncated = c["finishReason"] == "MAX_TOKENS";
                        output
                    })
                    .collect::<Vec<_>>()
            };
            ensure!(
                !outputs.is_empty()
                    && outputs
                        .iter()
                        .all(|o| !o.as_text().is_empty() && !o.truncated),
                "Google produced empty or truncated output; raw response retained"
            );
            Ok(outputs)
        };
        let outputs = parsed_outputs().map_err(|error| {
            self.failure(
                Disposition::CompletedOutput,
                Some(200),
                None,
                true,
                error.to_string(),
            )
        })?;
        let result = LmResponse {
            model: Some(self.model.clone()),
            outputs,
            response_id: raw[if self.interactions {
                "id"
            } else {
                "responseId"
            }]
            .as_str()
            .map(str::to_owned),
            ..Default::default()
        };
        write_new(
            &self.directory.join(format!("{key}.receipt.json")),
            &serde_json::to_vec_pretty(
                &json!({"model":self.model,"api":if self.interactions{"interactions-stateless"}else{"generateContent"},"request_sha256":digest(fs::read(started)?),"elapsed_ms":begun.elapsed().as_millis(),"timing_scope":"Whole generation request wall time includes retries and pacing; each attempt/result elapsed_ms measures its HTTP response and wait artifacts record pacing separately.","usage":raw[if self.interactions{"usage"}else{"usageMetadata"}],"response_schema":false,"physical_submissions":attempts,"shared_physical_calls":self.budget.spent()}),
            )?,
        )?;
        write_new(&cached, &serde_json::to_vec_pretty(&result)?)?;
        self.limiter.success(tokens).await;
        write_new(
            &self
                .directory
                .join(format!("{key}.pacing-after-success.json")),
            &serde_json::to_vec_pretty(&self.limiter.snapshot().await)?,
        )?;
        Ok(result)
    }
}

impl ChatModel for Google {
    async fn forward(&self, _http: &reqwest::Client, request: &LmRequest) -> Result<LmResponse> {
        let result = self.request(request).await;
        if result.as_ref().err().is_some_and(|error| {
            disposition(error).is_some_and(|f| {
                matches!(
                    f.disposition,
                    Disposition::Uncertain
                        | Disposition::Authentication
                        | Disposition::PriorUnresolved
                )
            })
        }) {
            self.failed.store(true, Ordering::Relaxed);
        }
        result
    }
    async fn capabilities(&self, _http: &reqwest::Client) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dsrust::lm::api::{LmMessage, LmPart};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex as StdMutex},
        thread,
    };

    const SUCCESS: &str = r#"{"candidates":[{"content":{"parts":[{"text":"Observed response"}]},"finishReason":"STOP"}]}"#;

    pub(crate) enum Reply {
        Complete(u16, &'static str),
        CompleteWithRetry(u16, &'static str, &'static str),
        Disconnect,
        Partial(u16, &'static str),
    }

    pub(crate) struct Mock {
        pub(crate) api: String,
        pub(crate) requests: Arc<StdMutex<Vec<Value>>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    fn read_mock_request(stream: &mut TcpStream) -> Value {
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        let header = String::from_utf8(header).unwrap();
        let length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|value| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        json!({"path":header.split_whitespace().nth(1).unwrap(),
            "body":if body.is_empty(){Value::Null}else{serde_json::from_slice::<Value>(&body).unwrap()},"body_sha256":digest(&body)})
    }

    impl Mock {
        pub(crate) fn new(replies: Vec<Reply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let api = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(StdMutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let captured = requests.clone();
            let stopped = stop.clone();
            let thread = thread::spawn(move || {
                let mut replies = replies.into_iter();
                while !stopped.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(error) => panic!("Local mock accept: {error}"),
                    };
                    captured
                        .lock()
                        .unwrap()
                        .push(read_mock_request(&mut stream));
                    let reply = replies
                        .next()
                        .unwrap_or(Reply::Complete(500, "Unexpected request"));
                    match reply {
                        Reply::Disconnect => {}
                        Reply::Complete(status, body) => {
                            write!(stream,"HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                        }
                        Reply::CompleteWithRetry(status, body, retry) => {
                            write!(stream,"HTTP/1.1 {status} Mock\r\nRetry-After: {retry}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                        }
                        Reply::Partial(status, body) => {
                            write!(stream,"HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()+20).unwrap();
                        }
                    }
                }
            });
            Self {
                api,
                requests,
                stop,
                thread: Some(thread),
            }
        }

        fn model(&self, directory: &Path, max_calls: usize, allowance: usize) -> Google {
            let http = reqwest::Client::builder()
                .no_proxy()
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            let mut model =
                Google::new(STUDENT, http, directory.into(), max_calls, allowance).unwrap();
            model.api = self.api.clone();
            model.info = Mutex::new(Some(ModelInfo {
                name: format!("models/{STUDENT}"),
                supported_generation_methods: vec!["generateContent".into()],
                input_token_limit: 100_000,
                output_token_limit: 8192,
            }));
            model
        }
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let result = self.thread.take().unwrap().join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }

    fn prompt(text: &str) -> LmRequest {
        LmRequest::new(
            STUDENT,
            vec![
                LmMessage::system(vec![LmPart::text("Plain text only")]),
                LmMessage::user(vec![LmPart::text(text)]),
            ],
        )
    }

    fn counted(directory: &Path, request: &LmRequest, tokens: usize) -> String {
        let key = request.cache_key(STUDENT);
        fs::write(
            directory.join(format!("{key}.tokens.json")),
            json!({"totalTokens":tokens}).to_string(),
        )
        .unwrap();
        key
    }

    fn artifact(directory: &Path, key: &str, suffix: &str) -> Value {
        serde_json::from_slice(&fs::read(directory.join(format!("{key}.{suffix}"))).unwrap())
            .unwrap()
    }

    fn completed_student_slot(
        directory: &Path,
        request: &LmRequest,
        cap: usize,
    ) -> crate::continuation::StudentRecovery {
        let key = request.cache_key(STUDENT);
        let body = json!({"systemInstruction":{"parts":[{"text":request.system()}]},"contents":[{"role":"user","parts":[{"text":request.wire_messages()[1]["content"]}]}],"generationConfig":{"temperature":0.0,"maxOutputTokens":4096,"thinkingConfig":{"thinkingLevel":"MINIMAL","includeThoughts":false}}});
        let logical =
            serde_json::to_vec_pretty(&json!({"model":STUDENT,"request":request,"body":body}))
                .unwrap();
        fs::write(directory.join(format!("{key}.request.json")), &logical).unwrap();
        for (attempt, status) in [(1, 500), (2, 429), (3, 500)] {
            let response = format!(
                "{{\"error\":{{\"code\":{status},\"message\":\"Synthetic completed transient\"}}}}"
            );
            fs::write(directory.join(format!("{key}.attempt-{attempt}.request.json")), json!({"logical_key":key,"attempt":attempt,"physical_call":attempt,"model":STUDENT,"request_sha256":digest(&logical),"body":body,"body_sha256":digest(serde_json::to_vec(&body).unwrap())}).to_string()).unwrap();
            fs::write(
                directory.join(format!("{key}.attempt-{attempt}.body")),
                &response,
            )
            .unwrap();
            fs::write(directory.join(format!("{key}.attempt-{attempt}.result.json")), json!({"logical_key":key,"attempt":attempt,"physical_call":attempt,"http_status":status,"response_complete":true,"automatic_retry":attempt<3,"body_sha256":digest(response)}).to_string()).unwrap();
        }
        fs::write(directory.join(format!("{key}.failure.json")), json!({"http_status":500,"response_complete":true,"automatic_retry":false,"disposition":"quota_or_unavailable","physical_submissions":3}).to_string()).unwrap();
        let witnesses = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_str().unwrap().to_owned(),
                    digest(fs::read(entry.path()).unwrap()),
                )
            })
            .collect();
        let mut slot = crate::continuation::StudentRecovery {
            version: 1,
            model: STUDENT.into(),
            original_key: key,
            original_request_sha256: digest(logical),
            closure_receipt_sha256: digest("Synthetic authoritative closure"),
            recovery_key: String::new(),
            witnesses,
            maximum_new_attempts: 3,
            first_physical_ordinal: 4,
            last_physical_ordinal: 6,
            original_physical_cap: cap,
        };
        slot.recovery_key = slot.expected_key();
        slot
    }

    #[tokio::test]
    async fn explicit_completed_student_recovery_uses_distinct_artifacts_and_reuses_success() {
        let mock = Mock::new(vec![
            Reply::Complete(200, SUCCESS),
            Reply::Complete(200, SUCCESS),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let request = prompt("Synthetic original first message");
        let slot = completed_student_slot(directory.path(), &request, 20);
        fs::write(
            directory
                .path()
                .join(format!("{}.tokens.json", slot.recovery_key)),
            json!({"totalTokens":20}).to_string(),
        )
        .unwrap();
        let mut model = mock.model(directory.path(), 20, 100_000);
        model.budget = CallBudget::seeded(20, 3).unwrap();
        let model = model.with_student_recovery(slot.clone()).unwrap();
        let response = model
            .forward(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        assert_eq!(response.first_text(), "Observed response");
        assert_eq!(model.budget.spent(), 4);
        assert_eq!(
            artifact(
                directory.path(),
                &slot.recovery_key,
                "attempt-1.request.json"
            )["physical_call"],
            4
        );
        assert_eq!(
            artifact(directory.path(), &slot.recovery_key, "recovery.json"),
            json!(slot)
        );
        for (name, hash) in &slot.witnesses {
            assert_eq!(
                digest(fs::read(directory.path().join(name)).unwrap()),
                *hash
            );
        }
        let reused = model
            .forward(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        assert!(reused.cache_hit);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        assert_eq!(model.budget.spent(), 4);
        let next = prompt("Unrelated fresh work after a settled recovery");
        counted(directory.path(), &next, 20);
        model.forward(&reqwest::Client::new(), &next).await.unwrap();
        assert_eq!(model.budget.spent(), 5);
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
        let response_path = directory
            .path()
            .join(format!("{}.response.json", slot.recovery_key));
        let mut altered = artifact(directory.path(), &slot.recovery_key, "response.json");
        altered["outputs"][0]["parts"][0]["text"] = json!("Invented replacement");
        fs::write(&response_path, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert!(model.cached_response(&request).is_err());
    }

    #[tokio::test]
    async fn unresolved_explicit_slot_blocks_an_unrelated_uncached_prompt_without_spending_259() {
        let mock = Mock::new(vec![]);
        let directory = tempfile::tempdir().unwrap();
        let original = prompt("Exact synthetic failed first message");
        let mut slot = completed_student_slot(directory.path(), &original, 18_000);
        for attempt in 1..=3 {
            for suffix in ["request.json", "result.json"] {
                let name = format!("{}.attempt-{attempt}.{suffix}", slot.original_key);
                let mut data = artifact(
                    directory.path(),
                    &slot.original_key,
                    &format!("attempt-{attempt}.{suffix}"),
                );
                data["physical_call"] = json!(255 + attempt);
                fs::write(
                    directory.path().join(&name),
                    serde_json::to_vec(&data).unwrap(),
                )
                .unwrap();
                slot.witnesses.insert(
                    name.clone(),
                    digest(fs::read(directory.path().join(name)).unwrap()),
                );
            }
        }
        slot.first_physical_ordinal = 259;
        slot.last_physical_ordinal = 261;
        let mut model = mock.model(directory.path(), 18_000, 100_000);
        model.budget = CallBudget::seeded(18_000, 258).unwrap();
        let model = model.with_student_recovery(slot).unwrap();
        let unrelated = prompt("Unrelated uncached request before recovery");
        let error = model
            .forward(&reqwest::Client::new(), &unrelated)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unrelated fresh submissions are withheld")
        );
        assert_eq!(model.budget.spent(), 258);
        assert!(mock.requests.lock().unwrap().is_empty());
        assert!(
            !directory
                .path()
                .join(format!("{}.request.json", unrelated.cache_key(STUDENT)))
                .exists()
        );
    }

    #[tokio::test]
    async fn explicit_student_recovery_cannot_repeat_a_failed_or_uncertain_fresh_slot() {
        for uncertain in [false, true] {
            let replies = if uncertain {
                vec![Reply::Disconnect]
            } else {
                vec![
                    Reply::Complete(500, "Known complete transient"),
                    Reply::Complete(500, "Known complete transient"),
                    Reply::Complete(500, "Known complete transient"),
                ]
            };
            let mock = Mock::new(replies);
            let directory = tempfile::tempdir().unwrap();
            let request = prompt("One explicit synthetic recovery");
            let slot = completed_student_slot(directory.path(), &request, 6);
            fs::write(
                directory
                    .path()
                    .join(format!("{}.tokens.json", slot.recovery_key)),
                json!({"totalTokens":20}).to_string(),
            )
            .unwrap();
            let mut model = mock.model(directory.path(), 6, 100_000);
            model.budget = CallBudget::seeded(6, 3).unwrap();
            let model = model.with_student_recovery(slot.clone()).unwrap();
            let error = model
                .forward(&reqwest::Client::new(), &request)
                .await
                .unwrap_err();
            assert_eq!(
                disposition(&error).unwrap().disposition,
                if uncertain {
                    Disposition::Uncertain
                } else {
                    Disposition::QuotaOrUnavailable
                }
            );
            let calls = mock.requests.lock().unwrap().len();
            assert_eq!(calls, if uncertain { 1 } else { 3 });
            assert!(
                model
                    .forward(&reqwest::Client::new(), &request)
                    .await
                    .is_err()
            );
            assert_eq!(mock.requests.lock().unwrap().len(), calls);
            assert_eq!(model.budget.spent(), if uncertain { 4 } else { 6 });
            for (name, hash) in &slot.witnesses {
                assert_eq!(
                    digest(fs::read(directory.path().join(name)).unwrap()),
                    *hash
                );
            }
        }
    }

    #[test]
    fn explicit_student_slot_rejects_changed_request_witness_or_reset_budget_before_network() {
        let mock = Mock::new(vec![]);
        let directory = tempfile::tempdir().unwrap();
        let request = prompt("Original synthetic question");
        let slot = completed_student_slot(directory.path(), &request, 6);
        let model = mock.model(directory.path(), 6, 100_000);
        assert!(model.with_student_recovery(slot.clone()).is_err());
        let mut model = mock.model(directory.path(), 6, 100_000);
        model.budget = CallBudget::seeded(6, 3).unwrap();
        let model = model.with_student_recovery(slot.clone()).unwrap();
        fs::write(
            directory
                .path()
                .join(format!("{}.attempt-2.body", slot.original_key)),
            "Modified old evidence",
        )
        .unwrap();
        assert!(model.cached_response(&request).is_err());
        assert!(mock.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn retry_policy_is_bounded_and_only_for_complete_known_transients() {
        for status in [429, 500, 502, 503, 504] {
            assert_eq!(
                retry_delay(status, true, 1, 2),
                Some(Duration::from_secs(1))
            );
            assert_eq!(
                retry_delay(status, true, 2, 1),
                Some(Duration::from_secs(2))
            );
            assert_eq!(retry_delay(status, true, 3, 5), None);
            assert_eq!(retry_delay(status, true, 0, 5), None);
            assert_eq!(retry_delay(status, false, 1, 2), None);
            assert_eq!(retry_delay(status, true, 1, 0), None);
        }
        for status in [200, 400, 401, 403, 404, 408, 501, 505] {
            assert_eq!(retry_delay(status, true, 1, 2), None);
        }
    }

    #[tokio::test]
    async fn complete_transients_retry_exact_wire_then_cache_only_the_success() {
        let mock = Mock::new(vec![
            Reply::Complete(200, r#"{"totalTokens":100}"#),
            Reply::Complete(500, "First complete failure"),
            Reply::Complete(503, "Second complete failure"),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let model = mock.model(dir.path(), 3, 14_000);
        let request = prompt("Synthetic question");
        let key = request.cache_key(STUDENT);
        let response = model.forward(&model.http, &request).await.unwrap();
        assert_eq!(response.outputs[0].as_text(), "Observed response");
        assert!(!response.cache_hit);
        let cached = model.forward(&model.http, &request).await.unwrap();
        assert!(cached.cache_hit);
        assert_eq!(cached.outputs, response.outputs);
        assert_eq!(model.spent.load(Ordering::Relaxed), 3);
        assert_eq!(
            model
                .limiter
                .state
                .lock()
                .await
                .window
                .iter()
                .map(|(_, n)| n)
                .sum::<usize>(),
            300
        );
        let captured = mock.requests.lock().unwrap();
        assert_eq!(captured.len(), 4);
        assert!(
            captured[0]["path"]
                .as_str()
                .unwrap()
                .ends_with(":countTokens")
        );
        for index in 2..4 {
            assert_eq!(captured[index], captured[1]);
        }
        let original = artifact(dir.path(), &key, "request.json");
        for attempt in 1..=3 {
            let saved = artifact(dir.path(), &key, &format!("attempt-{attempt}.request.json"));
            assert_eq!(saved["body"], original["body"]);
            assert_eq!(saved["body_sha256"], captured[1]["body_sha256"]);
            assert_eq!(saved["physical_call"], attempt);
            assert_eq!(saved["input_tokens"], 100);
            let expected_delay = [0, 1000, 2000][attempt - 1];
            assert_eq!(saved["delay_before_ms"], expected_delay);
        }
        assert_eq!(
            fs::read(dir.path().join(format!("{key}.attempt-1.body"))).unwrap(),
            b"First complete failure"
        );
        assert_eq!(
            fs::read(dir.path().join(format!("{key}.attempt-2.body"))).unwrap(),
            b"Second complete failure"
        );
        assert_eq!(
            artifact(dir.path(), &key, "receipt.json")["physical_submissions"],
            3
        );
        assert!(!dir.path().join(format!("{key}.failure.json")).exists());
    }

    #[tokio::test]
    async fn exhausted_transients_stop_and_a_new_instance_cannot_replay_the_failed_key() {
        let mock = Mock::new(vec![
            Reply::Complete(429, "Busy"),
            Reply::Complete(500, "Failure"),
            Reply::Complete(502, "Gateway"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("Bounded failures");
        let key = counted(dir.path(), &request, 100);
        let model = mock.model(dir.path(), 9, 14_000);
        let error = model.forward(&model.http, &request).await.unwrap_err();
        assert!(error.to_string().contains("3 physical submissions"));
        assert_eq!(model.spent.load(Ordering::Relaxed), 3);
        assert!(!model.failed.load(Ordering::Relaxed));
        assert!(!dir.path().join(format!("{key}.response.json")).exists());
        assert_eq!(
            artifact(dir.path(), &key, "failure.json")["http_status"],
            502
        );
        assert_eq!(
            artifact(dir.path(), &key, "attempt-3.result.json")["automatic_retry"],
            false
        );
        let new = mock.model(dir.path(), 100, 14_000);
        assert!(
            new.request(&request)
                .await
                .unwrap_err()
                .to_string()
                .contains("will not be replayed")
        );
        assert_eq!(new.spent.load(Ordering::Relaxed), 0);
        assert_eq!(mock.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn nontransient_http_errors_never_retry() {
        for status in [400, 401, 403, 404, 408, 501] {
            let mock = Mock::new(vec![Reply::Complete(status, "Not retryable")]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Nontransient");
            let key = counted(dir.path(), &request, 100);
            let model = mock.model(dir.path(), 3, 14_000);
            assert!(model.forward(&model.http, &request).await.is_err());
            assert_eq!(model.spent.load(Ordering::Relaxed), 1);
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
            assert_eq!(
                artifact(dir.path(), &key, "attempt-1.result.json")["automatic_retry"],
                false
            );
        }
    }

    #[tokio::test]
    async fn uncertain_send_or_transient_response_body_never_retries() {
        for reply in [Reply::Disconnect, Reply::Partial(500, "Partial")] {
            let mock = Mock::new(vec![reply]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Unsettled transport");
            let key = counted(dir.path(), &request, 100);
            let model = mock.model(dir.path(), 3, 14_000);
            assert!(model.forward(&model.http, &request).await.is_err());
            assert_eq!(model.spent.load(Ordering::Relaxed), 1);
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
            let saved = artifact(dir.path(), &key, "attempt-1.result.json");
            assert_eq!(saved["response_complete"], false);
            assert_eq!(saved["automatic_retry"], false);
            assert!(!dir.path().join(format!("{key}.response.json")).exists());
        }
    }

    #[tokio::test]
    async fn complete_http_success_with_parse_or_output_failure_never_retries() {
        for body in [
            "Invalid JSON",
            "{}",
            r#"{"candidates":[]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"Incomplete"}]},"finishReason":"MAX_TOKENS"}]}"#,
        ] {
            let mock = Mock::new(vec![Reply::Complete(200, body)]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Invalid output");
            let key = counted(dir.path(), &request, 100);
            let model = mock.model(dir.path(), 3, 14_000);
            assert!(model.forward(&model.http, &request).await.is_err());
            assert_eq!(model.spent.load(Ordering::Relaxed), 1);
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
            assert_eq!(
                fs::read(dir.path().join(format!("{key}.attempt-1.body"))).unwrap(),
                body.as_bytes()
            );
            assert_eq!(
                artifact(dir.path(), &key, "attempt-1.result.json")["response_complete"],
                true
            );
            assert!(!dir.path().join(format!("{key}.response.json")).exists());
        }
    }

    #[tokio::test]
    async fn physical_cap_prevents_an_allowed_retry_and_blocks_following_work() {
        let mock = Mock::new(vec![Reply::Complete(504, "Timed out upstream")]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("One reserved submission");
        let key = counted(dir.path(), &request, 100);
        let model = mock.model(dir.path(), 1, 14_000);
        assert!(model.forward(&model.http, &request).await.is_err());
        assert_eq!(model.spent.load(Ordering::Relaxed), 1);
        assert_eq!(
            artifact(dir.path(), &key, "attempt-1.result.json")["retry_delay_ms"],
            Value::Null
        );
        let error = model
            .forward(&model.http, &prompt("Other work"))
            .await
            .unwrap_err();
        assert_eq!(
            disposition(&error).unwrap().disposition,
            Disposition::BudgetExhausted
        );
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn every_retry_consumes_the_shared_token_window() {
        let mock = Mock::new(vec![
            Reply::Complete(429, MINUTE_QUOTA),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("First logical request");
        counted(dir.path(), &request, 7000);
        let model = mock.model(dir.path(), 3, 14_000);
        model.forward(&model.http, &request).await.unwrap();
        assert_eq!(
            model
                .limiter
                .state
                .lock()
                .await
                .window
                .iter()
                .map(|(_, n)| n)
                .sum::<usize>(),
            14_000
        );
        let next = prompt("Distinct logical request");
        counted(dir.path(), &next, 7000);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), model.request(&next))
                .await
                .is_err()
        );
        assert_eq!(model.spent.load(Ordering::Relaxed), 2);
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn context_limit_or_zero_physical_budget_submits_nothing() {
        for (cap, tokens) in [(0, 100), (3, 100_001)] {
            let mock = Mock::new(vec![]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Preflight failure");
            counted(dir.path(), &request, tokens);
            let model = mock.model(dir.path(), cap, 14_000);
            assert!(model.forward(&model.http, &request).await.is_err());
            assert_eq!(model.spent.load(Ordering::Relaxed), 0);
            assert!(mock.requests.lock().unwrap().is_empty());
            assert!(model.limiter.state.lock().await.window.is_empty());
            if tokens > 100_000 {
                let key = request.cache_key(STUDENT);
                let proof = artifact(dir.path(), &key, "preflight.json");
                assert_eq!(proof["input_tokens"], tokens);
                assert_eq!(proof["configured_input_tokens_per_minute"], 14_000);
                assert_eq!(proof["model_input_token_limit"], 100_000);
                assert_eq!(proof["admitted"], false);
                assert_eq!(proof["generation_submitted"], false);
                assert_eq!(proof["request"]["model"], STUDENT);
                assert_eq!(
                    proof["body"]["contents"][0]["parts"][0]["text"],
                    "Preflight failure"
                );
                assert!(!dir.path().join(format!("{key}.request.json")).exists());
            }
        }
    }
    #[test]
    fn identity_uses_the_requested_teacher_and_student() {
        assert_eq!(TEACHER, "gemini-3.8-flash");
        assert_eq!(STUDENT, "gemma-4-26b-a4b-it");
    }
    #[tokio::test]
    async fn a_schema_cannot_reach_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let model = Google::new(
            STUDENT,
            reqwest::Client::new(),
            dir.path().into(),
            1,
            14_000,
        )
        .unwrap();
        let mut request = LmRequest::default();
        request.config.response_format = Some(json!({"type":"object"}));
        assert!(
            model
                .request(&request)
                .await
                .unwrap_err()
                .to_string()
                .contains("unconstrained")
        );
        assert_eq!(model.spent.load(Ordering::Relaxed), 0);
    }
    const MINUTE_QUOTA: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateContentInputTokensPerMinutePerModel","quotaMetric":"generativelanguage.googleapis.com/generate_content_input_tokens","quotaDimensions":{"model":"gemma-4-26b-a4b-it"},"quotaValue":"20000"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"0.01s"}]}}"#;
    const DAILY_QUOTA: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateContentRequestsPerDayPerModel","quotaDimensions":{"model":"gemma-4-26b-a4b-it"},"quotaValue":"100"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"0.01s"}]}}"#;
    const ZERO_QUOTA: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateContentRequestsPerDayPerModel","quotaValue":"0"}]}]}}"#;

    #[test]
    fn quota_group_match_is_limited_to_the_observed_fixed_student_pair() {
        for dimension in [
            STUDENT,
            "models/gemma-4-26b-a4b-it",
            "gemma-4-26b",
            "models/gemma-4-26b",
        ] {
            assert!(quota_model_matches(STUDENT, dimension));
        }
        for dimension in [
            "gemma-4-12b",
            "gemma-4-26b-a4b",
            "gemma-4-26b-other",
            "gemma-4-26",
            "gemma-4",
            "models/models/gemma-4-26b",
            "prefix/gemma-4-26b",
            "gemini-3.8-flash",
        ] {
            assert!(!quota_model_matches(STUDENT, dimension), "{dimension}");
        }
        assert!(!quota_model_matches(TEACHER, "gemma-4-26b"));
        assert!(!quota_model_matches("gemma-4-12b-it", "gemma-4-26b"));
        assert!(!quota_model_matches("gemma-4-26b-other", "gemma-4-26b"));
        assert!(quota_model_matches(TEACHER, "models/gemini-3.8-flash"));
    }

    #[tokio::test]
    async fn observed_quota_group_adopts_16000_without_halving_the_probe_or_rewriting_evidence() {
        let violation = json!({
            "quotaMetric":"generativelanguage.googleapis.com/generate_content_free_tier_input_token_count",
            "quotaId":"GenerateContentInputTokensPerModelPerMinute-FreeTier",
            "quotaDimensions":{"location":"global","model":"gemma-4-26b"},
            "quotaValue":"16000"
        });
        let body = json!({"error":{"code":429,"details":[
            {"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[violation.clone()]},
            {"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"37s"}
        ]}});
        let advice = quota_advice(&serde_json::to_vec(&body).unwrap(), None, STUDENT);
        assert_eq!(advice.tokens_per_minute, Some(16_000));
        assert_eq!(advice.retry_after, Some(Duration::from_secs(37)));
        assert_eq!(advice.violations, vec![violation]);
        let limiter = RateLimiter::with_probe_ceiling(14_000, 224_000).unwrap();
        for _ in 0..4 {
            limiter.success(100).await;
        }
        let probed = limiter.snapshot().await["probed_hint_tpm"].clone();
        limiter.observe(&advice).await;
        limiter.success(100).await;
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot["provider_reported_tpm"], 16_000);
        assert_eq!(snapshot["probed_hint_tpm"], probed);
        assert!(snapshot["cooldown_remaining_ms"].as_u64().unwrap() > 0);
    }

    #[test]
    fn foreign_quota_groups_keep_raw_violations_without_applying_rate_zero_or_daily_flags() {
        for dimension in [
            "gemma-4-12b",
            "gemma-4-26b-other",
            "models/models/gemma-4-26b",
        ] {
            let violations = vec![
                json!({"quotaId":"GenerateContentInputTokensPerModelPerMinute-FreeTier","quotaDimensions":{"model":dimension},"quotaValue":"16000"}),
                json!({"quotaId":"GenerateContentRequestsPerDayPerModel","quotaDimensions":{"model":dimension},"quotaValue":"0"}),
            ];
            let body = json!({"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":violations}]}});
            let advice = quota_advice(&serde_json::to_vec(&body).unwrap(), None, STUDENT);
            assert_eq!(advice.violations, violations);
            assert_eq!(advice.tokens_per_minute, None);
            assert_eq!(advice.requests_per_minute, None);
            assert!(!advice.zero_quota);
            assert!(!advice.daily_quota);
        }
    }

    #[tokio::test]
    async fn aliased_zero_and_daily_quotas_keep_existing_deferral_and_bounded_retry_guards() {
        const ALIASED_ZERO: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateContentRequestsPerDayPerModel","quotaDimensions":{"model":"gemma-4-26b"},"quotaValue":"0"}]}]}}"#;
        const ALIASED_DAILY: &str = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateContentRequestsPerDayPerModel","quotaDimensions":{"model":"models/gemma-4-26b"},"quotaValue":"100"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"0.01s"}]}}"#;
        for (body, submits) in [(ALIASED_ZERO, 1), (ALIASED_DAILY, 2)] {
            let mut replies = vec![Reply::Complete(429, body)];
            if submits == 2 {
                replies.push(Reply::Complete(200, SUCCESS));
            }
            let mock = Mock::new(replies);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Synthetic aliased quota guard");
            let key = counted(dir.path(), &request, 100);
            let model = mock.model(dir.path(), 2, 14_000);
            let response = model.forward(&model.http, &request).await;
            if submits == 1 {
                let error = response.unwrap_err();
                assert_eq!(
                    disposition(&error).unwrap().disposition,
                    Disposition::QuotaDeferred
                );
                assert_eq!(disposition(&error).unwrap().retry_after, None);
                assert_eq!(
                    artifact(dir.path(), &key, "failure.json")["eligible_retry_unix_ms"],
                    Value::Null
                );
            } else {
                response.unwrap();
                assert_eq!(
                    artifact(dir.path(), &key, "attempt-1.result.json")["automatic_retry"],
                    true
                );
            }
            let result = artifact(dir.path(), &key, "attempt-1.result.json");
            assert_eq!(result["provider_quota_advice"]["daily_quota"], true);
            assert_eq!(result["provider_quota_advice"]["zero_quota"], submits == 1);
            assert_eq!(model.budget.spent(), submits);
            assert_eq!(mock.requests.lock().unwrap().len(), submits);
        }
    }

    #[tokio::test]
    async fn prompt_larger_than_rate_hint_is_admitted_and_charged_not_called_context_failure() {
        let mock = Mock::new(vec![Reply::Complete(200, SUCCESS)]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("Synthetic larger exchange");
        let key = counted(dir.path(), &request, 20_000);
        let model = mock.model(dir.path(), 1, 14_000);
        model.forward(&model.http, &request).await.unwrap();
        assert_eq!(
            artifact(dir.path(), &key, "preflight.json")["admitted"],
            true
        );
        assert_eq!(
            model.limiter.snapshot().await["charged_input_tokens"],
            20_000
        );
        assert_eq!(model.spent.load(Ordering::Relaxed), 1);
        assert!(
            dir.path()
                .join(format!("{key}.attempt-1.wait.json"))
                .is_file()
        );
    }
    #[tokio::test]
    async fn provider_minute_quota_and_retry_headers_adapt_pacing_and_retain_waits() {
        let mock = Mock::new(vec![
            Reply::CompleteWithRetry(429, MINUTE_QUOTA, "1"),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("Synthetic quota adaptation");
        let key = counted(dir.path(), &request, 6000);
        let model = mock.model(dir.path(), 2, 14_000);
        model.forward(&model.http, &request).await.unwrap();
        let result = artifact(dir.path(), &key, "attempt-1.result.json");
        assert_eq!(result["provider_quota_advice"]["tokens_per_minute"], 20_000);
        assert_eq!(result["retry_delay_ms"], 1000);
        assert_eq!(
            model.limiter.snapshot().await["provider_reported_tpm"],
            20_000
        );
        assert_eq!(model.budget.spent(), 2);
        assert_eq!(
            artifact(dir.path(), &key, "attempt-2.wait.json")["retry_backoff_ms"],
            1000
        );
    }
    #[tokio::test]
    async fn nonzero_daily_quota_uses_supplied_retry_time_with_same_bounded_accounting() {
        let mock = Mock::new(vec![
            Reply::Complete(429, DAILY_QUOTA),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("Synthetic daily reset wait");
        let key = counted(dir.path(), &request, 100);
        let model = mock.model(dir.path(), 2, 14_000);
        model.forward(&model.http, &request).await.unwrap();
        assert_eq!(model.budget.spent(), 2);
        assert_eq!(
            artifact(dir.path(), &key, "attempt-1.result.json")["automatic_retry"],
            true
        );
        assert_eq!(
            artifact(dir.path(), &key, "attempt-2.request.json")["physical_call"],
            2
        );
    }
    #[tokio::test]
    async fn zero_quota_is_durable_deferral_without_invented_retry_or_more_calls() {
        let mock = Mock::new(vec![Reply::Complete(429, ZERO_QUOTA)]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("Synthetic absent quota");
        let key = counted(dir.path(), &request, 100);
        let model = mock.model(dir.path(), 3, 14_000);
        let error = model.forward(&model.http, &request).await.unwrap_err();
        assert_eq!(
            disposition(&error).unwrap().disposition,
            Disposition::QuotaDeferred
        );
        assert_eq!(disposition(&error).unwrap().retry_after, None);
        assert_eq!(
            artifact(dir.path(), &key, "failure.json")["eligible_retry_unix_ms"],
            Value::Null
        );
        let error = model
            .forward(&model.http, &prompt("A different synthetic task"))
            .await
            .unwrap_err();
        assert_eq!(
            disposition(&error).unwrap().disposition,
            Disposition::QuotaDeferred
        );
        assert!(!disposition(&error).unwrap().generation_submitted);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn successful_rate_probes_are_bounded_and_reported_quota_replaces_them() {
        let limiter = RateLimiter::with_probe_ceiling(10_000, 20_000).unwrap();
        for _ in 0..10 {
            limiter.success(100).await;
        }
        assert_eq!(limiter.snapshot().await["probed_hint_tpm"], 20_000);
        let mut advice = quota_advice(MINUTE_QUOTA.as_bytes(), None, STUDENT);
        advice.tokens_per_minute = Some(9000);
        limiter.observe(&advice).await;
        limiter.success(100).await;
        assert_eq!(limiter.snapshot().await["provider_reported_tpm"], 9000);
        assert_eq!(limiter.snapshot().await["probed_hint_tpm"], 20_000);
    }
    #[tokio::test]
    async fn separate_student_jobs_share_one_limiter_and_physical_budget() {
        let mock = Mock::new(vec![Reply::Complete(200, SUCCESS)]);
        let dir = tempfile::tempdir().unwrap();
        let budget = CallBudget::new(2);
        let limiter = RateLimiter::with_probe_ceiling(14_000, 14_000).unwrap();
        let info = ModelInfo {
            name: format!("models/{STUDENT}"),
            supported_generation_methods: vec!["generateContent".into()],
            input_token_limit: 100_000,
            output_token_limit: 8192,
        };
        let http = reqwest::Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .build()
            .unwrap();
        let mut first = Google::from_listing(
            info.clone(),
            http.clone(),
            dir.path().join("job1"),
            budget.clone(),
            limiter.clone(),
        )
        .unwrap();
        first.test_api(&mock.api);
        let mut second = Google::from_listing(
            info,
            http.clone(),
            dir.path().join("job2"),
            budget.clone(),
            limiter.clone(),
        )
        .unwrap();
        second.test_api(&mock.api);
        let a = prompt("Synthetic first job");
        let b = prompt("Synthetic second job");
        counted(&first.directory, &a, 6000);
        counted(&second.directory, &b, 9000);
        first.forward(&http, &a).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), second.forward(&http, &b))
                .await
                .is_err()
        );
        assert_eq!(budget.spent(), 1);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        assert_eq!(limiter.snapshot().await["charged_input_tokens"], 6000);
    }
    #[tokio::test]
    async fn actual_model_metadata_context_limit_blocks_generation_independently_of_rate() {
        const PROFILE: &str = r#"{"name":"models/gemma-4-26b-a4b-it","supportedGenerationMethods":["generateContent"],"inputTokenLimit":10000,"outputTokenLimit":8192}"#;
        let mock = Mock::new(vec![Reply::Complete(200, PROFILE)]);
        let dir = tempfile::tempdir().unwrap();
        let input = prompt("Synthetic context overflow");
        let key = counted(dir.path(), &input, 10001);
        let mut model = mock.model(dir.path(), 1, 1_000_000);
        model.info = Mutex::new(None);
        let error = model.forward(&model.http, &input).await.unwrap_err();
        assert_eq!(
            disposition(&error).unwrap().disposition,
            Disposition::ContextLimit
        );
        assert_eq!(
            artifact(dir.path(), &key, "preflight.json")["model_input_token_limit"],
            10000
        );
        assert_eq!(model.budget.spent(), 0);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn read_only_count_quota_and_uncertainty_are_preserved_and_not_replayed() {
        for reply in [
            Reply::CompleteWithRetry(429, MINUTE_QUOTA, "2"),
            Reply::Partial(200, "{\"totalTokens\":"),
        ] {
            let mock = Mock::new(vec![reply]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Synthetic read-only count");
            let http = reqwest::Client::builder()
                .no_proxy()
                .retry(reqwest::retry::never())
                .build()
                .unwrap();
            let error = count_tokens_at(&http, &request, dir.path(), &mock.api)
                .await
                .unwrap_err();
            let kind = disposition(&error).unwrap().disposition;
            assert!(matches!(
                kind,
                Disposition::QuotaOrUnavailable | Disposition::Uncertain
            ));
            let second = count_tokens_at(&http, &request, dir.path(), &mock.api)
                .await
                .unwrap_err();
            assert!(matches!(
                disposition(&second).unwrap().disposition,
                Disposition::QuotaOrUnavailable | Disposition::PriorUnresolved
            ));
            assert_eq!(mock.requests.lock().unwrap().len(), 1);
            if kind == Disposition::QuotaOrUnavailable {
                let record: Value = serde_json::from_slice(
                    &fs::read(
                        dir.path()
                            .join(format!("{}.json", request.cache_key(STUDENT))),
                    )
                    .unwrap(),
                )
                .unwrap();
                assert_eq!(record["quota_hints"]["tokens_per_minute"], 20_000);
                assert_eq!(record["quota_hints"]["retry_after_ms"], 2000);
            }
        }
    }
}
