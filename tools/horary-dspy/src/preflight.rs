//! Read-only budget checks of the port's actual baseline text exchanges.
//! No student generation, teacher call, optimizer, or semantic score is made.
use crate::{book::Book, catalogue, functions, google, reading, runner, signatures};
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
    request_with_demos(signature, &[], example)
}

fn request_with_demos(
    signature: &Signature,
    demos: &[Example],
    example: &Example,
) -> Result<LmRequest> {
    let selected = example.inputs()?;
    let inputs = selected
        .fields()
        .map(|(name, value)| Input::new(name, value.clone()))
        .collect::<Vec<_>>();
    let (system, turns) = ChatAdapter::without_json_fallback().format(signature, demos, &inputs)?;
    let mut messages = vec![LmMessage::system(vec![LmPart::text(system)])];
    for turn in turns {
        messages.push(LmMessage::new(
            turn.role.as_str(),
            vec![LmPart::text(
                turn.content
                    .text()
                    .context("Budget preflight is text only")?,
            )],
        ));
    }
    // Predict leaves the normalized model blank; the selected provider binds it.
    // The regression below captures Predict's actual request byte-for-byte.
    Ok(LmRequest::new("", messages).configured(LmConfig {
        temperature: Some(0.),
        max_tokens: Some(4096),
        ..Default::default()
    }))
}

/// Render the port's real direct-reader exchanges, including its accepted
/// demonstrations. No provider is called; tokenizer preflight stays separate.
pub fn reader_requests(
    fixtures: &[functions::Fixture],
    predictor: &dsrust::Predict,
) -> Result<Vec<Value>> {
    fixtures.iter().map(|fixture| {
        let mut request = request_with_demos(&predictor.signature, &predictor.demos, &fixture.example)?;
        let config = predictor.config();
        ensure!(config.rollout_id.is_none(), "Budget preflight does not infer a rollout cache configuration");
        request.config.temperature = config.temperature;
        request.config.max_tokens = config.max_tokens;
        request.config.n = config.completions;
        let generation_tokens = request.config.max_tokens.context("Missing explicit generation budget")?;
        Ok(json!({"case_id":fixture.example.get("case_id"),"split":fixture.split,
            "request_sha256":google::digest(serde_json::to_vec(&request)?),"request":request,
            "messages":request.wire_messages(),"generation_tokens":generation_tokens,"generation_submitted":false}))
    }).collect()
}

/// Exact baseline exchanges for every declared fixture. A standalone reader
/// has an unobserved branch, so enumerate its possible requests instead of
/// supplying an expected branch. These are budget alternatives, not executions.
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
                if function == "reading" {
                    // Standalone banks deliberately have no gold-derived
                    // branch annotation. Count the selector and every legal
                    // reader alternative without pretending any was observed.
                    requests.push((
                        label.clone(),
                        format!("{}:selector", case.id),
                        request(
                            &reading::selector_signature(),
                            &reading::selector_input(&example, method)?,
                        )?,
                    ));
                    for branch in catalogue::coverage_card(method)?["judgment_subbranches"]
                        .as_array()
                        .context("No reading branches")?
                    {
                        let branch = branch["id"].as_str().context("No branch ID")?;
                        requests.push((
                            label.clone(),
                            format!("{}:reader-alternative:{branch}", case.id),
                            request(
                                &signature,
                                &reading::reader_input(&example, method, branch, book)?,
                            )?,
                        ));
                    }
                } else {
                    requests.push((label, case.id.clone(), request(&signature, &example)?));
                }
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
            "all_formatted_exchanges":total,"schema_constrained":false,"bootstrap_demos":0,"reading_branch_alternatives_are_not_observed_selections":true,
            "functions":inventory.iter().map(|(function, sizes)|json!({"function":function,
                "formatted_exchanges_or_alternatives":sizes.len(),"maximum_request_bytes":sizes.iter().max()})).collect::<Vec<_>>()
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
mod wire_tests {
    use super::*;
    use dsrust::{
        Module,
        lm::{
            Capabilities, ChatModel,
            api::{LmOutput, LmResponse},
        },
    };
    use std::sync::{Arc, Mutex};

    struct Captured(Arc<Mutex<Option<LmRequest>>>);
    impl ChatModel for Captured {
        async fn forward(
            &self,
            _http: &reqwest::Client,
            request: &LmRequest,
        ) -> Result<LmResponse> {
            *self.0.lock().unwrap() = Some(request.clone());
            Ok(LmResponse {
                outputs: vec![LmOutput::text(
                    "[[ ## status ## ]]\nmethod_limit\n[[ ## answer ## ]]\nSynthetic received answer.\n[[ ## analysis ## ]]\nSynthetic mechanics only.\n[[ ## evidence ## ]]\ne0\n[[ ## unresolved ## ]]\nUnknown computation.\n[[ ## completed ## ]]",
                )],
                ..Default::default()
            })
        }
        async fn capabilities(&self, _http: &reqwest::Client) -> Capabilities {
            Capabilities::default()
        }
    }

    #[tokio::test]
    async fn reader_preflight_matches_actual_predict_request_with_saved_demo() {
        let captured = Arc::new(Mutex::new(None));
        let mut predictor = crate::optimize::plain(
            signatures::reading("contact").unwrap(),
            Arc::new(Captured(captured.clone())),
        );
        predictor.demos.push(dsrust::example! {
            question:"A worked example question.", consultation:"Accepted demonstration context.",
            chart_evidence:"Demonstration native evidence.", status:"answer", answer:"Worked answer.",
            analysis:"Worked source reasoning.", evidence:"e0", unresolved:""
        }.with_inputs(["question","consultation","chart_evidence"]));
        let input = dsrust::example! {
            source_text:"Constant salient primary source.", question:"Actual live question.",
            consultation:"Actual accepted context.", chart_evidence:"Actual complete native facts."
        }
        .with_inputs(["source_text", "question", "consultation", "chart_evidence"]);
        let expected = request_with_demos(&predictor.signature, &predictor.demos, &input).unwrap();
        predictor.forward(input).await.unwrap();
        let actual = captured.lock().unwrap().clone().unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert!(
            actual.model.is_empty() && actual.output_schema().is_none() && actual.tools.is_empty()
        );
        let live = actual
            .messages
            .last()
            .unwrap()
            .parts
            .iter()
            .find_map(|part| match part {
                dsrust::lm::api::LmPart::Text { text, .. } => Some(text),
                _ => None,
            })
            .unwrap();
        assert!(live.find("source_text").unwrap() < live.find("question").unwrap());
    }

    #[tokio::test]
    async fn reception_preflight_matches_actual_short_predict_request() {
        let captured = Arc::new(Mutex::new(None));
        let predictor = crate::optimize::plain(
            crate::reception::signature(),
            Arc::new(Captured(captured.clone())),
        )
        .with_config(dsrust::lm::LmConfig {
            temperature: Some(0.),
            max_tokens: Some(1024),
            ..Default::default()
        });
        let input = dsrust::input! {source_text:"Constant primary pages",question:"Actual enquiry",
        accepted_roles:"Actual supplied actors",native_edge:"Actual native directed edge"};
        let fixtures = [functions::Fixture {
            split: "train".into(),
            example: input.clone(),
        }];
        let expected = reader_requests(&fixtures, &predictor).unwrap();
        // Captured emits the old reader fields; only the actual request is being
        // checked here, not a fabricated reception or source-correct judgment.
        assert!(predictor.forward(input).await.is_err());
        let actual = captured.lock().unwrap().clone().unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            expected[0]["request"]
        );
        assert_eq!(expected[0]["generation_tokens"], 1024);
        assert!(
            actual.model.is_empty() && actual.output_schema().is_none() && actual.tools.is_empty()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_source_is_a_stable_prefix_before_changing_live_facts() {
        let signature = signatures::reading("contact").unwrap();
        let exchange = |question: &str, consultation: &str, evidence: &str| {
            let example =
                dsrust::example! {source_text:"EXACT CONSTANT BOOK TEXT",question:question,
                consultation:consultation,chart_evidence:evidence}
                .with_inputs(["source_text", "question", "consultation", "chart_evidence"]);
            request(&signature, &example).unwrap().wire_messages()
        };
        let first = exchange("FIRST QUESTION", "FIRST CONTEXT", "FIRST NATIVE FACTS");
        let second = exchange("SECOND QUESTION", "SECOND CONTEXT", "SECOND NATIVE FACTS");
        assert_eq!(first[0], second[0]);
        let first = first.last().unwrap()["content"].as_str().unwrap();
        let second = second.last().unwrap()["content"].as_str().unwrap();
        let boundary = first.find("[[ ## question ## ]]").unwrap();
        assert_eq!(&first[..boundary], &second[..boundary]);
        assert!(first[..boundary].contains("EXACT CONSTANT BOOK TEXT"));
        for (text, fields) in [
            (
                first,
                ["FIRST QUESTION", "FIRST CONTEXT", "FIRST NATIVE FACTS"],
            ),
            (
                second,
                ["SECOND QUESTION", "SECOND CONTEXT", "SECOND NATIVE FACTS"],
            ),
        ] {
            for field in fields {
                assert!(text[boundary..].contains(field));
            }
        }
    }
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
        assert!(
            request.model.is_empty(),
            "Predict binds the model in the provider, not the normalized request"
        );
    }
}
