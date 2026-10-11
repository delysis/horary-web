//! A DSPy Module: original words -> classify -> extract -> Rust chart -> reading.
//! Expected methods/facts/chart seeds are evaluation labels, never oracle inputs.
use crate::{
    book::Book,
    catalogue::{self, Case, Device, Fact},
    optimize::{fields, plain, text},
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Example, Module, Predict,
    example::Prediction,
    lm::DynChatModel,
    module::{NamedPredictor, TraceStep, relabel},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub fn example(case: &Case) -> Example {
    dsrust::example!{question:case.question,device:serde_json::to_string(&case.device).expect("Serializable device"),case_id:case.id,expected:json!({"method":case.method,"rubric":case.expected})}.with_inputs(["question","device"])
}
fn key(example: &Example) -> Result<String> {
    Ok(crate::google::digest(serde_json::to_vec(
        &example.inputs()?.rendered(),
    )?))
}

pub struct Journey {
    classifier: Predict,
    extractor: Predict,
    reader: Predict,
    judge: Predict,
    family: String,
    model: Arc<dyn DynChatModel>,
    book: Arc<Book>,
    source: BTreeMap<String, String>,
    component_extractors: BTreeMap<String, Predict>,
    component_readers: BTreeMap<String, Predict>,
    component_sources: BTreeMap<String, String>,
    case_bank_sha256: String,
    labels: BTreeMap<String, Example>,
    stream: Option<PathBuf>,
    pub observations: Arc<Mutex<Vec<Value>>>,
}
impl Journey {
    pub fn new(
        family: &str,
        cases: &[Case],
        book: Arc<Book>,
        model: Arc<dyn DynChatModel>,
        teacher: Arc<dyn DynChatModel>,
    ) -> Result<Self> {
        let mut source = BTreeMap::new();
        for method in signatures::METHODS {
            source.insert(
                (*method).into(),
                catalogue::method_source(&book, method, true)?,
            );
        }
        let labels = cases
            .iter()
            .map(|case| {
                let e = example(case);
                Ok((key(&e)?, e))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        ensure!(labels.len() == cases.len(), "Duplicate full-journey inputs");
        Ok(Self {
            classifier: plain(signatures::auxiliary("classification")?, model.clone()),
            extractor: plain(signatures::extraction(family)?, model.clone()),
            reader: plain(signatures::reading(family)?, model.clone()),
            judge: plain(signatures::auxiliary("judge")?, teacher),
            family: family.into(),
            model,
            book,
            source,
            component_extractors: BTreeMap::new(),
            component_readers: BTreeMap::new(),
            component_sources: BTreeMap::new(),
            case_bank_sha256: crate::google::digest(serde_json::to_vec(cases)?),
            labels,
            stream: None,
            observations: Arc::new(Mutex::new(Vec::new())),
        })
    }
    pub fn with_stream(mut self, path: PathBuf) -> Result<Self> {
        fs::create_dir_all(path.parent().context("No stream directory")?)?;
        self.stream = Some(path);
        Ok(self)
    }

    /// Compose measured function artifacts for a fresh original-message gate.
    /// The actual classifier still selects the family; the evaluation label is
    /// never used as a route or as an accepted fact. No runtime installation.
    pub fn with_components(mut self, queue_root: &Path) -> Result<Self> {
        let plan = crate::queue::verify_composition_plan(queue_root)?;
        ensure!(
            plan.case_banks_canonical_sha256.get(&self.family) == Some(&self.case_bank_sha256),
            "Journey cases differ from the compiled queue's sealed canonical bank"
        );
        let classifier = queue_root.join("jobs/classification/compiled.json");
        if classifier.exists() {
            crate::optimize::load_component_for_dataset(
                &mut self.classifier,
                &classifier,
                "all",
                "classification",
                plan.component_datasets_sha256
                    .get("classification")
                    .context("No sealed classification dataset")?,
            )?;
            self.component_sources.insert(
                "classification".into(),
                crate::google::digest(fs::read(classifier)?),
            );
        }
        for method in signatures::METHODS
            .iter()
            .copied()
            .filter(|m| !matches!(*m, "wish" | "unclassified"))
        {
            for function in ["extraction", "reading"] {
                let path = queue_root.join(format!("jobs/{method}-{function}/compiled.json"));
                if !path.exists() {
                    continue;
                }
                let mut predictor = if function == "extraction" {
                    plain(signatures::extraction(method)?, self.model.clone())
                } else {
                    plain(signatures::reading(method)?, self.model.clone())
                };
                crate::optimize::load_component_for_dataset(
                    &mut predictor,
                    &path,
                    method,
                    function,
                    plan.component_datasets_sha256
                        .get(&format!("{method}-{function}"))
                        .context("No sealed component dataset")?,
                )?;
                self.component_sources.insert(
                    format!("{function}.{method}"),
                    crate::google::digest(fs::read(path)?),
                );
                if method == self.family {
                    if function == "extraction" {
                        self.extractor = predictor;
                    } else {
                        self.reader = predictor;
                    }
                } else if function == "extraction" {
                    self.component_extractors.insert(method.into(), predictor);
                } else {
                    self.component_readers.insert(method.into(), predictor);
                }
            }
        }
        Ok(self)
    }
    fn record_native(
        &self,
        input: &Example,
        stages: &serde_json::Map<String, Value>,
        phase: &str,
    ) -> Result<()> {
        if let Some(path) = &self.stream {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path.with_extension("native.jsonl"))?;
            writeln!(
                file,
                "{}",
                serde_json::to_string(
                    &json!({"phase":phase,"original_inputs":input.rendered(),"stages":stages,"compiled_components":self.component_sources,"independently_reviewed":false})
                )?
            )?;
            file.sync_data()?;
        }
        Ok(())
    }
    async fn oracle(
        &self,
        input: &Example,
        trace: &mut Vec<TraceStep>,
    ) -> Result<(Prediction, Value, Vec<String>)> {
        let question = input
            .get("question")
            .and_then(Value::as_str)
            .context("No original question")?;
        let device: Device = serde_json::from_str(
            input
                .get("device")
                .and_then(Value::as_str)
                .context("No device context")?,
        )?;
        let mut stages = serde_json::Map::new();
        let from = trace.len();
        let classification=self.classifier.forward_traced(dsrust::input!{question:question,conversation:"[]",catalogue:catalogue::method_index()?},trace).await?;
        relabel(trace, from, "classification");
        stages.insert("classification".into(), fields(&classification));
        self.record_native(input, &stages, "classification")?;
        let method = text(&classification, "method")?;
        let branch = text(&classification, "branch")?;
        let extraction_source = if signatures::METHODS.contains(&method) {
            catalogue::branch_source(&self.book, method, branch, false)
        } else {
            Err(anyhow::anyhow!(
                "Classifier returned an unknown method: {method}"
            ))
        };
        let extraction_source = match extraction_source {
            Ok(source) => source,
            Err(error) => {
                let failures = vec![format!("Classification rejected: {error}")];
                stages.insert(
                    "withheld".into(),
                    json!({"extraction":true,"chart":true,"reading":true,"reason":failures}),
                );
                self.record_native(input, &stages, "classification_rejected")?;
                let answer = text(&classification, "clarification")?.to_owned();
                return Ok((
                    Prediction::new(
                        dsrust::example! {status:"need_information",answer:answer,analysis:"",evidence:"",unresolved:failures.join("; ")},
                        "Native branch guard withheld extraction/chart/reading",
                    ),
                    Value::Object(stages),
                    failures,
                ));
            }
        };
        let reading_source = catalogue::branch_source(&self.book, method, branch, true)?;
        let fallback_extractor;
        let extractor = if method == self.family {
            &self.extractor
        } else if let Some(predictor) = self.component_extractors.get(method) {
            predictor
        } else {
            fallback_extractor = plain(signatures::extraction(method)?, self.model.clone());
            &fallback_extractor
        };
        let from = trace.len();
        let extraction=extractor.forward_traced(dsrust::input!{question:question,conversation:"[]",context:serde_json::to_string(&json!({"device":device,"classified_facet":classification.get("facet"),"selected_branch":branch,"accepted_facts":[]}))?,requirements:format!("{}\n{}",catalogue::requirements(method)?,extraction_source)},trace).await?;
        relabel(trace, from, &format!("extraction.{method}"));
        stages.insert("extraction".into(), fields(&extraction));
        self.record_native(input, &stages, "extraction")?;
        let mut failures = Vec::new();
        let facts = match serde_json::from_str::<Vec<Fact>>(text(&extraction, "facts")?) {
            Ok(facts) => facts,
            Err(error) => {
                failures.push(format!("Fact array rejected: {error}"));
                Vec::new()
            }
        };
        let missing = match serde_json::from_str::<Vec<String>>(text(&extraction, "missing")?) {
            Ok(missing) => missing,
            Err(error) => {
                failures.push(format!("Missing-input array rejected: {error}"));
                Vec::new()
            }
        };
        let mut paths = std::collections::BTreeSet::new();
        for fact in &facts {
            if fact.field.trim().is_empty() || !paths.insert(fact.field.as_str()) {
                failures
                    .push("Native fact array contains an empty or duplicate canonical path".into());
            }
            if !fact.quote.is_empty() && !question.contains(&fact.quote) {
                failures.push(format!("{} uses a nonliteral quote", fact.field));
            }
            if fact.quote.is_empty() && text(&extraction, "inferences")?.trim().is_empty() {
                failures.push(format!(
                    "{} has no quotation or inference witness",
                    fact.field
                ));
            }
        }
        let anchor = match crate::anchors::resolve(question, &device, &facts) {
            Ok(anchor) => Some(anchor),
            Err(error) => {
                failures.push(format!("Native anchor/tool validation: {error}"));
                None
            }
        };
        if !missing.is_empty() {
            failures.push(format!("Unresolved inputs: {missing:?}"));
        }
        if !failures.is_empty() {
            let answer = text(&extraction, "clarification")?.to_owned();
            return Ok((
                Prediction::new(
                    dsrust::example! {status:"need_information",answer:answer,analysis:"",evidence:"",unresolved:failures.join("; ")},
                    "Native input guard withheld chart/reading",
                ),
                Value::Object(stages),
                failures,
            ));
        }
        let anchor = anchor.context("No chart anchor")?;
        let chart = horary_ai_core::astronomy::chart(
            anchor.at.timestamp_millis() as f64,
            anchor.place.latitude,
            anchor.place.longitude,
        )
        .map_err(anyhow::Error::msg)?;
        let native = crate::native_evidence::facts(Some(&chart));
        let evidence = json!({"anchor":{"utc":anchor.at.to_rfc3339(),"place":anchor.place,"authority":"Native reader device or explicit source-bound historical instant; event venue/time are separate context"},"facts":catalogue::compact_native_facts(&native),"event_context":anchor.events,"search_limitations":chart["calculationNote"],"moon":chart["derived"]["voidOfCourseMoon"]});
        stages.insert("chart".into(), evidence.clone());
        stages.insert("full_native_facts".into(), serde_json::to_value(&native)?);
        self.record_native(input, &stages, "chart")?;
        let fallback_reader;
        let reader = if method == self.family {
            &self.reader
        } else if let Some(predictor) = self.component_readers.get(method) {
            predictor
        } else {
            fallback_reader = plain(signatures::reading(method)?, self.model.clone());
            &fallback_reader
        };
        let from = trace.len();
        let prediction=reader.forward_traced(dsrust::input!{question:question,consultation:serde_json::to_string(&json!({"method":method,"branch":branch,"facet":classification.get("facet"),"facts":facts,"device":device}))?,chart_evidence:serde_json::to_string(&evidence)?,source_text:reading_source},trace).await?;
        relabel(trace, from, &format!("reading.{method}"));
        stages.insert("reading".into(), fields(&prediction));
        self.record_native(input, &stages, "reading")?;
        let citation_pattern = regex::Regex::new(r"\be\d+\b")?;
        for field in ["answer", "analysis", "evidence", "unresolved"] {
            for cited in citation_pattern.find_iter(text(&prediction, field)?) {
                if !native.iter().any(|fact| fact.id == cited.as_str()) {
                    failures.push(format!(
                        "Unknown native fact citation {} in {field}",
                        cited.as_str()
                    ));
                }
            }
        }
        Ok((prediction, Value::Object(stages), failures))
    }
    async fn ask(&self, input: Example, trace: &mut Vec<TraceStep>) -> Result<Prediction> {
        let label = self
            .labels
            .get(&key(&input)?)
            .context("Unknown journey input")?;
        let (mut prediction, stages, failures) = self.oracle(&input, trace).await?;
        // Correct-method source is a reviewer label, never an oracle input.
        // A wrong classification must not also deprive its judge of the book.
        let expected_method = label
            .get("expected")
            .and_then(|v| v["method"].as_str())
            .context("No authored method label")?;
        let mut source = self
            .source
            .get(expected_method)
            .context("No reviewer primary source")?
            .clone();
        if let Some(other) = stages["classification"]["method"]
            .as_str()
            .filter(|m| *m != expected_method)
            .and_then(|actual| self.source.get(actual))
        {
            source
                .push_str("\n\nActual selected method source, for checking the observed route:\n");
            source.push_str(other);
        }
        // The TSV already carries every exact native detail. Its structured
        // duplicate is retained in receipts, not repeated in the grading ask.
        let mut review_stages = stages.clone();
        review_stages
            .as_object_mut()
            .context("Invalid stages")?
            .remove("full_native_facts");
        let review=self.judge.forward(dsrust::input!{function:"complete_first_message_reading",inputs:serde_json::to_string(&input.rendered())?,prediction:serde_json::to_string(&review_stages)?,rubric:serde_json::to_string(label.get("expected").context("No authored rubric")?)?,source_text:source}).await?;
        let score = review
            .get("score")
            .and_then(Value::as_f64)
            .context("No reviewer score")?;
        ensure!(
            score.is_finite() && (0.0..=1.0).contains(&score),
            "Invalid reviewer score"
        );
        for dimension in ["classification", "extraction", "reading", "answer_quality"] {
            ensure!(
                matches!(text(&review, dimension)?, "pass" | "fail" | "unobserved"),
                "Invalid semantic dimension"
            );
        }
        ensure!(
            !text(&review, "feedback")?.trim().is_empty(),
            "Independent review omitted its evidence"
        );
        let semantic_pass = ["classification", "extraction", "reading", "answer_quality"]
            .iter()
            .all(|d| text(&review, d).is_ok_and(|s| s == "pass"));
        let passed = failures.is_empty()
            && semantic_pass
            && review.get("passed") == Some(&json!(true))
            && text(&prediction, "status")? == "answer";
        let score = if !failures.is_empty() {
            0.
        } else if passed {
            1.
        } else {
            score.min(0.99)
        };
        // GEPA receives this only for its training minibatch. Provide the actual
        // source to the prompt writer as well as to the independent grader.
        let feedback = format!(
            "Native guards: {failures:?}. {}\nSALIENT PRIMARY FRAWLEY CONTEXT FOR THIS QUESTION TYPE:\n{source}",
            text(&review, "feedback")?
        );
        prediction.example.set("__score", json!(score));
        prediction.example.set("__feedback", json!(feedback));
        prediction.example.set("__passed", json!(passed));
        let observation = json!({"case_id":label.get("case_id"),"original_inputs":input.rendered(),"stages":stages,"compiled_components":self.component_sources,"review":fields(&review),"native_failures":failures,"score":score,"passed":prediction.get("__passed")});
        if let Some(path) = &self.stream {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)?;
            writeln!(file, "{}", serde_json::to_string(&observation)?)?;
            file.sync_data()?;
        }
        self.observations
            .lock()
            .map_err(|_| anyhow::anyhow!("Trace lock poisoned"))?
            .push(observation);
        eprintln!(
            "journey {} score {:.3}",
            label.get("case_id").unwrap_or(&Value::Null),
            score
        );
        Ok(prediction)
    }
    pub fn save_observations(&self, path: &Path) -> Result<()> {
        fs::write(
            path,
            serde_json::to_vec_pretty(
                &*self
                    .observations
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Trace lock poisoned"))?,
            )?,
        )?;
        Ok(())
    }
    pub fn compact_demos(&mut self) -> Result<()> {
        for predictor in self.named_predictors() {
            for demo in predictor.demos {
                *demo = crate::demo::compact(demo)?;
            }
        }
        Ok(())
    }
}
impl Module for Journey {
    fn forward<'a>(
        &'a self,
        input: Example,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(async move { self.ask(input, &mut Vec::new()).await })
    }
    fn forward_traced<'a>(
        &'a self,
        input: Example,
        trace: &'a mut Vec<TraceStep>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(self.ask(input, trace))
    }
    fn named_predictors(&mut self) -> Vec<NamedPredictor<'_>> {
        // A family-specific compiler must not learn an always-this-family
        // classifier. Classification is shared and measured on the full bank.
        let mut predictors = Vec::new();
        let mut extraction = self.extractor.named_predictors();
        for p in &mut extraction {
            p.name = format!("extraction.{}", self.family);
        }
        let mut reading = self.reader.named_predictors();
        for p in &mut reading {
            p.name = format!("reading.{}", self.family);
        }
        predictors.extend(extraction);
        predictors.extend(reading);
        predictors
    }
}
