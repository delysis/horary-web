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
use dsrust::{BootstrapFewShot, Evaluate, Example, GEPA, LabeledFewShot, Module, Predict};
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

pub(crate) fn input_key(input: &Example) -> Result<String> {
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

/// The finite native ID domain comes from declared rows, never prose citations.
pub(crate) fn native_evidence_ids(chart: &Value) -> Result<std::collections::BTreeSet<String>> {
    let mut ids = std::collections::BTreeSet::new();
    let mut add = |id: &str| -> Result<()> {
        ensure!(
            id.strip_prefix('e').is_some_and(
                |digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            ),
            "Invalid native evidence ID {id}"
        );
        ensure!(
            ids.insert(id.to_owned()),
            "Duplicate native evidence ID {id}"
        );
        Ok(())
    };
    if chart["presentation"] == "concise-native-reader-1" {
        for (name, columns) in [
            ("facts", &["id", "kind", "label", "native_detail"][..]),
            (
                "receptions",
                &[
                    "id",
                    "regarding_planet",
                    "regarded_planet",
                    "positive_qualities",
                    "negative_qualities",
                ][..],
            ),
            (
                "events",
                &[
                    "id",
                    "endpoint1",
                    "endpoint2",
                    "aspect",
                    "within_current_signs",
                    "astronomical_hours",
                    "native_current_angular_gap_or_detail",
                    "motion",
                ][..],
            ),
        ] {
            ensure!(
                chart[name]["columns"] == json!(columns),
                "Native {name} columns differ"
            );
            for row in chart[name]["rows"]
                .as_array()
                .with_context(|| format!("No native {name} rows"))?
            {
                let row = row
                    .as_array()
                    .with_context(|| format!("Malformed native {name} row"))?;
                ensure!(
                    row.len() == columns.len(),
                    "Native {name} row width differs"
                );
                add(row[0].as_str().context("Native row ID is not text")?)?;
            }
        }
    } else if let Some(table) = chart["facts"].as_str() {
        let mut rows = table.lines();
        ensure!(
            rows.next() == Some("ID\tKIND\tLABEL\tCALCULATED TESTIMONY"),
            "Native testimony header differs"
        );
        for row in rows {
            let row = row.splitn(4, '\t').collect::<Vec<_>>();
            ensure!(row.len() == 4, "Malformed native testimony row");
            add(row[0])?;
        }
    } else if let Some(facts) = chart["native_facts"].as_array() {
        for fact in facts {
            add(fact["id"].as_str().context("Native fact lacks an ID")?)?;
        }
    }
    ensure!(
        !ids.is_empty(),
        "Reading component lacks native evidence IDs"
    );
    Ok(ids)
}

/// Validate reading output against the exact supplied native chart only.
pub fn reading_native_failures(input: &Example, prediction: &Prediction) -> Result<Vec<String>> {
    let mut errors = Vec::new();
    let supplied = input
        .get("chart_evidence")
        .and_then(Value::as_str)
        .context("No actual chart")?;
    let ids = if supplied.contains("<facts>") {
        catalogue::relevant_chart_ids(supplied)?
    } else {
        native_evidence_ids(&serde_json::from_str(supplied)?)?
    };
    let pattern = regex::Regex::new(r"\be[0-9]+\b")?;
    for field in ["answer", "analysis", "evidence", "unresolved"] {
        for found in pattern.find_iter(text(prediction, field)?) {
            if !ids.contains(found.as_str()) {
                errors.push(format!(
                    "{field}: cites absent native evidence {}",
                    found.as_str()
                ));
            }
        }
    }
    if !matches!(
        text(prediction, "status")?,
        "answer" | "need_information" | "method_limit"
    ) {
        errors.push("Unknown reading status".into());
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
        let mut judge = plain(
            signatures::auxiliary(if function == "reception" {
                "reception_judge"
            } else {
                "judge"
            })?,
            teacher,
        );
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
                    max_tokens: Some(match function {
                        "classification" => 512,
                        "reception" => 1024,
                        _ => 4096,
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
        let mut oracle_stages = trace[from..].iter().map(|step|json!({"predictor":step.predictor,"inputs":step.inputs.rendered(),"outputs":step.outputs.rendered()})).collect::<Vec<_>>();
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
                // A single Predict has an unambiguous failed function. Retain
                // its received raw output in the ordinary optimizer trace so
                // reflection can address this format failure. A composed
                // program's absent trace cannot identify its failed predictor.
                if matches!(&self.oracle, Oracle::Single(_)) {
                    trace.push(TraceStep {
                        predictor: self.predictor_name.clone(),
                        inputs: input.clone(),
                        outputs: dsrust::example! {
                            __received_output: raw,
                            __format_rejection: observed.reason,
                        },
                    });
                    oracle_stages = trace[from..].iter().map(|step|json!({"predictor":step.predictor,"inputs":step.inputs.rendered(),"outputs":step.outputs.rendered()})).collect();
                }
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
        } else if self.function == "reception" {
            errors.extend(crate::reception::native_failures(&input, &prediction)?);
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
            errors.extend(reading_native_failures(&input, &prediction)?);
        }
        let mut review_input = dsrust::input! {
            inputs:serde_json::to_string(&example.get("__review_inputs").cloned().unwrap_or_else(||json!(input.rendered())))?,prediction:serde_json::to_string(&fields(&prediction))?,rubric:serde_json::to_string(example.get("expected").context("No independently authored rubric")?)?,source_text:source
        };
        if self.function != "reception" {
            review_input.set("function", json!(self.function));
            review_input.set("reference_answer", example.get("__reference_answer").cloned().unwrap_or_else(||json!("No approved worked reference; assess against the authored criteria and primary source.")));
            review_input = review_input.with_inputs([
                "function",
                "inputs",
                "prediction",
                "reference_answer",
                "rubric",
                "source_text",
            ]);
        }
        let review = self.judge.forward(review_input).await?;
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
        let review_dimensions: &[&str] = if self.function == "reception" {
            &["contextual_meaning"]
        } else {
            &["classification", "extraction", "reading", "answer_quality"]
        };
        for dimension in review_dimensions {
            ensure!(
                matches!(text(&review, dimension)?, "pass" | "fail" | "unobserved"),
                "Invalid function review dimension"
            );
        }
        let dimensions = functions::dimensions(&self.function)?;
        if example.get("__reference_answer").is_some()
            && dimensions.iter().any(|dimension| {
                text(&review, dimension).is_ok_and(|decision| decision == "unobserved")
            })
        {
            self.record(false,&json!({"case_id":example.get("case_id"),"function":self.function,"inputs":input.rendered(),"prediction":fields(&prediction),"review":fields(&review),"applicable_semantic_grade":"unobserved","score":null}))?;
            anyhow::bail!(
                "Independent actual-answer review leaves applicable meaning unobserved; no semantic score inferred"
            );
        }
        if self.function == "reception" {
            ensure!(
                text(&review, "contextual_meaning")? != "unobserved",
                "Reception contextual meaning remains unobserved; no semantic score inferred"
            );
        }
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
        prediction
            .example
            .set("__review_summary_consistent", json!(summary_consistent));
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
    fn full_pass_metric(example: &Example, prediction: &Prediction) -> f64 {
        if prediction.get("__passed") == Some(&json!(true))
            && prediction.get("__review_summary_consistent") == Some(&json!(true))
        {
            Self::metric(example, prediction)
        } else {
            0.
        }
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

/// A standard Module view: only the chosen predictor's instruction is mutable.
/// Forward still executes the complete program. A composed program retains its
/// actual selector; a direct reader has one Predict. Saving uses the full state.
struct FunctionView<'a> {
    program: &'a mut Graded,
    name: String,
}
impl Module for FunctionView<'_> {
    fn forward<'a>(
        &'a self,
        input: Example,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        self.program.forward(input)
    }
    fn forward_traced<'a>(
        &'a self,
        input: Example,
        trace: &'a mut Vec<TraceStep>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        self.program.forward_traced(input, trace)
    }
    fn named_predictors(&mut self) -> Vec<NamedPredictor<'_>> {
        self.program
            .named_predictors()
            .into_iter()
            .filter(|predictor| predictor.name == self.name)
            .collect()
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
        "reception" => Ok("reception.interpretation".into()),
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
        ("gold.rs", include_bytes!("gold.rs").as_slice()),
        ("signatures.rs", include_bytes!("signatures.rs").as_slice()),
        ("catalogue.rs", include_bytes!("catalogue.rs").as_slice()),
        ("optimize.rs", include_bytes!("optimize.rs").as_slice()),
        ("codex.rs", include_bytes!("codex.rs").as_slice()),
        ("book.rs", include_bytes!("book.rs").as_slice()),
        ("reading.rs", include_bytes!("reading.rs").as_slice()),
        ("reception.rs", include_bytes!("reception.rs").as_slice()),
        (
            "new_job_reader.md",
            include_bytes!("../prompts/new_job_reader.md").as_slice(),
        ),
        (
            "native-new-job-context.md",
            include_bytes!("../../../src-tauri/src/horary_prompts/new_job_context.md").as_slice(),
        ),
        (
            "native-new-job-contacts.md",
            include_bytes!("../../../src-tauri/src/horary_prompts/new_job_contacts.md").as_slice(),
        ),
        (
            "native-new-job-reception.md",
            include_bytes!("../../../src-tauri/src/horary_prompts/new_job_reception.md").as_slice(),
        ),
        (
            "native-new-job-judgment.md",
            include_bytes!("../../../src-tauri/src/horary_prompts/new_job_judgment.md").as_slice(),
        ),
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
    validated_state_kind(path, method, function, false)
}

fn direct_source_receipt(method: &str, compiled: &[u8], dataset: &str) -> Result<Value> {
    let mut receipt = source_receipt(method, "reading", compiled, dataset)?;
    receipt["version"] = json!(3);
    receipt["program_kind"] = json!("direct_family_reading");
    receipt["predictor_names"] = json!([predictor_name(method, "reading")?]);
    receipt["source_selection"] = json!(format!(
        "Book-defined complete {method} branch union; no expected branch"
    ));
    receipt
        .as_object_mut()
        .unwrap()
        .remove("branch_declaration_sha256");
    Ok(receipt)
}

fn validated_state_kind(
    path: &Path,
    method: &str,
    function: &str,
    direct: bool,
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
        receipt
            == if direct {
                direct_source_receipt(method, &compiled, dataset_digest)?
            } else {
                source_receipt(method, function, &compiled, dataset_digest)?
            },
        "Compiled component source identity differs"
    );
    let value: Value = serde_json::from_slice(&compiled)?;
    let declarations = if direct {
        BTreeMap::from([(
            predictor_name(method, "reading")?,
            signatures::reading(method)?,
        )])
    } else {
        declarations(method, function)?
    };
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
    compile_fixtures_inner(
        fixtures, method, function, book, models, directory, options, None, false, None, None,
    )
    .await
}

/// Diagnostic reader-only view of the original composed program. Its actual
/// selector still executes; only the reader's instruction is optimizable.
#[allow(clippy::too_many_arguments)]
pub async fn compile_reader_fixtures(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
    baseline: &str,
) -> Result<Value> {
    ensure!(
        options.bootstrap_demos == 0,
        "Reader-only GEPA is instruction-only"
    );
    ensure!(!baseline.trim().is_empty(), "Reader baseline is empty");
    compile_fixtures_inner(
        fixtures,
        method,
        "reading",
        book,
        models,
        directory,
        options,
        Some(baseline),
        false,
        None,
        None,
    )
    .await
}

/// Stock GEPA on one declared reading function with the complete primary
/// family/branch union. No redundant neural selector or gold branch is supplied.
#[allow(clippy::too_many_arguments)]
pub async fn compile_direct_reader_fixtures(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
    baseline: &str,
) -> Result<Value> {
    ensure!(
        options.bootstrap_demos == 0,
        "Direct reader GEPA is instruction-only"
    );
    ensure!(!baseline.trim().is_empty(), "Reader baseline is empty");
    compile_fixtures_inner(
        fixtures,
        method,
        "reading",
        book,
        models,
        directory,
        options,
        Some(baseline),
        true,
        None,
        None,
    )
    .await
}

/// Continue with a completed, source-matched BootstrapFewShot program. The
/// conventional GEPA compiler starts afresh; teacher work is not repurchased.
#[allow(clippy::too_many_arguments)]
pub async fn compile_direct_reader_from_bootstrap(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
    baseline: &str,
    bootstrap: &Path,
) -> Result<Value> {
    ensure!(
        options.bootstrap_demos == 0,
        "Use the retained completed bootstrap only"
    );
    compile_fixtures_inner(
        fixtures,
        method,
        "reading",
        book,
        models,
        directory,
        options,
        Some(baseline),
        true,
        Some(bootstrap),
        None,
    )
    .await
}

/// Standard labeled demonstrations followed by the same stock GEPA compiler.
/// Labels are actual reviewed TRAIN answers, not criteria or evaluation gold.
#[allow(clippy::too_many_arguments)]
pub async fn compile_direct_reader_from_gold(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
    baseline: &str,
    labeled_training: &[Example],
    labeled_demos: usize,
) -> Result<Value> {
    ensure!(
        options.bootstrap_demos == 0 && labeled_demos > 0,
        "Use the reviewed labels without repurchasing a bootstrap"
    );
    compile_fixtures_inner(
        fixtures,
        method,
        "reading",
        book,
        models,
        directory,
        options,
        Some(baseline),
        true,
        None,
        Some((labeled_training, labeled_demos)),
    )
    .await
}

fn direct_program(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    student: Arc<dyn DynChatModel>,
    teacher: Arc<dyn DynChatModel>,
    baseline: &str,
    stream: &Path,
) -> Result<Graded> {
    ensure!(!baseline.trim().is_empty(), "Reader baseline is empty");
    let source = catalogue::family_reader_source(&book, method)?;
    ensure!(
        fixtures.iter().all(
            |f| matches!(f.split.as_str(), "train" | "development" | "test")
                && f.example.get("source_text").and_then(Value::as_str) == Some(&source)
                && f.example.get("__review_source").and_then(Value::as_str) == Some(&source)
        ),
        "Direct bootstrap requires exact source and original known splits"
    );
    let all = fixtures
        .iter()
        .map(|f| f.example.clone())
        .collect::<Vec<_>>();
    let mut program = Graded::new(
        method,
        "reading",
        &all,
        student.clone(),
        teacher,
        Some(book),
    )?
    .with_stream(stream.to_path_buf())?;
    program.oracle = Oracle::Single(plain(signatures::reading(method)?, student));
    for predictor in program.named_predictors() {
        predictor.signature.instructions = baseline.into();
    }
    program.judge.signature.instructions.push_str(" Return ONLY score, passed, classification, extraction, reading, answer_quality and feedback in their requested field markers. Never echo inputs. Give specific observed reasons and printed source references.");
    Ok(program)
}

/// Teacher-only phase of upstream BootstrapFewShot. It walks training once,
/// stops at one fully correct source-reviewed reading, and never calls student.
pub async fn bootstrap_direct_reader_fixtures(
    fixtures: &[Fixture],
    method: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    baseline: &str,
) -> Result<Value> {
    fs::create_dir_all(directory)?;
    let train = fixtures
        .iter()
        .filter(|f| f.split == "train")
        .map(|f| f.example.clone())
        .collect::<Vec<_>>();
    ensure!(!train.is_empty(), "Bootstrap needs original training cases");
    let mut student = direct_program(
        fixtures,
        method,
        book.clone(),
        models.student.clone(),
        models.teacher.clone(),
        baseline,
        &directory.join("unused-student-observations.jsonl"),
    )?;
    let mut teacher = direct_program(
        fixtures,
        method,
        book,
        models.teacher.clone(),
        models.teacher.clone(),
        baseline,
        &directory.join("teacher-observations.jsonl"),
    )?;
    let mut bootstrap = BootstrapFewShot::new(Graded::full_pass_metric);
    bootstrap.max_bootstrapped_demos = 1;
    bootstrap.max_labeled_demos = 0;
    bootstrap.max_rounds = 1;
    bootstrap.metric_threshold = Some(1.);
    bootstrap.max_errors = 1;
    let outcome = bootstrap
        .compile_with_teacher(&mut student, &mut teacher, &train)
        .await;
    teacher.save_observations(&directory.join("teacher-observations.json"))?;
    student.save(&directory.join("bootstrapped-full.json"))?;
    let demos = outcome?;
    ensure!(
        demos == 1,
        "No fully correct source-reviewed training demonstration; no student evaluation submitted"
    );
    for predictor in student.named_predictors() {
        for demo in predictor.demos {
            *demo = crate::demo::compact(demo)?;
        }
    }
    student.save(&directory.join("bootstrapped.json"))?;
    let bytes = fs::read(directory.join("bootstrapped.json"))?;
    let dataset = fixtures_digest(fixtures)?;
    crate::queue::write_json_atomic(
        &directory.join("component-source.json"),
        &direct_source_receipt(method, &bytes, &dataset)?,
    )?;
    let Oracle::Single(predictor) = &student.oracle else {
        anyhow::bail!("Direct bootstrap is not a single Predict")
    };
    crate::queue::write_json_atomic(
        &directory.join("student-preflight.json"),
        &json!({"provider_calls":0,"requests":crate::preflight::reader_requests(fixtures, predictor)?}),
    )?;
    let result = json!({"phase":"bootstrap_only","method":method,"optimizer":"upstream BootstrapFewShot","accepted_demos":demos,"student_calls":0,"max_labeled_demos":0,"max_rounds":1,"metric_threshold":1.0,"max_errors":1,"training_cases_available":train.len(),"source_summary_consistent_required":true,"compiled_sha256":crate::google::digest(&bytes),"dataset_sha256":dataset,"teacher_observations_sha256":crate::google::digest(fs::read(directory.join("teacher-observations.json"))?),"application_promoted":false,"token_preflight_qualified":false});
    crate::queue::write_json_atomic(&directory.join("bootstrap-result.json"), &result)?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn compile_fixtures_inner(
    fixtures: &[Fixture],
    method: &str,
    function: &str,
    book: Arc<Book>,
    models: &crate::runner::Models,
    directory: &Path,
    options: CompileOptions,
    reader_baseline: Option<&str>,
    direct_family_reader: bool,
    bootstrap_program: Option<&Path>,
    labeled_training: Option<(&[Example], usize)>,
) -> Result<Value> {
    ensure!(
        options.max_metrics > 0,
        "GEPA needs a positive metric budget"
    );
    fs::create_dir_all(directory)?;
    if direct_family_reader {
        let source = catalogue::family_reader_source(&book, method)?;
        let reference_source = if labeled_training.is_some() {
            catalogue::reference_reader_source(&book, method)?
        } else {
            source.clone()
        };
        ensure!(
            function == "reading" && reader_baseline.is_some(),
            "Direct reader requires its declared reading baseline"
        );
        let concise = fixtures.iter().all(|fixture| {
            fixture.example.get("__concise_student_projection") == Some(&json!(true))
        });
        ensure!(
            fixtures.iter().all(|fixture| {
                (if concise {
                    crate::gold::verify_concise_fixture(&fixture.example, method).is_ok()
                } else {
                    fixture.example.get("source_text").and_then(Value::as_str) == Some(&source)
                }) && fixture
                    .example
                    .get("__review_source")
                    .and_then(Value::as_str)
                    == Some(&reference_source)
            }),
            "Direct family reader requires the complete book-defined source union"
        );
    }
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
    // This family is already declared by its function signature. The direct
    // experiment supplies its full source union rather than another neural route.
    if direct_family_reader {
        program.oracle =
            Oracle::Single(plain(signatures::reading(method)?, models.student.clone()));
    }
    if labeled_training.is_some() {
        ensure!(
            direct_family_reader && bootstrap_program.is_none() && options.bootstrap_demos == 0,
            "Reviewed labels require one direct reader and no bootstrap replay"
        );
        for predictor in program.judge.named_predictors() {
            predictor.config.max_tokens = Some(8192);
        }
        for predictor in program.named_predictors() {
            predictor.config.max_tokens = Some(
                if fixtures.iter().all(|fixture| {
                    fixture.example.get("__concise_student_projection") == Some(&json!(true))
                }) {
                    2048
                } else {
                    8192
                },
            );
        }
    }
    let optimizable_predictors = if let Some(baseline) = reader_baseline {
        program.judge.signature.instructions.push_str(" Return ONLY the requested output fields score, passed, classification, extraction, reading, answer_quality and feedback, in their requested field markers, beginning with score. Never copy the input, prediction, rubric or source_text fields. Keep feedback specific and concise, with observed quotations and printed source references.");
        let name = predictor_name(method, function)?;
        let mut found = false;
        for predictor in program.named_predictors() {
            if predictor.name == name {
                predictor.signature.instructions = baseline.to_owned();
                found = true;
            }
        }
        ensure!(found, "Declared reader predictor is absent");
        vec![name]
    } else {
        program
            .named_predictors()
            .into_iter()
            .map(|p| p.name)
            .collect()
    };
    if bootstrap_program.is_some() {
        program.save(&directory.join("authored-program.json"))?;
    }
    let dataset_digest = fixtures_digest(fixtures)?;
    if let Some(path) = bootstrap_program {
        ensure!(
            direct_family_reader && options.bootstrap_demos == 0,
            "Load only a completed direct-reader bootstrap; do not repurchase it"
        );
        require_dataset(path, &dataset_digest)?;
        let state = validated_state_kind(path, method, "reading", true)?;
        let saved = state
            .get(&predictor_name(method, "reading")?)
            .context("No bootstrapped reader")?;
        ensure!(
            saved.signature.instructions == reader_baseline.unwrap() && saved.demos.len() == 1,
            "Bootstrap changed the instruction or lacks the single verified demonstration"
        );
        let result: Value = serde_json::from_slice(&fs::read(
            path.parent().unwrap().join("bootstrap-result.json"),
        )?)?;
        ensure!(
            result["phase"] == "bootstrap_only"
                && result["accepted_demos"] == 1
                && result["student_calls"] == 0
                && result["compiled_sha256"] == crate::google::digest(fs::read(path)?),
            "No completed teacher-only bootstrap receipt"
        );
        program.load_state(&state);
        crate::queue::write_json_atomic(
            &directory.join("bootstrap-import.json"),
            &json!({"path":path,"compiled_sha256":crate::google::digest(fs::read(path)?),"dataset_sha256":dataset_digest,"replay_teacher":false,"optimizer_state_imported":false}),
        )?;
    }
    if let Some((labels, count)) = labeled_training {
        ensure!(
            labels.iter().all(
                |label| train
                    .iter()
                    .any(|example| example.inputs().is_ok_and(|input| label
                        .inputs()
                        .is_ok_and(|labeled_input| input.rendered() == labeled_input.rendered())))
            ),
            "Labeled demonstrations must come only from this original TRAIN pool"
        );
        program.save(&directory.join("authored-program.json"))?;
        let authored = Evaluate::new(
            development.clone(),
            |input| program.forward(input),
            Graded::metric,
        )
        .run()
        .await;
        crate::queue::write_json_atomic(
            &directory.join("authored-development.json"),
            &evaluation_receipt(&authored),
        )?;
        program.save_observations(&directory.join("authored-observations.json"))?;
        ensure!(
            authored.failure_count() == 0,
            "Authored no-demo control has unobserved execution/review errors"
        );
        LabeledFewShot::new(count).compile(&mut program, labels);
        program.save(&directory.join("labeled-full.json"))?;
        for predictor in program.named_predictors() {
            for demo in predictor.demos {
                *demo = crate::demo::compact(demo)?;
            }
        }
        program.save(&directory.join("labeled.json"))?;
        crate::queue::write_json_atomic(
            &directory.join("labeled-source.json"),
            &json!({"optimizer":"upstream LabeledFewShot", "demos":count,"labeled_training_cases":labels.len(),"source_text_omitted_only_in_demos":true,"rubrics_are_not_outputs":true,"student_generation_output_tokens":if fixtures.iter().all(|fixture|fixture.example.get("__concise_student_projection")==Some(&json!(true))){2048}else{8192},"review_output_tokens":8192,"reference_answer_separate_from_rubric":true}),
        )?;
    }
    program.save(&directory.join("baseline-program.json"))?;
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
        program
            .named_predictors()
            .iter()
            .map(|predictor| predictor.demos.len())
            .max()
            .unwrap_or(0)
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
    let optimizer = GEPA::new(Graded::feedback, reflection)
        .with_max_metric_calls(options.max_metrics)
        .with_reflection_minibatch_size(3)
        .with_seed(23);
    let outcome = if reader_baseline.is_some() {
        optimizer
            .compile(
                &mut FunctionView {
                    program: &mut program,
                    name: optimizable_predictors[0].clone(),
                },
                &train,
                &development,
            )
            .await?
    } else {
        optimizer
            .compile(&mut program, &train, &development)
            .await?
    };
    program.save_observations(&directory.join("search-observations.json"))?;
    ensure!(
        !program.failed.load(Ordering::SeqCst),
        "GEPA encountered an unobserved execution/review failure; provisional engine scores/candidate are withheld"
    );
    program.save(&directory.join("compiled.json"))?;
    let bytes = fs::read(directory.join("compiled.json"))?;
    let receipt = if direct_family_reader {
        direct_source_receipt(method, &bytes, &dataset_digest)?
    } else {
        source_receipt(method, function, &bytes, &dataset_digest)?
    };
    crate::queue::write_json_atomic(&directory.join("component-source.json"), &receipt)?;
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
    let result = json!({"method":method,"function":function,"teacher":models.teacher_id,"student":models.student_id,"bootstrap_demos":demos,"optimizable_predictors":optimizable_predictors,"before_development":before.score,"after_development":if observed{Some(after.score)}else{None},"held_out":if observed{Some(held.score)}else{None},"gepa_best_index":outcome.best_idx,"gepa_metric_calls":outcome.total_num_evals,"gepa_iterations":outcome.iterations,"component_only":true,"declared_inputs":if function=="reading"{"Independent accepted consultation and native chart; no acquisition claimed"}else{"Actual function inputs only"},"execution_observed":observed,"independent_review_consistent":review_consistent,"component_pass":observed && review_consistent && after.results.iter().chain(&held.results).all(|row|row.prediction.as_ref().is_ok_and(|p|p.get("__passed")==Some(&json!(true)))),"application_promoted":false,"source_correct_reading_qualified":false});
    let mut result = result;
    if labeled_training.is_some() {
        let authored: Value =
            serde_json::from_slice(&fs::read(directory.join("authored-development.json"))?)?;
        result["authored_development"] = authored["score"].clone();
        result["labeled_development"] = json!(before.score);
        result["worked_answers_separate_from_rubric"] = json!(true);
        result["comparison"] = json!(
            "Authored zero-shot, then LabeledFewShot baseline, then LabeledFewShot plus GEPA; unchanged development/test and primary inputs"
        );
    }
    result["program_kind"] = json!(if direct_family_reader {
        "direct_family_reading"
    } else {
        "original_component"
    });
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        observed,
        "A provider/parser/reviewer error remains unqualified, never a fabricated semantic zero"
    );
    Ok(result)
}

pub const CONTACT_REFERENCE_CASES: [&str; 4] = [
    "dspy-contact-first-004",
    "dspy-contact-first-006",
    "dspy-contact-first-008",
    "dspy-contact-first-011",
];

struct ContactReferenceInputs {
    legacy: Vec<Fixture>,
    off: Vec<Fixture>,
    on: Vec<Fixture>,
}

/// The reference view is the only treatment. Typed event/motion values are
/// supplied identically to both arms, because the original TSV omits them.
fn contact_reference_inputs(cases: &[Case], book: &Book) -> Result<ContactReferenceInputs> {
    ensure!(cases.len() == 20, "Keep the complete original Contact bank");
    let source = catalogue::family_reader_source(book, "contact")?;
    let mut legacy = Vec::new();
    let mut off = Vec::new();
    let mut on = Vec::new();
    for case in cases {
        ensure!(case.method == "contact", "Wrong comparison family");
        let mut example = catalogue::example(case, "reading", book)?;
        example.set("source_text", json!(&source));
        example.set("__review_source", json!(&source));
        let mut chart: Value = serde_json::from_str(
            example
                .get("chart_evidence")
                .and_then(Value::as_str)
                .context("No native chart")?,
        )?;
        let mut aliases = chart
            .as_object_mut()
            .context("Chart must be an object")?
            .remove("role_reference")
            .context("No Contact role reference")?;
        let mut old = example.clone();
        old.set("chart_evidence", json!(serde_json::to_string(&chart)?));
        legacy.push(Fixture {
            split: case.split.clone(),
            example: old,
        });
        if !CONTACT_REFERENCE_CASES.contains(&case.id.as_str()) {
            continue;
        }
        ensure!(
            case.split == "train",
            "Only the four original training cases may run"
        );
        let at = chrono::DateTime::parse_from_rfc3339(&case.chart.utc)?;
        let raw = horary_ai_core::astronomy::chart(
            at.timestamp_millis() as f64,
            case.chart.latitude,
            case.chart.longitude,
        )
        .map_err(anyhow::Error::msg)?;
        let events: BTreeMap<_, _> = crate::native_evidence::facts(Some(&raw))
            .into_iter()
            .filter_map(|fact| fact.event.map(|event| (fact.id, json!(event))))
            .collect();
        for endpoint in aliases["event_endpoints"]
            .as_array_mut()
            .context("No event alias array")?
        {
            let id = endpoint["fact_id"]
                .as_str()
                .context("Event alias lacks its native ID")?;
            ensure!(
                events.get(id) == endpoint.get("candidate"),
                "Alias candidate differs from native testimony"
            );
            endpoint
                .as_object_mut()
                .context("Event alias must be an object")?
                .remove("candidate");
        }
        chart["event_candidates"] = json!(events);
        example.set("chart_evidence", json!(serde_json::to_string(&chart)?));
        off.push(Fixture {
            split: case.split.clone(),
            example: example.clone(),
        });
        chart["role_reference"] = aliases;
        example.set("chart_evidence", json!(serde_json::to_string(&chart)?));
        on.push(Fixture {
            split: case.split.clone(),
            example,
        });
    }
    ensure!(
        off.len() == 4 && on.len() == 4,
        "A comparison case is absent or duplicated"
    );
    for (before, after) in off.iter().zip(&on) {
        let mut removed = after.example.clone();
        let mut chart: Value = serde_json::from_str(
            removed
                .get("chart_evidence")
                .and_then(Value::as_str)
                .unwrap(),
        )?;
        ensure!(
            chart
                .as_object_mut()
                .unwrap()
                .remove("role_reference")
                .is_some(),
            "On view is absent"
        );
        removed.set("chart_evidence", json!(serde_json::to_string(&chart)?));
        ensure!(
            removed.rendered() == before.example.rendered(),
            "Comparison changes more than the reference view"
        );
    }
    Ok(ContactReferenceInputs { legacy, off, on })
}

// This experiment reuses one specifically approved, completed teacher
// example. These are artifact identities, not semantic answer labels.
const CONTACT_APPROVED_PROGRAM: &str =
    "65ee9776ffeaf9121949d27338232a679101eabb990e4348281a72425a01022a";
const CONTACT_APPROVED_FULL: &str =
    "6dc0b15ec1554f174a3d4c4805af95e82de6defc8a4ce7a8f6feb7c37ad4cef8";
const CONTACT_APPROVED_REVIEW: &str =
    "f2f64e8eb5910ee2cc6493487f28c5c1f5685a1c9456d903fca6b0f25c1e8438";
const CONTACT_APPROVED_SOURCE: &str =
    "a0a5c7f791e870dec16f112512d29baf55cfed66f81864213e852ac2c1464003";
const CONTACT_APPROVED_DATASET: &str =
    "4eb20da28b79fdbbae2711705ce09d3ea4de22c57e28938bf4ee708b324e6a69";

fn contact_demo_presentation(
    bootstrap: &Path,
    review: &Path,
    inputs: &ContactReferenceInputs,
    book: &Book,
) -> Result<(dsrust::module::ProgramState, Value)> {
    let bytes = fs::read(bootstrap)?;
    // Do not accept a merely plausible replacement or repurchase this teacher.
    ensure!(
        crate::google::digest(&bytes) == CONTACT_APPROVED_PROGRAM,
        "The approved Contact demonstration differs"
    );
    ensure!(
        crate::google::digest(fs::read(review)?) == CONTACT_APPROVED_REVIEW,
        "The accepted source review differs"
    );
    let parent = bootstrap.parent().context("No bootstrap directory")?;
    ensure!(
        crate::google::digest(fs::read(parent.join("component-source.json"))?)
            == CONTACT_APPROVED_SOURCE,
        "The original demonstration's source receipt differs"
    );
    let full_bytes = fs::read(parent.join("bootstrapped-full.json"))?;
    ensure!(
        crate::google::digest(&full_bytes) == CONTACT_APPROVED_FULL,
        "The full teacher demonstration differs"
    );
    let result: Value = serde_json::from_slice(&fs::read(parent.join("bootstrap-result.json"))?)?;
    ensure!(
        result["phase"] == "bootstrap_only"
            && result["accepted_demos"] == 1
            && result["student_calls"] == 0
            && result["compiled_sha256"] == CONTACT_APPROVED_PROGRAM
            && result["dataset_sha256"] == CONTACT_APPROVED_DATASET
            && fixtures_digest(&inputs.legacy)? == CONTACT_APPROVED_DATASET,
        "The original bootstrap inputs/source no longer match"
    );
    let review_value: Value = serde_json::from_slice(&fs::read(review)?)?;
    ensure!(
        review_value["source_book_sha256"] == crate::book::OCR_SHA256,
        "The source-approved book differs"
    );
    let mut state: dsrust::module::ProgramState = serde_json::from_slice(&bytes)?;
    let full: dsrust::module::ProgramState = serde_json::from_slice(&full_bytes)?;
    ensure!(
        state.metadata == dsrust::module::Metadata::default()
            && state.predictors.len() == 1
            && full.predictors.len() == 1,
        "The accepted example carries other program state"
    );
    let name = predictor_name("contact", "reading")?;
    let saved = state
        .predictors
        .get_mut(&name)
        .context("No Contact predictor")?;
    let complete = full.get(&name).context("No complete teacher predictor")?;
    let baseline = crate::reading::direct_reader_baseline("contact")?;
    let mut signature = signatures::reading("contact")?;
    signature.instructions = baseline;
    let current = dsrust::module::SignatureState::of(&signature);
    let mut previous = current.fields.clone();
    let primary = previous.remove(0);
    previous.insert(3, primary);
    ensure!(
        saved.signature.instructions == current.instructions
            && saved.signature.fields == previous
            && saved.traces.is_empty()
            && saved.train.is_empty()
            && saved.lm.is_none()
            && saved.demos.len() == 1
            && complete.demos.len() == 1,
        "Only the recorded source-first field ordering may change"
    );
    let teacher = Example::new(
        complete.demos[0]
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    )
    .with_inputs(["question", "consultation", "chart_evidence", "source_text"]);
    let compact = crate::demo::compact(&teacher)?;
    ensure!(
        fields(&Prediction::new(compact, "")) == Value::Object(saved.demos[0].clone()),
        "The compact demonstration changed its three inputs or five outputs"
    );
    let original = inputs
        .legacy
        .iter()
        .find(|f| f.example.get("case_id") == Some(&json!("dspy-contact-first-006")))
        .context("No original demo consultation")?;
    for field in ["question", "consultation", "chart_evidence", "source_text"] {
        ensure!(
            teacher.get(field) == original.example.get(field),
            "Current native/source input {field} differs from the original full demonstration"
        );
    }
    let primary = catalogue::family_reader_source(book, "contact")?;
    ensure!(
        teacher.get("source_text").and_then(Value::as_str) == Some(primary.as_str()),
        "Primary Contact source meaning differs"
    );
    // No new motion/view values are inserted into the old worked example.
    saved.signature = current;
    let receipt = json!({"origin_program":bootstrap,"origin_program_sha256":CONTACT_APPROVED_PROGRAM,
        "origin_full_program_sha256":CONTACT_APPROVED_FULL,"source_review":review,"source_review_sha256":CONTACT_APPROVED_REVIEW,
        "original_dataset_sha256":fixtures_digest(&inputs.legacy)?,"book_sha256":crate::book::OCR_SHA256,
        "presentation_change":"Move source_text first; names, types, descriptions, authored instruction, three compact inputs and five outputs unchanged",
        "new_values_inserted_into_demo":false,"teacher_repurchase":false,"scores_or_optimizer_state_imported":false});
    Ok((state, receipt))
}

/// Prepare the real stock-adapter exchanges without creating provider clients.
pub fn prepare_contact_reference(
    cases: &[Case],
    book: &Book,
    bootstrap: &Path,
    review: &Path,
    directory: &Path,
) -> Result<Value> {
    ensure!(!directory.exists(), "Preserve an existing preparation");
    let inputs = contact_reference_inputs(cases, book)?;
    let (state, origin) = contact_demo_presentation(bootstrap, review, &inputs, book)?;
    let signature = signatures::reading("contact")?;
    let mut predictor = Predict::from_signature(signature.clone());
    restore(
        &mut predictor,
        state.get("reading.contact").unwrap(),
        &signature,
    );
    let mut requests = Vec::new();
    for (arm, fixtures) in [("off", &inputs.off), ("on", &inputs.on)] {
        for mut request in crate::preflight::reader_requests(fixtures, &predictor)? {
            request["arm"] = json!(arm);
            requests.push(request);
        }
    }
    fs::create_dir_all(directory)?;
    let program_bytes = serde_json::to_vec_pretty(&state)?;
    fs::write(directory.join("program.json"), &program_bytes)?;
    let dataset = fixtures_digest(&inputs.off)?;
    crate::queue::write_json_atomic(
        &directory.join("component-source.json"),
        &direct_source_receipt("contact", &program_bytes, &dataset)?,
    )?;
    crate::queue::write_json_atomic(&directory.join("demo-origin.json"), &origin)?;
    crate::queue::write_json_atomic(&directory.join("wire-preflight.json"), &json!(requests))?;
    let result = json!({"phase":"contact_reference_preparation","executable_sha256":crate::google::digest(fs::read(std::env::current_exe()?)?),
        "case_ids":CONTACT_REFERENCE_CASES,"bank_sha256":crate::google::digest(serde_json::to_vec(cases)?),
        "off_dataset_sha256":dataset,"on_dataset_sha256":fixtures_digest(&inputs.on)?,
        "program_sha256":crate::google::digest(&program_bytes),
        "demo_origin_sha256":crate::google::digest(fs::read(directory.join("demo-origin.json"))?),
        "wire_preflight_sha256":crate::google::digest(fs::read(directory.join("wire-preflight.json"))?),
        "common_live_presentation":"eID-keyed original typed EventCandidate including PerfectionMotion in both arms; no old prose duplicated",
        "only_treatment":"Contact role-reference/edge aliases; duplicate event candidate payload removed from aliases",
        "source_first":true,"bootstrap_calls":0,"reflection_calls":0,"provider_calls":0,
        "student_call_cap":8,"teacher_call_cap":12,"whole_reading_concurrency":1,
        "token_preflight_qualified":false,"application_promoted":false});
    crate::queue::write_json_atomic(&directory.join("preparation.json"), &result)?;
    Ok(result)
}

fn validate_contact_token_preflight(prepared: &Path, counts: &Path) -> Result<()> {
    let receipt: Value = serde_json::from_slice(&fs::read(counts)?)?;
    ensure!(
        receipt["preparation_sha256"]
            == crate::google::digest(fs::read(prepared.join("preparation.json"))?)
            && receipt["context_per_slot"] == 32768
            && receipt["student_generation"] == false,
        "Tokenizer receipt does not bind this exact preparation"
    );
    ensure!(
        receipt["model"] == crate::local::MODEL
            && receipt["format"]["add_generation_prompt"] == true
            && receipt["format"]["add_special"] == true
            && receipt["format"]["parse_special"] == true,
        "Tokenizer model/template flags differ from the resident reading wire"
    );
    let requests: Vec<Value> =
        serde_json::from_slice(&fs::read(prepared.join("wire-preflight.json"))?)?;
    let rows = receipt["rows"]
        .as_array()
        .context("No observed token counts")?;
    ensure!(
        rows.len() == requests.len() && rows.len() == 8,
        "Eight actual wire counts are required"
    );
    for (request, row) in requests.iter().zip(rows) {
        let tokens = row["token_count"]
            .as_u64()
            .context("Missing actual token count")?;
        ensure!(
            tokens > 0
                && tokens + 4096 <= 32768
                && row["generation_allowance"] == 4096
                && row["request_sha256"] == request["request_sha256"]
                && row["case_id"] == request["case_id"]
                && row["arm"] == request["arm"],
            "Wire token count differs or exceeds the resident slot"
        );
    }
    Ok(())
}

/// Fixed training comparison through stock Evaluate. No reflection or selection.
pub async fn compare_contact_reference(
    cases: &[Case],
    book: Arc<Book>,
    prepared: &Path,
    counts: &Path,
    models: &crate::runner::Models,
    directory: &Path,
) -> Result<Value> {
    ensure!(
        !directory.join("result.json").exists()
            && !directory.join("off-observations.jsonl").exists()
            && !directory.join("on-observations.jsonl").exists(),
        "Preserve the preceding comparison attempt"
    );
    let inputs = contact_reference_inputs(cases, &book)?;
    let plan: Value = serde_json::from_slice(&fs::read(prepared.join("preparation.json"))?)?;
    ensure!(
        plan["phase"] == "contact_reference_preparation"
            && plan["executable_sha256"]
                == crate::google::digest(fs::read(std::env::current_exe()?)?)
            && plan["case_ids"] == json!(CONTACT_REFERENCE_CASES)
            && plan["bank_sha256"] == crate::google::digest(serde_json::to_vec(cases)?)
            && plan["off_dataset_sha256"] == fixtures_digest(&inputs.off)?
            && plan["on_dataset_sha256"] == fixtures_digest(&inputs.on)?,
        "Prepared comparison sources or native inputs differ"
    );
    for (file, key) in [
        ("program.json", "program_sha256"),
        ("demo-origin.json", "demo_origin_sha256"),
        ("wire-preflight.json", "wire_preflight_sha256"),
    ] {
        ensure!(
            plan[key] == crate::google::digest(fs::read(prepared.join(file))?),
            "Prepared artifact {file} differs"
        );
    }
    let origin: Value = serde_json::from_slice(&fs::read(prepared.join("demo-origin.json"))?)?;
    let (expected_state, expected_origin) = contact_demo_presentation(
        Path::new(
            origin["origin_program"]
                .as_str()
                .context("No original demonstration path")?,
        ),
        Path::new(
            origin["source_review"]
                .as_str()
                .context("No source-review path")?,
        ),
        &inputs,
        &book,
    )?;
    ensure!(
        origin == expected_origin,
        "The derived demonstration provenance differs"
    );
    validate_contact_token_preflight(prepared, counts)?;
    require_dataset(
        &prepared.join("program.json"),
        &fixtures_digest(&inputs.off)?,
    )?;
    let state = validated_state_kind(&prepared.join("program.json"), "contact", "reading", true)?;
    ensure!(
        state == expected_state,
        "Only the known approved demonstration presentation may execute"
    );
    let baseline = crate::reading::direct_reader_baseline("contact")?;
    ensure!(
        state.get("reading.contact").unwrap().signature.instructions == baseline
            && state.get("reading.contact").unwrap().demos.len() == 1,
        "Prepared instruction/demo differs"
    );
    fs::create_dir_all(directory)?;
    let mut results = BTreeMap::new();
    let mut all_observed = true;
    let mut all_consistent = true;
    for (arm, fixtures) in [("off", &inputs.off), ("on", &inputs.on)] {
        let mut program = direct_program(
            fixtures,
            "contact",
            book.clone(),
            models.student.clone(),
            models.teacher.clone(),
            &baseline,
            &directory.join(format!("{arm}-observations.jsonl")),
        )?;
        program.load_state(&state);
        program.save(&directory.join(format!("{arm}-program.json")))?;
        let evaluation = Evaluate::new(
            fixtures.iter().map(|f| f.example.clone()).collect(),
            |input| program.forward(input),
            Graded::metric,
        )
        .run()
        .await;
        let receipt = evaluation_receipt(&evaluation);
        crate::queue::write_json_atomic(&directory.join(format!("{arm}.json")), &receipt)?;
        program.save_observations(&directory.join(format!("{arm}-observations.json")))?;
        all_observed &= evaluation.failure_count() == 0;
        all_consistent &= program
            .observations
            .lock()
            .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?
            .iter()
            .all(|o| o["review_summary_consistent"] == true);
        results.insert(arm, receipt);
        // An unknown provider/reviewer outcome never becomes a semantic zero
        // or permission to buy another arm. Received model failures are retained.
        if !all_observed {
            break;
        }
    }
    let result = json!({"function":"reading.contact","comparison":"fixed four original training cases; role-reference view off/on",
        "teacher":models.teacher_id,"student":models.student_id,"arms":results,
        "execution_observed":all_observed && results.len() == 2,"independent_review_consistent":all_consistent,
        "preparation_sha256":crate::google::digest(fs::read(prepared.join("preparation.json"))?),
        "token_preflight_sha256":crate::google::digest(fs::read(counts)?),
        "bootstrap_calls":0,"reflection_calls":0,"selection_calls":0,"application_promoted":false,
        "source_correct_reading_qualified":false});
    crate::queue::write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(
        all_observed && results.len() == 2,
        "A provider/reviewer error leaves this comparison unqualified"
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

    #[test]
    fn contact_reference_is_the_only_treatment_and_motion_is_common() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let cases = catalogue::cases(&directory, "contact").unwrap();
        let prepared = contact_reference_inputs(&cases, &Book::fixture()).unwrap();
        assert_eq!(prepared.legacy.len(), 20);
        assert_eq!(prepared.off.len(), 4);
        for (off, on) in prepared.off.iter().zip(&prepared.on) {
            assert_eq!(off.split, "train");
            assert_eq!(
                off.example
                    .inputs()
                    .unwrap()
                    .fields()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>(),
                vec!["question", "consultation", "chart_evidence", "source_text"]
            );
            let off_chart: Value =
                serde_json::from_str(off.example.get("chart_evidence").unwrap().as_str().unwrap())
                    .unwrap();
            let mut on_chart: Value =
                serde_json::from_str(on.example.get("chart_evidence").unwrap().as_str().unwrap())
                    .unwrap();
            assert_eq!(off_chart["event_candidates"], on_chart["event_candidates"]);
            assert!(
                off_chart["event_candidates"]
                    .as_object()
                    .unwrap()
                    .values()
                    .any(|event| event.get("perfectionMotion").is_some())
            );
            let view = on_chart
                .as_object_mut()
                .unwrap()
                .remove("role_reference")
                .unwrap();
            assert_eq!(off_chart, on_chart);
            assert!(
                view["event_endpoints"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|endpoint| endpoint.get("candidate").is_none())
            );
            let roles = view["aliases"].as_array().unwrap();
            assert!(!roles.is_empty());
        }
        for (case, legacy) in cases.iter().zip(prepared.legacy) {
            assert_eq!(
                legacy.example.get("chart_evidence"),
                Some(&json!(
                    serde_json::to_string(&catalogue::chart_evidence(case).unwrap()).unwrap()
                ))
            );
        }
    }

    #[test]
    fn contact_reference_rejects_a_moved_training_case_before_any_model() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut cases = catalogue::cases(&directory, "contact").unwrap();
        cases
            .iter_mut()
            .find(|case| case.id == CONTACT_REFERENCE_CASES[0])
            .unwrap()
            .split = "development".into();
        assert!(
            contact_reference_inputs(&cases, &Book::fixture())
                .err()
                .unwrap()
                .to_string()
                .contains("original training")
        );
    }

    #[test]
    fn contact_reference_requires_exact_bound_token_counts_for_both_arms() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("preparation.json"),
            "Synthetic pinned preparation",
        )
        .unwrap();
        let requests=(0..8).map(|i|json!({"arm":if i<4{"off"}else{"on"},"case_id":CONTACT_REFERENCE_CASES[i%4],"request_sha256":format!("request{i}")})).collect::<Vec<_>>();
        crate::queue::write_json_atomic(&dir.path().join("wire-preflight.json"), &json!(requests))
            .unwrap();
        let rows=requests.iter().map(|r|json!({"arm":r["arm"],"case_id":r["case_id"],"request_sha256":r["request_sha256"],"token_count":17000,"generation_allowance":4096})).collect::<Vec<_>>();
        let original = json!({"preparation_sha256":crate::google::digest(fs::read(dir.path().join("preparation.json")).unwrap()),"context_per_slot":32768,"model":crate::local::MODEL,
            "format":{"add_generation_prompt":true,"add_special":true,"parse_special":true},"student_generation":false,"rows":rows});
        let path = dir.path().join("counts.json");
        crate::queue::write_json_atomic(&path, &original).unwrap();
        validate_contact_token_preflight(dir.path(), &path).unwrap();
        for mutation in 0..6 {
            let mut invalid = original.clone();
            match mutation {
                0 => invalid["rows"][4]["token_count"] = json!(28673),
                1 => invalid["rows"][4]["request_sha256"] = json!("different request"),
                2 => invalid["rows"][4]["arm"] = json!("off"),
                3 => invalid["preparation_sha256"] = json!("different preparation"),
                4 => invalid["format"]["add_generation_prompt"] = json!(false),
                _ => invalid["rows"].as_array_mut().unwrap().truncate(7),
            }
            crate::queue::write_json_atomic(&path, &invalid).unwrap();
            assert!(validate_contact_token_preflight(dir.path(), &path).is_err());
        }
    }

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
    async fn reader_view_changes_only_reader_and_still_observes_actual_selector() {
        let input = crate::reading::tests::input(json!({"facts":[]}));
        let oracle = Arc::new(DummyLM::new([
            dsrust::example! {branch:"pure_completion"},
            dsrust::example! {status:"method_limit",answer:"Synthetic interpretation.",analysis:"e1 is the supplied boundary.",evidence:"e1",unresolved:"Actual synthetic calculation limit."},
        ]));
        let judge = Arc::new(DummyLM::new([review(
            "unobserved",
            "unobserved",
            "pass",
            "pass",
            true,
        )]));
        let mut program = Graded::new(
            "movable_deal",
            "reading",
            std::slice::from_ref(&input),
            oracle.clone(),
            judge.clone(),
            Some(Arc::new(Book::fixture())),
        )
        .unwrap();
        let before = serde_json::to_value(program.dump_state()).unwrap();
        let mut view = FunctionView {
            program: &mut program,
            name: "reading.movable_deal".into(),
        };
        let predictors = view.named_predictors();
        assert_eq!(predictors.len(), 1);
        assert_eq!(predictors[0].name, "reading.movable_deal");
        predictors
            .into_iter()
            .next()
            .unwrap()
            .signature
            .instructions = "Synthetic reader instruction only".into();
        let mut trace = Vec::new();
        let prediction = view
            .forward_traced(input.inputs().unwrap(), &mut trace)
            .await
            .unwrap();
        assert_eq!(
            prediction.get("__reading_branch"),
            Some(&json!("pure_completion"))
        );
        assert_eq!(
            trace
                .iter()
                .map(|t| t.predictor.as_str())
                .collect::<Vec<_>>(),
            ["reading_branch.movable_deal", "reading.movable_deal"]
        );
        let after = serde_json::to_value(program.dump_state()).unwrap();
        assert_eq!(
            before["reading_branch.movable_deal"],
            after["reading_branch.movable_deal"]
        );
        assert_ne!(
            before["reading.movable_deal"],
            after["reading.movable_deal"]
        );
        assert_eq!(program.dump_state().predictors.len(), 2);
        assert_eq!(oracle.asked().len(), 2);
        assert!(
            oracle.asked()[1]
                .system
                .contains("Synthetic reader instruction only")
        );
        assert!(!format!("{:?}", oracle.asked()).contains("PRIVATE GOLD"));
        assert!(
            judge.asked()[0]
                .system
                .contains("Applicable dimensions: reading, answer_quality")
        );
    }

    #[tokio::test]
    async fn a_reading_needs_both_interpretation_and_a_coherent_answer() {
        for (reading, quality) in [("pass", "fail"), ("fail", "pass")] {
            let input = crate::reading::tests::input(
                json!({"facts":[],"branch":"pure_completion","method":"movable_deal"}),
            );
            let oracle = Arc::new(DummyLM::new([
                dsrust::example! {status:"method_limit",answer:"Synthetic only.",analysis:"e1",evidence:"e1",unresolved:"Synthetic limit"},
            ]));
            let judge = Arc::new(DummyLM::new([review(
                "unobserved",
                "unobserved",
                reading,
                quality,
                false,
            )]));
            let program = Graded::new(
                "movable_deal",
                "reading",
                std::slice::from_ref(&input),
                oracle,
                judge,
                Some(Arc::new(Book::fixture())),
            )
            .unwrap();
            let output = program.forward(input).await.unwrap();
            assert_eq!(output.get("__passed"), Some(&json!(false)));
            assert!(output.get("__score").unwrap().as_f64().unwrap() < 1.);
            assert!(!program.failed.load(Ordering::SeqCst));
        }
    }
    #[tokio::test]
    async fn stock_gepa_reader_only_saves_full_program_without_changing_selector() {
        let fixtures = ["train", "development", "test"].map(|split| {
            let mut example = crate::reading::tests::input(json!({"facts":[]}));
            example.set(
                "question",
                json!(format!("Synthetic {split} sale question")),
            );
            Fixture {
                split: split.into(),
                example,
            }
        });
        let oracle = Arc::new(DummyLM::new((0..50).flat_map(|_| [
            dsrust::example! {branch:"pure_completion"},
            dsrust::example! {status:"method_limit",answer:"Synthetic answer only.",analysis:"e1 boundary",evidence:"e1",unresolved:"Synthetic calculation limit"},
        ])));
        let judge =
            Arc::new(DummyLM::new((0..50).map(|_| {
                review("unobserved", "unobserved", "pass", "pass", true)
            })));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge,
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let book = Arc::new(Book::fixture());
        let result = compile_reader_fixtures(
            &fixtures,
            "movable_deal",
            book.clone(),
            &models,
            directory.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            "Source-adapted synthetic reader only",
        )
        .await
        .unwrap();
        assert_eq!(
            result["optimizable_predictors"],
            json!(["reading.movable_deal"])
        );
        assert_eq!(result["bootstrap_demos"], 0);
        let baseline: Value = serde_json::from_slice(
            &fs::read(directory.path().join("baseline-program.json")).unwrap(),
        )
        .unwrap();
        let compiled: Value =
            serde_json::from_slice(&fs::read(directory.path().join("compiled.json")).unwrap())
                .unwrap();
        assert_eq!(
            baseline["reading_branch.movable_deal"],
            compiled["reading_branch.movable_deal"]
        );
        assert!(compiled["reading_branch.movable_deal"].is_object());
        let mut reloaded = ReadingProgram::new("movable_deal", book, oracle).unwrap();
        load_reading_component(
            &mut reloaded,
            &directory.path().join("compiled.json"),
            "movable_deal",
            &fixtures_digest(&fixtures).unwrap(),
        )
        .unwrap();
        assert_eq!(reloaded.dump_state().predictors.len(), 2);
        assert_eq!(
            reloaded.reader.signature.instructions,
            "Source-adapted synthetic reader only"
        );
    }
    #[tokio::test]
    async fn direct_new_job_gepa_uses_one_predict_and_the_full_family_source_without_gold_branch() {
        let book = Arc::new(Book::fixture());
        let source = crate::reading::new_job_family_source(&book).unwrap();
        assert!(source.contains("printed_page=\"225\""));
        let fixtures = ["train", "development", "test"].map(|split| {
            let mut example = crate::reading::tests::input(json!({"facts":[]}));
            example.set("question", json!(format!("Synthetic {split} job question")));
            example.set("source_text", json!(&source));
            example.set("__review_source", json!(&source));
            Fixture {
                split: split.into(),
                example,
            }
        });
        let oracle = Arc::new(DummyLM::new((0..50).map(|_| {
            dsrust::example! {status:"method_limit",answer:"Synthetic answer only.",analysis:"e1 boundary",evidence:"e1",unresolved:"Synthetic calculation limit"}
        })));
        let judge =
            Arc::new(DummyLM::new((0..50).map(|_| {
                review("unobserved", "unobserved", "pass", "pass", true)
            })));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: judge,
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let result = compile_direct_reader_fixtures(
            &fixtures,
            "new_job",
            book.clone(),
            &models,
            directory.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            "Synthetic direct-family reader baseline",
        )
        .await
        .unwrap();
        assert_eq!(result["program_kind"], "direct_family_reading");
        assert_eq!(result["optimizable_predictors"], json!(["reading.new_job"]));
        let compiled: Value =
            serde_json::from_slice(&fs::read(directory.path().join("compiled.json")).unwrap())
                .unwrap();
        assert_eq!(compiled.as_object().unwrap().len(), 2);
        assert!(compiled["reading.new_job"].is_object());
        assert!(compiled.get("reading_branch.new_job").is_none());
        let receipt: Value = serde_json::from_slice(
            &fs::read(directory.path().join("component-source.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["version"], 3);
        assert_eq!(receipt["program_kind"], "direct_family_reading");
        assert!(receipt.get("branch_declaration_sha256").is_none());
        let observations: Vec<Value> =
            serde_json::from_slice(&fs::read(directory.path().join("observations.json")).unwrap())
                .unwrap();
        assert!(observations.iter().all(|row| {
            let stages = row["oracle_stages"].as_array().unwrap();
            let inputs: BTreeMap<String, String> =
                serde_json::from_value::<Vec<(String, String)>>(stages[0]["inputs"].clone())
                    .unwrap()
                    .into_iter()
                    .collect();
            let outputs: BTreeMap<String, String> =
                serde_json::from_value::<Vec<(String, String)>>(stages[0]["outputs"].clone())
                    .unwrap()
                    .into_iter()
                    .collect();
            stages.len() == 1
                && stages[0]["predictor"] == "reading.new_job"
                && inputs.get("source_text") == Some(&source)
                && !inputs.contains_key("expected")
                && !outputs.contains_key("__reading_branch")
        }));
        assert!(!format!("{:?}", oracle.asked()).contains("PRIVATE GOLD"));
        let mut legacy = ReadingProgram::new("new_job", book, oracle).unwrap();
        assert!(
            load_reading_component(
                &mut legacy,
                &directory.path().join("compiled.json"),
                "new_job",
                &fixtures_digest(&fixtures).unwrap(),
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn direct_new_job_rejects_incomplete_source_before_any_model_submission() {
        let fixtures = ["train", "development", "test"].map(|split| Fixture {
            split: split.into(),
            example: crate::reading::tests::input(json!({"facts":[]})),
        });
        let oracle = Arc::new(DummyLM::new([]));
        let teacher = Arc::new(DummyLM::new([]));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher: teacher.clone(),
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        assert!(
            compile_direct_reader_fixtures(
                &fixtures,
                "new_job",
                Arc::new(Book::fixture()),
                &models,
                directory.path(),
                CompileOptions {
                    max_metrics: 1,
                    bootstrap_demos: 0
                },
                "Synthetic baseline",
            )
            .await
            .is_err()
        );
        assert!(oracle.asked().is_empty());
        assert!(teacher.asked().is_empty());
    }

    #[tokio::test]
    async fn contact_direct_reader_reaches_the_target_without_a_selector_or_gold_branch() {
        let book = Arc::new(Book::fixture());
        let source = catalogue::family_reader_source(&book, "contact").unwrap();
        assert!(source.starts_with(&catalogue::method_source(&book, "contact", true).unwrap()));
        assert!(source.contains("printed_page=\"92\""));
        assert!(source.contains("printed_page=\"93\""));
        assert!(source.contains("printed_page=\"165\""));
        assert!(source.contains("printed_page=\"166\""));
        let fixtures = ["train", "development", "test"].map(|split| {
            let mut example = crate::reading::tests::input(json!({"facts":[]}));
            example.set(
                "question",
                json!(format!("Synthetic {split} contact question")),
            );
            example.set("source_text", json!(&source));
            example.set("__review_source", json!(&source));
            Fixture {
                split: split.into(),
                example,
            }
        });
        let oracle = Arc::new(DummyLM::new((0..50).map(|_| {
            dsrust::example! {status:"method_limit",answer:"Synthetic answer only.",analysis:"e1 boundary",evidence:"e1",unresolved:"Synthetic calculation limit"}
        })));
        let teacher =
            Arc::new(DummyLM::new((0..50).map(|_| {
                review("unobserved", "unobserved", "pass", "pass", true)
            })));
        let models = crate::runner::Models {
            student: oracle.clone(),
            teacher,
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let result = compile_direct_reader_fixtures(
            &fixtures,
            "contact",
            book,
            &models,
            directory.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            &crate::reading::direct_reader_baseline("contact").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(result["program_kind"], "direct_family_reading");
        assert_eq!(result["optimizable_predictors"], json!(["reading.contact"]));
        let state: Value =
            serde_json::from_slice(&fs::read(directory.path().join("compiled.json")).unwrap())
                .unwrap();
        assert_eq!(
            state
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            ["metadata", "reading.contact"].into_iter().collect()
        );
        assert!(!format!("{:?}", oracle.asked()).contains("PRIVATE GOLD"));
        let observations: Vec<Value> =
            serde_json::from_slice(&fs::read(directory.path().join("observations.json")).unwrap())
                .unwrap();
        assert!(observations.iter().all(|row| {
            let stages = row["oracle_stages"].as_array().unwrap();
            stages.len() == 1 && stages[0]["predictor"] == "reading.contact"
        }));
    }

    fn direct_contact_fixtures(book: &Book) -> Vec<Fixture> {
        let source = catalogue::family_reader_source(book, "contact").unwrap();
        ["train", "development", "test"]
            .into_iter()
            .map(|split| {
                let mut example = crate::reading::tests::input(json!({"facts":[]}));
                example.set(
                    "question",
                    json!(format!("Synthetic {split} contact question")),
                );
                example.set("source_text", json!(&source));
                example.set("__review_source", json!(&source));
                Fixture {
                    split: split.into(),
                    example,
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn reviewed_labels_measure_authored_labeled_and_selected_without_teacher_repurchase() {
        let book = Arc::new(Book::fixture());
        let mut fixtures = direct_contact_fixtures(&book);
        let primary = catalogue::reference_reader_source(&book, "contact").unwrap();
        for fixture in &mut fixtures {
            fixture.example.set("__review_source", json!(&primary));
            fixture.example.set(
                "__reference_answer",
                json!("Synthetic reference ANSWER separate from PRIVATE GOLD criteria"),
            );
        }
        let worked = crate::gold::WorkedAnswer {
            status: "method_limit".into(),
            answer: "Synthetic actual TRAIN worked answer".into(),
            analysis: "e1 supplied calculation limit".into(),
            evidence: "e1".into(),
            unresolved: "Unknown synthetic calculation".into(),
        };
        let label = worked.labeled_example(&fixtures[0].example).unwrap();
        let student=Arc::new(DummyLM::new((0..20).map(|_|dsrust::example!{status:"method_limit",answer:"Synthetic received answer",analysis:"e1 actual boundary",evidence:"e1",unresolved:"Unknown computation"})));
        let teacher =
            Arc::new(DummyLM::new((0..20).map(|_| {
                review("unobserved", "unobserved", "pass", "pass", true)
            })));
        let models = crate::runner::Models {
            student: student.clone(),
            teacher: teacher.clone(),
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let out = tempfile::tempdir().unwrap();
        let result = compile_direct_reader_from_gold(
            &fixtures,
            "contact",
            book.clone(),
            &models,
            out.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            &crate::reading::direct_reader_baseline("contact").unwrap(),
            &[label],
            1,
        )
        .await
        .unwrap();
        assert_eq!(result["authored_development"], 1.);
        assert_eq!(result["labeled_development"], 1.);
        assert!(out.path().join("authored-development.json").exists());
        let first = format!("{:?}", student.asked()[0]);
        assert!(!first.contains("Synthetic actual TRAIN worked answer"));
        let labeled = format!("{:?}", student.asked()[1]);
        assert!(labeled.contains("Synthetic actual TRAIN worked answer"));
        assert!(!format!("{:?}", student.asked()).contains("PRIVATE GOLD"));
        assert!(format!("{:?}", teacher.asked()).contains("reference_answer"));
        assert_eq!(student.asked().len(), 5);
        assert_eq!(teacher.asked().len(), 5);
        let mut wrong = worked.labeled_example(&fixtures[1].example).unwrap();
        wrong.set("question", json!("Synthetic development label"));
        let before = student.asked().len();
        let other = tempfile::tempdir().unwrap();
        assert!(
            compile_direct_reader_from_gold(
                &fixtures,
                "contact",
                book,
                &models,
                other.path(),
                CompileOptions {
                    max_metrics: 1,
                    bootstrap_demos: 0
                },
                &crate::reading::direct_reader_baseline("contact").unwrap(),
                &[wrong],
                1
            )
            .await
            .is_err()
        );
        assert_eq!(student.asked().len(), before);
    }

    #[tokio::test]
    async fn direct_bootstrap_calls_only_teacher_and_retains_one_fully_reviewed_training_demo() {
        let book = Arc::new(Book::fixture());
        let fixtures = direct_contact_fixtures(&book);
        let student = Arc::new(DummyLM::new((0..20).map(|_|dsrust::example!{status:"method_limit",answer:"Synthetic answer.",analysis:"e1 actual boundary",evidence:"e1",unresolved:"Unknown computation"})));
        let teacher = Arc::new(DummyLM::new(std::iter::once(dsrust::example!{status:"method_limit",answer:"Synthetic worked answer.",analysis:"e1 actual boundary",evidence:"e1",unresolved:"Unknown computation"}).chain((0..20).map(|_|review("unobserved","unobserved","pass","pass",true)))));
        let models = crate::runner::Models {
            student: student.clone(),
            teacher: teacher.clone(),
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let baseline = crate::reading::direct_reader_baseline("contact").unwrap();
        let result = bootstrap_direct_reader_fixtures(
            &fixtures,
            "contact",
            book.clone(),
            &models,
            directory.path(),
            &baseline,
        )
        .await
        .unwrap();
        assert_eq!(result["accepted_demos"], 1);
        assert_eq!(teacher.asked().len(), 2);
        assert!(student.asked().is_empty());
        assert!(!format!("{:?}", teacher.asked()[0]).contains("PRIVATE GOLD"));
        let path = directory.path().join("bootstrapped.json");
        let state = validated_state_kind(&path, "contact", "reading", true).unwrap();
        let demo = &state.get("reading.contact").unwrap().demos[0];
        assert_eq!(demo["question"], "Synthetic train contact question");
        assert!(!demo.contains_key("source_text"));
        assert!(demo.keys().all(|k| !k.starts_with("__")));
        let rendered: Value = serde_json::from_slice(
            &fs::read(directory.path().join("student-preflight.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(rendered["provider_calls"], 0);
        assert_eq!(rendered["requests"].as_array().unwrap().len(), 3);
        assert!(
            rendered["requests"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| !row["messages"].to_string().contains("PRIVATE GOLD"))
        );
        let mut changed = fixtures.clone();
        changed[0]
            .example
            .set("question", json!("Changed source bank"));
        let failed = tempfile::tempdir().unwrap();
        assert!(
            compile_direct_reader_from_bootstrap(
                &changed,
                "contact",
                book.clone(),
                &models,
                failed.path(),
                CompileOptions {
                    max_metrics: 1,
                    bootstrap_demos: 0
                },
                &baseline,
                &path
            )
            .await
            .is_err()
        );
        assert!(student.asked().is_empty());
        let compiled = tempfile::tempdir().unwrap();
        let run = compile_direct_reader_from_bootstrap(
            &fixtures,
            "contact",
            book,
            &models,
            compiled.path(),
            CompileOptions {
                max_metrics: 1,
                bootstrap_demos: 0,
            },
            &baseline,
            &path,
        )
        .await
        .unwrap();
        assert_eq!(run["bootstrap_demos"], 1);
        let seed: Value = serde_json::from_slice(
            &fs::read(compiled.path().join("baseline-program.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            seed["reading.contact"]["demos"].as_array().unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn bootstrap_withholds_a_contradictory_review_without_lowering_its_saved_raw_score() {
        let book = Arc::new(Book::fixture());
        let fixtures = direct_contact_fixtures(&book);
        let student = Arc::new(DummyLM::new([]));
        let teacher = Arc::new(DummyLM::new([
            dsrust::example! {status:"method_limit",answer:"Synthetic worked answer.",analysis:"e1 boundary",evidence:"e1",unresolved:"Unknown"},
            review("unobserved", "unobserved", "pass", "pass", false),
        ]));
        let models = crate::runner::Models {
            student: student.clone(),
            teacher: teacher.clone(),
            student_id: "Synthetic offline".into(),
            teacher_id: "Synthetic offline".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        assert!(
            bootstrap_direct_reader_fixtures(
                &fixtures,
                "contact",
                book,
                &models,
                directory.path(),
                &crate::reading::direct_reader_baseline("contact").unwrap()
            )
            .await
            .is_err()
        );
        assert!(student.asked().is_empty());
        assert_eq!(teacher.asked().len(), 2);
        assert!(!directory.path().join("bootstrapped.json").exists());
        let rows: Value = serde_json::from_slice(
            &fs::read(directory.path().join("teacher-observations.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(rows[0]["score"], 1.0);
        assert_eq!(rows[0]["review_summary_consistent"], false);
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
        let mut trace = Vec::new();
        let rejection = program
            .forward_traced(input.clone(), &mut trace)
            .await
            .unwrap();
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
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].predictor, "classification");
        assert_eq!(
            trace[0].inputs.rendered(),
            input.inputs().unwrap().rendered()
        );
        assert_eq!(trace[0].outputs.fields().count(), 2);
        assert!(
            trace[0]
                .outputs
                .get("__received_output")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("Observed copied input")
        );
        assert!(trace[0].outputs.get("__format_rejection").is_some());
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
