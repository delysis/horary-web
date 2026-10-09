//! GEPA owns the search. Horary owns prompts, truth, validation and receipts.
#![forbid(unsafe_code)]
pub mod journal;
pub mod metric;
pub mod native;
pub mod review;
pub mod teacher;

use gepa::{Candidate, EvalBatch, GepaAdapter, GepaEngine};
use horary_prompt_program::{digest, guide_parts, Evidence, Override, Program};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;
pub const ENGINE_REV: &str = "f24adde08c1d8850e4d7079d019643bb40f905cb";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, clap::ValueEnum, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    ArchivedCapture,
    FreshHosted,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, clap::ValueEnum, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Function {
    #[default]
    Classification,
    InputJourney,
}
impl Function {
    fn is_classification(&self) -> bool {
        *self == Self::Classification
    }
    pub fn entry(self) -> &'static str {
        match self {
            Self::Classification => "elicitation_eval::real_model_classification_function",
            Self::InputJourney => "elicitation_eval::real_model_input_journey_function",
        }
    }
    pub fn phase(self) -> &'static str {
        match self {
            Self::Classification => "classify_question",
            Self::InputJourney => "complete_selected_program",
        }
    }
    pub fn metric(self) -> &'static str {
        match self {
            Self::Classification => "classification",
            Self::InputJourney => "input_journey",
        }
    }
    pub fn generation_factor(self) -> u64 {
        match self {
            Self::Classification => 3, // one branch × native client's three completed-error attempts
            Self::InputJourney => 6,   // conservatively reserve two input branches per group
        }
    }
    fn expectations(self, fixture: &Value) -> Value {
        match self {
            Self::Classification => {
                json!({"method":fixture["method"], "allowed_methods":fixture["expected"]["allowed_methods"],
                "facet":fixture["expected"]["facet"], "allowed_facets":fixture["expected"]["allowed_facets"]})
            }
            Self::InputJourney => {
                json!({"method":fixture["method"], "expected":fixture["expected"],
                "follow_up_expected":fixture["follow_up_expected"], "rationale":fixture["rationale"]})
            }
        }
    }
    fn verify_fixture(
        self,
        example: &Example,
        fixture: &Value,
        target: Option<&str>,
    ) -> Result<()> {
        if fixture["id"] != example.id
            || example.expected != self.expectations(fixture)
            || self == Self::InputJourney && fixture["method"].as_str() != target
        {
            return Err(format!(
                "Materialized expectations or target method differ from the frozen fixture for {}",
                example.id
            ));
        }
        Ok(())
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BookContext {
    pub file: PathBuf,
    pub sha256: String,
    pub ocr_pages: Vec<usize>,
}
impl BookContext {
    pub fn excerpts(&self) -> Result<Value> {
        verify(&self.file, &self.sha256)?;
        let bytes = read(&self.file)?;
        let source = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        if self.ocr_pages.is_empty()
            || self.ocr_pages.len() > 24
            || self.ocr_pages.iter().collect::<BTreeSet<_>>().len() != self.ocr_pages.len()
        {
            return Err(
                "Independent input review requires 1–24 distinct explicit OCR pages".into(),
            );
        }
        let mut excerpts = Vec::new();
        let mut total = 0;
        for page in &self.ocr_pages {
            let marker = format!("<!-- page:{page:04} -->");
            if source.matches(&marker).count() != 1 {
                return Err(format!("OCR page {page} has no unique source marker"));
            }
            let start = source.find(&marker).ok_or("Missing source marker")?;
            let tail = &source[start..];
            let end = tail.find("\n<!-- page:").unwrap_or(tail.len());
            let text = &tail[..end];
            total += text.len();
            excerpts.push(json!({"ocr_page":page,"source_byte_offset":start,"text":text,"sha256":digest(text)}));
        }
        if total > 80_000 {
            return Err("Independent source packet exceeds its 80KB bound".into());
        }
        Ok(
            json!({"source_file":self.file,"source_sha256":self.sha256,"page_numbering":"OCR page identifiers; printed page labels remain in the exact source text", "pages":excerpts}),
        )
    }
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}
pub fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&read(path)?).map_err(|error| error.to_string())
}
pub fn keep(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    if path.exists() {
        return if read(path)? == bytes {
            Ok(())
        } else {
            Err(format!("Immutable artifact changed: {}", path.display()))
        };
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}
pub fn verify(path: &Path, sha: &str) -> Result<()> {
    if digest(read(path)?) != sha {
        return Err(format!("Source changed: {}", path.display()));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub id: String,
    pub source_case_directory: PathBuf,
    pub source_request_file: PathBuf,
    pub source_request_sha256: String,
    pub source_initial_sha256: String,
    pub source_fixture_sha256: String,
    pub source_origin_sha256: String,
    pub inspection_file: PathBuf,
    pub inspection_sha256: String,
    pub expected: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub engine_rev: String,
    pub controller_executable: PathBuf,
    pub controller_executable_sha256: String,
    pub campaign: PathBuf,
    pub manifest_sha256: String,
    pub split_file: PathBuf,
    pub split_sha256: String,
    pub native_executable: PathBuf,
    pub native_executable_sha256: String,
    pub codex: PathBuf,
    pub codex_executable_sha256: String,
    pub guide: String,
    pub guide_sha256: String,
    pub seed: Candidate,
    pub training: Vec<Example>,
    pub development: Vec<Example>,
    pub reserved_ids: Vec<String>,
    pub max_metric_calls: usize,
    pub max_teacher_calls: u64,
    pub max_physical_generation_attempts: u64,
    pub logical_calls_per_function: u64,
    pub function_seconds: u64,
    pub teacher_seconds: u64,
    pub rng_seed: u64,
    pub wait_owner_pid: Option<u32>,
    pub control_mode: ControlMode,
    #[serde(default, skip_serializing_if = "Function::is_classification")]
    pub function: Function,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_method: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_review_calls: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub review_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_book: Option<BookContext>,
    pub qualification: String,
}

impl Plan {
    pub fn signature(&self) -> horary_prompt_program::Signature<'_> {
        horary_prompt_program::Signature {
            stage: "intake",
            recognition_phase: Some(self.function.phase()),
            method: self.target_method.as_deref(),
        }
    }
    fn region_key(&self, index: usize) -> String {
        let method = self
            .target_method
            .as_ref()
            .map(|m| format!(".{m}"))
            .unwrap_or_default();
        format!("{}{method}.region.{index:03}", self.function.phase())
    }
    fn seed_for_guide(&self) -> Result<Candidate> {
        Ok(guide_parts(&self.guide)?
            .teaching
            .into_iter()
            .enumerate()
            .filter(|(_, text)| text.trim().len() >= 64)
            .map(|(index, text)| (self.region_key(index), text.into()))
            .collect())
    }
    pub fn validate(&self) -> Result<()> {
        if self.version
            != if self.function == Function::Classification {
                1
            } else {
                2
            }
            || self.engine_rev != ENGINE_REV
            || self.max_metric_calls == 0
            || self.max_teacher_calls == 0
            || self.logical_calls_per_function == 0
            || self.function_seconds == 0
            || self.teacher_seconds == 0
            || self.max_physical_generation_attempts
                < self
                    .logical_calls_per_function
                    .saturating_mul(self.function.generation_factor())
        {
            return Err("Positive finite budgets and the pinned GEPA engine are required".into());
        }
        match self.function {
            Function::Classification if self.target_method.is_none() && self.max_review_calls == 0 && self.review_book.is_none() => {},
            Function::InputJourney if self.control_mode == ControlMode::FreshHosted
                && self.max_review_calls > 0 && self.review_seconds > 0 && self.review_book.is_some()
                && self.target_method.as_ref().is_some_and(|m| !m.is_empty()
                    && m.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')) => {},
            _ => return Err("Classification has no method/reviewer; input journeys require an explicit method, fresh controls and bounded independent review".into()),
        }
        if let Some(book) = &self.review_book {
            book.excerpts()?;
        }
        if self.training.is_empty()
            || self.development.is_empty()
            || self.reserved_ids.is_empty()
            || self.seed.is_empty()
        {
            return Err(
                "Disjoint nonempty reflection, development and reserved sets are required".into(),
            );
        }
        if digest(&self.guide) != self.guide_sha256 {
            return Err("Baseline guide changed".into());
        }
        let seed = self.seed_for_guide()?;
        if self.seed != seed {
            return Err("Seed components do not reproduce the original teaching".into());
        }
        let mut ids = BTreeSet::new();
        for id in self
            .training
            .iter()
            .chain(&self.development)
            .map(|example| &example.id)
            .chain(&self.reserved_ids)
        {
            if !ids.insert(id) {
                return Err(format!("Duplicate/overlapping case {id}"));
            }
        }
        self.reconstruct(&self.seed)?;
        Ok(())
    }

    /// Only teaching regions are mutable. All source blocks remain local and exact.
    pub fn reconstruct(&self, candidate: &Candidate) -> Result<String> {
        if candidate.keys().ne(self.seed.keys()) {
            return Err("Candidate changed the component set".into());
        }
        let parts = guide_parts(&self.guide)?;
        let mut text = String::new();
        for (index, original) in parts.teaching.iter().enumerate() {
            let key = self.region_key(index);
            let teaching = candidate.get(&key).map(String::as_str).unwrap_or(original);
            if teaching.len() > 64_000
                || teaching.contains("<book_extracts>")
                || teaching.contains("</book_extracts>")
            {
                return Err(
                    "Teaching changed a protected source delimiter or exceeds its bound".into(),
                );
            }
            text.push_str(teaching);
            if let Some(source) = parts.quoted_source.get(index) {
                text.push_str(source);
            }
        }
        Ok(text)
    }

    pub fn program(&self, candidate: &Candidate) -> Result<Option<Program>> {
        if candidate == &self.seed {
            return Ok(None);
        }
        let replacement = self.reconstruct(candidate)?;
        let program = Program {
            version: 1,
            id: format!(
                "gepa-{}-{}",
                self.function.metric(),
                &digest(replacement.as_bytes())[..16]
            ),
            baseline_manifest_sha256: self.manifest_sha256.clone(),
            overrides: vec![Override {
                stage: "intake".into(),
                recognition_phase: Some(self.function.phase().into()),
                method: self.target_method.clone(),
                expected_guide_sha256: self.guide_sha256.clone(),
                replacement_text: replacement,
                edits: vec![],
            }],
            rationale: self.qualification.clone(),
            evidence: self
                .training
                .iter()
                .map(|example| Evidence {
                    case_id: example.id.clone(),
                    file: format!(
                        "cases/{}/{}",
                        example.id,
                        example.source_request_file.display()
                    ),
                    json_pointer: "/prompt/0/content".into(),
                    sha256: example.source_request_sha256.clone(),
                })
                .collect(),
            training_case_ids: self
                .training
                .iter()
                .map(|example| example.id.clone())
                .collect(),
            holdout_case_ids: self.reserved_ids.clone(),
        };
        program.validate()?;
        program
            .apply(self.signature(), &self.guide)?
            .ok_or("Candidate failed to apply to its actual native signature")?;
        Ok(Some(program))
    }

    pub fn verify_sources(&self) -> Result<()> {
        self.validate()?;
        verify(&self.campaign.join("manifest.json"), &self.manifest_sha256)?;
        verify(&self.split_file, &self.split_sha256)?;
        let split = load(&self.split_file)?;
        self.verify_split(&split)?;
        verify(&self.native_executable, &self.native_executable_sha256)?;
        verify(
            &self.controller_executable,
            &self.controller_executable_sha256,
        )?;
        verify(&self.codex, &self.codex_executable_sha256)?;
        for example in self.training.iter().chain(&self.development) {
            verify(&example.inspection_file, &example.inspection_sha256)?;
            verify(
                &example.source_case_directory.join("fixture.json"),
                &example.source_fixture_sha256,
            )?;
            let fixture = load(&example.source_case_directory.join("fixture.json"))?;
            self.function
                .verify_fixture(example, &fixture, self.target_method.as_deref())?;
            verify(
                &self
                    .campaign
                    .join("case-origins")
                    .join(format!("{}.json", example.id)),
                &example.source_origin_sha256,
            )?;
            verify(
                &example
                    .source_case_directory
                    .join(&example.source_request_file),
                &example.source_request_sha256,
            )?;
            verify(
                &example.source_case_directory.join("initial.json"),
                &example.source_initial_sha256,
            )?;
        }
        Ok(())
    }
    fn verify_split(&self, split: &Value) -> Result<()> {
        let rows = split["cases"]
            .as_array()
            .ok_or("Missing frozen split cases")?;
        for example in self.training.iter().chain(&self.development) {
            let matches = rows
                .iter()
                .filter(|row| row["id"] == example.id)
                .collect::<Vec<_>>();
            if matches.len() != 1 || matches[0]["partition"] != "training" {
                return Err(format!(
                    "Reserved, missing or duplicate split membership for {}",
                    example.id
                ));
            }
        }
        let reserved = rows
            .iter()
            .filter(|row| row["partition"] == "reserved_validation")
            .map(|row| {
                row["id"]
                    .as_str()
                    .ok_or_else(|| "Invalid reserved identity".to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        if reserved
            != self
                .reserved_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        {
            return Err("Materialized reserved IDs differ from the frozen split".into());
        }
        Ok(())
    }
}

pub fn prepare(mut plan: Plan, state: &Path, train: &[String], dev: &[String]) -> Result<Plan> {
    if plan.function == Function::InputJourney
        && (plan.control_mode != ControlMode::FreshHosted
            || plan.target_method.is_none()
            || plan.max_review_calls == 0
            || plan.review_seconds == 0)
    {
        return Err("Input journeys require an explicit method, fresh-hosted controls and bounded independent reviews".into());
    }
    let manifest = load(&plan.campaign.join("manifest.json"))?;
    if manifest["model"]["id"] != "gemma-4-26b-a4b-it"
        || manifest["model"]["local_inference"] != false
    {
        return Err("Pilot requires the frozen hosted unconstrained Gemma campaign".into());
    }
    let split = load(&plan.split_file)?;
    fs::create_dir_all(state).map_err(|error| error.to_string())?;
    let rows = split["cases"].as_array().ok_or("No frozen split cases")?;
    plan.reserved_ids = rows
        .iter()
        .filter(|row| row["partition"] == "reserved_validation")
        .map(|row| {
            row["id"]
                .as_str()
                .map(str::to_owned)
                .ok_or("Invalid reserved ID".to_owned())
        })
        .collect::<Result<_>>()?;
    let mut examples = Vec::new();
    for id in train.iter().chain(dev) {
        let row = rows
            .iter()
            .find(|row| row["id"] == *id)
            .ok_or_else(|| format!("Case {id} absent from split"))?;
        if row["partition"] != "training" {
            return Err(format!("Reserved case {id} may not enter GEPA search"));
        }
        let dir = plan.campaign.join("cases").join(id);
        let origin_path = plan
            .campaign
            .join("case-origins")
            .join(format!("{id}.json"));
        let origin = load(&origin_path)?;
        let initial_sha = origin["files"]["initial.json"]
            .as_str()
            .ok_or("Unsealed initial capture")?;
        verify(&dir.join("initial.json"), initial_sha)?;
        // A closed case, not a live partial directory, supplies the corpus.
        let outcome_sha = origin["files"]["outcome.json"]
            .as_str()
            .ok_or("Unsealed case outcome")?;
        verify(&dir.join("outcome.json"), outcome_sha)?;
        let fixture_sha = origin["files"]["fixture.json"]
            .as_str()
            .ok_or("Unsealed authored fixture")?;
        verify(&dir.join("fixture.json"), fixture_sha)?;
        let fixture = load(&dir.join("fixture.json"))?;
        if plan.function == Function::InputJourney
            && fixture["method"].as_str() != plan.target_method.as_deref()
        {
            return Err(format!(
                "Case {id} belongs to another method; do not impose a gold method on the oracle"
            ));
        }
        let mut requests = fs::read_dir(dir.join("calls"))
            .map_err(|error| error.to_string())?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>>>()?;
        requests.sort();
        let mut found = None;
        for path in requests
            .iter()
            .filter(|path| path.to_string_lossy().ends_with("-request.json"))
        {
            let request = load(path)?;
            let input = horary_prompt_program::original_input(&request["input"]);
            let signature = horary_prompt_program::signature("intake", input);
            let wanted = plan.signature();
            if request["stage"] != "intake"
                || signature.stage != wanted.stage
                || signature.recognition_phase != wanted.recognition_phase
                || signature.method != wanted.method
            {
                continue;
            }
            request["prompt"][0]["content"]
                .as_str()
                .ok_or("No captured classifier guide")?;
            let relative = path
                .strip_prefix(&dir)
                .map_err(|error| error.to_string())?
                .to_owned();
            let request_sha = origin["files"][relative.to_string_lossy().as_ref()]
                .as_str()
                .ok_or("Unsealed classifier call")?;
            verify(path, request_sha)?;
            found = Some(Example {
                id: id.clone(),
                source_case_directory: dir.clone(),
                source_request_file: relative,
                source_request_sha256: request_sha.into(),
                source_initial_sha256: initial_sha.into(),
                source_fixture_sha256: fixture_sha.into(),
                source_origin_sha256: digest(read(&origin_path)?),
                inspection_file: PathBuf::new(),
                inspection_sha256: String::new(),
                expected: plan.function.expectations(&fixture),
            });
            break;
        }
        let mut example = found.ok_or_else(|| {
            format!(
                "No sealed {} capture for {id}; prerequisite failures remain failures",
                plan.function.phase()
            )
        })?;
        let inspection_file = native::inspect(&plan, state, &example)?;
        let inspection = load(&inspection_file)?;
        let current_guide = inspection["prompt"][0]["content"]
            .as_str()
            .ok_or("Native inspection has no guide")?;
        let recorded = load(&dir.join(&example.source_request_file))?;
        if plan.control_mode == ControlMode::ArchivedCapture
            && recorded["prompt"][0]["content"] != current_guide
        {
            return Err("Archived classifier guide differs from the current native function; no calls submitted. Use an explicit fresh-hosted control round, never substitute archived answers.".into());
        }
        if plan.guide.is_empty() {
            plan.guide = current_guide.into();
            plan.guide_sha256 = digest(current_guide);
        }
        if current_guide != plan.guide {
            return Err(
                "Pilot cases must share one actual classifier guide; use a separate scoped round"
                    .into(),
            );
        }
        example.inspection_sha256 = digest(read(&inspection_file)?);
        example.inspection_file = inspection_file;
        examples.push(example);
    }
    plan.training = examples[..train.len()].to_vec();
    plan.development = examples[train.len()..].to_vec();
    plan.seed = plan.seed_for_guide()?;
    plan.manifest_sha256 = digest(read(&plan.campaign.join("manifest.json"))?);
    plan.split_sha256 = digest(read(&plan.split_file)?);
    plan.native_executable_sha256 = digest(read(&plan.native_executable)?);
    plan.controller_executable_sha256 = digest(read(&plan.controller_executable)?);
    plan.codex_executable_sha256 = digest(read(&plan.codex)?);
    plan.verify_sources()?;
    fs::create_dir_all(state).map_err(|error| error.to_string())?;
    keep(&state.join("plan.json"), &plan)?;
    Ok(plan)
}

struct Adapter {
    plan: Plan,
    journal: journal::Journal,
    captured: Vec<(Example, Value)>,
}
impl Adapter {
    fn evaluate(
        &mut self,
        examples: Vec<Example>,
        candidate: &Candidate,
        capture: bool,
    ) -> Result<EvalBatch> {
        self.plan.verify_sources()?;
        let mut scores = Vec::new();
        if capture {
            self.captured.clear();
        }
        for example in examples {
            let evaluation = native::evaluate(&self.plan, &mut self.journal, &example, candidate)?;
            let independent = if self.plan.function == Function::InputJourney {
                Some(review::evaluate(
                    &self.plan,
                    &mut self.journal,
                    &example,
                    &evaluation,
                )?)
            } else {
                None
            };
            let feedback = metric::grade(
                &evaluation["outcome"],
                independent.as_ref(),
                self.plan.function.metric(),
            )?;
            if !feedback.score.is_finite() || !(0.0..=1.0).contains(&feedback.score) {
                return Err("Invalid or misaligned fitness".into());
            }
            scores.push(feedback.score);
            if capture {
                self.captured
                    .push((example, json!({"evaluation":evaluation,"fitness":feedback,"independent_review":independent})));
            }
        }
        Ok(if capture {
            EvalBatch::traced(scores)
        } else {
            EvalBatch::scored(scores)
        })
    }
}
/// The upstream adapter has no Result channel. Preserve typed failures and abort
/// the search task; never let a failed/uncertain evaluation become score zero.
fn required<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| std::panic::panic_any(error))
}
impl GepaAdapter for Adapter {
    async fn evaluate_minibatch(
        &mut self,
        ids: &[usize],
        candidate: &Candidate,
        capture: bool,
    ) -> EvalBatch {
        let examples = required(
            ids.iter()
                .map(|id| {
                    self.plan
                        .training
                        .get(*id)
                        .cloned()
                        .ok_or("Bad training index".into())
                })
                .collect(),
        );
        required(self.evaluate(examples, candidate, capture))
    }
    async fn evaluate_valset(&mut self, candidate: &Candidate) -> EvalBatch {
        required(self.evaluate(self.plan.development.clone(), candidate, false))
    }
    async fn evaluate_valset_ids(&mut self, ids: &[usize], candidate: &Candidate) -> EvalBatch {
        let examples = required(
            ids.iter()
                .map(|id| {
                    self.plan
                        .development
                        .get(*id)
                        .cloned()
                        .ok_or("Bad development index".into())
                })
                .collect(),
        );
        required(self.evaluate(examples, candidate, false))
    }
    async fn propose_new_texts(
        &mut self,
        candidate: &Candidate,
        components: &[String],
        _captured: &EvalBatch,
    ) -> Candidate {
        required(teacher::reflect(
            &self.plan,
            &mut self.journal,
            candidate,
            components,
            &self.captured,
        ))
    }
}

pub async fn run(plan: Plan, state: PathBuf) -> Result<Value> {
    plan.verify_sources()?;
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    verify(&current, &plan.controller_executable_sha256)?;
    let journal = journal::Journal::open(&state, &plan)?;
    // Keep the same OS lock after the consumed engine drops its adapter, until
    // the final result or interruption receipt has been durably written.
    let _ownership = journal.retain_ownership()?;
    let engine = GepaEngine {
        trainset_size: plan.training.len(),
        valset_size: plan.development.len(),
        minibatch_size: plan.training.len().min(3),
        max_metric_calls: plan.max_metric_calls,
        perfect_score: 1.0,
        skip_perfect_score: true,
        use_merge: false,
        max_merge_invocations: 0,
        seed: plan.rng_seed,
        adapter: Adapter {
            plan: plan.clone(),
            journal,
            captured: vec![],
        },
    };
    let seed = plan.seed.clone();
    let result = tokio::spawn(async move { engine.optimize(seed).await }).await;
    match result {
        Ok(result) => {
            let program = plan.program(&result.best)?;
            let value = json!({"engine":"dsrust-gepa","revision":ENGINE_REV,"best_index":result.best_idx,"candidates":result.candidates,
                "parents":result.parents,"development_scores":result.val_aggregate_scores,"logical_metric_evaluations":result.total_num_evals,
                "iterations":result.iterations,"selected_program":program,"function":plan.function,"target_method":plan.target_method,
                "qualification":plan.qualification});
            keep(&state.join("search-result.json"), &value)?;
            if let Some(program) = program {
                keep(&state.join("selected-program.json"), &program)?;
            }
            Ok(value)
        }
        Err(error) => {
            let message = if error.is_panic() {
                let payload = error.into_panic();
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "Optimizer adapter panicked".into())
            } else {
                error.to_string()
            };
            let receipt = json!({"error":message,"qualification":"Interrupted search; preserve operations; no candidate promotion"});
            let interruptions = state.join("interruptions");
            fs::create_dir_all(&interruptions).map_err(|error| error.to_string())?;
            let count = fs::read_dir(&interruptions)
                .map_err(|error| error.to_string())?
                .count();
            keep(&interruptions.join(format!("{count:05}.json")), &receipt)?;
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn book_review_context_is_exact_bounded_and_source_hashed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("book.md");
        let bytes="intro\n<!-- page:0016 -->\n## Page 16\nPrinted page seven.\n<!-- page:0017 -->\n## Page 17\nPrinted page eight.\n";
        fs::write(&file, bytes).unwrap();
        let mut book = BookContext {
            file: file.clone(),
            sha256: digest(bytes),
            ocr_pages: vec![16, 17],
        };
        let excerpts = book.excerpts().unwrap();
        assert_eq!(
            excerpts["pages"][0]["text"],
            "<!-- page:0016 -->\n## Page 16\nPrinted page seven."
        );
        book.ocr_pages = vec![16, 16];
        assert!(book.excerpts().is_err());
        book.ocr_pages = vec![15];
        assert!(book.excerpts().is_err());
        book.ocr_pages = vec![16];
        fs::write(file, "altered source").unwrap();
        assert!(book.excerpts().unwrap_err().contains("Source changed"));
    }
    fn plan() -> Plan {
        let guide = format!(
            "{}\n<book_extracts>Original immutable source passage.</book_extracts>\n{}",
            "Original teaching and decision examples. ".repeat(4),
            "Supplementary original instruction. ".repeat(4)
        );
        let seed = guide_parts(&guide)
            .unwrap()
            .teaching
            .into_iter()
            .enumerate()
            .filter(|(_, text)| text.trim().len() >= 64)
            .map(|(index, text)| (format!("classify_question.region.{index:03}"), text.into()))
            .collect();
        let example = |id: &str| Example {
            id: id.into(),
            source_case_directory: id.into(),
            source_request_file: "calls/0001-request.json".into(),
            source_request_sha256: digest("request"),
            source_initial_sha256: digest("initial"),
            source_fixture_sha256: digest("fixture"),
            source_origin_sha256: digest("origin"),
            inspection_file: "inspection".into(),
            inspection_sha256: digest("inspection"),
            expected: json!({"facet":"event"}),
        };
        Plan {
            version: 1,
            engine_rev: ENGINE_REV.into(),
            controller_executable: "controller".into(),
            controller_executable_sha256: digest("controller"),
            campaign: "campaign".into(),
            manifest_sha256: digest("manifest"),
            split_file: "split".into(),
            split_sha256: digest("split"),
            native_executable: "native".into(),
            native_executable_sha256: digest("native"),
            codex: "codex".into(),
            codex_executable_sha256: digest("codex"),
            guide_sha256: digest(&guide),
            guide,
            seed,
            training: vec![example("train")],
            development: vec![example("development")],
            reserved_ids: vec!["reserved".into()],
            max_metric_calls: 12,
            max_teacher_calls: 2,
            max_physical_generation_attempts: 18,
            logical_calls_per_function: 3,
            function_seconds: 120,
            teacher_seconds: 120,
            rng_seed: 7,
            wait_owner_pid: None,
            control_mode: ControlMode::ArchivedCapture,
            function: Function::Classification,
            target_method: None,
            max_review_calls: 0,
            review_seconds: 0,
            review_book: None,
            qualification: "component test".into(),
        }
    }
    #[test]
    fn component_search_preserves_quoted_source_and_actual_signature() {
        let plan = plan();
        plan.validate().unwrap();
        assert_eq!(plan.reconstruct(&plan.seed).unwrap(), plan.guide);
        assert!(plan.program(&plan.seed).unwrap().is_none());
        let mut candidate = plan.seed.clone();
        candidate.insert(
            "classify_question.region.000".into(),
            "Changed decision teaching with worked event-versus-timing examples. ".repeat(3),
        );
        let program = plan.program(&candidate).unwrap().unwrap();
        let (changed, applied) = program
            .apply(
                horary_prompt_program::Signature {
                    stage: "intake",
                    recognition_phase: Some("classify_question"),
                    method: None,
                },
                &plan.guide,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            guide_parts(&changed).unwrap().quoted_source,
            guide_parts(&plan.guide).unwrap().quoted_source
        );
        assert_ne!(
            applied.original_guide_sha256,
            applied.replacement_guide_sha256
        );
        assert!(program
            .apply(
                horary_prompt_program::Signature {
                    stage: "intake",
                    recognition_phase: Some("complete_selected_program"),
                    method: None
                },
                &plan.guide
            )
            .unwrap()
            .is_none());
        candidate.insert(
            "classify_question.region.000".into(),
            "<book_extracts>Changed text</book_extracts>".into(),
        );
        assert!(plan.program(&candidate).is_err());
    }
    #[test]
    fn final_cases_and_invalid_seed_cannot_enter_search() {
        let mut plan = plan();
        plan.development[0].id = plan.reserved_ids[0].clone();
        assert!(plan.validate().unwrap_err().contains("overlapping"));
        let mut plan = self::plan();
        plan.seed
            .insert("invisible-component".into(), "ignored".into());
        assert!(plan.validate().unwrap_err().contains("Seed"));
    }
    #[test]
    fn materialized_gold_and_split_membership_are_rechecked_against_frozen_sources() {
        let mut plan = plan();
        let fixture = json!({"id":"train","method":"movable_deal","expected":{"facet":"profit","needs":[]},
            "follow_up_expected":null,"rationale":"Synthetic offline source"});
        let example = &mut plan.training[0];
        example.expected = Function::InputJourney.expectations(&fixture);
        Function::InputJourney
            .verify_fixture(example, &fixture, Some("movable_deal"))
            .unwrap();
        example.expected["expected"]["facet"] = json!("quality");
        assert!(Function::InputJourney
            .verify_fixture(example, &fixture, Some("movable_deal"))
            .is_err());
        example.expected = Function::InputJourney.expectations(&fixture);
        assert!(Function::InputJourney
            .verify_fixture(example, &fixture, Some("investment"))
            .is_err());
        let mut split = json!({"cases":[{"id":"train","partition":"training"},
            {"id":"development","partition":"training"},{"id":"reserved","partition":"reserved_validation"}]});
        plan.verify_split(&split).unwrap();
        split["cases"][1]["partition"] = json!("reserved_validation");
        assert!(plan
            .verify_split(&split)
            .unwrap_err()
            .contains("membership"));
    }
    struct EngineProbe {
        seen: Vec<String>,
    }
    impl GepaAdapter for EngineProbe {
        async fn evaluate_minibatch(
            &mut self,
            ids: &[usize],
            candidate: &Candidate,
            _capture: bool,
        ) -> EvalBatch {
            self.seen.push(format!("training:{ids:?}"));
            let score = if candidate["classifier"] == "improved" {
                1.0
            } else {
                0.0
            };
            EvalBatch::traced(vec![score; ids.len()])
        }
        async fn evaluate_valset(&mut self, candidate: &Candidate) -> EvalBatch {
            EvalBatch::scored(vec![if candidate["classifier"] == "improved" {
                1.0
            } else {
                0.0
            }])
        }
        async fn evaluate_valset_ids(
            &mut self,
            _ids: &[usize],
            candidate: &Candidate,
        ) -> EvalBatch {
            self.evaluate_valset(candidate).await
        }
        async fn propose_new_texts(
            &mut self,
            _candidate: &Candidate,
            components: &[String],
            _captured: &EvalBatch,
        ) -> Candidate {
            assert_eq!(components, ["classifier"]);
            [("classifier".into(), "improved".into())].into()
        }
    }
    #[tokio::test]
    async fn pinned_engine_evolves_and_selects_a_real_component() {
        let engine = GepaEngine {
            adapter: EngineProbe { seen: vec![] },
            trainset_size: 2,
            valset_size: 1,
            minibatch_size: 2,
            max_metric_calls: 6,
            perfect_score: 1.0,
            skip_perfect_score: true,
            seed: 7,
            use_merge: false,
            max_merge_invocations: 0,
        };
        let result = engine
            .optimize([("classifier".into(), "baseline".into())].into())
            .await;
        assert_eq!(result.best["classifier"], "improved");
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.parents[1], vec![0]);
    }
}
