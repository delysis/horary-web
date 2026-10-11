use crate::{book::Book, signatures};
use anyhow::{Context, Result, ensure};
use dsrust::Example;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub role: String,
    pub content: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub observed_at: String,
    pub time_zone: String,
    pub place: Place,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub field: String,
    pub value: String,
    pub quote: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub facet: String,
    pub facts: Vec<Fact>,
    pub missing: Vec<String>,
    pub reading_status: String,
    pub must_explain: Vec<String>,
    pub must_not: Vec<String>,
    pub source_pages: Vec<usize>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    pub utc: String,
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub method: String,
    pub split: String,
    pub variant: String,
    pub question: String,
    pub history: Vec<Turn>,
    pub device: Device,
    pub expected: Expected,
    pub chart: Chart,
}

pub fn cases(directory: &Path, method: &str) -> Result<Vec<Case>> {
    let text = fs::read_to_string(directory.join(format!("{method}.jsonl")))?;
    let cases = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Case>(line).map_err(anyhow::Error::from))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        cases.len() == 20,
        "{method} needs exactly twenty examples, found {}",
        cases.len()
    );
    let mut ids = BTreeSet::new();
    let mut splits = BTreeMap::<String, usize>::new();
    let mut variants = BTreeMap::<String, usize>::new();
    for case in &cases {
        ensure!(
            case.method == method && ids.insert(&case.id),
            "Wrong method or duplicate example identity"
        );
        ensure!(
            matches!(case.split.as_str(), "train" | "development" | "test"),
            "Unknown split"
        );
        ensure!(
            matches!(
                case.expected.reading_status.as_str(),
                "answer" | "need_information" | "method_limit"
            ),
            "Unknown expected reading status"
        );
        ensure!(
            !case.expected.source_pages.is_empty() && !case.expected.must_explain.is_empty(),
            "A rubric needs primary sources and actual method obligations"
        );
        for fact in &case.expected.facts {
            ensure!(
                fact.quote.is_empty()
                    || case.question.contains(&fact.quote)
                    || case
                        .history
                        .iter()
                        .any(|t| t.role == "user" && t.content.contains(&fact.quote)),
                "{}: {} is not an exact source quote",
                case.id,
                fact.field
            );
        }
        ensure!(
            case.chart.latitude.is_finite()
                && case.chart.latitude.abs() < 90.
                && case.chart.longitude.is_finite()
                && case.chart.longitude.abs() <= 180.,
            "Invalid chart coordinates"
        );
        chrono::DateTime::parse_from_rfc3339(&case.chart.utc)?;
        chrono::DateTime::parse_from_rfc3339(&case.device.observed_at)?;
        *splits.entry(case.split.clone()).or_default() += 1;
        *variants.entry(case.variant.clone()).or_default() += 1;
    }
    ensure!(
        splits.get("train") == Some(&12)
            && splits.get("development") == Some(&4)
            && splits.get("test") == Some(&4),
        "{method} needs the frozen 12/4/4 split"
    );
    for variant in ["explicit", "implicit"] {
        ensure!(
            variants.get(variant).copied().unwrap_or(0) >= 8,
            "{method} lacks at least eight {variant} examples"
        );
    }
    ensure!(
        cases
            .iter()
            .all(|c| c.history.is_empty() && c.expected.missing.is_empty()),
        "{method}: the first-run bank must be complete in the first message"
    );
    Ok(cases)
}

pub fn requirements(method: &str) -> Result<String> {
    let value: Value = serde_json::from_str(include_str!(
        "../../../docs/llm-process/reading-catalogue.json"
    ))?;
    let card = value["contracts"]
        .as_array()
        .context("Missing generated contracts")?
        .iter()
        .find(|c| c["method"] == method)
        .context("Unknown method")?;
    let coverage = coverage_card(method)?;
    Ok(format!(
        "Method: {method}. Conditional input requirements: {}.\nCANONICAL FACT DEFINITIONS: subject.name is the actual thing/person asked ABOUT, never a generic label for the speaker. subject.owner_id is its title owner when ownership matters. deal_actor is the contracting person, deal_capacity is buy/sell/negotiate, deal_party is the other party, deal_beneficiary is the person whose pocket receives profit. principal_id is used ONLY when someone explicitly relays another person's own question; concerning someone else is not a relay. people.<id>.relationship/capacity retains the affected person's actual role. Do not invent alternate paths such as object.description to omit the named subject. Canonical field paths use snake_case.\nFIRST choose the actual requested branch/facet, then require ONLY its necessary inputs. Pure agreement asks whether the contracting parties join: a profit beneficiary is not required when no profit question is asked. Quality asks about condition and does not require a future completion aspect. Profit needs the affected pocket. No general request for all fields. Reader location and question instant already come from the device unless the person explicitly overrides them. event_place/event_time remain context. Friday is not a request to recast Friday. Software implementation status is not a methodological limit.\nSource-grounded judgment branches:\n{}",
        card["requirements"],
        serde_json::to_string(&coverage["judgment_subbranches"])?
    ))
}

pub fn chart_evidence(case: &Case) -> Result<Value> {
    let at = chrono::DateTime::parse_from_rfc3339(&case.chart.utc)?;
    let raw = horary_ai_core::astronomy::chart(
        at.timestamp_millis() as f64,
        case.chart.latitude,
        case.chart.longitude,
    )
    .map_err(anyhow::Error::msg)?;
    // Astronomy already applies the book's calculation rules. Present every
    // native fact once, in the same notation as an original-message journey;
    // repeating structured and prose versions adds tokens, not testimony.
    let native = crate::native_evidence::facts(Some(&raw));
    let compact = json!({"anchor":case.chart,"facts":compact_native_facts(&native),
        "moon":raw["derived"]["voidOfCourseMoon"],
        "search_limitations":raw["calculationNote"],
        "input_scope":"Independently supplied chart for a reading-function test, not an observed extraction journey. All native fact IDs and details are present; no omitted fact is inferred absent."});
    Ok(compact)
}

/// Component examples have declared inputs. The reading component receives
/// an independently authored canonical consultation, not a claimed model journey.
pub fn example(case: &Case, task: &str, book: &Book) -> Result<Example> {
    let source = book.student_context(
        case.expected.source_pages.iter().copied(),
        task == "reading",
    )?;
    let history = serde_json::to_string(&case.history)?;
    let fields: Vec<(String, Value)> = match task {
        "reading" => vec![
            ("question".into(), json!(case.question)),
            (
                "consultation".into(),
                json!(serde_json::to_string(
                    &json!({"facts":case.expected.facts,"unknown":case.expected.missing,"device":case.device})
                )?),
            ),
            (
                "chart_evidence".into(),
                json!(serde_json::to_string(&chart_evidence(case)?)?),
            ),
            ("source_text".into(), json!(source)),
        ],
        "extraction" => vec![
            ("question".into(), json!(case.question)),
            ("conversation".into(), json!(history)),
            (
                "context".into(),
                json!(serde_json::to_string(
                    &json!({"device":case.device,"accepted_facts":[],"inference_authority":"Device/context is available for labelled inference, never a user quotation."})
                )?),
            ),
            (
                "requirements".into(),
                json!(format!("{}\n{}", requirements(&case.method)?, source)),
            ),
        ],
        "classification" => vec![
            ("question".into(), json!(case.question)),
            ("conversation".into(), json!(history)),
            ("catalogue".into(), json!(method_index()?)),
        ],
        _ => anyhow::bail!("Unknown case task {task}"),
    };
    let inputs = fields
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<String>>();
    let mut example = Example::new(fields).with_inputs(inputs);
    example.set("case_id", json!(case.id));
    example.set(
        "expected",
        json!({"method":case.method,"rubric":case.expected}),
    );
    // Correct-family primary context belongs only to the independent grader.
    // The independent teacher sees the complete salient family context, even
    // when this component chose a wrong branch. This is a private label field;
    // it never expands the student's declared inputs or primes its answer.
    example.set(
        "__review_source",
        json!(method_source(book, &case.method, task == "reading")?),
    );
    Ok(example)
}

pub fn validate_all(directory: &Path) -> Result<Value> {
    let mut records = Vec::new();
    let mut identities = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    for method in signatures::METHODS
        .iter()
        .filter(|method| !matches!(**method, "wish" | "unclassified"))
    {
        let cases = cases(directory, method)?;
        for case in &cases {
            ensure!(
                identities.insert(case.id.clone()),
                "Duplicate corpus identity: {}",
                case.id
            );
            ensure!(
                inputs.insert((case.question.clone(), serde_json::to_string(&case.device)?)),
                "Duplicate first-message/device inputs: {}",
                case.id
            );
            ensure!(
                case.expected.reading_status == "answer",
                "{} is not a complete reading case; keep actual boundaries in the later bank",
                case.id
            );
            ensure!(
                case.device.place.latitude.is_finite()
                    && case.device.place.latitude.abs() < 90.
                    && case.device.place.longitude.is_finite()
                    && case.device.place.longitude.abs() <= 180.,
                "Invalid device place"
            );
        }
        records.push(json!({"method":method,"examples":cases.len(),"sha256":crate::google::digest(fs::read(directory.join(format!("{method}.jsonl")))?)}));
    }
    Ok(
        json!({"types":records.len(),"examples":records.len()*20,"split":{"train_per_type":12,"development_per_type":4,"test_per_type":4},"cases":records}),
    )
}

pub fn method_index() -> Result<String> {
    let value: Value = serde_json::from_str(include_str!(
        "../../../docs/llm-process/reading-catalogue.json"
    ))?;
    let rows = value["contracts"]
        .as_array()
        .context("Missing generated native contracts")?;
    let entries=rows.iter().map(|c|{let source=coverage_card(c["method"].as_str().context("No method label")?)?;Ok(json!({"method":c["method"],"title":c["title"],"facets":c["facets"],"roles":c["roles"],"branches":source["judgment_subbranches"].as_array().context("No branches")?.iter().map(|b|json!({"id":b["id"],"task":b["decisive_judgment"]})).collect::<Vec<_>>()}))}).collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_string(&entries)?)
}

/// Select primary context from the model's actual branch, never case gold.
/// The independent judge still receives the family's complete source context.
pub fn branch_source(book: &Book, method: &str, branch: &str, reading: bool) -> Result<String> {
    let card = coverage_card(method)?;
    let selected = card["judgment_subbranches"]
        .as_array()
        .context("No book branches")?
        .iter()
        .find(|b| b["id"] == branch)
        .context("Classifier did not select a recognized book branch")?;
    let mut pages = selected["printed_pages"]
        .as_array()
        .context("No branch pages")?
        .iter()
        .map(|p| p.as_u64().map(|n| n as usize).context("Invalid page"))
        .collect::<Result<BTreeSet<_>>>()?;
    // Common chapter allocations precede specialized branch examples.
    match method {
        "lost_object" | "lost_animal" | "missing_person" => pages.extend([137, 138]),
        "relationship" | "person_description" => pages.extend([192, 193]),
        "money" | "bet" => pages.extend([156, 158]),
        "property" | "rental" | "business_property" => pages.extend([167, 168]),
        "new_job" | "existing_job" | "return_to_job" | "job_offer" | "work_person" => {
            pages.extend([224, 225])
        }
        _ => {}
    }
    book.student_context(pages, reading)
}

pub fn compact_native_facts(facts: &[crate::native_evidence::Fact]) -> String {
    let mut text = String::from("ID\tKIND\tLABEL\tCALCULATED TESTIMONY\n");
    for f in facts {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            f.id, f.kind, f.label, f.detail
        ));
    }
    text
}

pub fn method_source(book: &Book, method: &str, reading: bool) -> Result<String> {
    let card = coverage_card(method)?;
    let mut pages = card["primary_context_printed_pages"]
        .as_array()
        .context("No full primary chapter context")?
        .iter()
        .map(|p| {
            p.as_u64()
                .map(|n| n as usize)
                .context("Invalid printed page")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    // Several topic recipes rely on another chapter (for example, a missing
    // person's safety). A family reviewer must see those primary pages even
    // when a student selected the wrong subbranch. This uses source taxonomy,
    // never an evaluation case's expected branch or answers.
    for branch in card["judgment_subbranches"]
        .as_array()
        .context("No declared judgment branches")?
    {
        for page in branch["printed_pages"]
            .as_array()
            .context("No declared branch pages")?
        {
            pages.insert(page.as_u64().context("Invalid branch page")? as usize);
        }
    }
    book.student_context(pages, reading)
}

pub fn coverage_card(method: &str) -> Result<Value> {
    let coverage: Value = serde_json::from_str(include_str!("../coverage.json"))?;
    coverage["methods"]
        .as_array()
        .context("No source coverage")?
        .iter()
        .find(|m| m["port_method"] == method)
        .cloned()
        .context("Method absent from primary source coverage")
}

#[cfg(test)]
mod source_context_tests {
    use super::*;

    #[test]
    fn component_chart_preserves_every_native_fact_without_duplicate_presentations() {
        let case: Case = serde_json::from_value(json!({
            "id":"synthetic-presentation", "method":"movable_deal", "split":"train",
            "variant":"explicit", "question":"Will my synthetic sale complete?", "history":[],
            "device":{"observed_at":"2031-04-17T11:30:00Z","time_zone":"America/New_York",
                "place":{"name":"Synthetic reader place","latitude":38.657,"longitude":-77.249}},
            "chart":{"utc":"2031-04-17T11:30:00Z","latitude":38.657,"longitude":-77.249},
            "expected":{"facet":"event","facts":[],"missing":[],"reading_status":"answer",
                "must_explain":["Synthetic test only"],"must_not":[],"source_pages":[7,8]}
        }))
        .unwrap();
        let raw = horary_ai_core::astronomy::chart(
            chrono::DateTime::parse_from_rfc3339(&case.chart.utc)
                .unwrap()
                .timestamp_millis() as f64,
            case.chart.latitude,
            case.chart.longitude,
        )
        .unwrap();
        let native = crate::native_evidence::facts(Some(&raw));
        let presented = chart_evidence(&case).unwrap();
        let rows = presented["facts"]
            .as_str()
            .unwrap()
            .lines()
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), native.len());
        for (row, fact) in rows.iter().zip(native) {
            assert_eq!(
                *row,
                format!(
                    "{}\t{}\t{}\t{}",
                    fact.id, fact.kind, fact.label, fact.detail
                )
            );
        }
        assert_eq!(presented["moon"], raw["derived"]["voidOfCourseMoon"]);
        assert_eq!(presented["search_limitations"], raw["calculationNote"]);
        assert!(presented.get("native_facts").is_none());
        assert!(presented.get("calculated_testimony").is_none());
        assert!(
            presented["input_scope"]
                .as_str()
                .unwrap()
                .contains("not an observed extraction journey")
        );
    }

    #[test]
    fn family_teacher_context_contains_every_declared_branch_page() {
        let book = Book::fixture();
        for method in crate::signatures::METHODS {
            let card = coverage_card(method).unwrap();
            let context = method_source(&book, method, true).unwrap();
            for branch in card["judgment_subbranches"].as_array().unwrap() {
                for page in branch["printed_pages"].as_array().unwrap() {
                    let page = page.as_u64().unwrap();
                    assert!(
                        context.contains(&format!("<Frawley printed_page=\"{page}\"")),
                        "{method}: teacher is missing declared source page {page}"
                    );
                }
            }
        }
    }
}
