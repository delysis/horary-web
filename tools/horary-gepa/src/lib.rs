//! GEPA owns the search. Horary owns prompts, truth, validation and receipts.
#![forbid(unsafe_code)]
pub mod journal;
pub mod metric;
pub mod native;
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
    pub qualification: String,
}

impl Plan {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.engine_rev != ENGINE_REV
            || self.max_metric_calls == 0
            || self.max_teacher_calls == 0
            || self.logical_calls_per_function == 0
            || self.function_seconds == 0
            || self.teacher_seconds == 0
            || self.max_physical_generation_attempts
                < self.logical_calls_per_function.saturating_mul(3)
        {
            return Err("Positive finite budgets and the pinned GEPA engine are required".into());
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
        let seed: Candidate = guide_parts(&self.guide)?
            .teaching
            .into_iter()
            .enumerate()
            .filter(|(_, text)| text.trim().len() >= 64)
            .map(|(index, text)| (format!("classify_question.region.{index:03}"), text.into()))
            .collect();
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
            let key = format!("classify_question.region.{index:03}");
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
            version: 1, id: format!("gepa-classification-{}", &digest(replacement.as_bytes())[..16]),
            baseline_manifest_sha256: self.manifest_sha256.clone(),
            overrides: vec![Override { stage: "intake".into(), recognition_phase: Some("classify_question".into()), method: None,
                expected_guide_sha256: self.guide_sha256.clone(), replacement_text: replacement, edits: vec![] }],
            rationale: "GEPA classification pilot; native labelled fitness only. Development selects; reserved cases do not feed reflection. Not a reading qualification.".into(),
            evidence: self.training.iter().map(|example| Evidence { case_id: example.id.clone(), file: format!("cases/{}/{}", example.id, example.source_request_file.display()), json_pointer: "/prompt/0/content".into(), sha256: example.source_request_sha256.clone() }).collect(),
            training_case_ids: self.training.iter().map(|example| example.id.clone()).collect(),
            holdout_case_ids: self.reserved_ids.clone(),
        };
        program.validate()?;
        program
            .apply(
                horary_prompt_program::Signature {
                    stage: "intake",
                    recognition_phase: Some("classify_question"),
                    method: None,
                },
                &self.guide,
            )?
            .ok_or("Candidate failed to apply to its actual classifier signature")?;
        Ok(Some(program))
    }

    pub fn verify_sources(&self) -> Result<()> {
        self.validate()?;
        verify(&self.campaign.join("manifest.json"), &self.manifest_sha256)?;
        verify(&self.split_file, &self.split_sha256)?;
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
}

pub fn prepare(mut plan: Plan, state: &Path, train: &[String], dev: &[String]) -> Result<Plan> {
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
            if request["stage"] != "intake" || input["recognition_phase"] != "classify_question" {
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
                expected: json!({"method":fixture["method"], "allowed_methods":fixture["expected"]["allowed_methods"],
                    "facet":fixture["expected"]["facet"], "allowed_facets":fixture["expected"]["allowed_facets"]}),
            });
            break;
        }
        let mut example = found.ok_or_else(|| format!("No classifier capture for {id}"))?;
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
    plan.seed = guide_parts(&plan.guide)?
        .teaching
        .into_iter()
        .enumerate()
        .filter(|(_, text)| text.trim().len() >= 64)
        .map(|(index, text)| (format!("classify_question.region.{index:03}"), text.into()))
        .collect();
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
            let feedback = metric::grade(&evaluation["outcome"], None, "classification")?;
            if !feedback.score.is_finite() || !(0.0..=1.0).contains(&feedback.score) {
                return Err("Invalid or misaligned fitness".into());
            }
            scores.push(feedback.score);
            if capture {
                self.captured
                    .push((example, json!({"evaluation":evaluation,"fitness":feedback})));
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
                "iterations":result.iterations,"selected_program":program,"qualification":"Classification fitness only; no semantic reading, held-out, on-device or deployment qualification. Candidate must pass fresh complete-journey and reserved final gates."});
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
