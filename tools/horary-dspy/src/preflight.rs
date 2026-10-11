//! Read-only budget checks of the port's actual baseline text exchanges.
//! No student generation, teacher call, optimizer, or semantic score is made.
use crate::{book::Book, catalogue, functions, google, runner, signatures};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Example,
    adapter::{Adapter, ChatAdapter, Input},
    lm::api::{LmConfig, LmMessage, LmPart, LmRequest},
    signature::Signature,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::Write, path::Path, time::Duration};

fn request(signature: &Signature, example: &Example) -> Result<LmRequest> {
    let selected = example.inputs()?;
    let inputs = selected
        .fields()
        .map(|(name, value)| Input::new(name, value.clone()))
        .collect::<Vec<_>>();
    let (system, turns) = ChatAdapter::without_json_fallback().format(signature, &[], &inputs)?;
    let mut messages = vec![LmMessage::system(vec![LmPart::text(system)])];
    for turn in turns {
        messages.push(LmMessage::new(
            turn.role.as_str(),
            vec![LmPart::text(
                turn.content
                    .text()
                    .context("First-pass budget preflight is text only")?,
            )],
        ));
    }
    Ok(
        LmRequest::new(google::STUDENT, messages).configured(LmConfig {
            temperature: Some(0.),
            max_tokens: Some(4096),
            ..Default::default()
        }),
    )
}

/// Exact baseline exchanges for every declared fixture, not just one sample.
pub fn requests(directory: &Path, book: &Book) -> Result<Vec<(String, String, LmRequest)>> {
    let mut requests = Vec::new();
    for method in runner::methods("all")? {
        for case in catalogue::cases(directory, method)? {
            for function in ["classification", "extraction", "reading"] {
                let signature = match function {
                    "classification" => signatures::auxiliary(function)?,
                    "extraction" => signatures::extraction(method)?,
                    _ => signatures::reading(method)?,
                };
                let label = if function == "classification" {
                    function.into()
                } else {
                    format!("{method}-{function}")
                };
                let example = catalogue::example(&case, function, book)?;
                requests.push((label, case.id.clone(), request(&signature, &example)?));
            }
        }
    }
    for function in functions::AUXILIARY {
        let signature = signatures::auxiliary(function)?;
        for fixture in functions::fixtures(directory, function, book)? {
            let id = fixture
                .example
                .get("case_id")
                .and_then(Value::as_str)
                .context("No fixture ID")?;
            requests.push((
                (*function).into(),
                id.into(),
                request(&signature, &fixture.example)?,
            ));
        }
    }
    Ok(requests)
}

/// Inspect all exchanges locally and count either all or the largest rendered
/// exchange per function. Every generation still counts its own exact request.
/// A rate hint never excludes an exchange; only a listed context limit can.
pub async fn run(
    directory: &Path,
    book: &Book,
    http: &reqwest::Client,
    output: &Path,
    input_context_limit: usize,
    all_counts: bool,
) -> Result<Value> {
    ensure!(
        input_context_limit > 0,
        "The listed model context limit must be positive"
    );
    fs::create_dir(output)?;
    let exchanges = requests(directory, book)?;
    let total = exchanges.len();
    let mut inventory = BTreeMap::<String, Vec<usize>>::new();
    for (function, _, request) in &exchanges {
        inventory
            .entry(function.clone())
            .or_default()
            .push(serde_json::to_vec(request)?.len());
    }
    fs::write(
        output.join("inventory.json"),
        serde_json::to_vec_pretty(&json!({
            "all_formatted_exchanges":total,"schema_constrained":false,"bootstrap_demos":0,
            "functions":inventory.iter().map(|(function, sizes)|json!({"function":function,
                "cases":sizes.len(),"maximum_request_bytes":sizes.iter().max()})).collect::<Vec<_>>()
        }))?,
    )?;
    let selected = if all_counts {
        exchanges
    } else {
        let mut largest = BTreeMap::<String, (String, LmRequest, usize)>::new();
        for (function, id, request) in exchanges {
            let size = serde_json::to_vec(&request)?.len();
            if largest
                .get(&function)
                .is_none_or(|(_, _, previous)| size > *previous)
            {
                largest.insert(function, (id, request, size));
            }
        }
        largest
            .into_iter()
            .map(|(function, (id, request, _))| (function, id, request))
            .collect()
    };
    let mut record = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("counts.jsonl"))?;
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    let mut over = Vec::new();
    for (function, id, request) in selected {
        let count = google::count_tokens(http, &request, &output.join("requests")).await?;
        let tokens = count["input_tokens"].as_u64().context("No counted input")? as usize;
        let row = json!({"function":function,"case_id":id,"request_key":request.cache_key(google::STUDENT),
            "input_tokens":tokens,"within_listed_context_limit":tokens<=input_context_limit,"generation_submitted":false});
        writeln!(record, "{}", serde_json::to_string(&row)?)?;
        record.sync_data()?;
        if tokens > input_context_limit {
            over.push(row);
        }
        groups.entry(function).or_default().push(tokens);
        // Count requests have their own traffic; don't flood the project while
        // checking admission. No generated-token allowance is consumed here.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let result = json!({"scope":"Baseline declared-input function exchanges; not full-journey or semantic qualification",
        "book_sha256":crate::book::OCR_SHA256,"executable_sha256":google::digest(fs::read(std::env::current_exe()?)?),
        "listed_input_context_limit":input_context_limit,"verified_project_quota":false,
        "all_formatted_exchanges":total,"count_selection":if all_counts {"all"} else {"largest_rendered_exchange_per_function"},
        "sampled_counts_prove_all_context_admission":false,
        "student_generations":0,"teacher_calls":0,"bootstrap_demos":0,"oversized":over,
        "functions":groups.iter().map(|(name,counts)| json!({"function":name,"cases":counts.len(),
            "max_tokens":counts.iter().max(),"sum_tokens":counts.iter().sum::<usize>(),
            "mean_tokens":counts.iter().sum::<usize>() as f64/counts.len() as f64})).collect::<Vec<_>>(),
        "passed":over.is_empty()});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    ensure!(
        over.is_empty(),
        "Counted baseline prompts exceed the listed model context limit; no generation submitted. See preflight result"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exchange_uses_only_declared_fields_and_no_provider_schema() {
        let example = dsrust::example! {question:"A synthetic question",conversation:"[]",catalogue:"Synthetic routes",
            case_id:"private-id",expected:"PRIVATE GOLD",__review_source:"PRIVATE JUDGE"}.with_inputs(["question","conversation","catalogue"]);
        let request = request(&signatures::auxiliary("classification").unwrap(), &example).unwrap();
        assert!(request.output_schema().is_none());
        assert!(request.tools.is_empty());
        let rendered = serde_json::to_string(&request).unwrap();
        assert!(rendered.contains("A synthetic question"));
        for forbidden in ["private-id", "PRIVATE GOLD", "PRIVATE JUDGE"] {
            assert!(!rendered.contains(forbidden));
        }
        assert_eq!(request.model, google::STUDENT);
    }
}
