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
    chart_evidence_for_consultation(case, None)
}

fn chart_evidence_for_consultation(case: &Case, consultation: Option<&Value>) -> Result<Value> {
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
    let mut compact = json!({"anchor":case.chart,"facts":compact_native_facts(&native),
        "moon":raw["derived"]["voidOfCourseMoon"],
        "search_limitations":raw["calculationNote"],
        "input_scope":"Independently supplied chart for a reading-function test, not an observed extraction journey. All native fact IDs and details are present; no omitted fact is inferred absent."});
    if case.method == "contact"
        && let Some(consultation) = consultation
    {
        compact["role_reference"] = component_contact_role_reference(consultation, &native)?;
    }
    Ok(compact)
}

fn component_contact_role_reference(
    consultation: &Value,
    native: &[crate::native_evidence::Fact],
) -> Result<Value> {
    // The helper consumes only declared reader inputs after assembly, not the
    // case's private expected method, branch, roles or answer rubric.
    let mut projection = consultation.clone();
    projection["source_identity"] = json!(crate::google::digest(serde_json::to_vec(consultation)?));
    projection["source_scope"] = json!("declared_component_consultation_not_an_extraction_journey");
    Ok(crate::native_evidence::contact_role_reference(
        &projection,
        native,
        None,
    ))
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
        "reading" => {
            let consultation = json!({"facts":case.expected.facts,"unknown":case.expected.missing,"device":case.device});
            vec![
                ("question".into(), json!(case.question)),
                (
                    "consultation".into(),
                    json!(serde_json::to_string(&consultation)?),
                ),
                (
                    "chart_evidence".into(),
                    json!(serde_json::to_string(&chart_evidence_for_consultation(
                        case,
                        Some(&consultation)
                    )?)?),
                ),
                ("source_text".into(), json!(source)),
            ]
        }
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

/// The classifier and its validator use the same authored native facet enum.
/// A judgment branch names a procedure; it is not a question facet.
pub fn method_facets(method: &str) -> Result<Vec<String>> {
    let value: Value = serde_json::from_str(include_str!(
        "../../../docs/llm-process/reading-catalogue.json"
    ))?;
    let card = value["contracts"]
        .as_array()
        .context("Missing generated native contracts")?
        .iter()
        .find(|card| card["method"] == method)
        .context("Method absent from native contracts")?;
    card["facets"]
        .as_array()
        .context("Missing native method facets")?
        .iter()
        .map(|facet| {
            facet
                .as_str()
                .map(str::to_owned)
                .context("Invalid native method facet")
        })
        .collect()
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
    pages.extend(common_branch_pages(method));
    book.student_context(pages, reading)
}

fn common_branch_pages(method: &str) -> &'static [usize] {
    match method {
        "lost_object" | "lost_animal" | "missing_person" => &[137, 138],
        "relationship" | "person_description" => &[192, 193],
        "money" | "bet" => &[156, 158],
        "property" | "rental" | "business_property" => &[167, 168],
        "new_job" | "existing_job" | "return_to_job" | "job_offer" | "work_person" => &[224, 225],
        _ => &[],
    }
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

/// A source-reviewed Contact procedure, not a replacement for the full primary
/// source retained by reference authors and independent reviewers.
pub fn concise_reader_source(method: &str) -> Result<&'static str> {
    ensure!(
        method == "contact",
        "No reviewed concise procedure for {method}"
    );
    Ok(CONCISE_CONTACT_PROCEDURE)
}

const CONCISE_CONTACT_PROCEDURE: &str = "CONTACT PROCEDURE — Frawley printed pp. 19–32, 44, 57, 65, 71–77, 84–97, 127–136, 165–166.
Preserve the original contact question, principal and stated circumstances. Use the supplied role table with its authority and unknowns. A significator's assigned house differs from its body's occupied house. Retain every alias when rulers coincide. The Moon normally cosignifies the querent unless claimed as the quesited's main ruler. Turning counts the person's house as 1.

For a call or personal contact, connect the actual person's ruler with Lord 1 or the available contextual Moon. Mum is 10; an unspecified other is 7, but a declared relationship or operative capacity takes its appropriate house. The sender's turned third is an optional message route, not a mandatory substitute for the person. A parcel asks about the object and its arrival instead. Application direction does not say who calls or travels. WILL with a deadline asks occurrence within that bound, not automatically WHEN.

Separate capacity, inclination and occasion. Weigh supplied essential condition, occupied-house capacity, solar condition and actual motion; retrograde may fit returning rather than automatically forbidding it. Reception A→B means A regards B. Domicile/exaltation are major; triplicity moderate; term/face minor. Preserve all positive AND negative qualities, including mixed reception. Do not infer reciprocity or treat chart inclination as independently reported private feelings.

Use actual candidate order and each body's motion, not endpoint 1 or an assumed average speed. Examine main connections and supported translation/collection or prevention. An incidental contact is not automatically prohibition or an identified intermediary. Usually consider the next aspect, or next two for translation; do not chase a long chain to obtain the desired answer. Ease of aspect does not itself make the result good. A current void Moon supports the actual default, but stronger main testimony can override it.

After-ingress candidates require a meaningful change of circumstance/reception; do not carry current reception unchanged. Frawley limits this to roughly three or four degrees into the next sign. For an explicitly bounded question, the relevant planet's present-sign end can mark the stated timeframe (p. 136). Astronomical search hours are never automatically civil timing. To time an established event, use the applying body's travel to moving perfection, then contextual or calibrated units. Missing candidates or uncomputed techniques remain unknown, not proof of absence.

Return only status, answer, analysis, evidence, unresolved. Give the direct useful judgment first, then concise material reasons with native eIDs and source pages. Retain substantial contrary evidence and calculation limits; do not invent an exact date, motive or future event.
";

/// Present an existing declared component input with the same finite native
/// evidence, once. Private references, expected labels and verdicts are not read.
/// This is a new student presentation; its full original remains judge evidence.
pub fn concise_reader_inputs(original: &Example, method: &str) -> Result<Example> {
    let question = original
        .get("question")
        .and_then(Value::as_str)
        .context("No original reading question")?;
    let consultation_text = original
        .get("consultation")
        .and_then(Value::as_str)
        .context("No declared consultation")?;
    let consultation: Value = serde_json::from_str(consultation_text)?;
    let chart: Value = serde_json::from_str(
        original
            .get("chart_evidence")
            .and_then(Value::as_str)
            .context("No original native chart")?,
    )?;
    let compact = concise_reader_chart(&chart, &consultation, method)?;
    Ok(
        dsrust::input! {source_text:concise_reader_source(method)?,question:question,
        consultation:consultation_text,chart_evidence:serde_json::to_string(&compact)?},
    )
}

/// The existing calculator is used only to recover its typed native rows. Every
/// original eID/kind/label/detail must match before presentation can change.
pub fn concise_reader_chart(original: &Value, consultation: &Value, method: &str) -> Result<Value> {
    Ok(verified_concise_chart(original, consultation, method)?.0)
}

fn verified_concise_chart(
    original: &Value,
    consultation: &Value,
    method: &str,
) -> Result<(Value, Vec<crate::native_evidence::Fact>)> {
    concise_reader_source(method)?;
    let anchor = &original["anchor"];
    let at = chrono::DateTime::parse_from_rfc3339(
        anchor["utc"]
            .as_str()
            .context("No declared chart instant")?,
    )?;
    let latitude = anchor["latitude"]
        .as_f64()
        .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v))
        .context("Invalid declared latitude")?;
    let longitude = anchor["longitude"]
        .as_f64()
        .filter(|v| v.is_finite() && (-180.0..=180.0).contains(v))
        .context("Invalid declared longitude")?;
    let raw = horary_ai_core::astronomy::chart(at.timestamp_millis() as f64, latitude, longitude)
        .map_err(anyhow::Error::msg)?;
    let native = crate::native_evidence::facts(Some(&raw));
    ensure!(
        original["facts"].as_str() == Some(compact_native_facts(&native).as_str()),
        "Declared native fact rows differ; do not rebase a reference input silently"
    );
    ensure!(
        original["moon"] == raw["derived"]["voidOfCourseMoon"]
            && original["search_limitations"] == raw["calculationNote"],
        "Declared native boundaries differ"
    );
    let reference = component_contact_role_reference(consultation, &native)?;
    let supplied = original.get("role_reference");
    let mut motion = BTreeMap::new();
    if let Some(supplied) = supplied {
        for field in [
            "version",
            "source_identity",
            "source_scope",
            "principal_basis",
            "role_rows",
            "aliases",
            "alias_authority",
            "moon",
            "unknowns",
            "directed_reception",
        ] {
            ensure!(
                supplied.get(field) == reference.get(field),
                "Declared role reference differs at {field}"
            );
        }
        for row in supplied["event_endpoints"]
            .as_array()
            .context("No supplied event rows")?
        {
            let id = row["fact_id"].as_str().context("No supplied event ID")?;
            let fact = native
                .iter()
                .find(|f| f.id == id)
                .context("Foreign supplied event ID")?;
            let event = fact
                .event
                .as_ref()
                .context("Supplied event ID is not an event")?;
            let expected = reference["event_endpoints"]
                .as_array()
                .context("No native endpoint rows")?
                .iter()
                .find(|r| r["fact_id"] == id)
                .context("Foreign supplied event aliases")?;
            ensure!(
                row["endpoints"] == expected["endpoints"],
                "Supplied endpoint aliases differ: {id}"
            );
            ensure!(
                row["candidate"]["withinCurrentSigns"] == json!(event.within_current_signs)
                    && row["candidate"]["astronomicalHours"] == json!(event.astronomical_hours),
                "Supplied event data differ: {id}"
            );
            let value = row["candidate"]
                .get("perfectionMotion")
                .filter(|v| !v.is_null());
            if let Some(value) = value {
                ensure!(
                    value == &json!(event.perfection_motion),
                    "Supplied motion differs: {id}"
                );
            }
            ensure!(
                motion.insert(id.to_owned(), value.cloned()).is_none(),
                "Duplicate supplied event: {id}"
            );
        }
    }
    let table = concise_native_table(original, &raw, &native, &reference, &motion)?;
    Ok((table, native))
}

/// A source-scoped Contact presentation. Selection reads only the declared
/// consultation and verified native facts, never a case label or answer. Full
/// canonical inputs remain the consumer's private reviewer/journal evidence.
pub fn relevant_reader_inputs(original: &Example, method: &str) -> Result<Example> {
    let question = original
        .get("question")
        .and_then(Value::as_str)
        .context("No original question")?;
    let consultation: Value = serde_json::from_str(
        original
            .get("consultation")
            .and_then(Value::as_str)
            .context("No canonical consultation")?,
    )?;
    let chart: Value = serde_json::from_str(
        original
            .get("chart_evidence")
            .and_then(Value::as_str)
            .context("No canonical native chart")?,
    )?;
    Ok(
        dsrust::input! {question:question,consultation:relevant_reader_consultation(&consultation)?,
        chart_evidence:relevant_reader_chart(&chart,&consultation,method,None)?,source_text:concise_reader_source(method)?},
    )
}

/// Keep actual situational values, including conflicting accounts and genuine
/// unknowns. Quotation/provenance and coordinates are private once this Contact
/// chart has supplied its houses; they are not additional neural input facts.
pub fn relevant_reader_consultation(consultation: &Value) -> Result<String> {
    let facts = consultation["facts"]
        .as_array()
        .context("No declared fact rows")?;
    let mut lines = Vec::new();
    for key in ["method", "facet", "branch"] {
        if let Some(value) = consultation.get(key).filter(|v| !v.is_null()) {
            lines.push(format!(
                "{key}={}",
                plain_relevant(value.as_str().context("Invalid declared context")?)
            ));
        }
    }
    for fact in facts {
        let field = fact["field"]
            .as_str()
            .context("Invalid declared fact field")?;
        let value = fact["value"]
            .as_str()
            .context("Invalid declared fact value")?;
        if field != "reader_place" {
            lines.push(format!(
                "{}={}",
                plain_relevant(field),
                plain_relevant(value)
            ));
        }
    }
    if let Some(device) = consultation.get("device") {
        for key in ["observed_at", "time_zone"] {
            if let Some(value) = device[key].as_str() {
                lines.push(format!("device_{key}={}", plain_relevant(value)));
            }
        }
    }
    if let Some(unknowns) = consultation["unknown"].as_array() {
        for unknown in unknowns {
            lines.push(format!(
                "UNKNOWN {}",
                plain_relevant(unknown.as_str().context("Invalid declared unknown")?)
            ));
        }
    }
    Ok(lines.join("\n"))
}

/// Plain finite evidence. Explicit extra IDs are for cited demonstration rows
/// only; live input construction always passes None. The canonical input is
/// verified before either form is selected and never rewritten.
pub fn relevant_reader_chart(
    original: &Value,
    consultation: &Value,
    method: &str,
    extra_demo_ids: Option<&BTreeSet<String>>,
) -> Result<String> {
    ensure!(
        method == "contact",
        "No reviewed relevant procedure for {method}"
    );
    let (table, native) = verified_concise_chart(original, consultation, method)?;
    let selected = relevant_contact_ids(&table, &native, extra_demo_ids)?;
    let mut text = format!(
        "Chart UTC {}\n",
        plain_relevant(
            original["anchor"]["utc"]
                .as_str()
                .context("No chart instant")?
        )
    );
    for role in table["role_reference"]["roles"]
        .as_array()
        .context("No native role references")?
    {
        let house = role[3]
            .as_u64()
            .map_or_else(|| "—".into(), |h| format!("H{h}"));
        let planet = role[4].as_str().unwrap_or("?");
        text.push_str(&format!(
            "{}({}) = {house}/{}{}\n",
            plain_relevant(role[0].as_str().context("Invalid role ID")?),
            plain_relevant(role[1].as_str().unwrap_or("?")),
            planet,
            if role[5] == true { " optional" } else { "" }
        ));
    }
    let moon = &table["role_reference"]["moon"];
    text.push_str(&format!(
        "Moon contextual={}; house claims={}{}\n",
        moon["available_as_contextual_reference"]
            .as_bool()
            .map_or("?", |v| if v { "yes" } else { "no" }),
        relevant_role_names(&moon["house_claims"]),
        if moon["conditional_house_claims_if_optional_role_used"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
        {
            format!(
                "; optional claims={}",
                moon["conditional_house_claims_if_optional_role_used"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            String::new()
        }
    ));
    for unknown in table["role_reference"]["unknowns"]
        .as_array()
        .into_iter()
        .flatten()
    {
        text.push_str(&format!(
            "UNKNOWN {}\n",
            plain_relevant(unknown.as_str().context("Invalid role unknown")?)
        ));
    }
    text.push_str("<facts>\n");
    for fact in native
        .iter()
        .filter(|f| selected.contains(&f.id) && f.kind != "event")
    {
        let notation =
            if let Some(edge) = fact.reception.as_ref().filter(|_| fact.kind == "reception") {
                let qualities = |polarity| {
                    edge.qualities
                        .iter()
                        .filter(|q| q.polarity == polarity)
                        .map(|q| q.dignity.native_name())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                format!(
                    "{}→{} +[{}] -[{}]",
                    edge.guest_planet,
                    edge.host_planet,
                    qualities(crate::native_evidence::ReceptionPolarity::Positive),
                    qualities(crate::native_evidence::ReceptionPolarity::Negative)
                )
            } else {
                fact.detail.clone()
            };
        text.push_str(&format!(
            "{}|{}|{}\n",
            fact.id,
            fact.kind,
            plain_relevant(&notation)
        ));
    }
    let mut events: Vec<_> = native
        .iter()
        .filter(|f| selected.contains(&f.id) && f.kind == "event")
        .collect();
    events.sort_by(|a, b| {
        a.event
            .as_ref()
            .unwrap()
            .astronomical_hours
            .total_cmp(&b.event.as_ref().unwrap().astronomical_hours)
    });
    for fact in events {
        let row = table["events"]["rows"]
            .as_array()
            .context("No native events")?
            .iter()
            .find(|row| row[0] == fact.id)
            .context("No selected typed event row")?;
        let signs = match row[4].as_bool() {
            Some(true) => "currentSigns",
            Some(false) => "afterIngress",
            None => "ingress?",
        };
        let gap = row[6].as_str().map_or_else(
            || row[6]["native_detail"].as_str().unwrap_or("?").to_owned(),
            str::to_owned,
        );
        text.push_str(&format!(
            "{}|event|{}~{} {} t≈{}astro-h {signs} gap={}; {}\n",
            fact.id,
            row[1].as_str().context("No event endpoint")?,
            row[2].as_str().context("No event endpoint")?,
            row[3].as_str().context("No aspect")?,
            relevant_number(&row[5]),
            plain_relevant(&gap),
            relevant_motion(&row[7])?
        ));
    }
    text.push_str("</facts>\nLegend: H assigns a role, not the body's occupied house. → is regard, preserving every +/− quality; ~ endpoints are storage order, not the applicant or caller. Events are in computed astronomical order; fine prevention order remains approximate. Motion endpoints/travel use hourly-bracket interpolation, rounded to 3 decimals (error ≤0.0005 in stated units); rounded zero is not stationarity. Astronomical hours are not earthly time. Only relevant source-selected facts are shown; omitted evidence or uncomputed techniques are unknown, not proof of absence.\n");
    ensure!(
        relevant_chart_ids(&text)? == selected,
        "Relevant text lost its native ID domain"
    );
    Ok(text)
}

/// Exact provided-ID domain: no IDs are scraped from model prose or legends.
pub fn relevant_chart_ids(text: &str) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut open = false;
    let mut seen = false;
    let mut closed = false;
    for line in text.lines() {
        match line {
            "<facts>" => {
                ensure!(!seen, "Duplicate facts block");
                open = true;
                seen = true;
            }
            "</facts>" => {
                ensure!(open, "Unmatched facts closing marker");
                open = false;
                closed = true;
            }
            _ if open => {
                let mut columns = line.splitn(3, '|');
                let id = columns.next().context("Missing fact ID")?;
                let number = id
                    .strip_prefix('e')
                    .filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                    .context("Malformed native fact ID")?;
                let number: u64 = number.parse().context("Malformed native fact number")?;
                ensure!(format!("e{number}") == id, "Noncanonical native fact ID");
                ensure!(
                    matches!(
                        columns.next(),
                        Some(
                            "position"
                                | "house"
                                | "condition"
                                | "reception"
                                | "event"
                                | "moon"
                                | "boundary"
                        )
                    ),
                    "Unknown native fact kind"
                );
                ensure!(
                    columns.next().is_some_and(|c| !c.trim().is_empty()),
                    "Missing native fact notation"
                );
                ensure!(ids.insert(id.to_owned()), "Duplicate native fact ID");
            }
            _ => {}
        }
    }
    ensure!(
        seen && closed && !open && !ids.is_empty(),
        "Missing or incomplete finite native facts block"
    );
    Ok(ids)
}

fn relevant_contact_ids(
    table: &Value,
    native: &[crate::native_evidence::Fact],
    extra_demo_ids: Option<&BTreeSet<String>>,
) -> Result<BTreeSet<String>> {
    let roles = table["role_reference"]["roles"]
        .as_array()
        .context("No role rows")?;
    let mut recipients = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut planets = BTreeSet::from(["Moon".to_owned()]);
    let mut ids = BTreeSet::new();
    for row in roles {
        if let Some(planet) = row[4].as_str() {
            planets.insert(planet.to_owned());
            match row[0].as_str() {
                Some("principal.person" | "principal.moon") => {
                    recipients.insert(planet.to_owned());
                }
                Some("target.person" | "target.message") => {
                    targets.insert(planet.to_owned());
                }
                _ => {}
            }
        }
        if let Some(id) = row[8].as_str() {
            ids.insert(id.to_owned());
        }
    }
    let mut events: Vec<_> = native
        .iter()
        .filter(|f| f.kind == "event" && f.event.is_some() && f.planets.len() == 2)
        .collect();
    events.sort_by(|a, b| {
        a.event
            .as_ref()
            .unwrap()
            .astronomical_hours
            .total_cmp(&b.event.as_ref().unwrap().astronomical_hours)
    });
    let lunar: Vec<_> = events
        .iter()
        .copied()
        .filter(|f| f.planets.iter().any(|p| p == "Moon"))
        .take(2)
        .collect();
    let mut pairs = BTreeSet::new();
    let direct: Vec<_> = events
        .iter()
        .copied()
        .filter(|f| {
            !f.planets.iter().any(|p| p == "Moon")
                && ((recipients.contains(&f.planets[0]) && targets.contains(&f.planets[1]))
                    || (recipients.contains(&f.planets[1]) && targets.contains(&f.planets[0])))
        })
        .filter(|f| {
            let mut pair = f.planets.clone();
            pair.sort();
            pairs.insert(pair)
        })
        .collect();
    let mut horizons = BTreeMap::<String, f64>::new();
    for event in lunar.iter().chain(&direct) {
        ids.insert(event.id.clone());
        let hours = event.event.as_ref().unwrap().astronomical_hours;
        for planet in event.planets.iter().filter(|p| p.as_str() != "Moon") {
            horizons
                .entry(planet.clone())
                .and_modify(|t| *t = t.max(hours))
                .or_insert(hours);
        }
    }
    for planet in recipients
        .iter()
        .chain(&targets)
        .filter(|p| p.as_str() != "Moon")
    {
        if !horizons.contains_key(planet)
            && let Some(second) = events
                .iter()
                .copied()
                .filter(|f| !f.planets.iter().any(|p| p == "Moon") && f.planets.contains(planet))
                .take(2)
                .last()
        {
            horizons.insert(
                planet.clone(),
                second.event.as_ref().unwrap().astronomical_hours,
            );
        }
    }
    for event in events
        .iter()
        .filter(|f| !f.planets.iter().any(|p| p == "Moon"))
    {
        if event.planets.iter().any(|p| {
            horizons
                .get(p)
                .is_some_and(|until| event.event.as_ref().unwrap().astronomical_hours <= *until)
        }) {
            ids.insert(event.id.clone());
        }
    }
    if let Some(extra) = extra_demo_ids {
        for id in extra {
            ensure!(
                native.iter().any(|f| f.id == *id),
                "Foreign demonstration fact {id}"
            );
            ids.insert(id.clone());
        }
    }
    let occasion_pairs: Vec<_> = events
        .iter()
        .filter(|f| ids.contains(&f.id))
        .map(|f| &f.planets)
        .collect();
    let mut bodies = planets.clone();
    for event in events.iter().filter(|f| ids.contains(&f.id)) {
        bodies.extend(event.planets.iter().cloned());
    }
    // Only actual retained occasions make an otherwise unrelated planet's
    // position/capacity useful for prevention/translation/collection review.
    for fact in native {
        match fact.kind.as_str() {
            "position" | "condition" if fact.planets.iter().any(|p| bodies.contains(p)) => {
                ids.insert(fact.id.clone());
            }
            "moon" | "boundary" => {
                ids.insert(fact.id.clone());
            }
            "reception"
                if fact.planets.len() == 2
                    && (fact.planets.iter().all(|p| planets.contains(p))
                        || occasion_pairs
                            .iter()
                            .any(|pair| pair.iter().all(|p| fact.planets.contains(p)))) =>
            {
                ids.insert(fact.id.clone());
            }
            _ => {}
        }
    }
    Ok(ids)
}

fn plain_relevant(value: &str) -> String {
    value.replace(['\n', '\r', '|'], " ")
}

fn relevant_role_names(roles: &Value) -> String {
    roles
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["role_id"].as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn relevant_number(value: &Value) -> String {
    value.as_f64().map_or_else(
        || "?".into(),
        |n| {
            let rounded = format!("{n:.3}");
            let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
            if trimmed == "-0" {
                "0".into()
            } else {
                trimmed.into()
            }
        },
    )
}

fn relevant_motion(motion: &Value) -> Result<String> {
    if motion.is_null() {
        return Ok("motion=? (not supplied)".into());
    }
    let bodies = motion[3].as_array().context("No supplied body motion")?;
    let opposite = bodies.len() == 2
        && bodies[0][3]
            .as_f64()
            .zip(bodies[1][3].as_f64())
            .is_some_and(|(a, b)| a.signum() != b.signum());
    let mut text = format!(
        "≈bracket {}..{}h intra={}",
        relevant_number(&motion[1][0]),
        relevant_number(&motion[1][1]),
        motion[2]
            .as_bool()
            .map_or("?", |v| if v { "resolved" } else { "?" })
    );
    for body in bodies {
        let name = body[0].as_str().context("No supplied body name")?;
        text.push_str(&format!(
            "; {name} L={}..{}° Δ={}° rev={} zeroMean={}",
            relevant_number(&body[1]),
            relevant_number(&body[2]),
            relevant_number(&body[3]),
            body[7],
            body[8]
        ));
        if body[7] == true
            || body[3]
                .as_f64()
                .zip(body[4].as_f64())
                .is_some_and(|(signed, path)| (signed.abs() - path).abs() > 0.0005)
        {
            text.push_str(&format!(" path={}°", relevant_number(&body[4])));
        }
        if opposite || body[7] == true || body[8] == true {
            text.push_str(&format!(
                " v={}..{}°/day",
                relevant_number(&body[5]),
                relevant_number(&body[6])
            ));
        }
    }
    Ok(text)
}

fn concise_native_table(
    original: &Value,
    raw: &Value,
    native: &[crate::native_evidence::Fact],
    reference: &Value,
    supplied_motion: &BTreeMap<String, Option<Value>>,
) -> Result<Value> {
    let mut details = Vec::new();
    let mut receptions = Vec::new();
    let mut events = Vec::new();
    for fact in native {
        if let Some(edge) = fact.reception.as_ref().filter(|_| fact.kind == "reception") {
            let qualities = |polarity| {
                edge.qualities
                    .iter()
                    .filter(|q| q.polarity == polarity)
                    .map(|q| q.dignity.native_name())
                    .collect::<Vec<_>>()
            };
            receptions.push(json!([
                fact.id,
                edge.guest_planet,
                edge.host_planet,
                qualities(crate::native_evidence::ReceptionPolarity::Positive),
                qualities(crate::native_evidence::ReceptionPolarity::Negative)
            ]));
        } else if let Some(event) = fact
            .event
            .as_ref()
            .filter(|_| fact.kind == "event" && fact.planets.len() == 2)
        {
            let candidates = raw["derived"]["eventSearch"]["events"]
                .as_array()
                .context("No native event search")?;
            let matches = candidates
                .iter()
                .filter(|e| {
                    e["planet1"] == fact.planets[0]
                        && e["planet2"] == fact.planets[1]
                        && e["estimatedPerfectsWithinHours"] == json!(event.astronomical_hours)
                })
                .collect::<Vec<_>>();
            ensure!(matches.len() == 1, "Native event is ambiguous: {}", fact.id);
            let aspect = matches[0]["aspectName"]
                .as_str()
                .context("No native aspect name")?;
            let signs = match event.within_current_signs {
                Some(true) => "before either changes sign",
                Some(false) => "after a change of sign",
                None => "sign-change status unestablished",
            };
            // Factor the calculator's exact display format; this neither parses
            // a user statement nor creates a new numeric/semantic testimony.
            let prefix = format!(
                "{aspect} candidate in approximately {:.2} astronomical hours, {signs}.",
                event.astronomical_hours
            );
            let gap = fact
                .detail
                .strip_prefix(&prefix)
                .and_then(|v| {
                    v.strip_suffix(" This is not the predicted timing of the earthly event.")
                })
                .and_then(|v| v.strip_prefix(" Current angular distance from the aspect: "))
                .and_then(|v| v.strip_suffix('.'));
            let motion = supplied_motion
                .get(&fact.id)
                .and_then(Option::as_ref)
                .map(concise_event_motion);
            let gap_or_detail =
                gap.map_or_else(|| json!({"native_detail":fact.detail}), |gap| json!(gap));
            events.push(json!([
                fact.id,
                fact.planets[0],
                fact.planets[1],
                aspect,
                event.within_current_signs,
                event.astronomical_hours,
                gap_or_detail,
                motion
            ]));
        } else {
            details.push(json!([fact.id, fact.kind, fact.label, fact.detail]));
        }
    }
    let role_fields = [
        "role_id",
        "actor_id",
        "label",
        "significator_house",
        "ruler_planet",
        "optional",
        "authority",
        "selected_id",
        "cusp_fact_id",
        "position_fact_ids",
        "source_fields",
    ];
    let mut roles = reference["role_rows"]
        .as_array()
        .context("No role rows")?
        .iter()
        .map(|row| {
            role_fields
                .iter()
                .map(|field| row[*field].clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut aliases = BTreeMap::<String, Vec<Value>>::new();
    for alias in reference["aliases"]
        .as_array()
        .context("No supplied role aliases")?
    {
        let id = alias["role_id"].as_str().context("No role alias ID")?;
        let planet = alias["planet"].as_str().context("No role alias planet")?;
        if !roles.iter().any(|row| row[0] == id) {
            roles.push(vec![
                alias["role_id"].clone(),
                alias["actor_id"].clone(),
                alias["label"].clone(),
                alias["significator_house"].clone(),
                alias["planet"].clone(),
                alias["optional"].clone(),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ]);
        }
        aliases.entry(planet.into()).or_default().push(json!(id));
    }
    Ok(
        json!({"presentation":"concise-native-reader-1", "anchor":original["anchor"],
        "input_scope":original["input_scope"], "search_limitations":original["search_limitations"],
        "moon_search":original["moon"],
        "facts":{"columns":["id","kind","label","native_detail"],"rows":details},
        "receptions":{"columns":["id","regarding_planet","regarded_planet","positive_qualities","negative_qualities"],"rows":receptions},
        "events":{"columns":["id","endpoint1","endpoint2","aspect","within_current_signs","astronomical_hours","native_current_angular_gap_or_detail","motion"],"rows":events},
        "motion_columns":["estimate_method","bracket_hours","intra_interval_resolved","bodies"],
        "body_motion_columns":["planet","chart_longitude_degrees","estimated_perfection_longitude_degrees","signed_travel_degrees","path_distance_degrees","first_interval_speed_deg_per_day","perfection_bracket_speed_deg_per_day","sampled_reversal","zero_mean_interval"],
        "role_reference":{"source_identity":reference["source_identity"],"source_scope":reference["source_scope"],
            "principal_basis":reference["principal_basis"],"alias_authority":reference["alias_authority"],
            "role_columns":role_fields,"roles":roles,"aliases_by_planet":aliases,
            "moon":reference["moon"],"unknowns":reference["unknowns"]},
        "legend":"Reception direction is regarding→regarded, with every supplied quality retained; no weighting or verdict. Event endpoints are storage order, not applicant/caller order. Native gap is not travel. Motion positions/travel are approximate sample-bracket interpolation; speeds are interval means, not certified exact speeds. Reversal flags cover samples only; intra-interval motion remains unresolved. Null motion was not supplied in this input. All native eIDs remain; unknown or omitted computation is not proof of absence."}),
    )
}

fn concise_event_motion(motion: &Value) -> Value {
    json!([
        motion["estimateMethod"],
        motion["bracketHours"],
        motion["intraIntervalMotionResolved"],
        motion["bodyMotion"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|body| json!([
                body["planet"],
                body["chartLongitudeDegrees"],
                body["estimatedPerfectionLongitudeDegrees"],
                body["estimatedSignedTravelDegrees"],
                body["estimatedPathDistanceDegrees"],
                body["initialIntervalMeanSpeedDegreesPerDay"],
                body["perfectionBracketMeanSpeedDegreesPerDay"],
                body["sampledMotionReversalObserved"],
                body["zeroMeanMotionIntervalObserved"]
            ]))
            .collect::<Vec<_>>()
    ])
}

pub fn method_source(book: &Book, method: &str, reading: bool) -> Result<String> {
    book.student_context(method_source_pages(method)?, reading)
}

/// The union of the existing family and every actual branch allocation. It is
/// chosen by the declared component family, never a fixture's expected branch.
pub fn family_reader_source(book: &Book, method: &str) -> Result<String> {
    let pages = method_source_pages(method)?;
    let mut extra: BTreeSet<_> = common_branch_pages(method)
        .iter()
        .copied()
        .filter(|page| !pages.contains(page))
        .collect();
    // Contact teaching needs both translation variants, not only the shared
    // excerpt's already-separated example. Original case/gold bytes are unchanged.
    if method == "contact" {
        extra.extend([92, 93]);
    }
    let mut source = method_source(book, method, true)?;
    source.push_str(&book.pages(&extra)?);
    Ok(source)
}

/// Full salient primary pages for authoring and reviewing reference answers.
/// Students use their separately bound compact topic source; this function does
/// not enlarge student calls or turn reference answers into input facts.
/// This allocation uses the declared family/source contracts, never a private
/// expected branch, verdict or answer. Having a rule available does not make it
/// applicable: state/quality questions do not acquire an aspect prerequisite.
pub fn reference_reader_pages(method: &str) -> Result<BTreeSet<usize>> {
    ensure!(
        !matches!(method, "wish" | "unclassified"),
        "Reference authoring requires a substantive reading family"
    );
    let mut pages = Book::context_pages(method_source_pages(method)?, true);
    pages.extend(common_branch_pages(method));
    pages.extend(14..=38); // Houses/capacities, turning, principal, rulers and Moon collisions.
    pages.extend(40..=77); // Signs; complete essential/accidental and directed-reception context.
    pages.extend(84..=101); // Occasion, actual motion/signs, translation/collection and prevention.
    pages.extend(137..=141); // Principal/relay, actual default and chart qualifications.
    pages.extend(243..=246); // Delivering a useful answer, not extra adjudication rules.
    if method_facets(method)?
        .iter()
        .any(|facet| matches!(facet.as_str(), "event" | "timing"))
    {
        pages.extend(127..=136);
    }
    // The natural/person/heart examples are relevant to this family only; do
    // not universalize romantic identities or psychology into other readings.
    if method == "relationship" {
        pages.extend(78..=83);
    }
    if matches!(method, "missing_person" | "existing_job") {
        // Their medical example starts on p186; p187 is its worked explanation.
        // This is source continuity, not a new universal illness requirement.
        pages.insert(187);
    }
    if method == "lost_object" {
        // The thief description on p154 explicitly refers to pp143–145,
        // including the natural-Mercury/dispositor distinction. Gold source
        // only: the separately declared compact student baseline is unchanged.
        pages.extend(143..=145);
    }
    Ok(pages)
}

/// The first paragraph of p188 completes that medical example. Treatment after
/// it is outside these two allocations and is neither imported nor demanded.
pub fn reference_reader_excerpt_pages(method: &str) -> BTreeSet<usize> {
    if matches!(method, "missing_person" | "existing_job") {
        BTreeSet::from([188])
    } else {
        BTreeSet::new()
    }
}

pub fn reference_reader_source(book: &Book, method: &str) -> Result<String> {
    let mut source = book.pages(&reference_reader_pages(method)?)?;
    for page in reference_reader_excerpt_pages(method) {
        source.push_str(&book.first_paragraph(page)?);
    }
    Ok(source)
}

fn method_source_pages(method: &str) -> Result<BTreeSet<usize>> {
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
    // Explicit primary-text cross-chapter dependencies. These form a new,
    // corrected topic baseline for BOTH arms of future comparisons. Old banks,
    // compiled campaigns and their captured input/source bytes are untouched.
    match method {
        // Letting reuses the property/price procedure (p170); p169 completes it.
        "rental" => {
            pages.insert(169);
        }
        // Examinations are explicitly profit from knowledge (p218).
        "exam" => {
            pages.extend([216, 217]);
        }
        // Pregnancy/conception bodily-aspect restrictions begin on p173.
        "fertility" => {
            pages.insert(173);
        }
        _ => {}
    }
    Ok(pages)
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
mod concise_projection_tests {
    use super::*;

    #[test]
    fn relevant_contact_keeps_actual_roles_mixed_edges_and_sparse_unknowns_without_metadata() {
        let (input, _, native) = declared(Some("mother"));
        let relevant = relevant_reader_inputs(&input, "contact").unwrap();
        assert_eq!(relevant.get("question"), input.get("question"));
        let text = relevant.get("chart_evidence").unwrap().as_str().unwrap();
        let ids = relevant_chart_ids(text).unwrap();
        assert!(ids.len() < native.len());
        assert!(text.contains("target.person(alma) = H10/Mars"));
        assert!(text.contains("e47|reception|Mars→Saturn +[termRuler,faceRuler] -[detriment]"));
        assert!(text.contains("e57|reception|Saturn→Mars +[domicile] -[]"));
        assert!(text.contains("afterIngress"));
        assert!(text.contains("intra=?"));
        assert!(text.contains("omitted evidence or uncomputed techniques are unknown"));
        for field in [
            "source_identity",
            "source_fields",
            "input_scope",
            "alias_authority",
            "body_motion_columns",
            "\"quote\"",
            "PRIVATE ANSWER",
        ] {
            assert!(!text.contains(field));
        }
        let mut chart: Value =
            serde_json::from_str(input.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        chart.as_object_mut().unwrap().remove("role_reference");
        let consultation: Value =
            serde_json::from_str(input.get("consultation").unwrap().as_str().unwrap()).unwrap();
        let sparse = relevant_reader_chart(&chart, &consultation, "contact", None).unwrap();
        assert!(sparse.contains("motion=? (not supplied)"));
        assert!(!sparse.contains("Δ="));
    }

    #[test]
    fn relevant_contact_retains_shared_aliases_moon_priority_and_unknown_target() {
        let (input, _, _) = declared(Some("friend"));
        let relevant = relevant_reader_inputs(&input, "contact").unwrap();
        let text = relevant.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(text.contains("principal.person(querent) = H1/Saturn"));
        assert!(text.contains("target.message(alma) = H1/Saturn optional"));
        let (input, _, _) = declared(Some("partner"));
        let relevant = relevant_reader_inputs(&input, "contact").unwrap();
        let text = relevant.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(text.contains("target.person(alma) = H7/Moon"));
        assert!(text.contains("Moon contextual=no; house claims=target.person"));
        assert!(!text.contains("principal.moon("));
        let (input, _, _) = declared(None);
        let relevant = relevant_reader_inputs(&input, "contact").unwrap();
        let text = relevant.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(text.contains("target.person(alma) = —/?"));
        assert!(text.contains("UNKNOWN The target's operative relationship is absent"));
        assert!(!text.contains("target.person(alma) = H7"));
    }

    #[test]
    fn relevant_contact_preserves_ordered_incoming_outgoing_prevention_without_lunar_chasing() {
        let event = |id: &str, a: &str, b: &str, hours: f64| crate::native_evidence::Fact {
            id: id.into(),
            kind: "event".into(),
            label: format!("{a} · {b}"),
            detail: "synthetic actual candidate".into(),
            planets: vec![a.into(), b.into()],
            condition_facet: None,
            reception: None,
            event: Some(crate::native_evidence::EventCandidate {
                within_current_signs: Some(false),
                astronomical_hours: hours,
                perfection_motion: None,
            }),
        };
        let native = vec![
            event("e1", "Moon", "Venus", 1.),
            event("e2", "Mars", "Moon", 2.),
            event("e3", "Moon", "Saturn", 3.),
            event("e4", "Venus", "Saturn", 4.),
            event("e5", "Mars", "Venus", 5.),
            event("e6", "Saturn", "Mars", 6.),
            event("e7", "Saturn", "Mars", 20.),
            event("e8", "Mercury", "Sun", 2.5),
        ];
        let table = json!({"role_reference":{"roles":[
            ["principal.person","querent","Q",1,"Saturn",false,null,null,null,[],[]],
            ["target.person","iris","I",10,"Mars",false,null,null,null,[],[]]]}});
        let ids = relevant_contact_ids(&table, &native, None).unwrap();
        assert_eq!(
            ids,
            BTreeSet::from(["e1", "e2", "e4", "e5", "e6"].map(str::to_owned))
        );
        // Both incoming and outgoing endpoint orders are retained. The third
        // Moon occasion and distant repeat do not become a long success chain.
        assert!(!ids.contains("e3") && !ids.contains("e7") && !ids.contains("e8"));
    }

    #[test]
    fn relevant_demo_only_citations_extend_a_verified_domain_and_parser_is_strict() {
        let (input, chart, _) = declared(Some("mother"));
        let consultation: Value =
            serde_json::from_str(input.get("consultation").unwrap().as_str().unwrap()).unwrap();
        let live = relevant_reader_chart(&chart, &consultation, "contact", None).unwrap();
        let extras = BTreeSet::from(["e67", "e68", "e69", "e70"].map(str::to_owned));
        let demo = relevant_reader_chart(&chart, &consultation, "contact", Some(&extras)).unwrap();
        let live_ids = relevant_chart_ids(&live).unwrap();
        let demo_ids = relevant_chart_ids(&demo).unwrap();
        assert!(live_ids.is_subset(&demo_ids) && extras.is_subset(&demo_ids));
        assert!(
            relevant_reader_chart(
                &chart,
                &consultation,
                "contact",
                Some(&BTreeSet::from(["e999".into()]))
            )
            .is_err()
        );
        assert_eq!(
            live,
            relevant_reader_chart(&chart, &consultation, "contact", None).unwrap()
        );
        assert_eq!(
            relevant_chart_ids("Mention e999 outside.\n<facts>\ne1|boundary|limits\n</facts>\n")
                .unwrap(),
            BTreeSet::from(["e1".into()])
        );
        for invalid in [
            "e1|boundary|No block",
            "<facts>\ne1|boundary|missing close",
            "<facts>\ne01|boundary|noncanonical\n</facts>",
            "<facts>\ne1|boundary|first\ne1|event|duplicate\n</facts>",
            "<facts>\ne1|unknown_kind|foreign\n</facts>",
            "<facts>\ne1|boundary|\n</facts>",
            "<facts>\ne1|boundary|a\n</facts>\n<facts>\ne2|boundary|b\n</facts>",
        ] {
            assert!(relevant_chart_ids(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn relevant_consultation_preserves_values_unknowns_and_clock_without_provenance() {
        let value = json!({"method":"contact","facet":"event","facts":[
            {"field":"event_place","value":"Fairfax market","quote":"PRIVATE QUOTE"},
            {"field":"horizon","value":"Friday, 2026-10-09","quote":"Friday"},
            {"field":"reader_place","value":"PRIVATE COORDINATES","quote":"here"}],
            "unknown":["people.alma.capacity"],"source_identity":"PRIVATE HASH",
            "device":{"observed_at":"2026-10-08T18:33:00Z","time_zone":"America/New_York",
                "place":{"name":"Springfield","latitude":38.657,"longitude":-77.249}}});
        let saved = value.clone();
        let text = relevant_reader_consultation(&value).unwrap();
        assert_eq!(value, saved);
        for needed in [
            "event_place=Fairfax market",
            "horizon=Friday, 2026-10-09",
            "UNKNOWN people.alma.capacity",
            "device_observed_at=2026-10-08T18:33:00Z",
            "device_time_zone=America/New_York",
        ] {
            assert!(text.contains(needed));
        }
        for private in [
            "PRIVATE",
            "38.657",
            "-77.249",
            "quote",
            "source_identity",
            "{",
        ] {
            assert!(!text.contains(private));
        }
    }

    #[test]
    #[ignore = "Reads an explicitly supplied original TRAIN input; no model calls"]
    fn relevant_actual_training_input_readonly_receipt() {
        let path = std::env::var("HORARY_RELEVANT_TRAIN_INPUT").expect("Original TRAIN input path");
        let bytes = fs::read(&path).unwrap();
        let fields: Vec<(String, Value)> = serde_json::from_slice(&bytes).unwrap();
        let original = Example::new(fields).with_inputs([
            "question",
            "consultation",
            "chart_evidence",
            "source_text",
        ]);
        let projected = relevant_reader_inputs(&original, "contact").unwrap();
        let output = std::env::var("HORARY_RELEVANT_OUTPUT").expect("Unique new output path");
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
            .unwrap();
        use std::io::Write;
        file.write_all(
            serde_json::to_string_pretty(&projected.rendered())
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        println!(
            "{}",
            json!({"input_path":path,"original_sha256":crate::google::digest(&bytes),
            "output_path":output,"provided_fact_ids":relevant_chart_ids(projected.get("chart_evidence").unwrap().as_str().unwrap()).unwrap(),
            "chart_bytes":projected.get("chart_evidence").unwrap().as_str().unwrap().len(),"model_calls":0,
            "tokens":"unmeasured: count the complete rendered request"})
        );
    }

    fn declared(relationship: Option<&str>) -> (Example, Value, Vec<crate::native_evidence::Fact>) {
        // TRAIN Contact001's published synthetic instant. No Case.expected or
        // reference answer enters the projection, including its optional roles.
        let utc = "2026-10-08T18:33:00Z";
        let at = chrono::DateTime::parse_from_rfc3339(utc).unwrap();
        let raw = horary_ai_core::astronomy::chart(at.timestamp_millis() as f64, 38.657, -77.249)
            .unwrap();
        let native = crate::native_evidence::facts(Some(&raw));
        let mut facts = vec![json!({"field":"subject.owner_id","value":"alma","quote":"Alma"})];
        if let Some(relationship) = relationship {
            facts.push(json!({"field":"people.alma.relationship","value":relationship,"quote":relationship}));
        }
        let consultation = json!({"facts":facts,"unknown":[]});
        let chart = json!({"anchor":{"utc":utc,"latitude":38.657,"longitude":-77.249},
            "facts":compact_native_facts(&native),"moon":raw["derived"]["voidOfCourseMoon"],
            "search_limitations":raw["calculationNote"],"input_scope":"Declared synthetic reading component, not an extraction journey.",
            "role_reference":component_contact_role_reference(&consultation,&native).unwrap()});
        let mut input = dsrust::input! {source_text:"OLD FULL SOURCE",question:"Will Alma contact me this week?",
        consultation:serde_json::to_string(&consultation).unwrap(),chart_evidence:serde_json::to_string(&chart).unwrap()};
        input.set("expected", json!("PRIVATE ANSWER MUST NOT BE READ"));
        (input, chart, native)
    }

    #[test]
    fn concise_projection_keeps_every_native_id_direction_quality_and_motion_number() {
        let (input, original, native) = declared(Some("mother"));
        let compact = concise_reader_inputs(&input, "contact").unwrap();
        assert_eq!(compact.get("question"), input.get("question"));
        assert_eq!(compact.get("consultation"), input.get("consultation"));
        assert!(compact.get("expected").is_none());
        let chart: Value =
            serde_json::from_str(compact.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        let mut ids = BTreeSet::new();
        for section in ["facts", "receptions", "events"] {
            for row in chart[section]["rows"].as_array().unwrap() {
                assert!(ids.insert(row[0].as_str().unwrap()));
            }
        }
        assert_eq!(ids, native.iter().map(|f| f.id.as_str()).collect());
        for row in chart["receptions"]["rows"].as_array().unwrap() {
            let edge = native
                .iter()
                .find(|f| f.id.as_str() == row[0].as_str().unwrap())
                .unwrap()
                .reception
                .as_ref()
                .unwrap();
            assert_eq!(row[1], edge.guest_planet);
            assert_eq!(row[2], edge.host_planet);
            let positives = edge
                .qualities
                .iter()
                .filter(|q| q.polarity == crate::native_evidence::ReceptionPolarity::Positive)
                .map(|q| q.dignity.native_name())
                .collect::<Vec<_>>();
            let negatives = edge
                .qualities
                .iter()
                .filter(|q| q.polarity == crate::native_evidence::ReceptionPolarity::Negative)
                .map(|q| q.dignity.native_name())
                .collect::<Vec<_>>();
            assert_eq!(row[3], json!(positives));
            assert_eq!(row[4], json!(negatives));
        }
        let mixed = chart["receptions"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[0] == "e47")
            .unwrap();
        assert_eq!(
            &mixed.as_array().unwrap()[1..],
            &[
                json!("Mars"),
                json!("Saturn"),
                json!(["termRuler", "faceRuler"]),
                json!(["detriment"])
            ]
        );
        for row in chart["events"]["rows"].as_array().unwrap() {
            let supplied = original["role_reference"]["event_endpoints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["fact_id"] == row[0])
                .unwrap();
            assert_eq!(row[4], supplied["candidate"]["withinCurrentSigns"]);
            assert_eq!(row[5], supplied["candidate"]["astronomicalHours"]);
            assert_eq!(
                row[7],
                concise_event_motion(&supplied["candidate"]["perfectionMotion"])
            );
        }
        let moon_saturn = chart["events"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[0] == "e68")
            .unwrap();
        assert!(moon_saturn[7][3][0][3].as_f64().unwrap() > 0.);
        assert!(moon_saturn[7][3][1][3].as_f64().unwrap() < 0.);
        assert_eq!(moon_saturn[7][2], false);
        assert!(chart["role_reference"].get("event_endpoints").is_none());
        assert!(chart["role_reference"].get("directed_reception").is_none());
        assert!(
            compact
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap()
                .len()
                < input.get("chart_evidence").unwrap().as_str().unwrap().len() / 2
        );
    }

    #[test]
    fn concise_projection_refuses_foreign_rows_boundaries_motion_aliases_and_bad_anchor() {
        let (_, original, _) = declared(Some("mother"));
        let consultation = json!({"facts":[{"field":"subject.owner_id","value":"alma","quote":"Alma"},{"field":"people.alma.relationship","value":"mother","quote":"mother"}],"unknown":[]});
        let mut changed = original.clone();
        changed["facts"] = json!(format!(
            "{}e999\tcondition\tForeign\tInvented\n",
            original["facts"].as_str().unwrap()
        ));
        assert!(concise_reader_chart(&changed, &consultation, "contact").is_err());
        changed = original.clone();
        changed["moon"]["isVoid"] = json!(false);
        assert!(concise_reader_chart(&changed, &consultation, "contact").is_err());
        changed = original.clone();
        changed["role_reference"]["event_endpoints"][0]["candidate"]["perfectionMotion"]["bodyMotion"]
            [0]["estimatedSignedTravelDegrees"] = json!(-1.);
        assert!(concise_reader_chart(&changed, &consultation, "contact").is_err());
        changed = original.clone();
        changed["role_reference"]["aliases"][0]["planet"] = json!("Moon");
        assert!(concise_reader_chart(&changed, &consultation, "contact").is_err());
        changed = original.clone();
        changed["anchor"]["longitude"] = json!(181.);
        assert!(concise_reader_chart(&changed, &consultation, "contact").is_err());
        assert!(concise_reader_source("new_job").is_err());
    }

    #[test]
    fn concise_projection_does_not_invent_sparse_motion_or_missing_role_information() {
        let (_, mut original, _) = declared(None);
        for row in original["role_reference"]["event_endpoints"]
            .as_array_mut()
            .unwrap()
        {
            row["candidate"]
                .as_object_mut()
                .unwrap()
                .remove("perfectionMotion");
        }
        let consultation = json!({"facts":[{"field":"subject.owner_id","value":"alma","quote":"Alma"}],"unknown":[]});
        let chart = concise_reader_chart(&original, &consultation, "contact").unwrap();
        assert!(
            chart["events"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r[7].is_null())
        );
        let target = chart["role_reference"]["roles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[0] == "target.person")
            .unwrap();
        assert!(target[3].is_null() && target[4].is_null());
        assert!(
            !chart["role_reference"]["unknowns"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn concise_projection_preserves_shared_ruler_aliases_and_moon_priority() {
        let (input, _, _) = declared(Some("friend"));
        let compact = concise_reader_inputs(&input, "contact").unwrap();
        let chart: Value =
            serde_json::from_str(compact.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        assert_eq!(
            chart["role_reference"]["aliases_by_planet"]["Saturn"],
            json!(["principal.person", "target.message"])
        );
        let (input, _, _) = declared(Some("partner"));
        let compact = concise_reader_inputs(&input, "contact").unwrap();
        let chart: Value =
            serde_json::from_str(compact.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        assert_eq!(
            chart["role_reference"]["moon"]["available_as_contextual_reference"],
            false
        );
        assert_eq!(
            chart["role_reference"]["aliases_by_planet"]["Moon"],
            json!(["target.person"])
        );
    }

    #[test]
    fn concise_motion_preserves_wrap_reversal_and_independent_body_numbers() {
        let motion = json!({"estimateMethod":"sample_interval_linear_interpolation","bracketHours":[5.,6.],"intraIntervalMotionResolved":false,"bodyMotion":[
            {"planet":"Venus","chartLongitudeDegrees":350.,"estimatedPerfectionLongitudeDegrees":5.,"estimatedSignedTravelDegrees":15.,"estimatedPathDistanceDegrees":17.,"initialIntervalMeanSpeedDegreesPerDay":-0.2,"perfectionBracketMeanSpeedDegreesPerDay":0.4,"sampledMotionReversalObserved":true,"zeroMeanMotionIntervalObserved":false},
            {"planet":"Mars","chartLongitudeDegrees":5.,"estimatedPerfectionLongitudeDegrees":4.,"estimatedSignedTravelDegrees":-1.,"estimatedPathDistanceDegrees":1.,"initialIntervalMeanSpeedDegreesPerDay":-0.1,"perfectionBracketMeanSpeedDegreesPerDay":-0.2,"sampledMotionReversalObserved":false,"zeroMeanMotionIntervalObserved":true}]});
        assert_eq!(
            concise_event_motion(&motion),
            json!([
                "sample_interval_linear_interpolation",
                [5., 6.],
                false,
                [
                    ["Venus", 350., 5., 15., 17., -0.2, 0.4, true, false],
                    ["Mars", 5., 4., -1., 1., -0.1, -0.2, false, true]
                ]
            ])
        );
    }

    #[test]
    fn concise_projection_preserves_untyped_native_details_without_minting_edges() {
        let (_, original, _) = declared(None);
        let reference = json!({"role_rows":[],"aliases":[],"unknowns":["UNKNOWN"],"moon":{"available_as_contextual_reference":false}});
        let fact = crate::native_evidence::Fact {
            id: "e0".into(),
            kind: "reception".into(),
            label: "Legacy".into(),
            detail: "Unobserved direction; do not manufacture a typed edge.".into(),
            planets: vec![],
            condition_facet: None,
            event: None,
            reception: None,
        };
        let chart = concise_native_table(
            &original,
            &json!({}),
            std::slice::from_ref(&fact),
            &reference,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            chart["facts"]["rows"],
            json!([[fact.id, fact.kind, fact.label, fact.detail]])
        );
        assert_eq!(chart["receptions"]["rows"], json!([]));
    }

    #[test]
    #[ignore = "Reads an explicitly supplied original TRAIN input; never calls a model"]
    fn concise_projection_actual_training_input_readonly_receipt() {
        let path = std::env::var("HORARY_CONCISE_TRAIN_INPUT")
            .expect("Supply the original TRAIN input path");
        let bytes = fs::read(&path).unwrap();
        let fields: Vec<(String, Value)> = serde_json::from_slice(&bytes).unwrap();
        let original = Example::new(fields).with_inputs([
            "source_text",
            "question",
            "consultation",
            "chart_evidence",
        ]);
        let compact = concise_reader_inputs(&original, "contact").unwrap();
        let old_chart = original.get("chart_evidence").unwrap().as_str().unwrap();
        let new_chart = compact.get("chart_evidence").unwrap().as_str().unwrap();
        let output = std::env::var("HORARY_CONCISE_OUTPUT").ok();
        if let Some(output) = &output {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)
                .unwrap();
            use std::io::Write;
            file.write_all(
                serde_json::to_string_pretty(&compact.rendered())
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap();
        }
        println!(
            "{}",
            json!({"original_input_path":path,"original_input_sha256":crate::google::digest(&bytes),
            "original_chart_bytes":old_chart.len(),"concise_chart_bytes":new_chart.len(),
            "concise_source_bytes":compact.get("source_text").unwrap().as_str().unwrap().len(),
            "compact_input_hash":crate::optimize::input_key(&compact).unwrap(),"output_path":output,
            "model_calls":0,"token_count":"not_measured; whole wire requires tokenizer admission"})
        );
    }
}

#[cfg(test)]
mod source_context_tests {
    use super::*;

    #[test]
    fn reference_source_contains_full_common_and_all_family_branch_pages() {
        let book = Book::fixture();
        let common = (14..=38)
            .chain(40..=77)
            .chain(84..=101)
            .chain(137..=141)
            .chain(243..=246)
            .collect::<BTreeSet<_>>();
        for method in crate::signatures::METHODS
            .iter()
            .copied()
            .filter(|m| !matches!(*m, "wish" | "unclassified"))
        {
            let pages = reference_reader_pages(method).unwrap();
            assert!(
                common.is_subset(&pages),
                "{method}: missing common primary procedure"
            );
            assert!(
                method_source_pages(method).unwrap().is_subset(&pages),
                "{method}: missing family/branch primary pages"
            );
            let source = reference_reader_source(&book, method).unwrap();
            for page in pages {
                assert!(source.contains(&format!(
                    "<Frawley printed_page=\"{page}\" ocr_page=\"{}\">",
                    page + 9
                )));
            }
            assert!(!source.contains("<rule id="));
        }
    }
    #[test]
    fn reference_timing_and_relationship_context_use_family_not_case_gold() {
        let contact = reference_reader_pages("contact").unwrap();
        assert!((127..=136).all(|p| contact.contains(&p)));
        let trust = reference_reader_pages("trust").unwrap();
        assert!((132..=136).all(|p| !trust.contains(&p)));
        assert!(
            reference_reader_pages("relationship")
                .unwrap()
                .contains(&79)
        );
        assert!(!reference_reader_pages("new_job").unwrap().contains(&79));
        assert!(
            reference_reader_pages("missing_person")
                .unwrap()
                .contains(&187)
        );
        assert_eq!(
            reference_reader_excerpt_pages("missing_person"),
            BTreeSet::from([188])
        );
        assert_eq!(
            reference_reader_excerpt_pages("existing_job"),
            BTreeSet::from([188])
        );
        assert!(reference_reader_excerpt_pages("contact").is_empty());
        assert!((143..=145).all(|p| reference_reader_pages("lost_object").unwrap().contains(&p)));
        assert!(reference_reader_pages("wish").is_err());
        assert!(reference_reader_pages("unclassified").is_err());
    }
    #[test]
    fn reference_common_expansion_stays_out_of_compact_student_source() {
        let book = Book::fixture();
        for method in ["contact", "new_job", "relationship", "movable_deal"] {
            let student = family_reader_source(&book, method).unwrap();
            let reference = reference_reader_source(&book, method).unwrap();
            assert!(reference.contains("<Frawley printed_page=\"18\""));
            assert!(!student.contains("<Frawley printed_page=\"18\""));
        }
        let legacy = BTreeSet::from([
            7, 8, 30, 40, 44, 56, 57, 58, 59, 60, 61, 62, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74,
            75, 76, 77, 126, 127, 128, 129, 130, 131, 166,
        ]);
        assert_eq!(
            book.context([166], true).unwrap(),
            book.pages(&legacy).unwrap()
        );
    }
    #[test]
    fn corrected_student_topic_baseline_adds_only_three_verified_dependencies() {
        let book = Book::fixture();
        for method in crate::signatures::METHODS {
            let card = coverage_card(method).unwrap();
            let mut prior = card["primary_context_printed_pages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_u64().unwrap() as usize)
                .collect::<BTreeSet<_>>();
            for branch in card["judgment_subbranches"].as_array().unwrap() {
                prior.extend(
                    branch["printed_pages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| p.as_u64().unwrap() as usize),
                );
            }
            let expected = match *method {
                "rental" => BTreeSet::from([169]),
                "exam" => BTreeSet::from([216, 217]),
                "fertility" => BTreeSet::from([173]),
                _ => BTreeSet::new(),
            };
            let current = method_source_pages(method).unwrap();
            assert!(prior.is_subset(&current));
            assert_eq!(
                current.difference(&prior).copied().collect::<BTreeSet<_>>(),
                expected
            );
            let source = family_reader_source(&book, method).unwrap();
            for page in expected {
                assert!(source.contains(&format!("<Frawley printed_page=\"{page}\"")));
            }
        }
    }

    #[test]
    #[ignore = "requires the private hash-pinned book; source bytes only, no cases or models"]
    fn reference_real_book_source_size_witnesses() {
        let book = Book::open(Path::new(crate::book::DEFAULT_OCR)).unwrap();
        for method in crate::signatures::METHODS
            .iter()
            .copied()
            .filter(|m| !matches!(*m, "wish" | "unclassified"))
        {
            let pages = reference_reader_pages(method).unwrap();
            let reference = reference_reader_source(&book, method).unwrap();
            let mut actual = book.pages(&pages).unwrap();
            let excerpts = reference_reader_excerpt_pages(method);
            for page in &excerpts {
                actual.push_str(&book.first_paragraph(*page).unwrap());
            }
            assert_eq!(reference, actual);
            let student = family_reader_source(&book, method).unwrap();
            println!(
                "{}",
                json!({"method":method,"printed_pages":pages,"page_count":pages.len(),"excerpt_printed_pages":excerpts,
                "reference_source_bytes":reference.len(),"reference_source_sha256":crate::google::digest(&reference),
                "student_source_bytes":student.len(),"student_source_sha256":crate::google::digest(student),
                "rough_reference_tokens_characters_div_four":reference.chars().count().div_ceil(4),
                "rough_size_is_not_tokenizer_measurement_or_admission":true,"provider_calls":0})
            );
        }
    }

    #[test]
    fn contact_component_reference_uses_assembled_declared_inputs_without_private_role_gold() {
        let case: Case = serde_json::from_value(json!({
            "id":"synthetic-contact-view","method":"contact","split":"train","variant":"explicit",
            "question":"Will my mother Alma contact me?","history":[],
            "device":{"observed_at":"2031-04-17T11:30:00Z","time_zone":"America/New_York",
                "place":{"name":"Synthetic reader place","latitude":38.657,"longitude":-77.249}},
            "chart":{"utc":"2031-04-17T11:30:00Z","latitude":38.657,"longitude":-77.249},
            "expected":{"facet":"event","facts":[
                {"field":"subject.owner_id","value":"alma","quote":"Alma"},
                {"field":"people.alma.relationship","value":"mother","quote":"my mother Alma"}],
                "missing":[],"reading_status":"answer","must_explain":["PRIVATE ROLE GOLD MUST NOT ENTER VIEW"],
                "must_not":[],"source_pages":[165,166]}
        })).unwrap();
        let input = example(&case, "reading", &Book::fixture()).unwrap();
        let consultation: Value =
            serde_json::from_str(input.get("consultation").unwrap().as_str().unwrap()).unwrap();
        let evidence: Value =
            serde_json::from_str(input.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        let view = &evidence["role_reference"];
        assert_eq!(view["alias_authority"], "calculated_role_references");
        let target = view["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["role_id"] == "target.person")
            .unwrap();
        assert_eq!(target["significator_house"], 10);
        assert!(
            evidence["facts"]
                .as_str()
                .unwrap()
                .lines()
                .any(|row| row.starts_with(target["cusp_fact_id"].as_str().unwrap()))
        );
        assert_eq!(
            view["accepted_fact_references"][0]["quote"],
            "my mother Alma"
        );
        assert!(!view.to_string().contains("PRIVATE ROLE GOLD"));
        assert_eq!(
            consultation["facts"],
            serde_json::to_value(&case.expected.facts).unwrap()
        );
        let mut without = evidence;
        without.as_object_mut().unwrap().remove("role_reference");
        assert_eq!(
            without,
            chart_evidence(&case).unwrap(),
            "Removing only the view reproduces the original complete native chart presentation"
        );
        let mut declared = consultation;
        declared["facts"][1]["value"] = json!("friend");
        let raw = horary_ai_core::astronomy::chart(
            chrono::DateTime::parse_from_rfc3339(&case.chart.utc)
                .unwrap()
                .timestamp_millis() as f64,
            case.chart.latitude,
            case.chart.longitude,
        )
        .unwrap();
        let view =
            component_contact_role_reference(&declared, &crate::native_evidence::facts(Some(&raw)))
                .unwrap();
        assert_eq!(
            view["role_rows"][1]["significator_house"], 11,
            "The helper follows the supplied declaration, not the case's mother label"
        );
    }

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
