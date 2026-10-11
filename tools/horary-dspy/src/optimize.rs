//! Use the port's Predict, Module, BootstrapFewShot, Evaluate and GEPA directly.
//! This file supplies domain inputs/metrics; it contains no search algorithm.
use crate::{
    book::Book,
    catalogue::{self, Case, Fact},
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{BootstrapFewShot, Evaluate, Example, GEPA, Module, Predict};
use dsrust::{
    Feedback,
    example::Prediction,
    lm::{DynChatModel, LmConfig},
    module::{NamedPredictor, TraceStep},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
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

/// Only the oracle predictor is optimizable. The independent criterion does
/// not become a second conversational stage with authority to redefine its task.
pub struct Graded {
    oracle: Predict,
    judge: Predict,
    examples: BTreeMap<String, Example>,
    function: String,
    observations: Arc<Mutex<Vec<Value>>>,
}
impl Graded {
    pub fn new(
        method: &str,
        function: &str,
        examples: &[Example],
        student: Arc<dyn DynChatModel>,
        teacher: Arc<dyn DynChatModel>,
    ) -> Result<Self> {
        let signature = match function {
            "reading" => signatures::reading(method)?,
            "extraction" => signatures::extraction(method)?,
            _ => signatures::auxiliary(function)?,
        };
        let mut index = BTreeMap::new();
        for example in examples {
            ensure!(
                index
                    .insert(input_key(&example.inputs()?)?, example.clone())
                    .is_none(),
                "Duplicate neural inputs have ambiguous labels"
            );
        }
        Ok(Self {
            oracle: plain(signature, student),
            judge: plain(signatures::auxiliary("judge")?, teacher),
            examples: index,
            function: function.into(),
            observations: Arc::new(Mutex::new(Vec::new())),
        })
    }
    async fn ask(&self, input: Example, trace: &mut Vec<TraceStep>) -> Result<Prediction> {
        let example = self
            .examples
            .get(&input_key(&input)?)
            .context("Unknown evaluation input")?;
        let mut prediction = self.oracle.forward_traced(input.clone(), trace).await?;
        let source = example
            .get("__review_source")
            .or_else(|| input.get("source_text"))
            .or_else(|| input.get("requirements"))
            .or_else(|| input.get("catalogue"))
            .cloned()
            .unwrap_or(json!(
                "Native device/calendar context and Frawley printed7–8."
            ));
        let mut errors = Vec::new();
        if self.function == "extraction" {
            let facts: Vec<Fact> =
                serde_json::from_str(text(&prediction, "facts")?).context("Facts did not parse")?;
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
            for fact in &facts {
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
            serde_json::from_str::<Vec<String>>(text(&prediction, "missing")?)
                .context("Missing fields did not parse")?;
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
        let dimension = match self.function.as_str() {
            "classification" => "classification",
            "extraction" | "place" | "moment" | "update" => "extraction",
            "reading" => "reading",
            _ => anyhow::bail!("Unknown graded function"),
        };
        let passed = errors.is_empty()
            && review.get("passed") == Some(&json!(true))
            && text(&review, dimension)? == "pass";
        let score = if !errors.is_empty() {
            0.
        } else if passed {
            1.
        } else {
            judged.min(0.99)
        };
        prediction.example.set("__score", json!(score));
        prediction.example.set("__feedback",json!(format!("Native checks: {errors:?}. Independent review: {feedback}\nSALIENT PRIMARY FRAWLEY CONTEXT FOR THIS QUESTION TYPE:\n{source}")));
        prediction.example.set("__passed", json!(passed));
        self.observations.lock().map_err(|_|anyhow::anyhow!("Observation lock poisoned"))?.push(json!({"case_id":example.get("case_id"),"function":self.function,"inputs":input.rendered(),"prediction":fields(&prediction),"review":fields(&review),"native_failures":errors}));
        eprintln!(
            "{} {} score {:.3}",
            self.function,
            example.get("case_id").unwrap_or(&Value::Null),
            score
        );
        Ok(prediction)
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
        self.oracle.named_predictors()
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
    fs::create_dir_all(directory)?;
    let mut train = Vec::new();
    let mut development = Vec::new();
    let mut test = Vec::new();
    for case in cases {
        let example = catalogue::example(case, function, book)?;
        match case.split.as_str() {
            "train" => train.push(example),
            "development" => development.push(example),
            "test" => test.push(example),
            _ => anyhow::bail!("Unknown split"),
        }
    }
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
    )?;
    let before = Evaluate::new(
        development.clone(),
        |input| program.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    ensure!(
        before.failure_count() == 0,
        "Baseline contains execution/review errors; no learned comparison can be qualified"
    );
    let mut strong = Graded::new(
        method,
        function,
        &all,
        models.teacher.clone(),
        models.teacher.clone(),
    )?;
    let mut bootstrap = BootstrapFewShot::new(Graded::metric);
    bootstrap.max_bootstrapped_demos = 2;
    bootstrap.max_labeled_demos = 0;
    bootstrap.metric_threshold = Some(1.);
    bootstrap.max_errors = 1;
    let demos = bootstrap
        .compile_with_teacher(&mut program, &mut strong, &train)
        .await?;
    // Constant primary context is supplied on every real call. Keeping it in
    // every demonstration would repeat entire book chapters without adding data.
    for demo in &mut program.oracle.demos {
        *demo = crate::demo::compact(demo)?;
    }
    let outcome = GEPA::new(Graded::feedback, models.teacher.clone())
        .with_max_metric_calls(max_metrics)
        .with_seed(23)
        .compile(&mut program, &train, &development)
        .await?;
    let after = Evaluate::new(
        development.clone(),
        |input| program.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    let held = Evaluate::new(test, |input| program.forward(input), Graded::metric)
        .run()
        .await;
    ensure!(
        after.failure_count() == 0 && held.failure_count() == 0,
        "A provider/parser/reviewer error remains unqualified, never a fabricated semantic zero"
    );
    let result = json!({"method":method,"function":function,"teacher":models.teacher_id,"student":crate::google::STUDENT,"bootstrap_demos":demos,"before_development":before.score,"after_development":after.score,"held_out":held.score,"gepa_best_index":outcome.best_idx,"gepa_metric_calls":outcome.total_num_evals,"gepa_iterations":outcome.iterations,"component_only":true,"application_promoted":false});
    program.save(&directory.join("compiled.json"))?;
    fs::write(
        directory.join("observations.json"),
        serde_json::to_vec_pretty(
            &*program
                .observations
                .lock()
                .map_err(|_| anyhow::anyhow!("Observation lock poisoned"))?,
        )?,
    )?;
    fs::write(
        directory.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
