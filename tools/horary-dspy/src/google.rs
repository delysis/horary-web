//! Provider transport only. DsRust owns prompt rendering and optimizer execution.
use anyhow::{Context, Result, bail, ensure};
use dsrust::lm::{
    Capabilities, ChatModel,
    api::{LmOutput, LmRequest, LmResponse},
};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const TEACHER: &str = "gemini-3.8-flash";
pub const STUDENT: &str = "gemma-4-26b-a4b-it";
pub const CREDENTIAL_FILE: &str = "/Users/george/.codex/credentials/horary-google-api.key";
const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const MAX_SUBMISSIONS: usize = 3;

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
    let response = http
        .get(format!("{API}/models?pageSize=1000"))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Google model listing HTTP {}",
        response.status()
    );
    let body: Value = response.json().await?;
    let rows = body["models"].as_array().context("No Google model list")?;
    let select = |id: &str| {
        rows.iter().find(|m| m["name"] == format!("models/{id}")).map(|m| json!({"name":m["name"],"supportedGenerationMethods":m["supportedGenerationMethods"],"inputTokenLimit":m["inputTokenLimit"],"outputTokenLimit":m["outputTokenLimit"]}))
    };
    Ok(
        json!({"teacher":select(TEACHER),"student":select(STUDENT),"next_page":body["nextPageToken"]}),
    )
}

/// A shared rolling window, rather than an arbitrary pause between each function.
pub struct Google {
    model: &'static str,
    http: reqwest::Client,
    directory: PathBuf,
    spent: AtomicUsize,
    max_calls: usize,
    failed: AtomicBool,
    window: Mutex<VecDeque<(Instant, usize)>>,
    tokens_per_minute: usize,
    serial: Mutex<()>,
    // Fixed in production; credential-free unit tests substitute a local server.
    api: String,
}

impl Google {
    pub fn new(
        model: &'static str,
        http: reqwest::Client,
        directory: PathBuf,
        max_calls: usize,
        tokens_per_minute: usize,
    ) -> Result<Self> {
        ensure!(
            matches!(model, TEACHER | STUDENT),
            "Unapproved model identity"
        );
        fs::create_dir_all(&directory)?;
        Ok(Self {
            model,
            http,
            directory,
            spent: AtomicUsize::new(0),
            max_calls,
            failed: AtomicBool::new(false),
            window: Mutex::new(VecDeque::new()),
            tokens_per_minute,
            serial: Mutex::new(()),
            api: API.into(),
        })
    }
    async fn pace(&self, estimate: usize) -> Result<()> {
        ensure!(
            estimate <= self.tokens_per_minute,
            "One prompt exceeds the configured per-minute token allowance; shorten demonstrations or source selection"
        );
        loop {
            let mut window = self.window.lock().await;
            while window
                .front()
                .is_some_and(|(at, _)| at.elapsed() >= Duration::from_secs(61))
            {
                window.pop_front();
            }
            if window.iter().map(|(_, n)| n).sum::<usize>() + estimate <= self.tokens_per_minute {
                window.push_back((Instant::now(), estimate));
                return Ok(());
            }
            let delay = Duration::from_secs(61)
                .saturating_sub(window.front().context("Empty quota window")?.0.elapsed());
            drop(window);
            tokio::time::sleep(delay).await;
        }
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
        let endpoint = if self.model == TEACHER {
            format!("{}/interactions", self.api)
        } else {
            format!("{}/models/{}:generateContent", self.api, self.model)
        };
        let mut delay = Duration::ZERO;
        for attempt in 1..=MAX_SUBMISSIONS {
            ensure!(
                self.spent.load(Ordering::Relaxed) < self.max_calls,
                "Configured physical generation-call budget exhausted"
            );
            tokio::time::sleep(delay).await;
            self.pace(tokens).await?;
            let file = |suffix: &str| {
                self.directory
                    .join(format!("{key}.attempt-{attempt}.{suffix}"))
            };
            let physical_call = self.spent.load(Ordering::Relaxed) + 1;
            write_new(
                &file("request.json"),
                &serde_json::to_vec_pretty(&json!({"logical_key":key,"attempt":attempt,
                    "physical_call":physical_call,"model":self.model,"request_sha256":request_sha256,
                    "body":body,"body_sha256":digest(serde_json::to_vec(body)?),
                    "input_tokens":tokens,"delay_before_ms":delay.as_millis()}))?,
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
                    return Err(error).context("Google transport is unsettled; no retry");
                }
            };
            let status = response.status();
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
                        return Err(error).context("Google response is incomplete; no retry");
                    }
                }
            }
            write_new(&file("body"), &bytes)?;
            let next_delay = retry_delay(
                status.as_u16(),
                true,
                attempt,
                self.max_calls
                    .saturating_sub(self.spent.load(Ordering::Relaxed)),
            );
            write_new(
                &file("result.json"),
                &serde_json::to_vec_pretty(&json!({
                    "logical_key":key,"attempt":attempt,"physical_call":physical_call,
                    "http_status":status.as_u16(),"response_complete":true,
                    "body_sha256":digest(&bytes),"automatic_retry":next_delay.is_some(),
                    "retry_delay_ms":next_delay.map(|value|value.as_millis()),
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
                    "automatic_retry":false}))?,
            )?;
            bail!(
                "Google {} HTTP {}; stopped after {attempt} physical submissions",
                self.model,
                status
            );
        }
        bail!("No Google response after bounded physical submissions")
    }
    async fn request(&self, request: &LmRequest) -> Result<LmResponse> {
        ensure!(
            !self.failed.load(Ordering::Relaxed),
            "A prior transport/budget failure withholds further provider work"
        );
        ensure!(
            request.output_schema().is_none() && request.tools.is_empty(),
            "This experiment permits unconstrained text only"
        );
        let mut routed = request.clone();
        routed.model = self.model.into();
        let key = if self.model == TEACHER {
            digest(format!(
                "interactions-stateless-v1\0{}",
                routed.cache_key(self.model)
            ))
        } else {
            routed.cache_key(self.model)
        };
        let _guard = self.serial.lock().await;
        ensure!(
            !self.failed.load(Ordering::Relaxed),
            "A prior transport/budget failure withholds further provider work"
        );
        let cached = self.directory.join(format!("{key}.response.json"));
        if cached.exists() {
            let mut result: LmResponse = serde_json::from_slice(&fs::read(cached)?)?;
            result.cache_hit = true;
            return Ok(result);
        }
        let started = self.directory.join(format!("{key}.request.json"));
        ensure!(
            !started.exists(),
            "A previous Google request is unresolved or failed; it is preserved and will not be replayed"
        );
        ensure!(
            self.spent.load(Ordering::Relaxed) < self.max_calls,
            "Configured physical generation-call budget exhausted"
        );
        let wire = routed.wire_messages();
        let contents = wire.iter().filter(|m| m["role"] != "system").map(|m| {
            let text = m["content"].as_str().context("Nontext input not allowed in this curriculum")?;
            Ok(json!({"role":if m["role"] == "assistant" {"model"} else {"user"},"parts":[{"text":text}]}))
        }).collect::<Result<Vec<_>>>()?;
        let legacy_body = json!({"systemInstruction":{"parts":[{"text":routed.system()}]},"contents":contents,"generationConfig":{"temperature":routed.config.temperature.unwrap_or(0.0),"maxOutputTokens":routed.config.max_tokens.unwrap_or(4096),"thinkingConfig":{"thinkingLevel":"MINIMAL","includeThoughts":false}}});
        let body = if self.model == TEACHER {
            json!({"model":self.model,"store":false,"system_instruction":routed.system(),"input":contents.iter().map(|m|json!({"type":if m["role"]=="model"{"model_output"}else{"user_input"},"content":[{"type":"text","text":m["parts"][0]["text"]}]})).collect::<Vec<_>>(),"generation_config":{"temperature":routed.config.temperature.unwrap_or(0.),"max_output_tokens":routed.config.max_tokens.unwrap_or(4096),"thinking_level":"low"}})
        } else {
            legacy_body.clone()
        };
        let counted = self.directory.join(format!("{key}.tokens.json"));
        let count: Value = if counted.exists() {
            serde_json::from_slice(&fs::read(&counted)?)?
        } else {
            let response=self.http.post(format!("{}/models/{}:countTokens",self.api,self.model)).json(&json!({"generateContentRequest":{"model":format!("models/{}",self.model),"contents":legacy_body["contents"],"systemInstruction":legacy_body["systemInstruction"]}})).send().await?;
            ensure!(
                response.status().is_success(),
                "Google token count HTTP {}; no generation submitted",
                response.status()
            );
            let value: Value = response.json().await?;
            fs::write(&counted, serde_json::to_vec_pretty(&value)?)?;
            value
        };
        let tokens = count["totalTokens"]
            .as_u64()
            .context("No counted input tokens")? as usize;
        ensure!(
            tokens <= self.tokens_per_minute,
            "One prompt exceeds the configured per-minute token allowance; shorten demonstrations or source selection"
        );
        let saved_request =
            serde_json::to_vec_pretty(&json!({"model":self.model,"request":routed,"body":body}))?;
        write_new(&started, &saved_request)?;
        let begun = Instant::now();
        let (bytes, attempts) = self
            .generate(&key, &body, tokens, &digest(saved_request))
            .await?;
        let raw: Value = serde_json::from_slice(&bytes)?;
        write_new(
            &self.directory.join(format!("{key}.raw.json")),
            &serde_json::to_vec_pretty(&raw)?,
        )?;
        let outputs = if self.model == TEACHER {
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
        let result = LmResponse {
            model: Some(self.model.into()),
            outputs,
            response_id: raw[if self.model == TEACHER {
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
                &json!({"model":self.model,"api":if self.model==TEACHER{"interactions-stateless"}else{"generateContent"},"request_sha256":digest(fs::read(started)?),"elapsed_ms":begun.elapsed().as_millis(),"usage":raw[if self.model==TEACHER{"usage"}else{"usageMetadata"}],"response_schema":false,"physical_submissions":attempts}),
            )?,
        )?;
        write_new(&cached, &serde_json::to_vec_pretty(&result)?)?;
        Ok(result)
    }
}

impl ChatModel for Google {
    async fn forward(&self, _http: &reqwest::Client, request: &LmRequest) -> Result<LmResponse> {
        let result = self.request(request).await;
        if result.is_err() {
            self.failed.store(true, Ordering::Relaxed);
        }
        result
    }
    async fn capabilities(&self, _http: &reqwest::Client) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsrust::lm::api::{LmMessage, LmPart};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex as StdMutex},
        thread,
    };

    const SUCCESS: &str = r#"{"candidates":[{"content":{"parts":[{"text":"Observed response"}]},"finishReason":"STOP"}]}"#;

    enum Reply {
        Complete(u16, &'static str),
        Disconnect,
        Partial(u16, &'static str),
    }

    struct Mock {
        api: String,
        requests: Arc<StdMutex<Vec<Value>>>,
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
            .unwrap();
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        json!({"path":header.split_whitespace().nth(1).unwrap(),
            "body":serde_json::from_slice::<Value>(&body).unwrap(),"body_sha256":digest(&body)})
    }

    impl Mock {
        fn new(replies: Vec<Reply>) -> Self {
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
                .window
                .lock()
                .await
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
        assert!(model.failed.load(Ordering::Relaxed));
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
        assert!(
            model
                .forward(&model.http, &prompt("Other work"))
                .await
                .unwrap_err()
                .to_string()
                .contains("withholds further")
        );
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn every_retry_consumes_the_shared_token_window() {
        let mock = Mock::new(vec![
            Reply::Complete(429, "Limited"),
            Reply::Complete(200, SUCCESS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let request = prompt("First logical request");
        counted(dir.path(), &request, 7000);
        let model = mock.model(dir.path(), 3, 14_000);
        model.forward(&model.http, &request).await.unwrap();
        assert_eq!(
            model
                .window
                .lock()
                .await
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
    async fn oversized_prompt_or_zero_physical_budget_submits_nothing() {
        for (cap, tokens) in [(0, 100), (3, 14_001)] {
            let mock = Mock::new(vec![]);
            let dir = tempfile::tempdir().unwrap();
            let request = prompt("Preflight failure");
            counted(dir.path(), &request, tokens);
            let model = mock.model(dir.path(), cap, 14_000);
            assert!(model.forward(&model.http, &request).await.is_err());
            assert_eq!(model.spent.load(Ordering::Relaxed), 0);
            assert!(mock.requests.lock().unwrap().is_empty());
            assert!(model.window.lock().await.is_empty());
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
}
