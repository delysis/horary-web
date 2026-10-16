//! Declared component fixtures and function-local validation. These are not
//! substitutes for a journey that acquires its own consultation and chart.
use crate::{book::Book, catalogue::Fact, optimize, signatures};
use anyhow::{Context, Result, ensure};
use dsrust::{Example, example::Prediction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const AUXILIARY: &[&str] = &["place", "moment", "update"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub split: String,
    pub inputs: BTreeMap<String, String>,
    pub expected: Value,
}

/// Explicit split metadata never enters a predictor's declared inputs.
#[derive(Clone)]
pub struct Fixture {
    pub split: String,
    pub example: Example,
}

pub fn cases(directory: &Path, function: &str) -> Result<Vec<Case>> {
    ensure!(
        AUXILIARY.contains(&function),
        "Unknown auxiliary fixture bank"
    );
    let text = fs::read_to_string(
        directory
            .join("functions")
            .join(format!("{function}.jsonl")),
    )?;
    let cases = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
        .collect::<Result<Vec<Case>>>()?;
    validate(function, &cases)?;
    Ok(cases)
}

pub fn validate(function: &str, cases: &[Case]) -> Result<()> {
    ensure!(
        AUXILIARY.contains(&function),
        "Unknown auxiliary fixture bank"
    );
    ensure!(cases.len() == 20, "{function} needs twenty frozen fixtures");
    let signature = signatures::auxiliary(function)?;
    let inputs = signature
        .inputs
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let outputs = signature
        .outputs
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    let mut split_counts = BTreeMap::new();
    for case in cases {
        ensure!(
            !case.id.trim().is_empty() && ids.insert(&case.id),
            "Duplicate or empty auxiliary identity"
        );
        ensure!(
            case.inputs
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                == inputs,
            "{}: auxiliary inputs differ from the actual signature",
            case.id
        );
        ensure!(
            !case.inputs.values().all(|v| v.trim().is_empty()),
            "{}: empty fixture",
            case.id
        );
        ensure!(
            matches!(case.split.as_str(), "train" | "development" | "test"),
            "Unknown auxiliary split"
        );
        *split_counts.entry(case.split.as_str()).or_insert(0usize) += 1;
        let expected = case
            .expected
            .as_object()
            .context("Auxiliary rubric is not an object")?;
        for output in &outputs {
            ensure!(
                expected.get(*output).is_some_and(Value::is_string),
                "{}: rubric omits the {output} output",
                case.id
            );
        }
        let permitted = outputs
            .iter()
            .copied()
            .chain(["source_pages", "must_explain", "must_not"])
            .collect::<BTreeSet<_>>();
        ensure!(
            expected.keys().all(|key| permitted.contains(key.as_str())),
            "{}: unknown rubric fields",
            case.id
        );
        let pages = expected
            .get("source_pages")
            .and_then(Value::as_array)
            .context("Missing auxiliary source pages")?;
        ensure!(
            !pages.is_empty()
                && pages
                    .iter()
                    .all(|p| p.as_u64().is_some_and(|p| p > 0 && p <= 400)),
            "{}: invalid source pages",
            case.id
        );
        for key in ["must_explain", "must_not"] {
            let obligations = expected
                .get(key)
                .and_then(Value::as_array)
                .context("Missing auxiliary obligations")?;
            ensure!(
                !obligations.is_empty()
                    && obligations
                        .iter()
                        .all(|o| o.as_str().is_some_and(|s| !s.trim().is_empty())),
                "{}: empty auxiliary rubric",
                case.id
            );
        }
        // Check the authored structure, not its correctness by comparing it to
        // itself. Semantic dates, location choices and consent need the judge.
        let authored = Prediction::new(
            Example::new(
                outputs
                    .iter()
                    .map(|name| ((*name).to_owned(), expected[*name].clone())),
            ),
            "Authored fixture, not a model response",
        );
        let failures = native_failures(function, &example_inputs(case), &authored)?;
        ensure!(
            failures.is_empty(),
            "{}: invalid authored output: {failures:?}",
            case.id
        );
    }
    ensure!(
        split_counts == BTreeMap::from([("train", 12), ("development", 4), ("test", 4)]),
        "{function} needs the frozen 12/4/4 split"
    );
    Ok(())
}

fn example_inputs(case: &Case) -> Example {
    Example::new(
        case.inputs
            .iter()
            .map(|(name, value)| (name.clone(), json!(value))),
    )
    .with_inputs(case.inputs.keys().cloned())
}

pub fn fixtures(directory: &Path, function: &str, book: &Book) -> Result<Vec<Fixture>> {
    cases(directory, function)?
        .into_iter()
        .map(|case| {
            let pages = case.expected["source_pages"]
                .as_array()
                .context("No auxiliary source pages")?
                .iter()
                .map(|page| {
                    page.as_u64()
                        .context("Invalid printed page")
                        .map(|p| p as usize)
                })
                .collect::<Result<BTreeSet<_>>>()?;
            let mut example = example_inputs(&case);
            example.set("case_id", json!(case.id));
            example.set("expected", case.expected);
            example.set("__review_source", json!(book.pages(&pages)?));
            Ok(Fixture {
                split: case.split,
                example,
            })
        })
        .collect()
}

pub fn dimensions(function: &str) -> Result<&'static [&'static str]> {
    match function {
        "classification" => Ok(&["classification"]),
        "extraction" | "place" | "moment" | "update" => Ok(&["extraction"]),
        "reading" => Ok(&["reading", "answer_quality"]),
        "reception" => Ok(&["contextual_meaning"]),
        _ => anyhow::bail!("Unknown graded function {function}"),
    }
}

pub fn grading_instruction(function: &str) -> Result<String> {
    if function == "reception" {
        return Ok("Grade only the single supplied reception's qualified contextual meaning. Native identity/direction/quality checks cannot be compensated by your score. No classification, extraction, event, timing or whole-reading answer is observed or required. Cite actual output and printed pp. 44/71–73. Preserve missing or uncertain observations; never invent a semantic score for transport uncertainty.".into());
    }
    let applicable = dimensions(function)?.join(", ");
    Ok(format!(
        "This is ONLY the {function} function. Applicable dimensions: {applicable}. Mark all other dimensions unobserved. Grade only this declared-input function, not an absent conversation or downstream journey. `passed` and `score` summarize these applicable obligations only. Return observed output excerpts and printed source pages for each material finding. A reading component's accepted consultation and calculated chart are supplied independently: do not claim that classification or extraction ran. A coherent direct answer is part of a reading. A place/moment/update decision is input extraction and preservation, not a horoscope. Missing or contradictory information may correctly require clarification under its authored rubric."
    ))
}

fn input<'a>(example: &'a Example, name: &str) -> Result<&'a str> {
    example
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("No actual {name} input"))
}

/// Validate only facts actually returned by a model against its actual inputs.
/// No gold values or expected method are consulted here. Fully received fields
/// rejected by native validation are observed failures. Invalid actual inputs
/// remain authoring errors; provider/adapter uncertainty never enters here.
pub fn native_failures(
    function: &str,
    example: &Example,
    prediction: &Prediction,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    match function {
        "place" => {
            let envelope: Value = serde_json::from_str(input(example, "place_candidates")?)?;
            let candidates = envelope
                .as_array()
                .or_else(|| envelope.get("candidates").and_then(Value::as_array))
                .context(
                    "Actual place candidates are neither a list nor a native lookup envelope",
                )?;
            let ids = candidates
                .iter()
                .filter_map(|c| c["id"].as_str())
                .collect::<BTreeSet<_>>();
            let query = optimize::text(prediction, "geocode_query")?;
            for field in ["reader_place", "event_place"] {
                let value = optimize::text(prediction, field)?;
                // Unresolved named places must be explicitly pending a query.
                if !value.is_empty()
                    && !ids.contains(value)
                    && (query.trim().is_empty() || !value.contains("pending"))
                {
                    failures.push(format!(
                        "{field}: no supplied candidate ID or pending geocode query"
                    ));
                }
            }
            optimize::text(prediction, "clarification")?;
        }
        "moment" => {
            let chart = optimize::text(prediction, "chart_moment")?;
            let event = optimize::text(prediction, "event_moment")?;
            if !chart.is_empty() && chrono::DateTime::parse_from_rfc3339(chart).is_err() {
                failures.push(
                    "chart_moment: nonempty value is not an unambiguous RFC3339 instant".into(),
                );
            }
            if !event.is_empty()
                && chrono::DateTime::parse_from_rfc3339(event).is_err()
                && chrono::NaiveDate::parse_from_str(event, "%Y-%m-%d").is_err()
            {
                failures.push("event_moment: neither civil date nor unambiguous instant".into());
            }
            if !matches!(
                optimize::text(prediction, "occurrence")?,
                "" | "earlier" | "later"
            ) {
                failures.push("occurrence: unknown daylight-saving selection".into());
            }
            optimize::text(prediction, "clarification")?;
        }
        "update" => {
            let updates: Vec<Fact> =
                match serde_json::from_str(optimize::text(prediction, "updates")?) {
                    Ok(updates) => updates,
                    Err(error) => {
                        failures.push(format!("Native updates array rejected: {error}"));
                        Vec::new()
                    }
                };
            let retractions: Vec<String> =
                match serde_json::from_str(optimize::text(prediction, "retractions")?) {
                    Ok(retractions) => retractions,
                    Err(error) => {
                        failures.push(format!("Native retractions array rejected: {error}"));
                        Vec::new()
                    }
                };
            let words = input(example, "new_words")?;
            if !matches!(
                optimize::text(prediction, "intent")?,
                "clarify" | "correct" | "new_question" | "pause" | "resume" | "explain"
            ) {
                failures.push("intent: unknown consultation action".into());
            }
            let mut fields = BTreeSet::new();
            for fact in updates {
                if fact.field.trim().is_empty() || !fields.insert(fact.field.clone()) {
                    failures.push("updates: empty or duplicate canonical path".into());
                }
                if fact.quote.is_empty() || !words.contains(&fact.quote) {
                    failures.push(format!(
                        "{}: update lacks an exact new-words witness",
                        fact.field
                    ));
                }
            }
            let mut retracted = BTreeSet::new();
            for field in retractions {
                if field.trim().is_empty() || !retracted.insert(field) {
                    failures.push("retractions: empty or duplicate canonical path".into());
                }
            }
            optimize::text(prediction, "clarification")?;
        }
        _ => anyhow::bail!("No auxiliary native validator for {function}"),
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_banks_match_the_real_signatures_and_keep_labels_private() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        for function in AUXILIARY {
            let fixtures = fixtures(&directory, function, &Book::fixture()).unwrap();
            assert_eq!(fixtures.len(), 20);
            for fixture in fixtures {
                let input = fixture.example.inputs().unwrap();
                for name in ["expected", "case_id", "__review_source", "split"] {
                    assert!(input.get(name).is_none());
                }
                assert!(
                    fixture
                        .example
                        .get("__review_source")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .contains("printed_page=\"7\"")
                );
            }
        }
    }
    #[test]
    fn reading_requires_answer_but_never_the_absent_classification_stage() {
        assert_eq!(
            dimensions("reading").unwrap(),
            &["reading", "answer_quality"]
        );
        for function in ["extraction", "place", "moment", "update"] {
            assert_eq!(dimensions(function).unwrap(), &["extraction"]);
        }
        assert!(dimensions("unknown").is_err());
    }
    #[test]
    fn unknown_locations_and_forged_correction_spans_are_native_failures() {
        let place = dsrust::input! {place_candidates:"[]"};
        let output = Prediction::new(
            dsrust::example! {reader_place:"invented",event_place:"",geocode_query:"",clarification:""},
            "Scripted",
        );
        assert!(
            !native_failures("place", &place, &output)
                .unwrap()
                .is_empty()
        );
        let update = dsrust::input! {new_words:"Nora is my sister."};
        let output = Prediction::new(
            dsrust::example! {intent:"clarify",updates:"[{\"field\":\"person\",\"value\":\"Nora\",\"quote\":\"different words\"}]",retractions:"[]",clarification:""},
            "Scripted",
        );
        assert!(
            !native_failures("update", &update, &output)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn fully_received_malformed_correction_is_an_observed_native_rejection() {
        let output = Prediction::new(
            dsrust::example! {intent:"clarify",updates:"not JSON",retractions:"[]",clarification:""},
            "Scripted",
        );
        assert!(
            !native_failures("update", &dsrust::input! {new_words:"Nora."}, &output)
                .unwrap()
                .is_empty()
        );
    }
}
