//! Bounded reflection transport around stock GEPA, not a search algorithm.
//! Received overlong proposals remain recorded; only admitted instructions reach students.
use crate::{codex::Codex, functions::Fixture, google, preflight};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Predict,
    lm::{
        Capabilities, DynChatModel,
        api::{LmMessage, LmOutput, LmPart, LmRequest, LmResponse},
    },
};
use serde_json::json;
use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};

const CONTRACT: &str = "Return one replacement instruction in a single plain fenced block. HARD LIMIT: at most 250 whitespace-separated words AND 1800 UTF-8 bytes inside the fence. The student already receives the family procedure, chart dictionary and worked example; do not copy those inputs, restate the full book, hardcode an example's actors/eIDs/verdict, or change output fields. Spend the small instruction on generalizable corrections supported by the supplied TRAIN feedback. No commentary outside the fence.";

pub struct CompactTeacher {
    inner: Arc<Codex>,
    predictor: Predict,
    fixtures: Vec<Fixture>,
    directory: PathBuf,
    count_http: reqwest::Client,
}
impl CompactTeacher {
    pub fn new(
        inner: Arc<Codex>,
        predictor: Predict,
        fixtures: Vec<Fixture>,
        directory: PathBuf,
        count_http: reqwest::Client,
    ) -> Self {
        Self {
            inner,
            predictor,
            fixtures,
            directory,
            count_http,
        }
    }
    fn instruction(raw: &str) -> &str {
        let raw = raw.trim();
        if let Some(open) = raw.find("```")
            && let Some(end) = raw.rfind("```").filter(|end| *end > open)
        {
            let body = raw[open + 3..end].trim();
            return body
                .strip_prefix("text\n")
                .or_else(|| body.strip_prefix("markdown\n"))
                .unwrap_or(body)
                .trim();
        }
        raw
    }
    fn small(text: &str) -> bool {
        !text.is_empty() && text.len() <= 1800 && text.split_whitespace().count() <= 250
    }
    async fn admit(
        &self,
        _http: &reqwest::Client,
        instruction: &str,
        directory: &std::path::Path,
    ) -> Result<bool> {
        let mut predictor = Predict::from_signature(self.predictor.signature.clone())
            .with_config(self.predictor.config().clone());
        predictor.demos = self.predictor.demos.clone();
        predictor.signature.instructions = instruction.to_owned();
        let requests = preflight::reader_requests(&self.fixtures, &predictor)?;
        ensure!(
            requests.len() == 20,
            "Rewrite admission requires all twenty original fixtures"
        );
        std::fs::create_dir_all(directory)?;
        crate::queue::write_json_atomic(&directory.join("requests.json"), &json!(requests))?;
        let mut counts = Vec::new();
        for (index, row) in requests.iter().enumerate() {
            let mut request: LmRequest = serde_json::from_value(row["request"].clone())?;
            request.model = google::STUDENT.into();
            let result = google::count_tokens(
                &self.count_http,
                &request,
                &directory.join(format!("count-{index:02}")),
            )
            .await?;
            let count = result["input_tokens"]
                .as_u64()
                .context("No exact rewrite count")?;
            counts.push(json!({"case_id":row["case_id"],"tokens":count,"fits":count <= crate::gold::STUDENT_INPUT_LIMIT as u64}));
        }
        let fits = counts.iter().all(|count| count["fits"] == true);
        crate::queue::write_json_atomic(
            &directory.join("admission.json"),
            &json!({"counts":counts,"fits":fits,"instruction_bytes":instruction.len(),"instruction_words":instruction.split_whitespace().count(),"generation_submitted":false}),
        )?;
        Ok(fits)
    }
    fn compression_request(instruction: &str) -> LmRequest {
        LmRequest::new(
            "gepa-reflection",
            vec![LmMessage::user(vec![LmPart::text(format!(
                "{CONTRACT}\n\nCompress this fully received proposal without adding claims. Preserve its useful generalizable corrections and uncertainty. This is a mechanical instruction-budget repair, not a new reflection or an answer to the underlying example.\n\nPROPOSAL:\n{instruction}"
            ))])],
        )
    }
    async fn request(&self, http: &reqwest::Client, request: &LmRequest) -> Result<LmResponse> {
        if request.model != "gepa-reflection" {
            return self.inner.forward_dyn(http, request).await;
        }
        let key = google::digest(serde_json::to_vec(request)?);
        let directory = self.directory.join(key);
        std::fs::create_dir_all(&directory)?;
        // Exact previously paid reflection is reused before adding any new instruction.
        let mut response = if let Some(response) = self.inner.cached_response(request)? {
            response
        } else {
            let mut bounded = request.clone();
            bounded
                .messages
                .push(LmMessage::user(vec![LmPart::text(CONTRACT)]));
            self.inner.forward_dyn(http, &bounded).await?
        };
        for attempt in 0..=2 {
            let raw = response.first_text();
            let instruction = Self::instruction(&raw);
            crate::queue::write_json_atomic(
                &directory.join(format!("proposal-{attempt}.json")),
                &json!({"raw":raw,"cache_hit":response.cache_hit,"small":Self::small(instruction),"semantic_score":null}),
            )?;
            if Self::small(instruction)
                && self
                    .admit(
                        http,
                        instruction,
                        &directory.join(format!("admission-{attempt}")),
                    )
                    .await?
            {
                return Ok(LmResponse {
                    outputs: vec![LmOutput::text(format!("```\n{instruction}\n```"))],
                    ..response
                });
            }
            if attempt < 2 {
                let compression = Self::compression_request(instruction);
                response = self.inner.forward_dyn(http, &compression).await?;
            }
        }
        // An observed inadmissible proposal is a rejected mutation, not an unobserved
        // student failure. Returning the current instruction leaves stock GEPA unchanged.
        let prompt = request
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter_map(LmPart::as_text)
            .collect::<String>();
        let current = prompt
            .split("```")
            .nth(1)
            .context("No stock GEPA current instruction")?
            .trim();
        ensure!(
            self.admit(http, current, &directory.join("retained-current"))
                .await?,
            "Current instruction no longer fits original wires"
        );
        crate::queue::write_json_atomic(
            &directory.join("rejected.json"),
            &json!({"reason":"Received rewrite exceeds instruction/wire budget after two bounded compressions","current_instruction_retained":true,"semantic_zero":false,"student_generation_submitted":false}),
        )?;
        Ok(LmResponse {
            outputs: vec![LmOutput::text(format!("```\n{current}\n```"))],
            ..Default::default()
        })
    }
}
impl DynChatModel for CompactTeacher {
    fn forward_dyn<'a>(
        &'a self,
        http: &'a reqwest::Client,
        request: &'a LmRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LmResponse>> + Send + 'a>> {
        Box::pin(self.request(http, request))
    }
    fn native_reasoning_usable_dyn(&self) -> bool {
        self.inner.native_reasoning_usable_dyn()
    }
    fn capabilities_dyn<'a>(
        &'a self,
        http: &'a reqwest::Client,
    ) -> Pin<Box<dyn Future<Output = Capabilities> + Send + 'a>> {
        self.inner.capabilities_dyn(http)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_original_size_and_dense_byte_overflow() {
        assert!(!CompactTeacher::small(&"word ".repeat(1227)));
        assert!(!CompactTeacher::small(&"x".repeat(1801)));
        assert!(CompactTeacher::small(&"word ".repeat(250)));
        assert_eq!(
            CompactTeacher::instruction("```text\nKeep directions exact.\n```"),
            "Keep directions exact."
        );
    }
    #[tokio::test]
    async fn completed_oversized_rewrite_is_reused_and_compressed_before_all_twenty_wires() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("teacher-not-executable");
        std::fs::write(&executable, b"Synthetic public identity").unwrap();
        let budget = google::CallBudget::seeded(80, 15).unwrap();
        let teacher = Arc::new(
            Codex::new(executable.clone(), dir.path().join("teacher"), 80)
                .unwrap()
                .with_budget(budget.clone())
                .with_completed_cache_only(),
        );
        let cache = |request: &LmRequest, answer: &str| {
            let sha = google::digest(std::fs::read(&executable).unwrap());
            let key = google::digest(format!(
                "codex-login-text-v1\0{sha}\0{}",
                request.cache_key("codex-saved-login")
            ));
            let path = dir.path().join("teacher").join(key);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("response.json"),
                serde_json::to_vec(&LmResponse {
                    outputs: vec![LmOutput::text(answer)],
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
        };
        let original = LmRequest::new(
            "gepa-reflection",
            vec![LmMessage::user(vec![LmPart::text(
                "Current instruction:\n```\nOriginal concise instruction.\n```\nTRAIN feedback only.",
            )])],
        );
        let long = "corrective ".repeat(1227);
        cache(&original, &format!("```\n{long}\n```"));
        let short = "Preserve the actual roles and reception direction. Distinguish ability, inclination and occasion. Answer the requested horizon using supplied evidence; retain material uncertainty.";
        cache(
            &CompactTeacher::compression_request(long.trim()),
            &format!("```\n{short}\n```"),
        );
        let predictor = crate::optimize::plain(
            crate::signatures::reading("contact").unwrap(),
            teacher.clone(),
        );
        let fixtures=(0..20).map(|i|Fixture{split:if i<12{"train"}else if i<16{"dev"}else{"test"}.into(),example:dsrust::example!{case_id:format!("synthetic-{i}"),source_text:"Salient procedure.",question:"Will my mother call?",consultation:"Accepted mother role.",chart_evidence:"e1|native condition"}.with_inputs(["source_text","question","consultation","chart_evidence"])}).collect();
        let model = CompactTeacher::new(
            teacher,
            predictor,
            fixtures,
            dir.path().join("admission"),
            reqwest::Client::new(),
        );
        let key = google::digest(serde_json::to_vec(&original).unwrap());
        let mut predictor = Predict::from_signature(model.predictor.signature.clone())
            .with_config(model.predictor.config().clone());
        predictor.signature.instructions = short.into();
        for (i, row) in preflight::reader_requests(&model.fixtures, &predictor)
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut request: LmRequest = serde_json::from_value(row["request"].clone()).unwrap();
            request.model = google::STUDENT.into();
            let path = model
                .directory
                .join(&key)
                .join("admission-1")
                .join(format!("count-{i:02}"));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(format!("{}.json", request.cache_key(google::STUDENT))),
                serde_json::to_vec(&json!({"http_status":200,"input_tokens":5000})).unwrap(),
            )
            .unwrap();
        }
        let response = model
            .forward_dyn(&reqwest::Client::new(), &original)
            .await
            .unwrap();
        assert_eq!(CompactTeacher::instruction(&response.first_text()), short);
        assert_eq!(budget.spent(), 15);
        let proof: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                model
                    .directory
                    .join(&key)
                    .join("admission-1/admission.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(proof["counts"].as_array().unwrap().len(), 20);
        assert_eq!(proof["fits"], true);
        assert!(!model.directory.join(&key).join("admission-0").exists());
        assert!(
            std::fs::read_to_string(model.directory.join(&key).join("proposal-0.json"))
                .unwrap()
                .contains("corrective")
        );
    }
}
