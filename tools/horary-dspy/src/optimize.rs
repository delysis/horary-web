//! Use the port's Predict, Module, BootstrapFewShot, Evaluate and GEPA directly.
//! This file supplies domain inputs/metrics; it contains no search algorithm.
use crate::{
    book::Book,
    catalogue::{self, Case, Fact},
    functions::{self, Fixture},
    reading::ReadingProgram,
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{BootstrapFewShot, Evaluate, Example, GEPA, Module, Predict};
use dsrust::{
    Feedback,
    example::Prediction,
    lm::{DynChatModel, LmConfig},
    module::{NamedPredictor, TraceStep, relabel},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

fn input_key(input: &Example) -> Result<String> {
    Ok(crate::google::digest(serde_json::to_vec(
        &input.rendered(),
    )?))
}
pub(crate) fn plain(
    signature: dsrust::signature::Signature,
    model: Arc<dyn DynChatModel>,
) -> Predict {
    Predict::from_signature(signature)
        .with_lm(model)
        .with_adapter(dsrust::adapter::ChatAdapter::without_json_fallback())
        .with_config(LmConfig {
            max_tokens: Some(4096),
            temperature: Some(0.),
            ..Default::default()
        })
}
pub(crate) fn fields(prediction: &Prediction) -> Value {
    Value::Object(
        prediction
            .example
            .fields()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect(),
    )
}
pub(crate) fn text<'a>(prediction: &'a Prediction, name: &str) -> Result<&'a str> {
    prediction
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing {name}"))
}

fn classification_target(example: &Example) -> Result<(&str, &str)> {
    let expected = example
        .get("expected")
        .context("No private classification labels")?;
    let method = expected["method"]
        .as_str()
        .context("No authored classification method")?;
    let facet = expected["rubric"]["facet"]
        .as_str()
        .context("No authored classification facet")?;
    ensure!(
        !method.is_empty() && !facet.is_empty(),
        "Empty authored classification label"
    );
    Ok((method, facet))
}

/// Known labels belong to the metric only. A fluent teacher pass cannot erase
/// a received method/facet mismatch, and a missing semantic review stays unknown.
pub fn classification_label_failures(
    example: &Example,
    prediction: &Prediction,
) -> Result<Vec<String>> {
    let (method, facet) = classification_target(example)?;
    let mut errors = Vec::new();
    if text(prediction, "method")? != method {
        errors.push(format!(
            "Classification method differs from authored label {method}"
        ));
    }
    if text(prediction, "facet")? != facet {
        errors.push(format!(
            "Classification facet differs from authored label {facet}"
        ));
    }
    Ok(errors)
}

enum Oracle {
    Single(Predict),
    Reading(Box<ReadingProgram>),
}

/// Only the oracle program is optimizable. The independent criterion does
/// not become a second conversational stage with authority to redefine its task.
pub struct Graded {
    oracle: Oracle,
    judge: Predict,
    examples: BTreeMap<String, Example>,
    function: String,
    predictor_name: String,
    observations: Arc<Mutex<Vec<Value>>>,
    stream: Option<PathBuf>,
    failed: Arc<AtomicBool>,
}
impl Graded {
    pub fn new(
        method: &str,
        function: &str,
        examples: &[Example],
        student: Arc<dyn DynChatModel>,
        teacher: Arc<dyn DynChatModel>,
        book: Option<Arc<Book>>,
    ) -> Result<Self> {
        let signature = match function {
            "reading" => signatures::reading(method)?,
            "extraction" => signatures::extraction(method)?,
            _ => signatures::auxiliary(function)?,
        };
        let mut index = BTreeMap::new();
        for example in examples {
            if function == "classification" {
                classification_target(example)?;
            }
            ensure!(
                index
                    .insert(input_key(&example.inputs()?)?, example.clone())
                    .is_none(),
                "Duplicate neural inputs have ambiguous labels"
            );
        }
        let mut judge = plain(signatures::auxiliary("judge")?, teacher);
        judge
            .signature
            .instructions
            .push_str(&functions::grading_instruction(function)?);
        Ok(Self {
            oracle: if function == "reading" {
                Oracle::Reading(Box::new(ReadingProgram::new(
                    method,
                    book.context("Reading program requires the source-checked book")?,
                    student,
                )?))
            } else {
                Oracle::Single(plain(signature, student).with_config(LmConfig {
                    max_tokens: Some(if function == "classification" {
                        512
                    } else {
                        4096
                    }),
                    temperature: Some(0.),
                    ..Default::default()
                }))
            },
            judge,
            examples: index,
            function: function.into(),
            predictor_name: predictor_name(method, function)?,
            observations: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            failed: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn with_stream(mut self, path: PathBuf) -> Result<Self> {
        fs::create_dir_all(path.parent().context("No component stream directory")?)?;
        self.stream = Some(path);
        Ok(self)
    }
    fn record(&self, suffix: bool, observation: &Value) -> Result<()> {
        if let Some(path) = &self.stream {
            use std::io::Write;
            let path = if suffix {
                path.with_extension("native.jsonl")
            } else {
                path.clone()
            };
            let mut file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)?;
            writeln!(file, "{}", serde_json::to_string(observation)?)?;
            file.sync_data()?;
        }
        Ok(())
    }
    pub fn save_observations(&self, path: &Path) -> Result<()> {
        crate::queue::write_json_atomic(
            path,
            &json!(
                &*self
                    .observations
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?
            ),
        )
    }
    async fn ask(&self, input: Example, trace: &mut Vec<TraceStep>) -> Result<Prediction> {
        let input = input.inputs()?;
        let example = self
            .examples
            .get(&input_key(&input)?)
            .context("Unknown evaluation input")?;
        let source = example
            .get("__review_source")
            .or_else(|| input.get("source_text"))
            .or_else(|| input.get("requirements"))
            .or_else(|| input.get("catalogue"))
            .cloned()
            .context("No actual primary context for independent review")?;
        let from = trace.len();
        let attempted = match &self.oracle {
            Oracle::Single(predictor) => {
                let prediction = predictor.forward_traced(input.clone(), trace).await;
                relabel(trace, from, &self.predictor_name);
                prediction
            }
            Oracle::Reading(program) => program.forward_traced(input.clone(), trace).await,
        };
        let oracle_stages = trace[from..].iter().map(|step|json!({"predictor":step.predictor,"inputs":step.inputs.rendered(),"outputs":step.outputs.rendered()})).collect::<Vec<_>>();
        let mut prediction = match attempted {
            Ok(prediction) => prediction,
            Err(error) => {
                let Some(observed) = error.downcast_ref::<dsrust::predict::ObservedOutputFailure>()
                else {
                    return Err(error);
                };
                // The port only creates this type after receiving a response.
                // Missing output fields are an observed format rejection, with no
                // fabricated semantic fields and no extra judge or provider call.
                let raw = observed.response.first_text();
                let feedback = format!(
                    "Native output-format rejection: {}. No valid function output was supplied. Fully received model reply:\n{raw}\nSALIENT PRIMARY FRAWLEY CONTEXT FOR THIS QUESTION TYPE:\n{source}",
                    observed.reason
                );
                let mut rejected =
                    Prediction::new(Example::new(std::iter::empty::<(String, Value)>()), raw)
                        .with_usage(observed.response.spend());
                rejected.example.set("__score", json!(0.));
                rejected.example.set("__passed", json!(false));
                rejected.example.set("__feedback", json!(feedback));
                let observation = json!({"case_id":example.get("case_id"),"function":self.function,"inputs":input.rendered(),"oracle_stages":oracle_stages,"prediction":fields(&rejected),"received_response":observed.response,"native_failures":[observed.reason],"format_rejection":true,"review":null,"independently_reviewed":false,"review_summary_consistent":true,"review_consistency_scope":"No independent semantic summary exists; deterministic format rejection cannot earn a positive score","score":0.,"passed":false});
                self.record(true, &observation)?;
                self.record(false, &observation)?;
                self.observations
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?
                    .push(observation);
                eprintln!(
                    "{} {} received output-format rejection score 0",
                    self.function,
                    example.get("case_id").unwrap_or(&Value::Null)
                );
                return Ok(rejected);
            }
        };
        self.record(true, &json!({"case_id":example.get("case_id"),"function":self.function,"inputs":input.rendered(),"oracle_stages":oracle_stages,"prediction":fields(&prediction),"independently_reviewed":false}))?;
        let mut errors = Vec::new();
        if let Some(failures) = prediction.get("__native_failures") {
            for failure in failures
                .as_array()
                .context("Invalid internal native failures")?
            {
                errors.push(
                    failure
                        .as_str()
                        .context("Invalid internal native failure")?
                        .to_owned(),
                );
            }
        }
        if self.function == "extraction" {
            let facts: Vec<Fact> = match serde_json::from_str(text(&prediction, "facts")?) {
                Ok(facts) => facts,
                Err(error) => {
                    errors.push(format!("Native fact array rejected: {error}"));
                    Vec::new()
                }
            };
            let words = input
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let history: Vec<catalogue::Turn> = serde_json::from_str(
                input
                    .get("conversation")
                    .and_then(Value::as_str)
                    .unwrap_or("[]"),
            )?;
            let mut paths = std::collections::BTreeSet::new();
            for fact in &facts {
                if fact.field.trim().is_empty() || !paths.insert(fact.field.as_str()) {
                    errors.push(
                        "Native fact array contains an empty or duplicate canonical path".into(),
                    );
                }
                if !fact.quote.is_empty()
                    && !words.contains(&fact.quote)
                    && !history
                        .iter()
                        .any(|t| t.role == "user" && t.content.contains(&fact.quote))
                {
                    errors.push(format!(
                        "{}: quotation is not an exact source span",
                        fact.field
                    ));
                }
                if fact.quote.is_empty() && text(&prediction, "inferences")?.trim().is_empty() {
                    errors.push(format!(
                        "{}: no source quote or inference witness",
                        fact.field
                    ));
                }
            }
            if let Err(error) = serde_json::from_str::<Vec<String>>(text(&prediction, "missing")?) {
                errors.push(format!("Native missing-input array rejected: {error}"));
            }
        } else if functions::AUXILIARY.contains(&self.function.as_str()) {
            errors.extend(functions::native_failures(
                &self.function,
                &input,
                &prediction,
            )?);
        } else if self.function == "classification" {
            let method = text(&prediction, "method")?;
            let facet = text(&prediction, "facet")?;
            let branch = text(&prediction, "branch")?;
            errors.extend(classification_label_failures(example, &prediction)?);
            if !signatures::METHODS.contains(&method) {
                errors.push(format!("Unknown method {method}"));
            } else {
                if !catalogue::method_facets(method)?
                    .iter()
                    .any(|allowed| allowed == facet)
                {
                    errors.push(format!("Unknown {method} facet {facet}"));
                }
                if !catalogue::coverage_card(method)?["judgment_subbranches"]
                    .as_array()
                    .context("No branches")?
                    .iter()
                    .any(|b| b["id"] == branch)
                {
                    errors.push(format!("Unknown {method} branch {branch}"));
                }
            }
        } else if self.function == "reading" {
            let chart: Value = serde_json::from_str(
                input
                    .get("chart_evidence")
                    .and_then(Value::as_str)
                    .context("No actual chart")?,
            )?;
            let mut ids = std::collections::BTreeSet::new();
            if let Some(table) = chart["facts"].as_str() {
                ids.extend(
                    table
                        .lines()
                        .skip(1)
                        .filter_map(|row| row.split('\t').next())
                        .map(str::to_owned),
                );
            } else if let Some(facts) = chart["native_facts"].as_array() {
                ids.extend(
                    facts
                        .iter()
                        .filter_map(|fact| fact["id"].as_str())
                        .map(str::to_owned),
                );
            }
            ensure!(
                !ids.is_empty(),
                "Reading component lacks native evidence IDs"
            );
            let pattern = regex::Regex::new(r"\be[0-9]+\b")?;
            for field in ["answer", "analysis", "evidence", "unresolved"] {
                for found in pattern.find_iter(text(&prediction, field)?) {
                    if !ids.contains(found.as_str()) {
                        errors.push(format!(
                            "{field}: cites absent native evidence {}",
                            found.as_str()
                        ));
                    }
                }
            }
            if !matches!(
                text(&prediction, "status")?,
                "answer" | "need_information" | "method_limit"
            ) {
                errors.push("Unknown reading status".into());
            }
        }
        let review = self.judge.forward(dsrust::input!{
            function:self.function,inputs:serde_json::to_string(&input.rendered())?,prediction:serde_json::to_string(&fields(&prediction))?,rubric:serde_json::to_string(example.get("expected").context("No independently authored rubric")?)?,source_text:source
        }).await?;
        let judged = review
            .get("score")
            .and_then(Value::as_f64)
            .context("No semantic score")?;
        ensure!(
            judged.is_finite() && (0.0..=1.0).contains(&judged),
            "Invalid independent score"
        );
        let feedback = text(&review, "feedback")?;
        ensure!(
            !feedback.trim().is_empty(),
            "Independent review omitted its evidence"
        );
        for dimension in ["classification", "extraction", "reading", "answer_quality"] {
            ensure!(
                matches!(text(&review, dimension)?, "pass" | "fail" | "unobserved"),
                "Invalid function review dimension"
            );
        }
        let dimensions = functions::dimensions(&self.function)?;
        let semantic_pass = dimensions
            .iter()
            .all(|dimension| text(&review, dimension).is_ok_and(|v| v == "pass"));
        let raw_passed = review
            .get("passed")
            .and_then(Value::as_bool)
            .context("No independent pass decision")?;
        // Inapplicable stages do not veto a component. A contradictory summary
        // is retained visibly instead of being silently rewritten for a win.
        let summary_consistent = raw_passed == semantic_pass;
        let passed = errors.is_empty() && semantic_pass;
        let score = if !errors.is_empty() {
            0.
        } else if passed {
            1.
        } else {
            judged.min(0.99)
        };
        prediction.example.set("__score", json!(score));
        prediction.example.set("__feedback",json!(format!("Native checks: {errors:?}. Applicable dimensions: {dimensions:?}. Independent summary consistent with those dimensions: {summary_consistent}. Independent review: {feedback}\nSALIENT PRIMARY FRAWLEY CONTEXT FOR THIS QUESTION TYPE:\n{source}")));
        prediction.example.set("__passed", json!(passed));
        let observation = json!({"case_id":example.get("case_id"),"function":self.function,"inputs":input.rendered(),"oracle_stages":oracle_stages,"prediction":fields(&prediction),"review":fields(&review),"native_failures":errors,"applicable_dimensions":dimensions,"review_summary_consistent":summary_consistent,"score":score,"passed":passed});
        self.record(false, &observation)?;
        self.observations
            .lock()
            .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?
            .push(observation);
        eprintln!(
            "{} {} score {:.3}",
            self.function,
            example.get("case_id").unwrap_or(&Value::Null),
            score
        );
        Ok(prediction)
    }
    async fn guarded(&self, input: Example, trace: &mut Vec<TraceStep>) -> Result<Prediction> {
        ensure!(
            !self.failed.load(Ordering::SeqCst),
            "Component stopped after an unobserved execution/review failure; no new call submitted"
        );
        let result = self.ask(input, trace).await;
        if result.is_err() {
            self.failed.store(true, Ordering::SeqCst);
        }
        result
    }
    pub fn metric(_example: &Example, prediction: &Prediction) -> f64 {
        prediction
            .get("__score")
            .and_then(Value::as_f64)
            .unwrap_or(0.)
    }
    pub fn feedback(example: &Example, prediction: &Prediction) -> Feedback {
        Feedback::new(
            Self::metric(example, prediction),
            prediction
                .get("__feedback")
                .and_then(Value::as_str)
                .unwrap_or("Missing independent observation"),
        )
    }
}
impl Module for Graded {
    fn forward<'a>(
        &'a self,
        input: Example,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(async move { self.guarded(input, &mut Vec::new()).await })
    }
    fn forward_traced<'a>(
        &'a self,
        input: Example,
        trace: &'a mut Vec<TraceStep>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(self.guarded(input, trace))
    }
    fn named_predictors(&mut self) -> Vec<NamedPredictor<'_>> {
        match &mut self.oracle {
            Oracle::Reading(program) => program.named_predictors(),
            Oracle::Single(predictor) => predictor
                .named_predictors()
                .into_iter()
                .map(|mut predictor| {
                    predictor.name = self.predictor_name.clone();
                    predictor
                })
                .collect(),
        }
    }
}

pub async fn compile(
    cases: &[Case],
    method: &str,
    function: &str,
    book: &Book,
    models: &crate::runner::Models,
    directory: &Path,
    max_metrics: usize,
) -> Result<Value> {
    let fixtures = cases
        .iter()
        .map(|case| {
            Ok(Fixture {
                split: case.split.clone(),
                example: catalogue::example(case, function, book)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    compile_fixtures(
        &fixtures,
        method,
        function,
        Arc::new(book.clone()),
        models,
        directory,
        CompileOptions {
            max_metrics,
            bootstrap_demos: 2,
        },
    )
    .await
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompileOptions {
    pub max_metrics: usize,
    /// Zero is conventional instruction-only GEPA. Nonzero explicitly enables
    /// the port's BootstrapFewShot first; it is not a different search engine.
    pub bootstrap_demos: usize,
}

pub fn predictor_name(method: &str, function: &str) -> Result<String> {
    match function {
        "reading" | "extraction" => {
            ensure!(
                signatures::METHODS.contains(&method) && !matches!(method, "wish" | "unclassified"),
                "Select a substantive family"
            );
            Ok(format!("{function}.{method}"))
        }
        "classification" | "place" | "moment" | "update" => Ok(function.to_owned()),
        _ => anyhow::bail!("Unknown component function"),
    }
}

fn declaration(method: &str, function: &str) -> Result<dsrust::signature::Signature> {
    match function {
        "reading" => signatures::reading(method),
        "extraction" => signatures::extraction(method),
        _ => signatures::auxiliary(function),
    }
}

fn declaration_digest(method: &str, function: &str) -> Result<String> {
    let signature = declaration(method, function)?;
    Ok(crate::google::digest(serde_json::to_vec(&json!({
        "signature":dsrust::module::SignatureState::of(&signature),
        "inputs":signature.inputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>(),
        "outputs":signature.outputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>()
    }))?))
}

pub fn teaching_sources() -> BTreeMap<String, String> {
    [
        ("book.rs", include_bytes!("book.rs").as_slice()),
        ("reading.rs", include_bytes!("reading.rs").as_slice()),
        ("anchors.rs", include_bytes!("anchors.rs").as_slice()),
        (
            "coverage.json",
            include_bytes!("../coverage.json").as_slice(),
        ),
        (
            "reading-catalogue.json",
            include_bytes!("../../../docs/llm-process/reading-catalogue.json").as_slice(),
        ),
        (
            "source-manifest.json",
            include_bytes!("../../../docs/llm-process/source-manifest.json").as_slice(),
        ),
        (
            "passages.json",
            include_bytes!("../../../src-tauri/src/horary_prompts/passages.json").as_slice(),
        ),
        (
            "native-reading-method.rs",
            include_bytes!("../../../src-tauri/src/reading_method.rs").as_slice(),
        ),
        (
            "native-geocode.rs",
            include_bytes!("../../../src-tauri/src/geocode.rs").as_slice(),
        ),
        (
            "cities.json",
            include_bytes!("../../../src/data/cities.json").as_slice(),
        ),
        (
            "us_locations.json",
            include_bytes!("../../../src/data/us_locations.json").as_slice(),
        ),
        (
            "country_names.json",
            include_bytes!("../../../src/data/country_names.json").as_slice(),
        ),
        (
            "core/lib.rs",
            include_bytes!("../../../crates/horary-ai-core/src/lib.rs").as_slice(),
        ),
        (
            "core/astronomy.rs",
            include_bytes!("../../../crates/horary-ai-core/src/astronomy.rs").as_slice(),
        ),
        (
            "core/book_method.rs",
            include_bytes!("../../../crates/horary-ai-core/src/book_method.rs").as_slice(),
        ),
        (
            "core/chart_input.rs",
            include_bytes!("../../../crates/horary-ai-core/src/chart_input.rs").as_slice(),
        ),
        (
            "core/events.rs",
            include_bytes!("../../../crates/horary-ai-core/src/events.rs").as_slice(),
        ),
        (
            "core/anchor_authority.rs",
            include_bytes!("../../../crates/horary-ai-core/src/anchor_authority.rs").as_slice(),
        ),
        (
            "core/ephemeris_coefficients.rs",
            include_bytes!("../../../crates/horary-ai-core/src/ephemeris_coefficients.rs")
                .as_slice(),
        ),
        (
            "core/review.rs",
            include_bytes!("../../../crates/horary-ai-core/src/review.rs").as_slice(),
        ),
        (
            "horary-interpretation.schema.json",
            include_bytes!("../../../src/ai/horary-interpretation.schema.json").as_slice(),
        ),
        (
            "horary-judgement-pipeline.json",
            include_bytes!("../../../src/ai/horary-judgement-pipeline.json").as_slice(),
        ),
    ]
    .into_iter()
    .map(|(name, bytes)| (name.to_owned(), crate::google::digest(bytes)))
    .collect()
}

pub fn fixtures_digest(fixtures: &[Fixture]) -> Result<String> {
    Ok(crate::google::digest(serde_json::to_vec(
        &fixtures
            .iter()
            .map(|fixture| json!({"split":fixture.split,"example":fixture.example.rendered()}))
            .collect::<Vec<_>>(),
    )?))
}

fn source_receipt(
    method: &str,
    function: &str,
    compiled: &[u8],
    dataset_digest: &str,
) -> Result<Value> {
    let mut receipt = json!({"version":1,"method":method,"function":function,"predictor_name":predictor_name(method,function)?,
        "port_revision":crate::native_export::PORT_REVISION,"signatures_sha256":crate::google::digest(include_bytes!("signatures.rs")),
        "effective_port_revision":crate::local::PORT_REVISION,
        "port_predict_sha256":crate::google::digest(include_bytes!("../vendor/dsrust/src/predict.rs")),
        "port_openai_sha256":crate::google::digest(include_bytes!("../vendor/dsrust/src/lm/openai/mod.rs")),
        "declaration_sha256":declaration_digest(method,function)?,"catalogue_sha256":crate::google::digest(include_bytes!("catalogue.rs")),
        "functions_sha256":crate::google::digest(include_bytes!("functions.rs")),"optimizer_source_sha256":crate::google::digest(include_bytes!("optimize.rs")),
        "book_sha256":crate::book::OCR_SHA256,"teaching_sources_sha256":teaching_sources(),"compiled_sha256":crate::google::digest(compiled),"dataset_sha256":dataset_digest});
    if function == "reading" {
        receipt["version"] = json!(crate::reading::VERSION);
        receipt["program_kind"] = json!("branch_scoped_reading");
        receipt["predictor_names"] =
            json!(declarations(method, function)?.keys().collect::<Vec<_>>());
        receipt["reading_module_sha256"] =
            json!(crate::google::digest(include_bytes!("reading.rs")));
        let signature = crate::reading::selector_signature();
        receipt["branch_declaration_sha256"] = json!(crate::google::digest(serde_json::to_vec(
            &json!({
                "signature":dsrust::module::SignatureState::of(&signature),
                "inputs":signature.inputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>(),
                "outputs":signature.outputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>()
            })
        )?));
    }
    Ok(receipt)
}

fn declarations(
    method: &str,
    function: &str,
) -> Result<BTreeMap<String, dsrust::signature::Signature>> {
    let mut declarations = BTreeMap::from([(
        predictor_name(method, function)?,
        declaration(method, function)?,
    )]);
    if function == "reading" {
        declarations.insert(
            crate::reading::selector_name(method),
            crate::reading::selector_signature(),
        );
    }
    Ok(declarations)
}

fn exact_keys(value: &Value, keys: &[&str]) -> Result<()> {
    use std::collections::BTreeSet;
    ensure!(
        value
            .as_object()
            .context("Expected saved object")?
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == keys.iter().copied().collect(),
        "Saved state has missing or unexpected fields"
    );
    Ok(())
}

/// The port intentionally tolerates partial saved state; a composition gate
/// cannot. Verify source identity, exact predictor membership, ordered fields,
/// types and demo privacy before changing the existing live predictor.
fn validated_state(
    path: &Path,
    method: &str,
    function: &str,
) -> Result<dsrust::module::ProgramState> {
    let compiled = fs::read(path)?;
    let receipt: Value = serde_json::from_slice(&fs::read(
        path.parent()
            .context("No component parent")?
            .join("component-source.json"),
    )?)?;
    let dataset_digest = receipt["dataset_sha256"]
        .as_str()
        .context("Component source has no dataset identity")?;
    ensure!(
        dataset_digest.len() == 64 && dataset_digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid component dataset identity"
    );
    ensure!(
        receipt == source_receipt(method, function, &compiled, dataset_digest)?,
        "Compiled component source identity differs"
    );
    let value: Value = serde_json::from_slice(&compiled)?;
    let declarations = declarations(method, function)?;
    let mut keys = vec!["metadata"];
    keys.extend(declarations.keys().map(String::as_str));
    exact_keys(&value, &keys)?;
    exact_keys(&value["metadata"], &["dependency_versions"])?;
    exact_keys(&value["metadata"]["dependency_versions"], &["dspy"])?;
    for name in declarations.keys() {
        exact_keys(
            &value[name],
            &["traces", "train", "demos", "signature", "lm"],
        )?;
        exact_keys(&value[name]["signature"], &["instructions", "fields"])?;
        for field in value[name]["signature"]["fields"]
            .as_array()
            .context("No ordered fields")?
        {
            exact_keys(field, &["prefix", "description"])?;
        }
    }
    let state: dsrust::module::ProgramState = serde_json::from_value(value)?;
    ensure!(
        state.metadata == dsrust::module::Metadata::default(),
        "Pinned DSPy metadata differs"
    );
    for (name, signature) in declarations {
        let saved = state.get(&name).context("Required component missing")?;
        ensure!(
            saved.signature.fields == dsrust::module::SignatureState::of(&signature).fields,
            "Saved ordered fields differ"
        );
        ensure!(
            !saved.signature.instructions.trim().is_empty()
                && saved.traces.is_empty()
                && saved.train.is_empty()
                && saved.lm.is_none(),
            "Saved component carries empty teaching or runtime/training state"
        );
        let permitted = signature
            .inputs
            .iter()
            .map(|field| field.name.as_str())
            .chain(signature.outputs.iter().map(|field| field.name.as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        for demo in &saved.demos {
            ensure!(
                demo.keys().all(|key| permitted.contains(key.as_str()))
                    && demo.values().all(Value::is_string),
                "Saved demo contains private/unknown or differently typed fields"
            );
            ensure!(
                signature
                    .outputs
                    .iter()
                    .all(|field| demo.contains_key(&field.name)),
                "Saved demo lacks required outputs"
            );
        }
    }
    Ok(state)
}

fn require_baseline(predictor: &Predict, signature: &dsrust::signature::Signature) -> Result<()> {
    ensure!(
        dsrust::module::SignatureState::of(&predictor.signature)
            == dsrust::module::SignatureState::of(signature),
        "Target predictor is not the baseline declaration"
    );
    Ok(())
}

fn restore(
    predictor: &mut Predict,
    saved: &dsrust::module::PredictorState,
    signature: &dsrust::signature::Signature,
) {
    // All validation precedes mutation. Apply only the exact predictor's fields.
    saved.signature.restore(&mut predictor.signature);
    predictor.demos = saved
        .demos
        .iter()
        .map(|demo| {
            Example::new(
                demo.iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            )
            .with_inputs(signature.inputs.iter().map(|field| field.name.clone()))
        })
        .collect();
}

/// Both predictors validate before mutation. Older one-Predict reading states
/// are not silently upgraded to a different executable program.
pub fn load_reading_component(
    program: &mut ReadingProgram,
    path: &Path,
    method: &str,
    expected_dataset_sha256: &str,
) -> Result<()> {
    require_dataset(path, expected_dataset_sha256)?;
    let state = validated_state(path, method, "reading")?;
    let selector = crate::reading::selector_signature();
    let reader = signatures::reading(method)?;
    require_baseline(&program.selector, &selector)?;
    require_baseline(&program.reader, &reader)?;
    restore(
        &mut program.selector,
        state
            .get(&crate::reading::selector_name(method))
            .context("No selector")?,
        &selector,
    );
    restore(
        &mut program.reader,
        state
            .get(&predictor_name(method, "reading")?)
            .context("No reader")?,
        &reader,
    );
    Ok(())
}

pub fn load_component(
    predictor: &mut Predict,
    path: &Path,
    method: &str,
    function: &str,
) -> Result<()> {
    ensure!(
        function != "reading",
        "Standalone reading requires the versioned reading Module loader"
    );
    let state = validated_state(path, method, function)?;
    let signature = declaration(method, function)?;
    require_baseline(predictor, &signature)?;
    restore(
        predictor,
        state
            .get(&predictor_name(method, function)?)
            .context("Required component missing")?,
        &signature,
    );
    Ok(())
}

/// A fresh Journey owns a classified, validated branch. Validate the whole
/// reading artifact, then explicitly apply only its reader; the standalone
/// selector remains in the immutable source artifact and is not executed.
pub fn load_classified_reader(
    predictor: &mut Predict,
    path: &Path,
    method: &str,
    expected_dataset_sha256: &str,
) -> Result<Value> {
    require_dataset(path, expected_dataset_sha256)?;
    let state = validated_state(path, method, "reading")?;
    let signature = signatures::reading(method)?;
    require_baseline(predictor, &signature)?;
    let name = predictor_name(method, "reading")?;
    let selector_name = crate::reading::selector_name(method);
    let saved_selector = state.get(&selector_name).context("No saved selector")?;
    let receipt = json!({"reading_module_version":crate::reading::VERSION,"compiled_sha256":crate::google::digest(fs::read(path)?),"applied_predictor":name,"bypassed_predictor":selector_name,"bypassed_predictor_sha256":crate::google::digest(serde_json::to_vec(saved_selector)?),"branch_authority":"Actual fresh Journey classifier; guarded before extraction/chart/reading"});
    restore(
        predictor,
        state.get(&name).context("No saved reader")?,
        &signature,
    );
    Ok(receipt)
}

fn require_dataset(path: &Path, expected_dataset_sha256: &str) -> Result<()> {
    let receipt: Value = serde_json::from_slice(&fs::read(
        path.parent()
            .context("No component parent")?
            .join("component-source.json"),
    )?)?;
    ensure!(
        receipt["dataset_sha256"] == expected_dataset_sha256,
        "Compiled component belongs to a different fixture/input/label/split bank"
    );
    Ok(())
}

pub fn load_component_for_dataset(
    predictor: &mut Predict,
    path: &Path,
    method: &str,
    function: &str,
    expected_dataset_sha256: &str,
) -> Result<()> {
    require_dataset(path, expected_dataset_sha256)?;
    load_component(predictor, path, method, function)
}

/// Serialize observed grades separately from execution errors. DsRust's own
/// Evaluate keeps a failure_score for aggregation; it is never reported as an
/// observed semantic zero here.
pub fn evaluation_receipt(evaluation: &dsrust::Evaluation) -> Value {
    json!({"score":if evaluation.failure_count()==0{Some(evaluation.score)}else{None},
        "case_count":evaluation.results.len(),"execution_errors":evaluation.failure_count(),
        "observations":evaluation.results.iter().map(|row|match &row.prediction {
            Ok(prediction)=>json!({"case_id":row.example.get("case_id"),"score":row.score,"passed":prediction.get("__passed"),"prediction":fields(prediction)}),
            Err(error)=>json!({"case_id":row.example.get("case_id"),"score":null,"passed":null,"execution_error":error})
        }).collect::<Vec<_>>()})
}

struct ReflectionGuard {
    inner: Arc<dyn DynChatModel>,
    failed: Arc<AtomicBool>,
}
impl DynChatModel for ReflectionGuard {
    fn forward_dyn<'a>(
        &'a self,
        http: &'a reqwest::Client,
        request: &'a dsrust::lm::api::LmRequest,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<dsrust::lm::api::LmResponse>> + Send + 'a>>
    {
        Box::pin(async move {
            ensure!(
                !self.failed.load(Ordering::SeqCst),
                "Unobserved component failure withheld reflection; no teacher call submitted"
            );
            let result = self.inner.forward_dyn(http, request).await;
            if result.is_err() {
                self.failed.store(true, Ordering::SeqCst);
            }
            result
        })
    }
    fn capabilities_dyn<'a>(
        &'a self,
        http: &'a reqwest::Client,
    ) -> std::pin::Pin<Box<dyn Future<Output = dsrust::lm::Capabilities> + Send + 'a>> {
        self.inner.capabilities_dyn(http)
    }
    fn native_reasoning_usable_dyn(&self) -> bool {
        self.inner.native_reasoning_usable_dyn()
    }
}

pub async fn compile_fixtures(
    fixtures: &[Fixture],
    method: &str,
    function: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
) -> Result<Value> {
    ensure!(
        options.max_metrics > 0,
        "GEPA needs a positive metric budget"
    );
    fs::create_dir_all(directory)?;
    let mut train = Vec::new();
    let mut development = Vec::new();
    let mut test = Vec::new();
    for fixture in fixtures {
        match fixture.split.as_str() {
            "train" => train.push(fixture.example.clone()),
            "development" => development.push(fixture.example.clone()),
            "test" => test.push(fixture.example.clone()),
            _ => anyhow::bail!("Unknown component split"),
        }
    }
    ensure!(
        !train.is_empty() && !development.is_empty() && !test.is_empty(),
        "All three component pools are required"
    );
    let all = train
        .iter()
        .chain(&development)
        .chain(&test)
        .cloned()
        .collect::<Vec<_>>();
    let mut program = Graded::new(
        method,
        function,
        &all,
        models.student.clone(),
        models.teacher.clone(),
        Some(book.clone()),
    )?
    .with_stream(directory.join("observations.jsonl"))?;
    program.save(&directory.join("baseline-program.json"))?;
    let dataset_digest = fixtures_digest(fixtures)?;
    let before = Evaluate::new(
        development.clone(),
        |input| program.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    crate::queue::write_json_atomic(&directory.join("before.json"), &evaluation_receipt(&before))?;
    program.save_observations(&directory.join("before-observations.json"))?;
    ensure!(
        before.failure_count() == 0,
        "Baseline contains execution/review errors; no learned comparison can be qualified"
    );
    let demos = if options.bootstrap_demos == 0 {
        0
    } else {
        let mut strong = Graded::new(
            method,
            function,
            &all,
            models.teacher.clone(),
            models.teacher.clone(),
            Some(book),
        )?
        .with_stream(directory.join("teacher-observations.jsonl"))?;
        let mut bootstrap = BootstrapFewShot::new(Graded::metric);
        bootstrap.max_bootstrapped_demos = options.bootstrap_demos;
        bootstrap.max_labeled_demos = 0;
        bootstrap.metric_threshold = Some(1.);
        bootstrap.max_errors = 1;
        let result = bootstrap
            .compile_with_teacher(&mut program, &mut strong, &train)
            .await;
        strong.save_observations(&directory.join("teacher-observations.json"))?;
        result?
    };
    program.save(&directory.join("bootstrapped-full.json"))?;
    // Constant primary context is supplied on every real call. Keeping it in
    // every demonstration would repeat entire book chapters without adding data.
    for predictor in program.named_predictors() {
        for demo in predictor.demos {
            *demo = crate::demo::compact(demo)?;
        }
    }
    program.save(&directory.join("bootstrapped.json"))?;
    let reflection = Arc::new(ReflectionGuard {
        inner: models.teacher.clone(),
        failed: program.failed.clone(),
    });
    let outcome = GEPA::new(Graded::feedback, reflection)
        .with_max_metric_calls(options.max_metrics)
        .with_seed(23)
        .compile(&mut program, &train, &development)
        .await?;
    program.save_observations(&directory.join("search-observations.json"))?;
    ensure!(
        !program.failed.load(Ordering::SeqCst),
        "GEPA encountered an unobserved execution/review failure; provisional engine scores/candidate are withheld"
    );
    program.save(&directory.join("compiled.json"))?;
    crate::queue::write_json_atomic(
        &directory.join("component-source.json"),
        &source_receipt(
            method,
            function,
            &fs::read(directory.join("compiled.json"))?,
            &dataset_digest,
        )?,
    )?;
    let after = Evaluate::new(
        development.clone(),
        |input| program.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    crate::queue::write_json_atomic(&directory.join("after.json"), &evaluation_receipt(&after))?;
    let held = Evaluate::new(test, |input| program.forward(input), Graded::metric)
        .run()
        .await;
    crate::queue::write_json_atomic(&directory.join("held.json"), &evaluation_receipt(&held))?;
    program.save_observations(&directory.join("observations.json"))?;
    let observed = after.failure_count() == 0 && held.failure_count() == 0;
    let review_consistent = program
        .observations
        .lock()
        .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?
        .iter()
        .all(|observation| observation["review_summary_consistent"] == true);
    let result = json!({"method":method,"function":function,"teacher":models.teacher_id,"student":models.student_id,"bootstrap_demos":demos,"before_development":before.score,"after_development":if observed{Some(after.score)}else{None},"held_out":if observed{Some(held.score)}else{None},"gepa_best_index":outcome.best_idx,"gepa_metric_calls":outcome.total_num_evals,"gepa_iterations":outcome.iterations,"component_only":true,"declared_inputs":if function=="reading"{"Independent accepted consultation and native chart; no acquisition claimed"}else{"Actual function inputs only"},"execution_observed":observed,"independent_review_consistent":review_consistent,"component_pass":observed && review_consistent && after.results.iter().chain(&held.results).all(|row|row.prediction.as_ref().is_ok_and(|p|p.get("__passed")==Some(&json!(true)))),"application_promoted":false,"source_correct_reading_qualified":false});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        observed,
        "A provider/parser/reviewer error remains unqualified, never a fabricated semantic zero"
    );
    Ok(result)
}

/// A fixed, paired development comparison. This executes the port's Evaluate
/// only: no bootstrap, reflection, selection, held-out reuse or deployment.
pub async fn compare_classification(
    fixtures: &[Fixture],
    candidate_instructions: &str,
    models: &crate::runner::Models,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !fixtures.is_empty() && fixtures.iter().all(|f| f.split == "development"),
        "A fixed comparison requires original development fixtures only"
    );
    ensure!(
        !candidate_instructions.trim().is_empty(),
        "Candidate instruction is empty"
    );
    let examples = fixtures
        .iter()
        .map(|f| f.example.clone())
        .collect::<Vec<_>>();
    let mut baseline = Graded::new(
        "all",
        "classification",
        &examples,
        models.student.clone(),
        models.teacher.clone(),
        None,
    )?
    .with_stream(directory.join("baseline-observations.jsonl"))?;
    let mut candidate = Graded::new(
        "all",
        "classification",
        &examples,
        models.student.clone(),
        models.teacher.clone(),
        None,
    )?
    .with_stream(directory.join("candidate-observations.jsonl"))?;
    match &mut candidate.oracle {
        Oracle::Single(predictor) => {
            // Only the instruction changes. Declarations, inputs, output
            // types, native guards and independent criterion remain fixed.
            predictor.signature.instructions = candidate_instructions.into();
        }
        Oracle::Reading(_) => unreachable!("Classification is a single Predict"),
    }
    baseline.save(&directory.join("baseline-program.json"))?;
    candidate.save(&directory.join("candidate-program.json"))?;
    let before = Evaluate::new(
        examples.clone(),
        |input| baseline.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    crate::queue::write_json_atomic(&directory.join("before.json"), &evaluation_receipt(&before))?;
    baseline.save_observations(&directory.join("baseline-observations.json"))?;
    ensure!(
        before.failure_count() == 0,
        "Baseline execution/review error withholds the paired comparison"
    );
    let after = Evaluate::new(examples, |input| candidate.forward(input), Graded::metric)
        .run()
        .await;
    crate::queue::write_json_atomic(&directory.join("after.json"), &evaluation_receipt(&after))?;
    candidate.save_observations(&directory.join("candidate-observations.json"))?;
    let observed = after.failure_count() == 0;
    let review_consistent =
        [&baseline, &candidate]
            .into_iter()
            .try_fold(true, |all, program| {
                let observations = program
                    .observations
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?;
                Ok::<_, anyhow::Error>(
                    all && observations
                        .iter()
                        .all(|o| o["review_summary_consistent"] == true),
                )
            })?;
    let passed = |evaluation: &dsrust::Evaluation| {
        evaluation
            .results
            .iter()
            .filter(|r| {
                r.prediction
                    .as_ref()
                    .is_ok_and(|p| p.get("__passed") == Some(&json!(true)))
            })
            .count()
    };
    let result = json!({
        "function":"classification", "comparison":"fixed original development pool",
        "cases":fixtures.len(), "teacher":models.teacher_id, "student":models.student_id,
        "baseline_score":before.score, "candidate_score":if observed {Some(after.score)} else {None},
        "baseline_passed":passed(&before), "candidate_passed":passed(&after),
        "execution_observed":observed, "independent_review_consistent":review_consistent,
        "strict_component_gain":observed && review_consistent && after.score > before.score,
        "candidate_all_passed":observed && review_consistent && passed(&after) == fixtures.len(),
        "candidate_instructions_sha256":crate::google::digest(candidate_instructions),
        "dataset_sha256":fixtures_digest(fixtures)?, "reflection_calls":0,
        "application_promoted":false, "source_correct_reading_qualified":false
    });
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        observed,
        "Candidate execution/review error remains unqualified"
    );
    Ok(result)
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use dsrust::DummyLM;

    fn review(
        classification: &str,
        extraction: &str,
        reading: &str,
        answer_quality: &str,
        passed: bool,
    ) -> Example {
        dsrust::example! {score:0.75,passed:passed,classification:classification,extraction:extraction,reading:reading,answer_quality:answer_quality,feedback:"Scripted mechanics review with observed output; not a source-qualified judgment."}
    }
    fn classification_input() -> Example {
        dsrust::example!{question:"Will I sell my bicycle?",conversation:"[]",catalogue:"Actual declared catalogue only.",case_id:"private-case-id",expected:json!({"method":"movable_deal","rubric":{"facet":"event","private":"PRIVATE GOLD MUST STAY OUT OF ORACLE"}}),__review_source:"Private primary context for the independent reviewer."}
            .with_inputs(["question","conversation","catalogue"])
    }
    fn classification_reply() -> Example {
        dsrust::example! {method:"movable_deal",facet:"event",branch:"pure_completion",principal:"querent",clarification:""}
    }
    #[tokio::test]
    async fn fixed_comparison_changes_instruction_only_and_never_reflects_on_development() {
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {method:"movable_deal",facet:"timing",branch:"pure_completion",principal:"querent",clarification:""},
            classification_reply(),
        ]));
        let judge = Arc::new(DummyLM::new([
            review("fail", "unobserved", "unobserved", "unobserved", false),
            review("pass", "unobserved", "unobserved", "unobserved", true),
        ]));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge.clone(),
            student_id: "synthetic-student".into(),
            teacher_id: "synthetic-independent-criterion".into(),
        };
        let fixtures = [Fixture {
            split: "development".into(),
            example: classification_input(),
        }];
        let directory = tempfile::tempdir().unwrap();
        let result = compare_classification(
            &fixtures,
            "FIXED SOURCE-REVIEWED INSTRUCTION",
            &models,
            directory.path(),
        )
        .await
        .unwrap();
        assert_eq!(result["baseline_passed"], 0);
        assert_eq!(result["candidate_passed"], 1);
        assert_eq!(result["strict_component_gain"], true);
        assert_eq!(result["reflection_calls"], 0);
        assert_eq!(judge.asked().len(), 2);
        let requests = oracle.asked();
        assert_eq!(requests.len(), 2);
        assert!(
            !requests[0]
                .system
                .contains("FIXED SOURCE-REVIEWED INSTRUCTION")
        );
        assert!(
            requests[1]
                .system
                .contains("FIXED SOURCE-REVIEWED INSTRUCTION")
        );
        assert_eq!(requests[0].last_message(), requests[1].last_message());
        assert!(requests.iter().all(|r| r.config.max_tokens == Some(512)
            && !r.system.contains("PRIVATE GOLD")
            && !r.last_message().contains("PRIVATE GOLD")));
        let baseline: Value = serde_json::from_slice(
            &fs::read(directory.path().join("baseline-program.json")).unwrap(),
        )
        .unwrap();
        let candidate: Value = serde_json::from_slice(
            &fs::read(directory.path().join("candidate-program.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            baseline["classification"]["signature"]["fields"],
            candidate["classification"]["signature"]["fields"]
        );
        assert_eq!(candidate["classification"]["demos"], json!([]));
    }
    #[tokio::test]
    async fn fixed_comparison_rejects_relabelled_pools_before_any_model_call() {
        let oracle = Arc::new(DummyLM::new([]));
        let judge = Arc::new(DummyLM::new([]));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge.clone(),
            student_id: "synthetic-student".into(),
            teacher_id: "synthetic-independent-criterion".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        for split in ["train", "test"] {
            assert!(
                compare_classification(
                    &[Fixture {
                        split: split.into(),
                        example: classification_input()
                    }],
                    "FIXED INSTRUCTION",
                    &models,
                    directory.path()
                )
                .await
                .is_err()
            );
        }
        assert!(oracle.asked().is_empty());
        assert!(judge.asked().is_empty());
    }
    #[tokio::test]
    async fn classification_metric_ignores_absent_other_stages_and_keeps_gold_private() {
        let oracle = Arc::new(DummyLM::new([classification_reply()]));
        let judge = Arc::new(DummyLM::new([review("pass", "fail", "fail", "fail", true)]));
        let input = classification_input();
        let program = Graded::new(
            "all",
            "classification",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge.clone(),
            None,
        )
        .unwrap();
        let output = program.forward(input).await.unwrap();
        assert_eq!(output.get("__score"), Some(&json!(1.0)));
        assert_eq!(output.get("__passed"), Some(&json!(true)));
        let request = &oracle.asked()[0];
        assert!(!format!("{} {:?}", request.system, request.turns).contains("PRIVATE GOLD"));
        assert!(!format!("{} {:?}", request.system, request.turns).contains("private-case-id"));
        assert!(judge.asked()[0].last_message().contains("PRIVATE GOLD"));
        assert!(
            judge.asked()[0]
                .system
                .contains("Applicable dimensions: classification")
        );
    }
    #[tokio::test]
    async fn known_method_and_facet_mismatches_override_but_preserve_a_false_teacher_pass() {
        for (reply, error) in [
            (
                dsrust::example! {method:"property",facet:"event",branch:"purchase_or_sale_completion",principal:"querent",clarification:""},
                "Classification method differs from authored label movable_deal",
            ),
            (
                dsrust::example! {method:"movable_deal",facet:"timing",branch:"pure_completion",principal:"querent",clarification:""},
                "Classification facet differs from authored label event",
            ),
        ] {
            let input = classification_input();
            let oracle = Arc::new(DummyLM::new([reply]));
            let judge = Arc::new(DummyLM::new([review(
                "pass",
                "unobserved",
                "unobserved",
                "unobserved",
                true,
            )]));
            let program = Graded::new(
                "all",
                "classification",
                std::slice::from_ref(&input),
                oracle.clone(),
                judge.clone(),
                None,
            )
            .unwrap();
            let output = program.forward(input).await.unwrap();
            assert_eq!(output.get("__score"), Some(&json!(0.)));
            assert_eq!(output.get("__passed"), Some(&json!(false)));
            let observations = program.observations.lock().unwrap();
            assert_eq!(observations[0]["review"]["classification"], "pass");
            assert_eq!(observations[0]["review"]["passed"], true);
            assert!(
                observations[0]["native_failures"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(error))
            );
            assert_eq!(oracle.asked().len(), 1);
            assert_eq!(judge.asked().len(), 1);
            assert!(!oracle.asked()[0].last_message().contains("PRIVATE GOLD"));
        }
    }
    #[tokio::test]
    async fn missing_private_classification_labels_stop_before_any_submission() {
        let mut input = classification_input();
        input.set("expected", json!({"method":"movable_deal"}));
        let oracle = Arc::new(DummyLM::new([]));
        let judge = Arc::new(DummyLM::new([]));
        assert!(
            Graded::new(
                "all",
                "classification",
                &[input],
                oracle.clone(),
                judge.clone(),
                None
            )
            .is_err()
        );
        assert!(oracle.asked().is_empty());
        assert!(judge.asked().is_empty());
    }
    #[tokio::test]
    async fn classification_rejects_branch_as_facet_despite_positive_reviewer() {
        let input = classification_input();
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {method:"trust",facet:"person_in_capacity",branch:"person_in_capacity",principal:"querent",clarification:""},
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "pass",
            "unobserved",
            "unobserved",
            "unobserved",
            true,
        )]));
        let program = Graded::new(
            "all",
            "classification",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge,
            None,
        )
        .unwrap();
        let output = program.forward(input).await.unwrap();
        assert_eq!(output.get("__score"), Some(&json!(0.0)));
        assert_eq!(output.get("__passed"), Some(&json!(false)));
        assert!(!program.failed.load(Ordering::SeqCst));
        assert_eq!(oracle.asked().len(), 1);
        let observations = program.observations.lock().unwrap();
        assert_eq!(observations[0]["prediction"]["facet"], "person_in_capacity");
        assert_eq!(observations[0]["review"]["classification"], "pass");
        assert_eq!(observations[0]["review"]["passed"], true);
        assert!(
            observations[0]["native_failures"]
                .as_array()
                .unwrap()
                .contains(&json!("Unknown trust facet person_in_capacity"))
        );
    }
    #[tokio::test]
    async fn classification_rejects_empty_or_foreign_method_facets() {
        for facet in ["", "event", "arbitrary"] {
            let input = classification_input();
            let oracle = Arc::new(DummyLM::new([
                dsrust::example! {method:"trust",facet:facet,branch:"person_in_capacity",principal:"querent",clarification:""},
            ]));
            let judge = Arc::new(DummyLM::new([review(
                "pass",
                "unobserved",
                "unobserved",
                "unobserved",
                true,
            )]));
            let program = Graded::new(
                "all",
                "classification",
                std::slice::from_ref(&input),
                oracle,
                judge,
                None,
            )
            .unwrap();
            let output = program.forward(input).await.unwrap();
            assert_eq!(output.get("__score"), Some(&json!(0.0)));
            assert!(!program.failed.load(Ordering::SeqCst));
        }
    }
    #[tokio::test]
    async fn classification_keeps_valid_facets_subject_to_semantic_review() {
        for facet in catalogue::method_facets("trust").unwrap() {
            let mut input = classification_input();
            input.set(
                "expected",
                json!({"method":"trust","rubric":{"facet":facet}}),
            );
            let oracle = Arc::new(DummyLM::new([
                dsrust::example! {method:"trust",facet:facet,branch:"person_in_capacity",principal:"querent",clarification:""},
            ]));
            let judge = Arc::new(DummyLM::new([review(
                "fail",
                "unobserved",
                "unobserved",
                "unobserved",
                false,
            )]));
            let program = Graded::new(
                "all",
                "classification",
                std::slice::from_ref(&input),
                oracle,
                judge,
                None,
            )
            .unwrap();
            let output = program.forward(input).await.unwrap();
            assert_eq!(output.get("__passed"), Some(&json!(false)));
            assert_eq!(
                program.observations.lock().unwrap()[0]["native_failures"],
                json!([])
            );
        }
    }
    #[tokio::test]
    async fn contradictory_summary_is_retained_without_vetoing_scoped_learning() {
        let oracle = Arc::new(DummyLM::new([classification_reply()]));
        let judge = Arc::new(DummyLM::new([review(
            "pass", "fail", "fail", "fail", false,
        )]));
        let input = classification_input();
        let program = Graded::new(
            "all",
            "classification",
            std::slice::from_ref(&input),
            oracle,
            judge,
            None,
        )
        .unwrap();
        let output = program.forward(input).await.unwrap();
        assert_eq!(output.get("__score"), Some(&json!(1.0)));
        let observations = program.observations.lock().unwrap();
        assert_eq!(observations[0]["review"]["passed"], false);
        assert_eq!(observations[0]["review_summary_consistent"], false);
        assert_eq!(
            observations[0]["applicable_dimensions"],
            json!(["classification"])
        );
    }
    #[tokio::test]
    async fn reader_cannot_pass_by_procedure_while_failing_to_answer() {
        let input=dsrust::example!{question:"Synthetic question.",consultation:"{\"facts\":[],\"branch\":\"pure_completion\"}",chart_evidence:"{\"facts\":\"ID\\tKIND\\tLABEL\\tCALCULATED TESTIMONY\\ne1\\tboundary\\tLimits\\tSynthetic supplied boundary.\\n\"}",source_text:"Synthetic primary context.",expected:"Synthetic rubric.",case_id:"synthetic"}
            .with_inputs(["question","consultation","chart_evidence","source_text"]);
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {status:"answer",answer:"",analysis:"Synthetic e1 observation.",evidence:"e1",unresolved:""},
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "unobserved",
            "unobserved",
            "pass",
            "fail",
            false,
        )]));
        let output = Graded::new(
            "movable_deal",
            "reading",
            std::slice::from_ref(&input),
            oracle,
            judge,
            Some(Arc::new(Book::fixture())),
        )
        .unwrap()
        .forward(input)
        .await
        .unwrap();
        assert_eq!(output.get("__passed"), Some(&json!(false)));
        assert_ne!(output.get("__score"), Some(&json!(1.0)));
    }
    #[tokio::test]
    async fn received_invalid_fact_json_becomes_native_feedback_not_a_transport_retry() {
        let input=dsrust::example!{question:"My bicycle.",conversation:"[]",context:"Actual device only.",requirements:"Declared extraction contract.",expected:"Synthetic rubric.",__review_source:"Synthetic primary context."}
            .with_inputs(["question","conversation","context","requirements"]);
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {facts:"not JSON",missing:"[]",inferences:"",clarification:""},
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "unobserved",
            "fail",
            "unobserved",
            "unobserved",
            false,
        )]));
        let program = Graded::new(
            "movable_deal",
            "extraction",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge,
            None,
        )
        .unwrap();
        let prediction = program.forward(input).await.unwrap();
        assert_eq!(prediction.get("__score"), Some(&json!(0.0)));
        assert!(
            text(&prediction, "__feedback")
                .unwrap()
                .contains("Native fact array rejected")
        );
        assert!(!program.failed.load(Ordering::SeqCst));
        assert_eq!(oracle.asked().len(), 1);
        assert_eq!(
            program.observations.lock().unwrap()[0]["prediction"]["facts"],
            "not JSON"
        );
    }
    #[tokio::test]
    async fn unobserved_execution_stops_future_oracle_and_reflection_submissions() {
        let input = classification_input();
        let oracle = Arc::new(DummyLM::new([]));
        let judge = Arc::new(DummyLM::new([]));
        let program = Graded::new(
            "all",
            "classification",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge.clone(),
            None,
        )
        .unwrap();
        assert!(program.forward(input.clone()).await.is_err());
        assert!(program.forward(input).await.is_err());
        assert_eq!(oracle.asked().len(), 1);
        assert!(judge.asked().is_empty());
        let teacher = Arc::new(DummyLM::new([]));
        let reflection = ReflectionGuard {
            inner: teacher.clone(),
            failed: program.failed.clone(),
        };
        let request = dsrust::lm::api::LmRequest::new("Synthetic reflection", vec![]);
        assert!(
            reflection
                .forward_dyn(&reqwest::Client::new(), &request)
                .await
                .is_err()
        );
        assert!(teacher.asked().is_empty());
    }

    #[tokio::test]
    async fn received_format_rejection_reaches_gepa_feedback_without_fabricating_fields_or_blocking_next_case()
     {
        let input = classification_input();
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {catalogue:"Observed copied input, no classifier outputs"},
            classification_reply(),
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "pass",
            "unobserved",
            "unobserved",
            "unobserved",
            true,
        )]));
        let program = Graded::new(
            "all",
            "classification",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge.clone(),
            None,
        )
        .unwrap();
        let rejection = program.forward(input.clone()).await.unwrap();
        assert_eq!(rejection.get("__score"), Some(&json!(0.)));
        assert_eq!(rejection.get("__passed"), Some(&json!(false)));
        for field in ["method", "facet", "branch", "principal", "clarification"] {
            assert!(rejection.get(field).is_none());
        }
        assert!(
            Graded::feedback(&input, &rejection)
                .feedback
                .unwrap()
                .contains("Observed copied input")
        );
        assert!(judge.asked().is_empty());
        assert!(!program.failed.load(Ordering::SeqCst));
        let valid = program.forward(input).await.unwrap();
        assert_eq!(valid.get("__passed"), Some(&json!(true)));
        assert_eq!(judge.asked().len(), 1);
        assert_eq!(oracle.asked().len(), 2);
        assert!(
            oracle
                .asked()
                .iter()
                .all(|request| request.config.max_tokens == Some(512))
        );
        let observations = program.observations.lock().unwrap();
        assert_eq!(observations[0]["format_rejection"], true);
        assert!(observations[0]["review"].is_null());
        assert!(observations[0]["received_response"]["outputs"].is_array());
    }
    fn saved_component(
        directory: &Path,
        function: &str,
        method: &str,
        mutate: impl FnOnce(&mut Value),
    ) -> PathBuf {
        let state = dsrust::module::ProgramState::new(
            declarations(method, function)
                .unwrap()
                .into_iter()
                .map(|(name, signature)| {
                    (name, dsrust::module::PredictorState::of(&signature, &[]))
                })
                .collect(),
        );
        let mut value = serde_json::to_value(state).unwrap();
        mutate(&mut value);
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        let path = directory.join("compiled.json");
        fs::write(&path, &bytes).unwrap();
        fs::write(
            directory.join("component-source.json"),
            serde_json::to_vec_pretty(
                &source_receipt(
                    method,
                    function,
                    &bytes,
                    &crate::google::digest("Synthetic fixture corpus"),
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        path
    }
    #[test]
    fn exact_component_loads_but_wrong_scope_partial_fields_private_state_do_not() {
        let directory = tempfile::tempdir().unwrap();
        let model = Arc::new(DummyLM::new([]));
        let path = saved_component(directory.path(), "extraction", "movable_deal", |value| {
            value["extraction.movable_deal"]["signature"]["instructions"] =
                json!("Synthetic improved instruction.");
        });
        let mut target = plain(
            signatures::extraction("movable_deal").unwrap(),
            model.clone(),
        );
        load_component(&mut target, &path, "movable_deal", "extraction").unwrap();
        assert_eq!(
            target.signature.instructions,
            "Synthetic improved instruction."
        );
        assert!(load_component(&mut target, &path, "money", "extraction").is_err());
        for mutate in [
            |v: &mut Value| {
                v["extraction.money"] = v["extraction.movable_deal"].clone();
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["lm"] = json!({"key":"Synthetic forbidden runtime"});
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["demos"] = json!([{"__feedback":"private"}]);
            },
        ] {
            let path = saved_component(directory.path(), "extraction", "movable_deal", mutate);
            let mut target = plain(
                signatures::extraction("movable_deal").unwrap(),
                model.clone(),
            );
            let original = target.signature.instructions.clone();
            assert!(load_component(&mut target, &path, "movable_deal", "extraction").is_err());
            assert_eq!(target.signature.instructions, original);
        }
    }
    #[tokio::test]
    async fn real_port_compile_saves_loadable_function_and_three_observed_phase_receipts() {
        let fixtures = ["train", "development", "test"].map(|split| {
            let mut example = classification_input();
            example.set(
                "question",
                json!(format!("Synthetic {split}: will I sell my bicycle?")),
            );
            example.set("case_id", json!(format!("synthetic-{split}")));
            Fixture {
                split: split.into(),
                example,
            }
        });
        // Scripted replies test the actual pinned GEPA compiler, not a simulated
        // search or a claim of model-quality improvement.
        let oracle = Arc::new(DummyLM::new((0..100).map(|_| classification_reply())));
        let judge =
            Arc::new(DummyLM::new((0..100).map(|_| {
                review("pass", "unobserved", "unobserved", "unobserved", true)
            })));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge,
            teacher_id: "Synthetic offline model only".into(),
            student_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let result = compile_fixtures(
            &fixtures,
            "all",
            "classification",
            Arc::new(Book::fixture()),
            &models,
            directory.path(),
            CompileOptions {
                max_metrics: 2,
                bootstrap_demos: 0,
            },
        )
        .await
        .unwrap();
        assert_eq!(result["bootstrap_demos"], 0);
        assert_eq!(result["component_only"], true);
        assert_eq!(result["application_promoted"], false);
        assert!(oracle.asked().len() >= 4);
        for phase in ["before.json", "after.json", "held.json"] {
            let receipt: Value =
                serde_json::from_slice(&fs::read(directory.path().join(phase)).unwrap()).unwrap();
            assert_eq!(receipt["execution_errors"], 0);
            assert_eq!(receipt["score"], 1.0);
        }
        let mut live = plain(signatures::auxiliary("classification").unwrap(), oracle);
        load_component(
            &mut live,
            &directory.path().join("compiled.json"),
            "all",
            "classification",
        )
        .unwrap();
        assert!(live.demos.is_empty());
    }
    #[test]
    fn source_drift_and_state_tampering_fail_before_predictor_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = saved_component(directory.path(), "classification", "all", |_| {});
        let mut receipt: Value = serde_json::from_slice(
            &fs::read(directory.path().join("component-source.json")).unwrap(),
        )
        .unwrap();
        receipt["book_sha256"] = json!(crate::google::digest("Different source"));
        fs::write(
            directory.path().join("component-source.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        let mut target = plain(
            signatures::auxiliary("classification").unwrap(),
            Arc::new(DummyLM::new([])),
        );
        assert!(load_component(&mut target, &path, "all", "classification").is_err());
        let path = saved_component(directory.path(), "classification", "all", |_| {});
        fs::write(&path, b"{}").unwrap();
        assert!(load_component(&mut target, &path, "all", "classification").is_err());
    }
    #[test]
    fn expected_dataset_and_split_identity_are_required_for_a_composition_gate() {
        let directory = tempfile::tempdir().unwrap();
        let path = saved_component(directory.path(), "classification", "all", |_| {});
        let mut target = plain(
            signatures::auxiliary("classification").unwrap(),
            Arc::new(DummyLM::new([])),
        );
        assert!(
            load_component_for_dataset(
                &mut target,
                &path,
                "all",
                "classification",
                &crate::google::digest("Different current corpus")
            )
            .is_err()
        );
        load_component_for_dataset(
            &mut target,
            &path,
            "all",
            "classification",
            &crate::google::digest("Synthetic fixture corpus"),
        )
        .unwrap();
        let mut fixture = Fixture {
            split: "train".into(),
            example: classification_input(),
        };
        let before = fixtures_digest(std::slice::from_ref(&fixture)).unwrap();
        fixture.split = "test".into();
        assert_ne!(before, fixtures_digest(&[fixture]).unwrap());
    }

    #[tokio::test]
    async fn learned_selector_failure_is_scored_and_reviewed_without_hiding_full_family_source() {
        let input = crate::reading::tests::input(json!({"facts":[]}));
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {branch:"not_a_book_branch"},
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "unobserved",
            "unobserved",
            "pass",
            "pass",
            true,
        )]));
        let program = Graded::new(
            "movable_deal",
            "reading",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge.clone(),
            Some(Arc::new(Book::fixture())),
        )
        .unwrap();
        let output = program.forward(input).await.unwrap();
        assert_eq!(output.get("__score"), Some(&json!(0.0)));
        assert_eq!(output.get("__passed"), Some(&json!(false)));
        assert!(!program.failed.load(Ordering::SeqCst));
        assert_eq!(oracle.asked().len(), 1);
        assert!(
            judge.asked()[0]
                .last_message()
                .contains("FULL FAMILY REVIEW CONTEXT")
        );
        assert!(
            judge.asked()[0]
                .last_message()
                .contains("PRIVATE GOLD MUST NOT REACH THE STUDENT")
        );
        assert!(
            program.observations.lock().unwrap()[0]["native_failures"][0]
                .as_str()
                .unwrap()
                .contains("not_a_book_branch")
        );
    }
    #[test]
    fn reading_module_version_validates_both_predictors_and_classified_projection_before_mutation()
    {
        let directory = tempfile::tempdir().unwrap();
        let model = Arc::new(DummyLM::new([]));
        let digest = crate::google::digest("Synthetic fixture corpus");
        let path = saved_component(directory.path(), "reading", "movable_deal", |value| {
            value["reading_branch.movable_deal"]["signature"]["instructions"] =
                json!("Synthetic learned branch instruction.");
            value["reading.movable_deal"]["signature"]["instructions"] =
                json!("Synthetic learned reader instruction.");
        });
        let mut module =
            ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone()).unwrap();
        load_reading_component(&mut module, &path, "movable_deal", &digest).unwrap();
        assert_eq!(
            module.selector.signature.instructions,
            "Synthetic learned branch instruction."
        );
        assert_eq!(
            module.reader.signature.instructions,
            "Synthetic learned reader instruction."
        );
        let mut reader = plain(signatures::reading("movable_deal").unwrap(), model.clone());
        let projection =
            load_classified_reader(&mut reader, &path, "movable_deal", &digest).unwrap();
        assert_eq!(projection["reading_module_version"], 2);
        assert_eq!(
            projection["bypassed_predictor"],
            "reading_branch.movable_deal"
        );
        assert_eq!(
            reader.signature.instructions,
            "Synthetic learned reader instruction."
        );
        let receipt: Value = serde_json::from_slice(
            &fs::read(directory.path().join("component-source.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["version"], 2);
        assert_eq!(
            receipt["predictor_names"],
            json!(["reading.movable_deal", "reading_branch.movable_deal"])
        );
        for mutate in [
            |v: &mut Value| {
                v.as_object_mut()
                    .unwrap()
                    .remove("reading_branch.movable_deal");
            },
            |v: &mut Value| {
                v["reading_branch.movable_deal"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            },
            |v: &mut Value| {
                v["reading_branch.movable_deal"]["demos"] = json!([{"question":"Synthetic","branch":"pure_completion","expected":"PRIVATE GOLD"}]);
            },
            |v: &mut Value| {
                v["reading.movable_deal"]["lm"] = json!({"key":"forbidden runtime"});
            },
        ] {
            let path = saved_component(directory.path(), "reading", "movable_deal", mutate);
            let mut module =
                ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone())
                    .unwrap();
            let before = serde_json::to_value(module.dump_state()).unwrap();
            assert!(load_reading_component(&mut module, &path, "movable_deal", &digest).is_err());
            assert_eq!(serde_json::to_value(module.dump_state()).unwrap(), before);
            let mut reader = plain(signatures::reading("movable_deal").unwrap(), model.clone());
            let before = reader.signature.instructions.clone();
            assert!(load_classified_reader(&mut reader, &path, "movable_deal", &digest).is_err());
            assert_eq!(reader.signature.instructions, before);
        }
        let path = saved_component(directory.path(), "reading", "movable_deal", |_| {});
        let mut reader = plain(signatures::reading("movable_deal").unwrap(), model);
        assert!(load_component(&mut reader, &path, "movable_deal", "reading").is_err());
        assert!(
            load_classified_reader(
                &mut reader,
                &path,
                "movable_deal",
                &crate::google::digest("Different corpus")
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn pinned_gepa_compiles_and_reloads_both_reading_predictors_with_observed_phase_receipts()
    {
        let fixtures = ["train", "development", "test"].map(|split| {
            let mut example = crate::reading::tests::input(json!({"facts":[]}));
            example.set(
                "question",
                json!(format!("Synthetic {split}: will this sale complete?")),
            );
            example.set("case_id", json!(format!("synthetic-{split}")));
            Fixture {
                split: split.into(),
                example,
            }
        });
        let oracle = Arc::new(DummyLM::new((0..100).flat_map(|_|[
            dsrust::example! {branch:"pure_completion"},
            dsrust::example! {status:"method_limit",answer:"Synthetic only.",analysis:"Supplied e1 boundary.",evidence:"e1",unresolved:"Native unknowns remain unknown."},
        ])));
        let judge =
            Arc::new(DummyLM::new((0..100).map(|_| {
                review("unobserved", "unobserved", "pass", "pass", true)
            })));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge,
            teacher_id: "Synthetic offline".into(),
            student_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let book = Arc::new(Book::fixture());
        let result = compile_fixtures(
            &fixtures,
            "movable_deal",
            "reading",
            book.clone(),
            &models,
            directory.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
        )
        .await
        .unwrap();
        assert_eq!(result["component_only"], true);
        assert_eq!(result["application_promoted"], false);
        assert_eq!(result["source_correct_reading_qualified"], false);
        let mut target = ReadingProgram::new("movable_deal", book, oracle).unwrap();
        load_reading_component(
            &mut target,
            &directory.path().join("compiled.json"),
            "movable_deal",
            &fixtures_digest(&fixtures).unwrap(),
        )
        .unwrap();
        assert_eq!(target.dump_state().predictors.len(), 2);
        for phase in ["before.json", "after.json", "held.json"] {
            let receipt: Value =
                serde_json::from_slice(&fs::read(directory.path().join(phase)).unwrap()).unwrap();
            assert_eq!(receipt["execution_errors"], 0);
        }
    }
}
