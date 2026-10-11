//! Opt-in hosted inference for synthetic pipeline evaluation, never an app default.
#![forbid(unsafe_code)]

use crate::native_llama_worker::NativeGenerationResult;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) const MODEL: &str = "gemma-4-26b-a4b-it";
const ENDPOINT: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemma-4-26b-a4b-it:generateContent";
const COUNT_ENDPOINT: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemma-4-26b-a4b-it:countTokens";

struct TokenWindow {
    reservations: VecDeque<(Instant, u32)>,
    budget: u32,
    waiting: VecDeque<(u64, u32)>,
    next_ticket: u64,
}
impl TokenWindow {
    fn new(budget: u32) -> Self {
        Self {
            reservations: VecDeque::new(),
            budget,
            waiting: VecDeque::new(),
            next_ticket: 0,
        }
    }

    fn enqueue(&mut self, tokens: u32) -> Result<u64, String> {
        self.check_size(tokens)?;
        let ticket = self.next_ticket;
        self.next_ticket = ticket
            .checked_add(1)
            .ok_or("Hosted quota ticket overflow")?;
        self.waiting.push_back((ticket, tokens));
        Ok(ticket)
    }

    fn check_size(&self, tokens: u32) -> Result<(), String> {
        if tokens > self.budget {
            return Err(format!("This prompt needs {tokens} input tokens, above the configured hosted per-minute budget {}; no generation submitted",self.budget));
        }
        Ok(())
    }

    /// Admission is FIFO after token counting. A newer small prompt must not
    /// keep taking the capacity that an older large prompt is waiting for.
    fn reserve_ticket(&mut self, now: Instant, ticket: u64) -> Result<Option<Duration>, String> {
        let &(front, tokens) = self.waiting.front().ok_or("Missing hosted quota ticket")?;
        if ticket != front {
            if !self.waiting.iter().any(|(id, _)| *id == ticket) {
                return Err("Missing hosted quota ticket".into());
            }
            return Ok(Some(Duration::from_millis(100)));
        }
        let wait = self.reserve(now, tokens)?;
        if wait.is_none() {
            self.waiting.pop_front();
        }
        Ok(wait)
    }

    fn cancel(&mut self, ticket: u64) {
        self.waiting.retain(|(id, _)| *id != ticket);
        // An admitted reservation is deliberately not refunded: provider
        // uncertainty or later cancellation must not bypass quota accounting.
    }

    fn reserve(&mut self, now: Instant, tokens: u32) -> Result<Option<Duration>, String> {
        self.check_size(tokens)?;
        let window = Duration::from_millis(61_000);
        while self
            .reservations
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= window)
        {
            self.reservations.pop_front();
        }
        let used = self
            .reservations
            .iter()
            .map(|(_, n)| u64::from(*n))
            .sum::<u64>();
        if used + u64::from(tokens) <= u64::from(self.budget) {
            self.reservations.push_back((now, tokens));
            return Ok(None);
        }
        Ok(self
            .reservations
            .front()
            .map(|(at, _)| window.saturating_sub(now.duration_since(*at))))
    }
}

pub(crate) struct Client {
    http: reqwest::Client,
    credential: String,
    active: Mutex<usize>,
    changed: Condvar,
    concurrency: usize,
    tokens: Mutex<TokenWindow>,
}

struct Permit<'a>(&'a Client);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            *active -= 1;
            self.0.changed.notify_one();
        }
    }
}

struct QuotaTicket<'a> {
    client: &'a Client,
    ticket: u64,
}
impl Drop for QuotaTicket<'_> {
    fn drop(&mut self) {
        if let Ok(mut tokens) = self.client.tokens.lock() {
            tokens.cancel(self.ticket);
        }
    }
}

/// Both the exact HTTP request and response are retained without auth headers.
/// A failed branch is retained too; it is never replaced with a fabricated output.
pub(crate) struct Attempt {
    pub result: Result<NativeGenerationResult, String>,
    pub receipt: Value,
}

impl Client {
    pub(crate) fn from_file(path: &Path, concurrency: usize) -> Result<Self, String> {
        if !(1..=4).contains(&concurrency) {
            return Err("Hosted concurrency must be between one and four".into());
        }
        let metadata =
            fs::symlink_metadata(path).map_err(|_| "Cannot read hosted credential file")?;
        if !metadata.is_file() || metadata.len() > 1024 {
            return Err("Hosted credential must be a small ordinary file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err("Hosted credential file must be private to its owner (0600)".into());
            }
        }
        let credential =
            fs::read_to_string(path).map_err(|_| "Cannot read hosted credential file")?;
        let credential = credential.trim().to_owned();
        if credential.is_empty() || credential.chars().any(char::is_whitespace) {
            return Err("Hosted credential is empty or malformed".into());
        }
        let mut header =
            HeaderValue::from_str(&credential).map_err(|_| "Hosted credential is malformed")?;
        header.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", header);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|_| "Could not construct hosted HTTP client")?;
        let budget = std::env::var("HORARY_GOOGLE_INPUT_TPM")
            .ok()
            .map(|value| value.parse::<u32>())
            .transpose()
            .map_err(|_| "Invalid hosted input-token budget")?
            .unwrap_or(14_000);
        if budget == 0 {
            return Err("Hosted input-token budget must be positive".into());
        }
        Ok(Self {
            http,
            credential,
            active: Mutex::new(0),
            changed: Condvar::new(),
            concurrency,
            tokens: Mutex::new(TokenWindow::new(budget)),
        })
    }

    pub(crate) fn metadata(&self) -> Value {
        json!({"provider":"google_gemini_api","id":MODEL,"endpoint":ENDPOINT,
            "decoding":"hosted unconstrained text; prompt-specified contracts and native acceptance/repair unchanged",
            "thinking_level":"minimal","temperature":0,"seed":0,
            "http_concurrency":self.concurrency,"local_inference":false,
            "qualification":"Hosted exploration; does not qualify the eventual on-device 12B model",
            "credential_in_evidence":false,"automatic_http_retries":true,
            "retry_policy":"At most two retries of completed generation HTTP 500/502/503/504; every attempt retained and token-paced. No retries of uncertain transport, auth, quota, request or model-output errors",
            "input_tokens_per_61_seconds":self.tokens.lock().ok().map(|window|window.budget),
            "pacing":"Google countTokens before every generation; shared FIFO token-window admission; cancellations remove waiting tickets without refunding admitted reservations"})
    }

    fn permit(&self, cancelled: &AtomicBool) -> Result<Permit<'_>, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Hosted limiter unavailable")?;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("Hosted request cancelled before submission".into());
            }
            if *active < self.concurrency {
                *active += 1;
                return Ok(Permit(self));
            }
            active = self
                .changed
                .wait_timeout(active, Duration::from_millis(100))
                .map_err(|_| "Hosted limiter unavailable")?
                .0;
        }
    }

    fn redact(&self, text: &str) -> String {
        text.replace(&self.credential, "[REDACTED]")
    }

    fn post(
        &self,
        endpoint: &str,
        request: &Value,
        cancelled: &AtomicBool,
    ) -> Result<(u16, Option<String>, String), String> {
        tauri::async_runtime::block_on(async {
            let post = async {
                let response = self
                    .http
                    .post(endpoint)
                    .json(request)
                    .send()
                    .await
                    .map_err(|e| self.redact(&e.to_string()))?;
                let status = response.status().as_u16();
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .map(str::to_owned);
                let raw = response
                    .text()
                    .await
                    .map_err(|e| self.redact(&e.to_string()))?;
                Ok((status, retry_after, self.redact(&raw)))
            };
            tokio::select! {
                result=post=>result,
                _=async {while !cancelled.load(Ordering::Acquire) {tokio::time::sleep(Duration::from_millis(100)).await;}}=>Err("Hosted case deadline cancelled the HTTP request; no output claimed".into()),
            }
        })
    }

    fn pace(&self, tokens: u32, cancelled: &AtomicBool) -> Result<(), String> {
        let ticket = self
            .tokens
            .lock()
            .map_err(|_| "Hosted token limiter unavailable")?
            .enqueue(tokens)?;
        let _ticket = QuotaTicket {
            client: self,
            ticket,
        };
        let mut announced = false;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("Hosted request cancelled while waiting for token quota; no generation submitted".into());
            }
            let wait = self
                .tokens
                .lock()
                .map_err(|_| "Hosted token limiter unavailable")?
                .reserve_ticket(Instant::now(), ticket)?;
            let Some(wait) = wait else {
                return Ok(());
            };
            if !announced {
                println!(
                    "{}",
                    json!({"event":"hosted_token_wait","model":MODEL,"input_tokens":tokens,"initial_wait_ms":wait.as_millis(),"fifo_ticket":ticket})
                );
                announced = true;
            }
            std::thread::sleep(wait.min(Duration::from_millis(100)));
        }
    }

    fn wait_for_retry(&self, delay: Duration, cancelled: &AtomicBool) -> Result<(), String> {
        let until = Instant::now() + delay;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(
                    "Hosted case cancelled before the service retry; prior attempts retained"
                        .into(),
                );
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            std::thread::sleep(left.min(Duration::from_millis(100)));
        }
    }

    pub(crate) fn generate(
        &self,
        prompt: &str,
        schema: &Value,
        max_tokens: u32,
        cancelled: Arc<AtomicBool>,
    ) -> Attempt {
        let queued = Instant::now();
        let request = match wire_request(prompt, schema, max_tokens) {
            Ok(request) => request,
            Err(error) => {
                return Attempt {
                    result: Err(error.clone()),
                    receipt: json!({"submitted":false,"error":error}),
                }
            }
        };
        let permit = match self.permit(&cancelled) {
            Ok(permit) => permit,
            Err(error) => {
                return Attempt {
                    result: Err(error.clone()),
                    receipt: json!({"request":request,"submitted":false,"error":error}),
                }
            }
        };
        let mut count_request = request.clone();
        count_request["model"] = json!(format!("models/{MODEL}"));
        let count_request = json!({"generateContentRequest":count_request});
        let count_start = Instant::now();
        let count_response = self.post(COUNT_ENDPOINT, &count_request, &cancelled);
        let (tokens, count_receipt) = match count_response {
            Ok((status, retry_after, raw)) => {
                let body = serde_json::from_str::<Value>(&raw)
                    .unwrap_or_else(|_| json!({"invalid_json_body":raw}));
                let receipt = json!({"http_status":status,"retry_after":retry_after,"body":body,"wall_ms":count_start.elapsed().as_millis()});
                if status != 200 {
                    return Attempt {
                        result: Err(format!(
                            "Google token counting HTTP {status}; generation not submitted"
                        )),
                        receipt: json!({"request":request,"submitted":false,"token_count_response":receipt,"response":receipt}),
                    };
                }
                let Some(tokens) = body["totalTokens"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                else {
                    return Attempt {
                        result: Err(
                            "Google returned no valid input-token count; generation not submitted"
                                .into(),
                        ),
                        receipt: json!({"request":request,"submitted":false,"token_count_response":receipt}),
                    };
                };
                (tokens, receipt)
            }
            Err(error) => {
                return Attempt {
                    result: Err(error.clone()),
                    receipt: json!({"request":request,"submitted":false,"token_count_response":{"transport_error":error}}),
                }
            }
        };
        let mut queue_ms = 0;
        let mut http_wall_ms = 0;
        let attempts = retain_service_attempts(
            || {
                if let Err(error) = self.pace(tokens, &cancelled) {
                    return (false, Err(error));
                }
                queue_ms = queued.elapsed().as_millis().saturating_sub(http_wall_ms);
                let start = Instant::now();
                let response = self.post(ENDPOINT, &request, &cancelled);
                http_wall_ms += start.elapsed().as_millis();
                (true, response)
            },
            |delay| self.wait_for_retry(delay, &cancelled),
        );
        drop(permit);
        let submitted = attempts
            .receipts
            .iter()
            .any(|attempt| attempt["submitted"] == true);
        let (result, response) = match attempts.final_response {
            Ok((status, retry_after, raw)) => {
                let body = serde_json::from_str::<Value>(&raw)
                    .unwrap_or_else(|_| json!({"invalid_json_body":raw}));
                let result = if status != 200 {
                    Err(format!(
                        "Google hosted inference HTTP {status}: {}",
                        body["error"]["message"]
                            .as_str()
                            .unwrap_or("Unusable provider response")
                    ))
                } else {
                    decode_response(&body, http_wall_ms, queue_ms)
                };
                (
                    result,
                    json!({"http_status":status,"retry_after":retry_after,"body":body}),
                )
            }
            Err(error) => (Err(error.clone()), json!({"transport_error":error})),
        };
        let receipt = json!({"provider":"google_gemini_api","model":MODEL,"request":request,
            "submitted":submitted,"queue_ms":queue_ms,"http_wall_ms":http_wall_ms,"response":response,
            "generation_attempts":attempts.receipts,
            "token_count_response":count_receipt,"reserved_input_tokens":tokens,
            "native_result":result.as_ref().map_err(String::as_str)});
        Attempt { result, receipt }
    }
}

type HttpResult = Result<(u16, Option<String>, String), String>;
struct ServiceAttempts {
    final_response: HttpResult,
    receipts: Vec<Value>,
}

/// A completed service failure may be retried; an uncertain submission must not
/// be repeated. Format or worksheet rejection belongs to native semantic repair.
fn retain_service_attempts(
    mut submit: impl FnMut() -> (bool, HttpResult),
    mut wait: impl FnMut(Duration) -> Result<(), String>,
) -> ServiceAttempts {
    let mut receipts = Vec::new();
    loop {
        let started = Instant::now();
        let (submitted, response) = submit();
        let mut receipt = json!({"attempt":receipts.len()+1,"submitted":submitted,"wall_ms":started.elapsed().as_millis()});
        if let Ok((status, retry_after, raw)) = &response {
            receipt["http_status"] = json!(status);
            receipt["retry_after"] = json!(retry_after);
            receipt["body"] =
                serde_json::from_str(raw).unwrap_or_else(|_| json!({"invalid_json_body":raw}));
        } else if let Err(error) = &response {
            receipt["error"] = json!(error);
        }
        receipts.push(receipt);
        let retry = match &response {
            Ok((500 | 502 | 503 | 504, retry_after, _)) if receipts.len() < 3 => {
                let seconds = retry_after
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(2 * receipts.len() as u64)
                    .clamp(1, 60);
                Some(Duration::from_secs(seconds))
            }
            _ => None,
        };
        let Some(delay) = retry else {
            return ServiceAttempts {
                final_response: response,
                receipts,
            };
        };
        println!(
            "{}",
            json!({"event":"hosted_service_retry","completed_attempts":receipts.len(),"delay_ms":delay.as_millis(),"prior_http_status":receipts.last().and_then(|r|r["http_status"].as_u64())})
        );
        if let Err(error) = wait(delay) {
            return ServiceAttempts {
                final_response: Err(error),
                receipts,
            };
        }
    }
}

/// Preserve message roles and every repair turn. No expected values or rubric
/// are introduced when translating the app's prompt to Google's wire format.
fn wire_request(prompt: &str, schema: &Value, max_tokens: u32) -> Result<Value, String> {
    let messages: Value = serde_json::from_str(prompt).map_err(|e| e.to_string())?;
    let messages = messages
        .as_array()
        .ok_or("Hosted prompt must be a message array")?;
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for message in messages {
        let text = message["content"]
            .as_str()
            .ok_or("Hosted messages must have text content")?;
        match message["role"].as_str() {
            Some("system") if contents.is_empty() => system.push(json!({"text":text})),
            Some("user") => contents.push(json!({"role":"user","parts":[{"text":text}]})),
            Some("assistant") => contents.push(json!({"role":"model","parts":[{"text":text}]})),
            _ => return Err("Unsupported or misplaced message role in hosted request".into()),
        }
    }
    if system.is_empty() || contents.is_empty() || max_tokens == 0 || !schema.is_object() {
        return Err("Hosted request needs teaching, user input and a typed output contract".into());
    }
    Ok(
        json!({"systemInstruction":{"parts":system},"contents":contents,
        "generationConfig":{"temperature":0,"seed":0,"maxOutputTokens":max_tokens,
            "thinkingConfig":{"thinkingLevel":"minimal"}}}),
    )
}

fn decode_response(
    body: &Value,
    wall_ms: u128,
    queue_ms: u128,
) -> Result<NativeGenerationResult, String> {
    let candidates = body["candidates"]
        .as_array()
        .ok_or("Hosted response has no candidates")?;
    if candidates.len() != 1
        || !matches!(
            candidates[0]["finishReason"].as_str(),
            Some("STOP" | "MAX_TOKENS")
        )
    {
        return Err(format!(
            "Hosted response did not deliver one complete candidate (finish reason: {})",
            candidates
                .first()
                .map(|c| &c["finishReason"])
                .unwrap_or(&Value::Null)
        ));
    }
    let parts = candidates[0]["content"]["parts"]
        .as_array()
        .ok_or("Hosted response has no text parts")?;
    let mut content = String::new();
    for part in parts {
        if part["thought"] == true {
            continue;
        }
        content.push_str(
            part["text"]
                .as_str()
                .ok_or("Hosted response contains a non-text output")?,
        );
    }
    if content.trim().is_empty() {
        return Err("Hosted response contains no answer".into());
    }
    let tokens = |field: &str| -> Result<u32, String> {
        body["usageMetadata"][field]
            .as_u64()
            .unwrap_or(0)
            .try_into()
            .map_err(|_| "Provider token count overflow".into())
    };
    let prompt_tokens = tokens("promptTokenCount")?;
    let generated_tokens = tokens("candidatesTokenCount")?;
    let cached_prompt_tokens = tokens("cachedContentTokenCount")?;
    Ok(NativeGenerationResult {
        content,
        prompt_tokens,
        generated_tokens,
        elapsed_ms: wall_ms,
        total_wall_ms: Some(wall_ms + queue_ms),
        batch_size: None,
        lesson_bank_hit: None,
        lesson_prepare_ms: None,
        tokens_per_second: if wall_ms > 0 {
            generated_tokens as f64 * 1000. / wall_ms as f64
        } else {
            0.
        },
        prompt_cache_hit: cached_prompt_tokens > 0,
        cached_prompt_tokens,
        prefilled_prompt_tokens: prompt_tokens.saturating_sub(cached_prompt_tokens),
        first_token_ms: None,
        cold_cache_bytes: None,
    })
}

#[test]
fn hosted_translation_preserves_teaching_rejected_proposal_and_repair() {
    let messages = json!([{"role":"system","content":"Teaching"},{"role":"user","content":"Question"},{"role":"assistant","content":"Rejected proposal"},{"role":"user","content":"Native repair"}]);
    let schema = json!({"type":"object"});
    let wire = wire_request(&messages.to_string(), &schema, 1000).unwrap();
    assert_eq!(wire["systemInstruction"]["parts"][0]["text"], "Teaching");
    assert_eq!(wire["contents"][1]["role"], "model");
    assert_eq!(wire["contents"][2]["parts"][0]["text"], "Native repair");
    assert!(wire["generationConfig"].get("responseJsonSchema").is_none());
    assert!(wire["generationConfig"].get("responseMimeType").is_none());
    assert!(wire.get("key").is_none());
}

#[test]
fn hosted_truncation_reaches_native_repair_and_usage_does_not_invent_caching() {
    let mut body = json!({"candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"{\"intent\":"}]}}],"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":2}});
    let partial = decode_response(&body, 100, 5).unwrap();
    assert_eq!(partial.content, "{\"intent\":");
    assert!(crate::horary_contract::decode_json(&partial.content).is_err());
    // Output is preserved verbatim for production format validation, not made
    // into a service failure that bypasses its repair path.
    body["candidates"][0]["finishReason"] = json!("STOP");
    body["candidates"][0]["content"]["parts"][0]["text"] = json!("{}");
    let output = decode_response(&body, 100, 5).unwrap();
    assert_eq!(output.prefilled_prompt_tokens, 100);
    assert!(!output.prompt_cache_hit);
    assert_eq!(output.lesson_bank_hit, None);
}

#[test]
fn token_pacing_reserves_parallel_calls_and_releases_only_expired_windows() {
    let start = Instant::now();
    let mut window = TokenWindow::new(14_000);
    assert_eq!(window.reserve(start, 4_000).unwrap(), None);
    assert_eq!(window.reserve(start, 7_000).unwrap(), None);
    assert_eq!(
        window.reserve(start, 4_000).unwrap(),
        Some(Duration::from_secs(61))
    );
    assert_eq!(
        window
            .reserve(start + Duration::from_secs(60), 4_000)
            .unwrap(),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        window
            .reserve(start + Duration::from_secs(61), 4_000)
            .unwrap(),
        None
    );
    assert!(window
        .reserve(start + Duration::from_secs(61), 14_001)
        .is_err());
}

#[test]
fn fifo_quota_stops_small_repairs_starving_an_older_large_request() {
    let start = Instant::now();
    let mut window = TokenWindow::new(14_000);
    let first = window.enqueue(7_000).unwrap();
    assert_eq!(window.reserve_ticket(start, first).unwrap(), None);
    let large = window.enqueue(12_000).unwrap();
    let small = window.enqueue(4_000).unwrap();
    assert!(window.reserve_ticket(start, large).unwrap().is_some());
    // A small repair fits now, but may not jump ahead and perpetuate the wait.
    assert!(window.reserve_ticket(start, small).unwrap().is_some());
    let release = start + Duration::from_secs(61);
    assert!(window.reserve_ticket(release, small).unwrap().is_some());
    assert_eq!(window.reserve_ticket(release, large).unwrap(), None);
    assert!(window.reserve_ticket(release, small).unwrap().is_some());
    assert_eq!(
        window
            .reserve_ticket(release + Duration::from_secs(61), small)
            .unwrap(),
        None
    );
}

#[test]
fn cancelled_quota_waiter_does_not_block_peers_or_refund_admitted_tokens() {
    let start = Instant::now();
    let mut window = TokenWindow::new(14_000);
    let admitted = window.enqueue(7_000).unwrap();
    assert_eq!(window.reserve_ticket(start, admitted).unwrap(), None);
    let cancelled = window.enqueue(12_000).unwrap();
    let next = window.enqueue(4_000).unwrap();
    window.cancel(cancelled);
    assert_eq!(window.reserve_ticket(start, next).unwrap(), None);
    window.cancel(admitted);
    window.cancel(next);
    assert_eq!(window.reservations.len(), 2);
    let last = window.enqueue(4_000).unwrap();
    assert!(window.reserve_ticket(start, last).unwrap().is_some());
    assert!(window.reserve_ticket(start, cancelled).is_err());
    assert!(window.enqueue(14_001).is_err());
    assert_eq!(window.waiting.len(), 1);
}

#[test]
fn fifo_quota_keeps_parallel_admission_when_capacity_is_available() {
    let start = Instant::now();
    let mut window = TokenWindow::new(14_000);
    let tickets = [3_000, 4_000, 3_000, 4_000].map(|tokens| window.enqueue(tokens).unwrap());
    for ticket in tickets {
        assert_eq!(window.reserve_ticket(start, ticket).unwrap(), None);
    }
    assert!(window.waiting.is_empty());
    assert_eq!(window.reservations.len(), 4);
    let retry = window.enqueue(1_000).unwrap();
    assert!(window.reserve_ticket(start, retry).unwrap().is_some());
    assert_eq!(
        window
            .reserve_ticket(start + Duration::from_secs(61), retry)
            .unwrap(),
        None
    );
}

#[test]
fn completed_service_recovery_retains_failures_without_retrying_model_or_uncertain_output() {
    let mut responses = std::collections::VecDeque::from([
        Ok((
            500,
            None,
            r#"{"error":{"message":"Internal error"}}"#.into(),
        )),
        Ok((
            503,
            Some("7".into()),
            r#"{"error":{"message":"Unavailable"}}"#.into(),
        )),
        Ok((200, None, r#"{"candidates":[]}"#.into())),
    ]);
    let mut delays = Vec::new();
    let attempts = retain_service_attempts(
        || (true, responses.pop_front().unwrap()),
        |d| {
            delays.push(d);
            Ok(())
        },
    );
    assert_eq!(
        attempts
            .receipts
            .iter()
            .map(|r| r["http_status"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![500, 503, 200]
    );
    assert_eq!(delays, vec![Duration::from_secs(2), Duration::from_secs(7)]);
    let body: Value = serde_json::from_str(&attempts.final_response.unwrap().2).unwrap();
    assert!(decode_response(&body, 1, 0).is_err());
    for response in [
        Ok((400, None, "{}".into())),
        Ok((429, None, "{}".into())),
        Err("Submission outcome uncertain".into()),
    ] {
        let attempts = retain_service_attempts(
            || (true, response.clone()),
            |_| panic!("Uncertain or permanent failure cannot be resubmitted"),
        );
        assert_eq!(attempts.receipts.len(), 1);
    }
    let cancelled = retain_service_attempts(
        || (true, Ok((500, None, "{}".into()))),
        |_| Err("Cancelled".into()),
    );
    assert_eq!(cancelled.receipts.len(), 1);
    assert!(cancelled.final_response.is_err());
    let exhausted = retain_service_attempts(|| (true, Ok((503, None, "{}".into()))), |_| Ok(()));
    assert_eq!(exhausted.receipts.len(), 3);
    assert_eq!(exhausted.final_response.unwrap().0, 503);
}
