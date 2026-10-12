//! Thin commands around the port's evaluator and compilers, not a search engine.
use crate::{
    book::Book,
    catalogue::{self, Case},
    google,
    journey::{self, Journey},
    optimize::Graded,
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{BootstrapFewShot, Evaluate, GEPA, Module, lm::DynChatModel};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc};
pub struct Models {
    pub student: Arc<dyn DynChatModel>,
    pub teacher: Arc<dyn DynChatModel>,
    pub teacher_id: String,
    pub student_id: String,
}

pub fn methods(selected: &str) -> Result<Vec<&str>> {
    if selected == "all" {
        return Ok(signatures::METHODS
            .iter()
            .copied()
            .filter(|m| !matches!(*m, "wish" | "unclassified"))
            .collect());
    }
    ensure!(
        signatures::METHODS.contains(&selected) && !matches!(selected, "wish" | "unclassified"),
        "Select a substantive reading family or all"
    );
    Ok(vec![selected])
}

pub fn manifest(
    output: &Path,
    cases: &[Case],
    book_path: &Path,
    configuration: Value,
) -> Result<()> {
    fs::create_dir_all(output)?;
    let data = serde_json::to_vec_pretty(cases)?;
    let teacher = configuration
        .get("teacher")
        .and_then(Value::as_str)
        .unwrap_or(google::TEACHER);
    let student = configuration
        .get("student")
        .and_then(Value::as_str)
        .unwrap_or(google::STUDENT);
    let student_inputs = configuration
        .get("component_inputs")
        .and_then(Value::as_str)
        .unwrap_or("original question and actual device context only");
    let receipt = json!({"teacher":teacher,"student":student,"configuration":configuration,
        "executable_sha256":google::digest(fs::read(std::env::current_exe()?)?),
        "book_sha256":google::digest(fs::read(book_path)?),"dataset_sha256":google::digest(&data),
        "coverage_sha256":google::digest(include_bytes!("../coverage.json")),
        "signatures_sha256":google::digest(include_bytes!("signatures.rs")),
        "catalogue_sha256":google::digest(include_bytes!("catalogue.rs")),
        "journey_sha256":google::digest(include_bytes!("journey.rs")),
        "port_revision":"f24adde08c1d8850e4d7079d019643bb40f905cb",
        "effective_port_revision":crate::local::PORT_REVISION,
        "port_predict_sha256":google::digest(include_bytes!("../vendor/dsrust/src/predict.rs")),
        "port_openai_sha256":google::digest(include_bytes!("../vendor/dsrust/src/lm/openai/mod.rs")),
        "cargo_lock_sha256":google::digest(include_bytes!("../Cargo.lock")),
        "provider_schema":false,"student_inputs":student_inputs,
        "case_count":cases.len(),"qualified":false});
    let path = output.join("manifest.json");
    if path.exists() {
        ensure!(
            serde_json::from_slice::<Value>(&fs::read(&path)?)? == receipt,
            "Existing run provenance differs; preserve it and use a new output directory"
        );
    } else {
        fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    }
    fs::write(output.join("dataset.json"), data)?;
    Ok(())
}

fn evaluation_errors(evaluation: &dsrust::Evaluation) -> Vec<Value> {
    evaluation
        .results
        .iter()
        .filter_map(|r| {
            r.prediction
                .as_ref()
                .err()
                .map(|e| json!({"case_id":r.example.get("case_id"),"error":e}))
        })
        .collect()
}

pub async fn run(
    cases: &[Case],
    method: &str,
    book: Arc<Book>,
    models: &Models,
    output: &Path,
    training_check: bool,
) -> Result<Value> {
    fs::create_dir_all(output)?;
    let program = Journey::new(
        method,
        cases,
        book,
        models.student.clone(),
        models.teacher.clone(),
    )?
    .with_stream(output.join("observations.jsonl"))?;
    let evaluated = Evaluate::new(
        cases.iter().map(journey::example).collect(),
        |input| program.forward(input),
        Graded::metric,
    )
    .run()
    .await;
    program.save_observations(&output.join("observations.json"))?;
    let result = json!({"method":method,"cases":evaluated.results.len(),"score":if evaluated.failure_count()==0{Some(evaluated.score)}else{None},
        "execution_errors":evaluated.failure_count(),"errors":evaluation_errors(&evaluated),"training_transport_check":training_check,
        "teacher":models.teacher_id,"student":models.student_id,"source_correct_reading_qualified":false,"application_promoted":false});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    ensure!(
        evaluated.failure_count() == 0,
        "Execution errors remain unqualified; full requests and partial native traces preserved"
    );
    Ok(result)
}

pub async fn optimize(
    cases: &[Case],
    method: &str,
    book: Arc<Book>,
    models: &Models,
    output: &Path,
    max_metrics: usize,
) -> Result<Value> {
    fs::create_dir_all(output)?;
    let mut program = Journey::new(
        method,
        cases,
        book.clone(),
        models.student.clone(),
        models.teacher.clone(),
    )?
    .with_stream(output.join("student-observations.jsonl"))?;
    program.save(&output.join("baseline-program.json"))?;
    let train = cases
        .iter()
        .filter(|c| c.split == "train")
        .map(journey::example)
        .collect::<Vec<_>>();
    let dev = cases
        .iter()
        .filter(|c| c.split == "development")
        .map(journey::example)
        .collect::<Vec<_>>();
    let test = cases
        .iter()
        .filter(|c| c.split == "test")
        .map(journey::example)
        .collect::<Vec<_>>();
    let before = Evaluate::new(dev.clone(), |input| program.forward(input), Graded::metric)
        .run()
        .await;
    program.save_observations(&output.join("baseline-observations.json"))?;
    fs::write(
        output.join("baseline.json"),
        serde_json::to_vec_pretty(
            &json!({"score":if before.failure_count()==0{Some(before.score)}else{None},"errors":evaluation_errors(&before)}),
        )?,
    )?;
    ensure!(
        before.failure_count() == 0,
        "Baseline execution/review errors withhold optimization"
    );
    let mut strong = Journey::new(
        method,
        cases,
        book,
        models.teacher.clone(),
        models.teacher.clone(),
    )?
    .with_stream(output.join("teacher-observations.jsonl"))?;
    let mut bootstrap = BootstrapFewShot::new(Graded::metric);
    bootstrap.max_bootstrapped_demos = 1;
    bootstrap.max_labeled_demos = 0;
    bootstrap.metric_threshold = Some(1.);
    bootstrap.max_errors = 1;
    let demos = bootstrap
        .compile_with_teacher(&mut program, &mut strong, &train)
        .await?;
    // Store the complete learned teacher trace before projecting demonstration inputs.
    program.save(&output.join("bootstrapped-full.json"))?;
    program.compact_demos()?;
    program.save(&output.join("bootstrapped.json"))?;
    let search = GEPA::new(Graded::feedback, models.teacher.clone())
        .with_max_metric_calls(max_metrics)
        .with_seed(23)
        .compile(&mut program, &train, &dev)
        .await?;
    program.save(&output.join("compiled.json"))?;
    program.save_observations(&output.join("search-observations.json"))?;
    let after = Evaluate::new(dev, |input| program.forward(input), Graded::metric)
        .run()
        .await;
    let held = Evaluate::new(test, |input| program.forward(input), Graded::metric)
        .run()
        .await;
    program.save_observations(&output.join("observations.json"))?;
    let result = json!({"method":method,"examples":cases.len(),"bootstrap_demos":demos,"before_development":before.score,
        "after_development":if after.failure_count()==0{Some(after.score)}else{None},"held_out":if held.failure_count()==0{Some(held.score)}else{None},
        "errors":evaluation_errors(&after).into_iter().chain(evaluation_errors(&held)).collect::<Vec<_>>(),"best_index":search.best_idx,
        "metric_calls":search.total_num_evals,"iterations":search.iterations,"teacher":models.teacher_id,"student":models.student_id,
        "scope":"complete-first-message DSPy module","application_promoted":false});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    ensure!(
        after.failure_count() == 0 && held.failure_count() == 0,
        "Errors remain unqualified; completed calls are retained"
    );
    // A compile artifact is a candidate; integration has its own controlled gate.
    Ok(result)
}

pub fn load_all(directory: &Path) -> Result<Vec<Case>> {
    catalogue::validate_all(directory).context("Complete catalogue validation")?;
    methods("all")?
        .into_iter()
        .map(|m| catalogue::cases(directory, m))
        .collect::<Result<Vec<_>>>()
        .map(|banks| banks.into_iter().flatten().collect())
}

/// A small round selects whole existing cases; it cannot change their split or labels.
pub fn select_cases(
    cases: &[Case],
    training: &[String],
    development: &[String],
    test: &[String],
) -> Result<Vec<Case>> {
    if training.is_empty() && development.is_empty() && test.is_empty() {
        return Ok(cases.to_vec());
    }
    let mut selected = BTreeSet::new();
    for (split, ids) in [
        ("train", training),
        ("development", development),
        ("test", test),
    ] {
        ensure!(
            !ids.is_empty(),
            "Every selected split needs at least one case"
        );
        for id in ids {
            ensure!(selected.insert(id), "Repeated selected case {id}");
            let case = cases
                .iter()
                .find(|case| &case.id == id)
                .with_context(|| format!("Unknown selected case {id}"))?;
            ensure!(
                case.split == split,
                "Selected case {id} belongs to {}, not {split}",
                case.split
            );
        }
    }
    Ok(cases
        .iter()
        .filter(|case| selected.contains(&case.id))
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_excludes_only_routing_sentinels() {
        let m = methods("all").unwrap();
        assert_eq!(m.len(), 42);
        assert!(m.contains(&"election"));
        assert!(methods("wish").is_err());
        assert!(methods("unclassified").is_err());
        assert!(methods("unknown").is_err());
    }

    #[test]
    fn bounded_selection_preserves_cases_and_never_moves_their_splits() {
        let cases = catalogue::cases(
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples")),
            "contact",
        )
        .unwrap();
        let train = vec![cases[0].id.clone()];
        let dev = vec![cases[12].id.clone()];
        let test = vec![cases[16].id.clone()];
        let selected = select_cases(&cases, &train, &dev, &test).unwrap();
        assert_eq!(
            serde_json::to_value(selected).unwrap(),
            serde_json::to_value(vec![&cases[0], &cases[12], &cases[16]]).unwrap()
        );
        assert!(select_cases(&cases, &dev, &train, &test).is_err());
        assert!(select_cases(&cases, &train, &[], &test).is_err());
        assert!(select_cases(&cases, &[train[0].clone(), train[0].clone()], &dev, &test).is_err());
        assert!(select_cases(&cases, &["unknown".into()], &dev, &test).is_err());
        assert_eq!(select_cases(&cases, &[], &[], &[]).unwrap().len(), 20);
    }
}
