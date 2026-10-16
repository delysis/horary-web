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

fn component_family_role_reference(
    consultation: &Value,
    native: &[crate::native_evidence::Fact],
    method: &str,
) -> Result<Value> {
    ensure!(
        matches!(
            method,
            "new_job"
                | "movable_deal"
                | "relationship"
                | "money"
                | "property"
                | "rental"
                | "return_to_job"
                | "job_offer"
                | "investment"
                | "business_property"
                | "parcel"
                | "visit"
                | "trust"
                | "information"
                | "vehicle"
                | "person_description"
                | "tax"
                | "bet"
        ),
        "Unsupported role family"
    );
    if let Some(declared) = consultation.get("method") {
        ensure!(
            declared.as_str() == Some(method),
            "Declared reader method differs"
        );
    }
    let mut fields: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut unknowns = Vec::new();
    let mut valid = consultation["facts"].is_array();
    for fact in consultation["facts"].as_array().into_iter().flatten() {
        if let (Some(field), Some(_), Some(_)) = (
            fact["field"].as_str(),
            fact["value"].as_str(),
            fact["quote"].as_str(),
        ) {
            fields.entry(field).or_default().push(fact);
        } else {
            valid = false;
            unknowns.push("A declared fact row is malformed.".to_owned());
        }
    }
    if !valid {
        unknowns.push("The declared fact projection is unavailable or malformed.".into());
    }
    let unavailable: BTreeSet<_> = consultation["unknown"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let value = |field: &str| {
        (!unavailable.contains(field))
            .then(|| fields.get(field))
            .flatten()
            .filter(|rows| rows.len() == 1)
            .and_then(|rows| rows[0]["value"].as_str())
            .filter(|value| !value.is_empty())
    };
    for (field, rows) in &fields {
        if rows.len() != 1 || unavailable.contains(field) {
            unknowns.push(format!("{field} is conflicting or unresolved."));
        }
    }
    let principal = match value("principal_mode") {
        Some("relay") => value("principal_id"),
        Some("self" | "concerning_other") => Some("querent"),
        None if valid
            && !fields.contains_key("principal_mode")
            && !unavailable.contains("principal_mode") =>
        {
            Some("querent")
        }
        _ => None,
    };
    if principal.is_none() {
        unknowns.push(
            "The effective principal is unresolved; no first-house actor is invented.".into(),
        );
    }
    let person_house = |id: &str| {
        principal?;
        if id == "querent" || Some(id) == principal {
            return Some(1);
        }
        let relationship = format!("people.{id}.relationship");
        let capacity = format!("people.{id}.capacity");
        if [&relationship, &capacity].into_iter().any(|field| {
            (fields.contains_key(field.as_str()) || unavailable.contains(field.as_str()))
                && value(field).is_none()
        }) {
            return None;
        }
        match (value(&relationship), value(&capacity)) {
            (Some(left), Some(right)) if left != right => None,
            (Some(relationship), _) | (_, Some(relationship)) => {
                crate::native_evidence::relationship_house(relationship)
            }
            _ => None,
        }
    };
    let label = |id: &str| {
        value(&format!("people.{id}.label"))
            .or_else(|| value(&format!("people.{id}.name")))
            .unwrap_or(id)
            .to_owned()
    };
    let person_sources = |id: Option<&str>| {
        let mut sources = Vec::new();
        if let Some(id) = id {
            for suffix in ["relationship", "capacity", "label", "name"] {
                let field = format!("people.{id}.{suffix}");
                if fields.contains_key(field.as_str()) {
                    sources.push(field);
                }
            }
            if id == "querent" || Some(id) == principal {
                sources.extend(
                    ["principal_mode", "principal_id"]
                        .into_iter()
                        .filter(|field| fields.contains_key(field))
                        .map(str::to_owned),
                );
            }
        }
        sources
    };
    let facet = match consultation.get("facet").filter(|v| !v.is_null()) {
        Some(facet) if !unavailable.contains("facet") => {
            let facet = facet.as_str().context("Declared facet is not a string")?;
            ensure!(
                method_facets(method)?.iter().any(|f| f == facet),
                "Declared facet differs from native method contract"
            );
            Some(facet)
        }
        _ => None,
    };
    let mut rows = Vec::new();
    let mut add = |id: &str,
                   actor: Option<&str>,
                   house: Option<u8>,
                   optional: bool,
                   authority: &str,
                   name: String,
                   sources: Vec<String>| {
        let cusps: Vec<_> = native
            .iter()
            .filter(|f| f.kind == "house" && house.is_some_and(|h| f.label == format!("House {h}")))
            .collect();
        let cusp = (cusps.len() == 1).then(|| cusps[0]);
        let ruler = cusp.and_then(|f| (f.planets.len() == 1).then(|| f.planets[0].as_str()));
        if ruler.is_none() && !optional {
            unknowns.push(format!(
                "{id}: its declared actor capacity or unique native cusp/ruler is unavailable."
            ));
        }
        let positions: Vec<_> = native
            .iter()
            .filter(|f| {
                f.kind == "position" && ruler.is_some_and(|p| f.planets.iter().any(|v| v == p))
            })
            .map(|f| f.id.as_str())
            .collect();
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|field| {
                fields.contains_key(field.as_str()) || (field == "facet" && facet.is_some())
            })
            .collect();
        rows.push(json!({"role_id":id,"actor_id":actor,"label":name,
            "significator_house":house,"ruler_planet":ruler,"optional":optional,
            "authority":authority,"selected_id":null,"cusp_fact_id":cusp.map(|f|&f.id),
            "position_fact_ids":positions,"source_fields":sources}));
    };
    add(
        "principal.person",
        principal,
        principal.map(|_| 1),
        false,
        "calculated_reference",
        principal
            .map(label)
            .unwrap_or_else(|| "Unresolved principal".into()),
        person_sources(principal),
    );
    if matches!(method, "new_job" | "return_to_job" | "job_offer") {
        let worker = value("subject.owner_id");
        let worker_house = worker.and_then(person_house);
        let job_house = worker_house.map(|h| {
            if method == "return_to_job" || h == 10 {
                crate::native_evidence::turn(h, 10)
            } else {
                10
            }
        });
        let mut sources = person_sources(worker);
        sources.push("subject.owner_id".into());
        add(
            "worker.person",
            worker,
            worker_house,
            false,
            "calculated_reference",
            worker
                .map(label)
                .unwrap_or_else(|| "Unresolved applicant".into()),
            sources.clone(),
        );
        add(
            "subject.primary",
            None,
            job_house,
            false,
            "calculated_external_job_reference",
            value("subject.name").unwrap_or("External job").into(),
            sources.clone(),
        );
        add(
            "worker.pocket",
            worker,
            worker_house.map(|h| crate::native_evidence::turn(h, 2)),
            true,
            "conditional_applicant_bank_reference_not_acquisition",
            "The applicant's own money".into(),
            sources.clone(),
        );
        add(
            "job.wages",
            None,
            job_house.map(|h| crate::native_evidence::turn(h, 2)),
            true,
            "conditional_wages_reference_not_acquisition",
            "The job's wages".into(),
            sources,
        );
    } else if method == "vehicle" {
        add(
            "vehicle.undertaking",
            None,
            principal.map(|_| 1),
            false,
            "source_vehicle_role_not_title_ownership",
            value("subject.name").unwrap_or("Vehicle of journey").into(),
            vec!["subject.name".into()],
        );
        add(
            "vehicle.eighth",
            None,
            principal.map(|_| 8),
            true,
            "conditional_material_harm_reference",
            "Eighth-house harm if operative".into(),
            vec![],
        );
    } else if method == "person_description" {
        let future = value("subject.kind") == Some("person_role");
        let person = (!future).then(|| value("subject.owner_id")).flatten();
        let house = if future {
            principal.map(|_| 7)
        } else {
            person.and_then(person_house)
        };
        add(
            "description.person",
            person,
            house,
            false,
            "main_person_ruler_not_natural_cosignificator",
            value("subject.name").unwrap_or("Person described").into(),
            vec!["subject.kind".into(), "subject.owner_id".into()],
        );
    } else if method == "tax" {
        let taxpayer = value("subject.owner_id");
        let house = taxpayer.and_then(person_house);
        add(
            "taxpayer.person",
            taxpayer,
            house,
            false,
            "calculated_reference",
            taxpayer
                .map(label)
                .unwrap_or_else(|| "Unbound taxpayer".into()),
            vec!["subject.owner_id".into()],
        );
        add(
            "taxpayer.money",
            taxpayer,
            house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_reference",
            "Taxpayer's purse".into(),
            vec!["subject.owner_id".into()],
        );
        add(
            "government.authority",
            None,
            principal.map(|_| 10),
            false,
            "actual_government_reference_not_taxpayer_turned_boss",
            "Government".into(),
            vec![],
        );
        add(
            "government.coffers",
            None,
            principal.map(|_| 11),
            false,
            "actual_government_money_reference",
            "Government coffers".into(),
            vec![],
        );
    } else if method == "bet" {
        let player = value("subject.owner_id");
        let house = player.and_then(person_house);
        add(
            "bet.player",
            player,
            house,
            false,
            "calculated_reference",
            player.map(label).unwrap_or_else(|| "Unbound bettor".into()),
            vec!["subject.owner_id".into()],
        );
        add(
            "bet.pocket",
            player,
            house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_reference",
            "Bettor's pocket".into(),
            vec!["subject.owner_id".into()],
        );
        for (id, offset, label) in [
            ("bet.opponent", 7, "Bookmaker or opposing player"),
            ("bet.winnings", 8, "Opponent's money"),
            ("bet.lottery", 11, "Ordinary lottery gift"),
            ("bet.horse", 12, "Owned horse"),
            ("bet.horse_prize", 1, "Profit from owned horse"),
        ] {
            add(
                id,
                None,
                house.map(|h| crate::native_evidence::turn(h, offset)),
                true,
                "conditional_gambling_source_only_actual_question_selects",
                label.into(),
                vec!["subject.owner_id".into(), "context".into()],
            );
        }
    } else if method == "parcel" {
        let sender = value("sender");
        let sender_house = sender.and_then(person_house);
        let outgoing = sender.is_some() && sender == principal;
        let recipient = value("recipient").or_else(|| (!outgoing).then_some(principal).flatten());
        let recipient_house = recipient.and_then(person_house);
        add(
            "parcel.sender",
            sender,
            sender_house,
            false,
            "calculated_reference",
            sender
                .map(label)
                .unwrap_or_else(|| "Unresolved sender".into()),
            vec!["sender".into()],
        );
        add(
            "parcel.message",
            sender,
            sender_house.map(|h| crate::native_evidence::turn(h, 3)),
            false,
            "calculated_sender_third_not_person_contact",
            value("subject.name").unwrap_or("Parcel or letter").into(),
            vec!["sender".into(), "subject.name".into()],
        );
        add(
            "parcel.recipient",
            recipient,
            recipient_house,
            false,
            "calculated_reference",
            recipient
                .map(label)
                .unwrap_or_else(|| "Unbound outgoing recipient".into()),
            vec!["recipient".into()],
        );
        add(
            "parcel.pocket",
            recipient,
            recipient_house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_reference",
            "Recipient's possession".into(),
            vec!["recipient".into()],
        );
        if outgoing && recipient.is_none() {
            for field in fields
                .keys()
                .filter(|f| f.starts_with("people.") && f.ends_with(".relationship"))
            {
                let id = &field[7..field.len() - 13];
                add(
                    &format!("declared.{id}"),
                    Some(id),
                    person_house(id),
                    true,
                    "conditional_declared_person_not_selected_recipient",
                    label(id),
                    person_sources(Some(id)),
                );
                add(
                    &format!("declared.{id}.pocket"),
                    Some(id),
                    person_house(id).map(|h| crate::native_evidence::turn(h, 2)),
                    true,
                    "conditional_possession_of_declared_person",
                    format!("{id}'s pocket"),
                    person_sources(Some(id)),
                );
            }
        }
    } else if matches!(method, "visit" | "trust") {
        let person = if method == "visit" {
            value("visitor")
        } else {
            value("subject.owner_id")
        };
        let field = if method == "visit" {
            "visitor"
        } else {
            "subject.owner_id"
        };
        add(
            "target.person",
            person,
            person.and_then(person_house),
            false,
            "calculated_declared_person_reference",
            person.map(label).unwrap_or_else(|| "Unbound person".into()),
            vec![field.into()],
        );
        if method == "visit" {
            add(
                "arrival.ascendant",
                principal,
                principal.map(|_| 1),
                true,
                "arrival_reference_only_if_expected_or_independently_established",
                "Principal's Ascendant".into(),
                vec!["expected".into()],
            );
            add(
                "arrival.home",
                principal,
                principal.map(|_| 4),
                true,
                "conditional_home_cusp_not_arbitrary_destination",
                "Principal's home".into(),
                vec!["expected".into()],
            );
        }
    } else if method == "information" {
        for (id, house, label) in [
            (
                "claim.information",
                3,
                "Report or rumour, if actual information question",
            ),
            (
                "claim.prediction",
                9,
                "Prediction, if actual prediction question",
            ),
            ("angle.first", 1, "First angle"),
            ("angle.fourth", 4, "Fourth angle"),
            ("angle.seventh", 7, "Seventh angle"),
            ("angle.tenth", 10, "Tenth angle"),
        ] {
            add(
                id,
                None,
                principal.map(|_| house),
                true,
                "conditional_veracity_reference_not_branch_selection",
                label.into(),
                vec!["claim".into()],
            );
        }
    } else if method == "relationship" {
        // No neural branch or name-to-ID guess. A desired partner is seventh;
        // other declared persons remain conditional references for questions
        // about that person's feelings or another person's relationship.
        add(
            "target.partner",
            None,
            principal.map(|_| 7),
            true,
            "conditional_principal_partner_reference_for_actual_romantic_inquiry",
            value("subject.name").unwrap_or("Partner").into(),
            vec!["subject.name".into()],
        );
        for field in fields
            .keys()
            .filter(|field| field.starts_with("people.") && field.ends_with(".relationship"))
        {
            let id = &field[7..field.len() - 13];
            if Some(id) != principal && id != "querent" {
                let house = person_house(id);
                add(
                    &format!("declared.{id}"),
                    Some(id),
                    house,
                    true,
                    "conditional_declared_person_not_a_selected_romantic_role",
                    label(id),
                    person_sources(Some(id)),
                );
                add(
                    &format!("declared.{id}.partner"),
                    None,
                    house.map(|h| crate::native_evidence::turn(h, 7)),
                    true,
                    "conditional_partner_of_declared_person",
                    format!("{id}'s partner if that is the actual inquiry"),
                    person_sources(Some(id)),
                );
            }
        }
        // Natural Sun/Venus require declared applicable roles; do not infer
        // them from names, voices, or an author's private expected answer.
    } else if method == "investment" {
        let owner = value("subject.owner_id");
        let house = owner.and_then(person_house);
        add(
            "investment.owner",
            owner,
            house,
            false,
            "calculated_reference",
            owner
                .map(label)
                .unwrap_or_else(|| "Unresolved investor".into()),
            vec!["subject.owner_id".into()],
        );
        add(
            "subject.primary",
            owner,
            house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_owned_or_potential_asset_reference",
            value("subject.name").unwrap_or("Investment").into(),
            vec!["subject.owner_id".into(), "subject.name".into()],
        );
    } else if method == "money" {
        let recipient = value("subject.owner_id");
        let recipient_house = recipient.and_then(person_house);
        add(
            "money.recipient",
            recipient,
            recipient_house,
            false,
            "calculated_reference",
            recipient
                .map(label)
                .unwrap_or_else(|| "Unresolved recipient".into()),
            vec!["subject.owner_id".into()],
        );
        add(
            "money.pocket",
            recipient,
            recipient_house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_reference",
            "Recipient's pocket".into(),
            vec!["subject.owner_id".into()],
        );
        let source = value("money_source");
        let sender = value("sender");
        let source_house = match source {
            Some("government") => Some(10),
            Some("job") => recipient_house.map(|h| crate::native_evidence::turn(h, 10)),
            Some("customer" | "partner") => {
                recipient_house.map(|h| crate::native_evidence::turn(h, 7))
            }
            Some("relative") => sender.and_then(person_house),
            _ => None,
        };
        add(
            "money.source",
            sender,
            source_house,
            false,
            "calculated_declared_source_reference",
            source.unwrap_or("Unresolved money source").into(),
            vec!["money_source".into(), "sender".into()],
        );
        add(
            "money.incoming",
            sender,
            source_house.map(|h| crate::native_evidence::turn(h, 2)),
            false,
            "calculated_reference",
            "Source's money, not merely the source person".into(),
            vec!["money_source".into(), "sender".into()],
        );
    } else if matches!(method, "property" | "rental" | "business_property") {
        let actor = value("deal_actor");
        let actor_house = actor.and_then(person_house);
        let party = value("deal_party");
        // A free-prose party remains unbound. The generic opposing-party row
        // is explicitly conditional, not a claim that the named party is H7.
        add(
            "deal.actor",
            actor,
            actor_house,
            false,
            "calculated_reference",
            actor
                .map(label)
                .unwrap_or_else(|| "Unresolved contracting actor".into()),
            vec!["deal_actor".into()],
        );
        add(
            "deal.counterparty",
            party,
            party.and_then(person_house).or_else(|| {
                party
                    .is_none()
                    .then(|| actor_house.map(|h| crate::native_evidence::turn(h, 7)))
                    .flatten()
            }),
            false,
            "calculated_reference",
            party
                .map(label)
                .unwrap_or_else(|| "Other contracting party".into()),
            vec!["deal_party".into(), "deal_actor".into()],
        );
        if party.is_some_and(|id| person_house(id).is_none()) {
            add(
                "deal.generic_opponent",
                None,
                actor_house.map(|h| crate::native_evidence::turn(h, 7)),
                true,
                "conditional_generic_party_only_if_actual_words_establish_unrelated_party",
                "Generic opposing party if applicable".into(),
                vec!["deal_actor".into()],
            );
        }
        for (id, offset, name) in [
            ("subject.primary", 4, "Property"),
            (
                if method == "business_property" {
                    "business.profit"
                } else {
                    "property.price"
                },
                10,
                if method == "business_property" {
                    "Profit from working premises"
                } else {
                    "Property price"
                },
            ),
            (
                if method == "business_property" {
                    "business.residential_profit_reference"
                } else {
                    "property.profit"
                },
                5,
                if method == "business_property" {
                    "Residential letting profit only if actually relevant; not business profit"
                } else {
                    "Profit from property"
                },
            ),
            ("property.neighbours", 6, "Property's neighbours"),
        ]
        .into_iter()
        .filter(|(_, offset, _)| method != "business_property" || [4, 10].contains(offset))
        {
            add(
                id,
                None,
                actor_house.map(|h| crate::native_evidence::turn(h, offset)),
                id != "subject.primary",
                "conditional_property_reference_use_only_for_actual_concern",
                name.into(),
                vec!["deal_actor".into()],
            );
        }
        let beneficiary = value("deal_beneficiary");
        if beneficiary.is_some() {
            add(
                "deal.pocket",
                beneficiary,
                beneficiary
                    .and_then(person_house)
                    .map(|h| crate::native_evidence::turn(h, 2)),
                true,
                "conditional_financial_beneficiary_not_title_owner",
                "Beneficiary's pocket".into(),
                vec!["deal_beneficiary".into()],
            );
        }
    } else {
        let capacity = value("deal_capacity");
        ensure!(
            capacity.is_none_or(|value| matches!(value, "buy" | "sell" | "rent")),
            "Declared deal capacity differs from native input contract"
        );
        let actor_field = if fields.contains_key("deal_actor") || unavailable.contains("deal_actor")
        {
            "deal_actor"
        } else {
            "seller"
        };
        // A known legacy seller is the contracting actor only in a sale. It
        // never supplies title ownership or the recipient of proceeds.
        let actor = value("deal_actor").or_else(|| {
            (capacity == Some("sell")
                && !fields.contains_key("deal_actor")
                && !unavailable.contains("deal_actor"))
            .then(|| value("seller"))
            .flatten()
        });
        let actor_house = actor.and_then(person_house);
        let mut actor_sources = person_sources(actor);
        actor_sources.extend(
            [actor_field, "deal_capacity"]
                .into_iter()
                .map(str::to_owned),
        );
        add(
            "deal.actor",
            actor,
            actor_house,
            false,
            "calculated_reference",
            actor
                .map(label)
                .unwrap_or_else(|| "Unresolved deal actor".into()),
            actor_sources,
        );
        let same_actor = |id: &str| {
            actor.is_some_and(|actor| {
                actor == id
                    || (principal.is_some()
                        && [actor, id]
                            .into_iter()
                            .all(|id| id == "querent" || Some(id) == principal))
            })
        };
        let explicit_party =
            fields.contains_key("deal_party") || unavailable.contains("deal_party");
        let seller_party = (capacity == Some("buy") && !explicit_party)
            .then(|| value("seller"))
            .flatten()
            .filter(|id| !same_actor(id));
        let party = value("deal_party")
            .or(seller_party)
            .filter(|id| !same_actor(id));
        let anonymous = party.is_none()
            && !explicit_party
            && seller_party.is_none()
            && !fields.contains_key("seller")
            && !unavailable.contains("seller");
        // In a sell, seller may be the actor rather than the other party.
        let anonymous = anonymous
            || (party.is_none()
                && !explicit_party
                && capacity == Some("sell")
                && !unavailable.contains("seller"));
        let counterparty_house = party.and_then(person_house).or_else(|| {
            anonymous
                .then(|| actor_house.map(|h| crate::native_evidence::turn(h, 7)))
                .flatten()
        });
        let mut party_sources = person_sources(party);
        party_sources.push(
            if seller_party.is_some() {
                "seller"
            } else {
                "deal_party"
            }
            .into(),
        );
        party_sources.extend(
            [actor_field, "deal_capacity"]
                .into_iter()
                .map(str::to_owned),
        );
        add(
            "deal.counterparty",
            party,
            counterparty_house,
            false,
            "calculated_reference",
            party
                .map(label)
                .unwrap_or_else(|| "The other contracting party".into()),
            party_sources.clone(),
        );
        let completion = matches!(facet, Some("event" | "timing"));
        if !completion {
            let owner = match capacity {
                Some("buy" | "rent") => actor,
                Some("sell") => value("subject.owner_id"),
                _ => None,
            };
            let mut sources = person_sources(owner);
            sources.push(
                if matches!(capacity, Some("buy" | "rent")) {
                    "deal_actor"
                } else {
                    "subject.owner_id"
                }
                .into(),
            );
            sources.push("facet".into());
            sources.push("deal_capacity".into());
            add(
                "subject.primary",
                owner,
                owner
                    .and_then(person_house)
                    .map(|h| crate::native_evidence::turn(h, 2)),
                facet.is_none(),
                if facet.is_none() {
                    "conditional_object_reference_unselected_facet"
                } else {
                    "calculated_object_reference"
                },
                value("subject.name").unwrap_or("The movable object").into(),
                sources,
            );
            if facet.is_none() || facet == Some("profit") {
                let beneficiary = value("deal_beneficiary");
                let mut beneficiary_sources = person_sources(beneficiary);
                beneficiary_sources.push("deal_beneficiary".into());
                beneficiary_sources.push("facet".into());
                let sale = capacity == Some("sell");
                let authority = if facet.is_none() {
                    "conditional_sale_profit_reference_unselected_facet"
                } else {
                    "calculated_sale_profit_reference"
                };
                add(
                    "deal.incoming_money",
                    party,
                    sale.then(|| counterparty_house.map(|h| crate::native_evidence::turn(h, 2)))
                        .flatten(),
                    facet.is_none(),
                    authority,
                    "The buyers' money".into(),
                    party_sources,
                );
                add(
                    "deal.pocket",
                    beneficiary,
                    sale.then(|| {
                        beneficiary
                            .and_then(person_house)
                            .map(|h| crate::native_evidence::turn(h, 2))
                    })
                    .flatten(),
                    facet.is_none(),
                    authority,
                    "The beneficiary's pocket".into(),
                    beneficiary_sources,
                );
            }
        }
    }
    if method == "movable_deal" && facet.is_none() {
        unknowns.push("No facet is declared: object and sale-profit rows are conditional references, not selected roles or additional required inputs.".into());
    }
    if method == "vehicle" {
        for planet in ["Sun", "Mars", "Saturn"] {
            rows.push(json!({"role_id":format!("vehicle.reference.{planet}"),"actor_id":null,"label":"Conditional natural danger/solar reference, not a causal verdict",
                "significator_house":null,"ruler_planet":planet,"optional":true,"authority":"source_governed_conditional_body_reference",
                "selected_id":null,"cusp_fact_id":null,"position_fact_ids":native.iter().filter(|f|f.kind=="position" && f.planets.iter().any(|p|p==planet)).map(|f|f.id.clone()).collect::<Vec<_>>(),"source_fields":[]}));
        }
    }
    if method == "relationship" {
        for (planet, role) in [("Sun", "natural.sun"), ("Venus", "natural.venus")] {
            let claimed = rows.iter().any(|r| {
                matches!(
                    r["role_id"].as_str(),
                    Some("principal.person" | "target.partner")
                ) && r["ruler_planet"] == planet
            });
            if !claimed {
                rows.push(json!({"role_id":role,"actor_id":null,"label":"Conditional natural reference: actual question must establish applicable roles; never assign from names or between same-sex partners",
                "significator_house":null,"ruler_planet":planet,"optional":true,"authority":"conditional_original_question_natural_role_not_selected_actor",
                "selected_id":null,"cusp_fact_id":null,"position_fact_ids":native.iter().filter(|f|f.kind=="position" && f.planets.iter().any(|p|p==planet)).map(|f|f.id.clone()).collect::<Vec<_>>(),"source_fields":[]}));
            }
        }
    }
    let mut aliases: Vec<_> = rows.iter().filter(|r|r["ruler_planet"].is_string()).map(|r|
        json!({"role_id":r["role_id"],"actor_id":r["actor_id"],"label":r["label"],
            "significator_house":r["significator_house"],"planet":r["ruler_planet"],"optional":r["optional"]})).collect();
    let moon_claims: Vec<_> = aliases
        .iter()
        .filter(|r| r["planet"] == "Moon" && r["optional"] == false)
        .cloned()
        .collect();
    let conditional_moon_claims: Vec<_> = aliases
        .iter()
        .filter(|r| r["planet"] == "Moon" && r["optional"] == true)
        .map(|r| r["role_id"].clone())
        .collect();
    let moon_available = principal.is_some()
        && moon_claims.is_empty()
        && rows
            .iter()
            .filter(|r| r["optional"] == false)
            .all(|r| r["ruler_planet"].is_string())
        && (!matches!(
            method,
            "new_job" | "return_to_job" | "job_offer" | "money" | "investment"
        ) || value("subject.owner_id") == principal);
    if moon_available {
        aliases.push(json!({"role_id":"principal.moon","actor_id":principal,
            "label":if method == "new_job" {"The principal as worker"} else {"The principal's contextual Moon"},
            "significator_house":null,"planet":"Moon","optional":true}));
    }
    let matching = |planet: &str| {
        aliases
            .iter()
            .filter(|r| r["planet"] == planet)
            .collect::<Vec<_>>()
    };
    let receptions: Vec<_> = native
        .iter()
        .filter_map(|f| {
            let edge = f.reception.as_ref().filter(|_| f.kind == "reception")?;
            let guest = matching(&edge.guest_planet);
            let host = matching(&edge.host_planet);
            (!guest.is_empty() && !host.is_empty()).then(|| {
                json!({"fact_id":f.id,
            "guest_planet":edge.guest_planet,"guest_roles":guest,
            "host_planet":edge.host_planet,"host_roles":host,"qualities":edge.qualities})
            })
        })
        .collect();
    let events: Vec<_> = native.iter().filter_map(|f| {
        let event = f.event.as_ref().filter(|_|f.kind == "event" && f.planets.len() == 2)?;
        f.planets.iter().any(|p|!matching(p).is_empty()).then(||json!({"fact_id":f.id,
            "endpoints":f.planets.iter().map(|p|json!({"planet":p,"roles":matching(p)})).collect::<Vec<_>>(),"candidate":event}))
    }).collect();
    Ok(
        json!({"version":"family-role-reference-1","source_identity":crate::google::digest(serde_json::to_vec(consultation)?),
        "source_scope":"declared_component_consultation_not_an_extraction_journey",
        "principal_basis":if principal.is_none(){"unresolved"}else if fields.contains_key("principal_mode"){"accepted_principal_mode"}else{"default_querent_no_relay_asserted"},
        "role_rows":rows,"alias_authority":"calculated_role_references",
        "aliases":aliases,"moon":{"available_as_contextual_reference":moon_available,"house_claims":moon_claims,
            "conditional_house_claims_if_optional_role_used":conditional_moon_claims},
        "directed_reception":receptions,"event_endpoints":events,"unknowns":unknowns}),
    )
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

/// Source-reviewed short procedures, not a replacement for the full primary
/// source retained by reference authors and independent reviewers.
pub fn concise_reader_source(method: &str) -> Result<&'static str> {
    match method {
        "contact" => Ok(CONCISE_CONTACT_PROCEDURE),
        "new_job" => Ok(CONCISE_NEW_JOB_PROCEDURE),
        "movable_deal" => Ok(CONCISE_MOVABLE_DEAL_PROCEDURE),
        "relationship" => Ok(CONCISE_RELATIONSHIP_PROCEDURE),
        "money" => Ok(CONCISE_MONEY_PROCEDURE),
        "property" => Ok(CONCISE_PROPERTY_PROCEDURE),
        "rental" => Ok(CONCISE_RENTAL_PROCEDURE),
        "return_to_job" => Ok(CONCISE_RETURN_JOB_PROCEDURE),
        "job_offer" => Ok(CONCISE_JOB_OFFER_PROCEDURE),
        "investment" => Ok(CONCISE_INVESTMENT_PROCEDURE),
        "business_property" => Ok(CONCISE_BUSINESS_PROPERTY_PROCEDURE),
        "parcel" => Ok(CONCISE_PARCEL_PROCEDURE),
        "visit" => Ok(CONCISE_VISIT_PROCEDURE),
        "trust" => Ok(CONCISE_TRUST_PROCEDURE),
        "information" => Ok(CONCISE_INFORMATION_PROCEDURE),
        "vehicle" => Ok(CONCISE_VEHICLE_PROCEDURE),
        "person_description" => Ok(CONCISE_DESCRIPTION_PROCEDURE),
        "tax" => Ok(CONCISE_TAX_PROCEDURE),
        "bet" => Ok(CONCISE_BET_PROCEDURE),
        _ => anyhow::bail!("No reviewed concise procedure for {method}"),
    }
}

const CONCISE_CONTACT_PROCEDURE: &str = "CONTACT PROCEDURE — Frawley printed pp. 19–32, 44, 57, 65, 71–77, 84–97, 127–136, 165–166.
Preserve the original contact question, principal and stated circumstances. Use the supplied role table with its authority and unknowns. A significator's assigned house differs from its body's occupied house. Retain every alias when rulers coincide. The Moon normally cosignifies the querent unless claimed as the quesited's main ruler. Turning counts the person's house as 1.

For a call or personal contact, connect the actual person's ruler with Lord 1 or the available contextual Moon. Mum is 10; an unspecified other is 7, but a declared relationship or operative capacity takes its appropriate house. The sender's turned third is an optional message route, not a mandatory substitute for the person. A parcel asks about the object and its arrival instead. Application direction does not say who calls or travels. WILL with a deadline asks occurrence within that bound, not automatically WHEN.

Separate capacity, inclination and occasion. Weigh supplied essential condition, occupied-house capacity, solar condition and actual motion; retrograde may fit returning rather than automatically forbidding it. Reception A→B means A regards B. Domicile/exaltation are major; triplicity moderate; term/face minor. Preserve all positive AND negative qualities, including mixed reception. Do not infer reciprocity or treat chart inclination as independently reported private feelings.

Use actual candidate order and each body's motion, not endpoint 1 or an assumed average speed. Examine main connections and supported translation/collection or prevention. An incidental contact is not automatically prohibition or an identified intermediary. Usually consider the next aspect, or next two for translation; do not chase a long chain to obtain the desired answer. Ease of aspect does not itself make the result good. A current void Moon supports the actual default, but stronger main testimony can override it.

After-ingress candidates require a meaningful change of circumstance/reception; do not carry current reception unchanged. Frawley limits this to roughly three or four degrees into the next sign. For an explicitly bounded question, the relevant planet's present-sign end can mark the stated timeframe (p. 136). Astronomical search hours are never automatically civil timing. To time an established event, use the applying body's travel to moving perfection, then contextual or calibrated units. Missing candidates or uncomputed techniques remain unknown, not proof of absence.

Return only status, answer, analysis, evidence, unresolved. Give the direct useful judgment first, then concise material reasons with native eIDs and source pages. Retain substantial contrary evidence and calculation limits; do not invent an exact date, motive or future event.
";

const CONCISE_NEW_JOB_PROCEDURE: &str = "NEW JOB PROCEDURE — Frawley printed pp. 19–32, 44, 57, 65–77, 84–97, 127–136, 222–224.
Answer the actual new-job question and preserve its applicant, vacancy, application/no-offer context and deadline. A reported application or intermediary is not an offer. Use only declared role references; assigned houses differ from bodies' occupied houses. A genuine relayed principal takes first; someone merely asked about retains their own relationship house. The applicant is their actual ruler. For the principal's own application, Lord 1 and the available Moon show the applicant, with Moon as worker, not romantic feelings. A main house ruler has first claim on Moon. Do not transfer the principal's Moon to another worker. The external job is radical tenth, except a tenth-house applicant's job uses their turned tenth. Their existing career/boss is a different question. Wages are the job's second; an aspect to wages does not establish acquisition.

Separate capability, inclination and occasion. Attribute every condition/motion to its actual planet: direct applicant/retrograde job does not make the applicant retrograde. Weigh supplied essential condition, occupied-house capacity, solar condition and actual motion. Weakness can obstruct hiring but does not independently report qualifications, solvency or job requirements. Retrograde needs relevant return context, not an automatic positive label.

Reception A→B means A regards B. Preserve all positive AND negative qualities. Domicile/exaltation are major, triplicity moderate, term/face minor. Applicant→job indicates interest; applicant→their own money can indicate financial need. In an ordinary job even minor job→applicant reception matters, but does not erase detriment/fall or supply an offer. Exaltation describes overrating. Do not invent reciprocal interest or a vacancy from reception alone.

Acquisition needs the applicant and job joining, or supported translation/collection. Trine can ease acquisition; opposition can mean acquisition with regret. Lord 1 in tenth mainly shows wanting the job; Lord 10 inside first is stronger than merely on its cusp. Tenth-house fortunes, nodes or strong planets need not decide acquisition; use them only if supplied. Do not substitute wages, an unrelated planet or two applicant significators touching. The Sun substitutes only for a royal appointment. Consider actual next occasions and source-supported rivals/intermediaries; an incidental contact is not automatically prohibition or an invented hiring step. Usually the next aspect, or next two for translation, suffices. A void Moon is contrary to the required change but can be outweighed.

After-ingress candidates need changed circumstances/reception; do not carry old reception forward or chase a long chain. Use the applying body's travel to moving perfection for contextual timing, not astronomical hours. The relevant present-sign end can mark an explicit bounded timeframe (p. 136); otherwise do not invent an immediate or near-term offer. Missing computations remain unknown.

Return only status, answer, analysis, evidence, unresolved. Give a useful direct, proportionate judgment, cite native eIDs/source pages, and retain substantial contrary testimony and material limits.
";

const CONCISE_MOVABLE_DEAL_PROCEDURE: &str = "MOVABLE DEAL PROCEDURE — Frawley printed pp. 19–32, 44, 57, 65–77, 84–97, 127–136, 156–161, 167–172, 177.
Preserve the actual action and concern: agreement, object quality or sale profit. WILL with a deadline is occurrence within that bound, not automatically WHEN. Actor, title owner, seller/agent, other contracting party and financial beneficiary are distinct; use their declared identities and operative capacities. A genuine relay principal takes first. Assigned houses differ from occupied houses; retain coincident aliases. Conditional role rows are references only for the corresponding question, not required additional facts. Never use a missing recipient as an owner/seller default.

For whether the deal completes, judge the actual parties joining: first/seventh ordinarily; a named relative uses their own house. Do not substitute goods touching a buyer or demand unused title/asset/price data. Weigh willingness and ability alongside an applying connection, supported translation/collection and actual prevention. Negative reception matters; lack of reception is not automatically refusal. A non-property opposition can complete with regret. An incidental prior contact is not automatically prohibition or an invented agent; an intermediary needs a supported connecting role. Do not apply the special property opposition exception to a movable sale.

For SHOULD I buy this object, the object is the buyer's potential second-house possession, not the seller's ruler or an assertion of legal title. Judge supplied present condition and material upcoming changes. Debility can describe the object appropriately; exaltation can overrate it. Current quality does not require a future completion aspect. Seller trustworthiness is a separate concern, not the object's feelings. Keep a sell-side owner's goods distinct from their agent and the buyer.

For sale profit, identify whose pocket receives proceeds. Incoming money is the actual other party's second; the beneficiary's pocket is their second. Amount/quality is assessed from the money's condition; arrival needs a supported connection to its recipient or pocket, with an available contextual Moon only for the principal. Good goods or the seller's dignity alone does not establish profit. Distinguish a good amount from whether it arrives. Shared rulers keep both roles rather than silently merging them.

Reception A→B means A regards B. Retain all positive AND negative qualities: domicile/exaltation major, triplicity moderate, term/face minor. Weigh role-bound condition, house capacity, solar state and motion; do not infer reciprocity or private motives. Event endpoints are storage order, not the applying body. After ingress requires relevant changed circumstances/reception, usually only about three or four degrees into the next sign. Use actual travel to moving perfection and justified contextual units; astronomical hours are not civil timing. A relevant present-sign end can mark an explicit timeframe (p. 136). Missing techniques remain unknown, not absence. No general exact inventory-sale count is supplied; do not silently reframe a count question.

Return only status, answer, analysis, evidence, unresolved. Give the useful decision first with material native eIDs/source pages, contrary testimony and honest limits.
";

const CONCISE_RELATIONSHIP_PROCEDURE: &str = "RELATIONSHIP PROCEDURE — Frawley printed pp. 19–32, 44–83, 84–101, 120–123, 127–140, 191–201.
Preserve the actual person, question and baseline: hopes, arranged wedding and existing relationship have different defaults. The principal is first with available Moon; the partner specifically asked about is seventh, including an unnamed future partner or a friend considered as partner. A question specifically about a neighbour's feelings can use third. Declared-person rows are conditional alternatives, not extra lovers. A concerning-other inquiry does not turn that person into a relay principal. Turn an actual other person's partner from that person's house. Assigned houses differ from occupied houses; preserve shared rulers and house priority over Moon.

Use man/woman roles explicitly established in the original question or declared consultation, not a mandatory typed gender field. Only applicable roles permit Sun/Venus as natural cosignificators; house rulers have first claim, with no substitute Mars, arbitrary allocation between same-sex partners, or inference from names. Lord 1/7 concerns the person; Moon the principal's feelings; applicable Sun/Venus attraction. If these roles are unavailable, state that this testimony is unavailable without inventing it.

Reception A→B is A's regard for B, never B's response. Preserve positive AND negative qualities: domicile/exaltation major, triplicity moderate, term/face minor. Exaltation can exaggerate and change. Different significators can disagree; neither minor reception nor an easy aspect proves love. Current feelings need no new event aspect. Weigh role-bound condition, solar state, occupied-house capacity and motion contextually; vulnerability from loving the other is not a mechanical refusal.

Formation requires relevant reception and an occasion between actual significators; a cusp or Part of Marriage contact is not a substitute. Use native order, supported translation/collection and prevention; do not invent rivals. Commitment timing is not a booked ceremony date. Meeting place uses contextual who-goes-to-whom and the receiving significator's occupied house, not the applying planet's identity. Description uses main partner ruler; partner wealth is their second, work their tenth. Such ancillary roles require their own native evidence.

Existing arrangements/relationships continue unless actual ending testimony/action outweighs that default. A square describes difficulty reaching the event, not necessarily later unhappiness; opposition can bring regret. Judge durability through receptions and supplied Part of Marriage/dispositor, not an invented calculation. For 'ever', age/circumstances materially change the default; fertility, relevant placements and Sun/Moon testimony can matter. Only after occurrence is supported consider the source's broader timing latitude; do not leap several signs, stations or solar conjunctions.

Reception alone does not establish an affair. Close conjunction still needs contextual identity and discretion; no unprompted sexuality allegation. Event hours are astronomical, not civil dates; missing techniques remain unknown. Return status, answer, analysis, evidence, unresolved: useful judgment first, material eIDs/pages, contrary testimony and limits.
";

const CONCISE_MONEY_PROCEDURE: &str = "MONEY PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 156–163.
First distinguish amount/quality, arrival, timing, repayment, gift/grant, legacy and one's existing investments. Preserve payer, recipient, beneficiary, owed entitlement versus discretionary favour, and actual principal. A genuine relay principal takes first; concerning another person does not. Assigned houses differ from occupied houses; retain shared roles. Unknown sender capacity remains unknown.

Money belongs to its actual source until received. Employee wages are second from the job, normally eleventh; a client's money is second from seventh, eighth, even for a corporate client. Do not treat a freelancer as an employee. A relative's repayment/gift or specific legacy uses that person's actual house and turned second, not automatically eighth. Spouse/business partner money is eighth. Government is tenth and its money eleventh. For another recipient turn their client/job/pocket roles as actually appropriate; do not turn the government away from its actual authority merely because the recipient is related. The recipient's second is their pocket. A donor's person and their money are distinct.

For amount/quality, judge the money ruler's essential and accidental condition and supplied benefits/afflictions to its house; no arrival aspect is universally required. For arrival, require a supported connection from money to recipient, their available contextual Moon or pocket. An aspect between debtor and recipient alone may show agreement, not payment. Money strong in its own pocket may remain there. Weigh actual application, supported translation/collection, and material impediments; don't invent intervening claimants. Identify a real claimant by a supplied role, not merely an unassigned planet.

Reception A→B means A regards B. Preserve all positive AND negative qualities, with domicile/exaltation major, triplicity moderate, term/face minor. For an owed government entitlement, personal favour is not obligatory; discretionary awards require relevant awarding-body inclination, negative reception being adverse. A legacy also concerns donor attitude and competing claims. Do not conflate good amount, willingness and delivery.

Owned stocks/savings remain one's second-house money; evaluate present/future condition and relevant sign change for an actual switch. Specific commodity rulers/Parts require supplied calculations. Gambling and lotteries have their distinct source recipes; do not import recreational fifth indiscriminately. Astronomical hours are not civil timing. Establish arrival before timing with actual applying travel and contextual units; a relevant present-sign end can bound an explicit timeframe. Missing calculations remain unknown, not proof of nonpayment.

Return only status, answer, analysis, evidence, unresolved. Answer the actual financial question directly, with material eIDs/pages, substantial contrary evidence and honest calculation limits; no invented sum, deadline or private motive.
";

const CONCISE_PROPERTY_PROCEDURE: &str = "PROPERTY PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 167–171.
Distinguish deal completion, condition/price/fit and investment profit. Preserve actual contracting actor, title owner, other party and beneficiary; agents are not automatically parties or owners. Genuine relay takes first; concerning a relative preserves that person's actual house. Ordinary parties are first/seventh; a specifically related party uses its appropriate house. A free-prose unknown actor remains unbound, not an invented identity. Conditional role rows are references, not compulsory extra facts. Turn another actual actor's context explicitly; do not confuse assigned houses with bodies' occupied houses.

Completion chiefly needs the contracting parties joining, supported translation/collection or equivalent sound testimony. Property or price touching the seller is minor and cannot replace the deal. Judge willingness, capacity and actual intervening events: buyer's afflicted money can prevent completion, a supported other buyer can intervene, and an actual translating/collecting planet can represent an agent. Do not require an agent biography or call every prior aspect prohibition. Unlike the ordinary regret rule, opposition can complete a property sale without implying regret. Negative reception matters; no reception is not automatically refusal. Being on a cusp can signify leverage; being inside can signify being controlled.

Property is fourth; price tenth. Their supplied condition compares value and cost, relative to actual purpose. Domicile suggests soundness; exaltation can mask cosmetic exaggeration; debility indicates contextual defects, whereas a peregrine mobile home can fit its nature. Elemental associations suggest checks, not independently known defects: air roof/windows, water plumbing/damp, fire heating/walls, earth foundations. Consider supplied afflictions/occupants of fourth. A fixed price may resist bargaining; near its sign end change can become possible. Do not invent future dignity or market movement when uncomputed.

Property profit is second from fourth, fifth, distinct from home quality and the beneficiary's pocket. Property neighbours are third from fourth, sixth, only where relevant. Preserve all positive AND negative directed receptions: A→B means A regards B; an inanimate role is contextual fit, not literal feeling. A buyer's exaltation of the property can overvalue it.

Quality/available investment does not require a new completion aspect. Working premises use the separate business-property recipe: tenth profit and fourth outcome, not ordinary tenth price. Rental opposition concerns an ongoing regrettable relation, unlike outright sale. Use the applying body's actual travel and justified contextual units for supported timing; astronomical hours are not civil dates, and omitted/uncomputed testimony remains unknown.

Return only status, answer, analysis, evidence, unresolved: direct useful judgment, material eIDs/pages, contrary testimony and limits, without an invented transaction or price.
";

const CONCISE_RENTAL_PROCEDURE: &str = "RENTAL PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 167–171.
Preserve whether the actor lets out or takes a dwelling, actual title ownership, opposing party, financial beneficiary and existing/proposed tenancy. Do not conflate tenant, landlord, agent and owner. Genuine relay takes first; concerning another person retains their actual relationship house. Modern landlord/tenant agreement is a deal between first/seventh, not an automatic sixth-house servant relation. A specifically related party uses its actual house. Unknown canonical party remains unknown; conditional generic opposing-party rows do not assert its identity. Assigned houses differ from occupied houses; retain shared ruler aliases and unavailable Moon claims.

For agreement completion, use the actual parties joining, supported translation/collection and relevant impediments. Property touching a party or price touching a seller is minor, not a substitute for agreement. Judge willingness and capacity alongside occasion. Negative reception matters; absent reception alone need not refuse the deal. Unlike outright property sale, opposition promises regret in the continuing tenant/landlord relationship. Do not invent an intermediary or label every earlier contact prohibition.

For taking an available dwelling, compare fourth-house property condition, tenth-house price and actual needs; no future agreement aspect is required merely to judge condition or suitability. Exaltation may exaggerate quality or price; debility suggests contextual defects rather than independently verified damage. Property profit is fifth, second from fourth; preserve whose pocket benefits. Property neighbours are sixth, third from fourth, when material. Existing tenancy merit is not forced into a new-contract acquisition test.

For 'should I let to these people?', judge actual tenant condition/trustworthiness and whether their ruler harms property or its profit. A debilitated benefic may look better than its condition warrants. Do not make all unknown tenants evil or assign them sixth. For sell versus let, seventh's cardinal/fixed sign can favour sale, double-bodied letting; weigh harm/benefit and property/price changes. A future improvement can support letting temporarily then selling only if actually supplied, not invented.

A place to work on/in uses the business-property recipe, not ordinary residential price/profit allocation: tenth prospective profit, fourth outcome; the premises are assumed available, so no obligatory first/seventh acquisition aspect. Keep this conditional if actual use is undeclared.

Reception A→B is A's regard for B; preserve every positive AND negative quality and distinguish inclination, capacity and occurrence. Use actual event/motion order and sound prevention reasoning. Astronomical hours are not calendar timing. A relevant sign end may bound an explicit timeframe; unknown calculations are not absent events. Return status, answer, analysis, evidence, unresolved: useful judgment first with eIDs/pages, contrary evidence and honest limits.
";

const CONCISE_RETURN_JOB_PROCEDURE: &str = "RETURN TO JOB PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 222–226.
Preserve that this is return to the same former job, not a speculative new application or keeping a current post. Preserve who worked there, departure and actually reported return context. Genuine relay principal takes first; concerning another worker retains their actual relationship house. The worker has that person's ruler; their former job/career uses their turned tenth, not the external-new-job radical tenth exception. Wages are second from that job and the worker's own pocket their second. Assigned houses differ from occupied houses. Keep coincident aliases and let a house ruler claim Moon before any contextual cosignification. The principal's Moon is not transferred to another worker.

A relevant retrograde or supplied recent direction reversal can describe return, not an automatic defect or automatic successful reinstatement. Do not claim recent retrogradation or a future station when not calculated. Actual re-entry of worker ruler into the former job's house, its cusp sign or the other sign ruled by the job ruler is strong return testimony because the prior employment supplies the concrete context. Do not confuse wanting a new job by being in tenth with leaving and actually returning. Native current positions alone do not prove future re-entry.

Job ruler inside first or an applying connection between worker and job can support return. Being exactly on a cusp is distinct from being inside. Examine supported translation/collection, actual candidate order, relevant rivals/intermediaries and prevention without inventing a person or reading every incidental contact as a prohibition. A wage-to-worker connection is payment, not reinstatement. Separate willingness, capacity and occurrence.

Reception A→B means A regards B. Keep every positive AND negative quality: domicile/exaltation major, triplicity moderate, term/face minor. Applicant interest can exaggerate; even minor employer interest can matter, but negative regard and actual ability remain material. Attribute each planet's dignity, house capacity, solar state and motion to its actual role. Avoid unsupported claims about employer finances or the worker's qualifications.

A void Moon supports the actual default of no change unless stronger return testimony outweighs it. Timing comes from an established return and actual applying travel or supplied re-entry, with justified contextual units. Astronomical search hours are not civil dates. Relevant sign-end evidence may bound an explicit timeframe; after-ingress circumstances and receptions cannot be assumed unchanged. Missing stations, ingresses, antiscia or precise order remain unknown, not proof of no possible return.

Return only status, answer, analysis, evidence, unresolved. Answer reinstatement directly, cite material eIDs/pages, retain contrary testimony and calculation limits; no invented offer or exact date.
";

const CONCISE_JOB_OFFER_PROCEDURE: &str = "AVAILABLE JOB PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 222–230.
Preserve that the job is actually offered/available and the worker's priorities. 'Should I take it?' differs from 'Will I be offered it?': do not require an acquisition aspect merely to assess an available job. Genuine relay principal takes first; another worker keeps their actual house. For an external offered job use radical tenth, with the tenth-house-worker exception requiring their turned tenth. Their current career/job is their turned tenth. Do not merge an explicitly compared present and future job. Keep worker, job, wages (job's second) and worker's pocket distinct, and do not transfer the principal's Moon to another worker.

Judge the job ruler's essential quality, accidental ability, solar condition and actual motion. A good job can be constrained by external affliction; normal satisfactory work need not be a market leader or show exceptional employer affection. Consider supplied tenth-house occupants/benefits and relevant coworker conflicts. Wages concern their ruler's condition and connection to worker/pocket; wages alone do not prove an unmade offer. A supplied opposition to the job matters even though no aspect is required: regret or short tenure may result.

Reception A→B means A regards B; keep every positive AND negative quality. Exaltation can overrate a job without proving its quality. Ordinary employers need not adore their staff; minor interest can be favourable. Distinguish pay-only priorities from congenial work. Specific descriptive debility may appropriately describe the proposed work rather than harm it; do not ignore unrelated weakness.

The book's hypothetical placement of worker or available emotional Moon just inside the job cusp can assess fit. This is a separate recalculation at that location, not the worker's actual position or a guaranteed angularity bonus. If those hypothetical dignities/solar contacts are not supplied, report that limit instead of inventing them.

For explicit current-versus-next job, the present tenth-cusp sign and next sign may provide references, but actual tenure/circumstances can change which sign represents which job. Do not silently pick a next job from a private expected branch or calculate future dignity mentally as fact. Supplied worker sign change can represent the proposed job change; evaluate its actual new condition. Vocational choice needs actual options and appropriate house/natural roles, not generic tenth for every possible career.

Use native event order and contextual prevention; do not fabricate coworkers, collapse boss/job collisions or infer a company death from weak dignity alone. Astronomical hours are not civil timing. Return status, answer, analysis, evidence, unresolved: decision first with eIDs/pages, contrary evidence, explicit uncomputed hypothetical/alternative-role limits and no invented future offer.
";

const CONCISE_INVESTMENT_PROCEDURE: &str = "INVESTMENT PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 156–163.
Preserve the actual investor, holding/commodity, proposed action or alternatives, horizon and whether the question concerns existing value or an unmade switch. Genuine relay principal takes first; concerning another investor retains that person's actual relationship house. Owned stocks, bonds and shares are one's own second-house money in another form, not automatically eighth-house other people's money. A proposed asset can be treated as potential second-house possession without asserting completed title. Use the actual investor's turned second and preserve shared rulers. Do not give another investor the principal's Moon.

Judge the asset/money ruler's essential and accidental condition, supplied benefits/afflictions to its house and actual actions. Present quality does not require a future arrival aspect. Nor does a forecast of an investment's value imply payment into a bank account. Keep acquisition, dividend receipt, amount, profit/loss and quality distinct; a specific payer question needs the appropriate payment recipe instead.

Sign change is often central to a proposed switch: where the money ruler goes can show its future if the change is made. Compare actual present/future dignity and fertility; do not invent next-sign calculations or silently recast a future chart. A supplied node contact may favour/harm a change, but a preceding adverse contact in the current sign can describe staying put if a meaningful change provides the stronger context. Be flexible only with source-supported actual testimony, not to manufacture a desired answer.

Specific commodity natural rulers or Arabian Parts can supplement the actual investment context when their identity/calculation is justified and supplied. A general Sun–Saturn contact is not a universal gold-market forecast; Mercury does not automatically signify every stock. Unknown commodity assignments, Parts, future condition and stations remain unknown. Preserve existing asset role even when a secondary natural ruler is unavailable.

Reception A→B means A regards B. Preserve every positive AND negative quality without inferring reciprocity, market sentiment or independently known corporate facts. Weigh supplied capacity, solar condition, motion and relevant contacts contextually; essential debility can describe the object appropriately. An event endpoint's order is not an applier arrow. Do not chase a long aspect sequence until a gain appears or declare absence from an incomplete bounded search.

Give a proportionate comparison or quality judgment rather than an invented exact price, return percentage or civil deadline. If timing is truly requested, establish the event and use actual travel with justified contextual units; astronomical hours are not calendar time. Return only status, answer, analysis, evidence, unresolved, citing material native eIDs/pages, contrary testimony and limits.
";

const CONCISE_BUSINESS_PROPERTY_PROCEDURE: &str = "WORKING PREMISES PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 167–172.
Preserve the actual place to be worked on/in, operator/contracting actor, seller/lessor, title owner, intended use and whose profit matters. Taking a farm, shop, studio or workshop is not the same as buying a passive dwelling investment. Do not merge operator with owner or beneficiary. Genuine relay principal takes first; concerning another operator retains their actual relationship house. Unknown capacity stays unknown. Conditional generic-counterparty rows are alternatives, not invented actor bindings. Assigned houses differ from occupied houses; retain coincident rulers.

This recipe assumes the premises can be obtained if the person chooses them: no obligatory first/seventh acquisition aspect. The operator is first, opposing seller/lessor seventh unless specifically related, tenth prospective profit from working the place, fourth final outcome. Turn a concerning-other operator's context explicitly. Do not label tenth as ordinary residential price or fifth as the operating business profit; passive property letting/investment is a different concern. A financial beneficiary's pocket remains separate.

Judge whether operator ruler in first or in a sign aspecting Ascendant by trine/sextile supports a beneficial deal; closeness and essential strength add weight. Distinguish actual degrees from mere same-house placement. Strong occupants of first can benefit, debilitated occupants harm, with the source's exception: opposing-party ruler in the Ascendant sign is necessarily debilitated there, so do not automatically count that as harm to the deal. On the Ascendant cusp it can show pressure to transact; just inside it can put the other party in the operator's power.

Assess seller/lessor trustworthiness through actual condition; severe debility or twelfth-house concealment is reason to inspect terms, not an independently proven fraud. Seventh-house affliction can warn about the contract. Consider tenth/profit condition and supplied benefits/afflictions to that house. Fourth/outcome describes eventual satisfaction or regret through its ruler and occupants; a benefic with dignity can help and a debilitated planet harm. Fit must respect actual use: descriptive conditions are not automatically defects.

Reception A→B means A regards B; retain every positive AND negative quality. Separate willingness, capacity, prospective benefit and completion. Existing property/price dealings can provide context but are not a forced first/seventh event test for an available working place. Do not fabricate future profit, exact price or occupant testimony that is not supplied.

Use actual motion/event order for material changes and impediments; no automatic applier/caller direction, no incidental-contact prohibition. Astronomical hours are not civil timing. Missing calculations, future dignity, Parts or precise order remain unknown. Return status, answer, analysis, evidence, unresolved: practical judgment first with material eIDs/pages, contrary evidence and explicit limits.
";

const CONCISE_PARCEL_PROCEDURE: &str = "PARCEL AND LETTER PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 165–166.
Preserve the actual object, sender, intended recipient, incoming/outgoing direction and past-receipt versus future-arrival question. A posted object is not merely contact with its seller; a sender-person aspect does not by itself deliver a parcel. Genuine relay principal takes first; concerning someone else does not. The sender's actual relationship determines their house. A sender's letter/parcel is their turned third, ordinarily ninth for a seventh-house seller. The recipient's second is their possession after receipt, not an assertion that they already own or hold it.

For incoming arrival, look for the actual parcel ruler joining the recipient, available contextual Moon or recipient pocket. Inspect actual application, supported translation/collection and material prevention. Do not infer who travels from endpoint order. Timing follows supported arrival using actual applying travel and contextual units; astronomical search hours are not civil dates. An explicit bound can use relevant present-sign-end testimony; after-ingress circumstances and reception may change, with the usual roughly three-to-four-degree allowance rather than a long success chain.

'Has she received my letter?' is past, not an unmade delivery. The principal's letter can be third; separating contact with recipient, or letter already in recipient's own house or second, can indicate receipt. Outgoing recipient must be identified from actual inquiry; declared persons/pockets are conditional references when the canonical recipient ID is absent, not a licence to mint a recipient. No future aspect requirement overrides actual past-placement testimony. If separating contacts are not calculated, disclose that limit.

Use actual role assignments rather than bodies' occupied houses; retain shared rulers and house priority over Moon. Reception A→B is A's regard for B, preserving all positive AND negative qualities. Condition/capacity can affect delivery but does not independently establish damage, a lost parcel, postal blame or secret motive. Optional person-contact routes cannot replace the physical object in this family.

No universal title-owner, address, destination name or sender biography is needed beyond the actual recipe. Reuse the understanding chart, not the proposed delivery day. Missing candidates, fine event order, antiscia and unavailable techniques remain unknown, not proof of no possible receipt. Return status, answer, analysis, evidence, unresolved: answer the actual past/future question directly with material native eIDs/pages, contrary evidence and limits.
";

const CONCISE_VISIT_PROCEDURE: &str = "VISITOR PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 166, 190.
Preserve the actual visitor, relationship/capacity, expected arrangement, place and timeframe. A friend uses eleventh, an employee/plumber sixth, parent or sibling their actual house; coworker and social friend differ in operative capacity. Genuine relay principal takes first; merely concerning someone else does not. Unknown relationship remains unknown. Assigned houses differ from occupied houses; retain shared rulers and house priority over contextual Moon.

For a visitor whose physical arrival is assumed or otherwise established, the source prefers contact to Ascendant, then principal ruler, available Moon, then fourth cusp as principal home. These are alternative arrivals, not a compulsory extra destination questionnaire. An office visit is not automatically arrival at the home cusp. A mere visit hope or 'will I ever see my long-lost friend?' does not establish arrival: application to Ascendant can show the person weighing on the principal's mind rather than knocking at the door.

Do not fabricate a cusp crossing from a planetary aspect table. If cusp-event timing or separating testimony is not computed, report that boundary; native cusp/position data remains evidence for contextual assessment, not certified exact perfection. For a legitimately expected short-timescale arrival, small arc may mean immediately, and the source's minutes-of-arc/minutes-of-time calibration can be appropriate. This is contextual calibration, not converting astronomical search hours into civil minutes or applying that rule to an uncertain reunion.

Inspect actual candidate order, supported translation/collection and material prevention. Which body applies does not mean the principal must go collect the visitor; who goes to whom comes from the question. A prior incidental contact is not automatically prohibition or a new intermediary. A void Moon supports the actual default, not one universal answer for arranged and unarranged visits.

Reception A→B is A's regard for B. Preserve every positive AND negative quality and weigh inclination separately from ability and occasion. Attribute essential condition, house capacity, solar state and motion to the actual body; retrograde can fit coming back without proving arrival. After ingress needs changed context/reception and source-bounded travel, not an endless aspect chain.

Keep understanding time and supplied reader place; do not recast for an appointment day. Missing candidates or techniques remain unknown rather than evidence of absence. Return status, answer, analysis, evidence, unresolved: useful arrival judgment first, material eIDs/pages, contrary testimony and honest timing limits.
";

const CONCISE_TRUST_PROCEDURE: &str = "TRUST PROCEDURE — Frawley printed pp. 19–32, 44–77, 165, 172, 189–190.
Preserve the actual person and capacity in which their honesty/reliability is asked about. Trustworthiness is not automatically rumour veracity, affection, future contact, an acquisition contest or an allegation of a specific completed act. Genuine relay principal takes first; concerning another person preserves that person's relationship. Use the person's applicable house: other contracting party/partner seventh, sibling third, boss tenth, equal coworker seventh, subordinate sixth, friend eleventh in that social context. Do not infer an actor from a name or substitute a title-owner for an operative agent. Unknown capacity remains unknown.

Judge that person's actual significator's essential condition. More dignity supports honourable conduct; debility is contrary. Fixity is not inherently good: a debilitated fixed significator can persist in dishonesty. Keep descriptive fitness distinct from moral debility where context warrants it, and do not translate every weak planet into an independently verified crime.

The source cautions that Mercury can be amoral even when dignified: honesty may last only as long as it suits the person. Gemini can indicate playful disregard, Virgo technical small-print justification. A person whose strength comes from Mercury's sign can inherit that caution. These are qualified chart judgments in the actual concern, not facts establishing a particular deception or a universal claim about every Mercurial person.

Reception A→B means A regards B, never reciprocal interest. Preserve all positive AND negative supplied qualities and weigh the person's motives toward the relevant principal separately from honesty and ability. Liking the principal does not alone establish trustworthiness, and absence of reception does not prove fraud. Occupied-house capacity and solar/motion afflictions can qualify ability to act but are not substitutes for the source's honesty test.

A situation/quality inquiry requires no new event aspect. If the actual question asks whether the person will perform a specified undertaking, separately inspect supported occasion and capacity; do not call willingness completed action. Known contract terms or observed conduct remain observations, not overwritten by the chart. Do not invent concealed clauses, theft, sexuality, other partners or private motive.

Return only status, answer, analysis, evidence, unresolved. Give a proportionate answer to the stated trust concern with material eIDs/pages, substantial contrary testimony and unknown calculations; no unqualified factual allegation or invented timing.
";

const CONCISE_INFORMATION_PROCEDURE: &str = "INFORMATION VERACITY PROCEDURE — Frawley printed pp. 19–32, 44–77, 164–165.
Preserve the exact claim and distinguish ordinary information/rumour from prediction or prophetic dream. Use the shortest actual topic route: 'is my boyfriend cheating?' is principally relationship, not automatically third-house truth; 'will I get the job?' is not converted to rumour because someone predicted it. This family is the residual genuine veracity inquiry, not a universal answer to every fact question.

For a report or rumour, assess fixed signs on the angles; Lord 1, Lord 3, Moon and Moon's dispositor in fixed signs and angular houses, or at least fixed signs and succedent houses. Angularity and fixity give solidity; the source asks for a majority, not unanimity or a fabricated numeric threshold. With few positive truth testimonies the source default is false, but express uncertainty proportionately to actual supplied contrary evidence and missing calculations.

For a prediction or prophetic dream, use ninth and Lord 9 instead of third and Lord 3 with the same source tests. Conditional third/ninth rows do not select the branch; original question and declared context determine which is relevant. Do not infer a hidden source type from the desired answer. The Moon's dispositor is its actual domicile ruler, not the ruler of whichever sign it will enter next. Assigned angle/house roles differ from bodies' occupied houses.

Third-house affliction may indicate false information, but can also show that truthful information harms the principal; context decides which is more plausible. Do not turn debilitated Saturn into automatic falsehood when the report itself concerns harmful news. A void Moon indicates nothing will come of the matter whether the report is true or false; it does not by itself adjudicate its factual truth.

Use actual supplied signs, house positions, essential condition and afflictions; do not invent absent angle fixity, exact cusp conjunctions, future events or an independent fact-check. Reception direction A→B is A's regard for B and retains all positive AND negative qualities, but personal liking is not the general truth test. A future aspect or delivery date is not universally required to judge information already received.

Genuine relay principal takes first; concerning another person does not. Preserve the understood chart moment and supplied reader place. Missing techniques remain unknown. Return only status, answer, analysis, evidence, unresolved: a direct qualified veracity judgment, material eIDs/pages and both positive/negative testimonies, with no invented real-world confirmation or civil-time prediction.
";

const CONCISE_VEHICLE_PROCEDURE: &str = "VEHICLE-OF-JOURNEY PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 142–143.
Preserve the actual journey and vehicle undertaking, whether the principal is aboard, and the bounded concern. 'The ship that I sail in' is first-house vehicle/body of the undertaking even if the principal is not presently aboard or has never boarded. Owner identity or a vehicle's being hired does not turn that role into second. A car considered as saleable possession or proposed purchase belongs to the movable-object recipe instead. Genuine relay principal takes first; do not invent an actor or vessel from private expected labels.

Use Lord 1 as the vehicle's main significator. Judge genuinely harmful coming testimony: debilitated Mars/Saturn, Lord 8, combustion and a supplied South Node conjunction are serious possibilities; weaker afflictions from debilitated Venus/Jupiter suggest trouble rather than necessarily disaster. A natural malefic that is the vehicle's own ruler cannot be treated as a separate attacker merely because its name is Saturn or Mars. Assess actual condition and role-bound connection. Essential dignity, occupied-house capacity, solar state and actual motion qualify the vehicle's condition; do not invent an engineering defect from a weak ruler alone.

Inspect the supplied chronological event set involving the vehicle, distinguishing material harm from every incidental aspect. A benefic with debility can harm and a dignified planet can help; do not use a crude planet-name score. Candidate endpoints are storage order, not a causal/applier arrow. Sign change and reversal can alter later condition; do not carry current reception unchanged or invent a station. Hourly interpolation can miss fine order or intra-hour reversals.

The source's absence-of-affliction testimony supports safety only within the actual supplied coverage and resolved techniques. A seven-day search cannot promise an unbounded future journey or all physical safety; absent nodes, fixed stars or antiscia are uncomputed, not favourable evidence. Do not substitute unsupported chart certainty for real-world reports or mechanically call every opposition a fatal accident.

If the actual question asks when a material event occurs, establish it first and use actual applying travel plus justified context; astronomical search hours are not civil times. Retain understanding moment/reader place, not a chart recast for the departure date. Reception A→B is A's regard for B, preserving all qualities; it is not mechanical proof that the car has feelings or an accident occurs. Return status, answer, analysis, evidence, unresolved: direct bounded judgment with material eIDs/pages, severity distinctions, contrary evidence and explicit limits.
";

const CONCISE_DESCRIPTION_PROCEDURE: &str = "PERSON DESCRIPTION PROCEDURE — Frawley printed pp. 19–32, 44–77, 143–145, 191–192, 196.
Preserve who is described and any actual known context. Use that person's MAIN operative house ruler, not an arbitrary natural cosignificator or the person's career ruler. An unnamed future marriage partner is seventh, not an invented identity; a declared relative uses their actual relationship. Genuine relay principal takes first; concerning another person does not. A subject.kind=person_role is a requested role, not the current owner's physical body. Assigned house differs from occupied house; shared rulers retain their aliases.

Begin with the main planet's broad build: Saturn tall/slim; Jupiter big and fleshy, especially in water; Mars short/muscular/solid; Sun tall/well-built but not Jupiter's size; Venus short/soft; Mercury mid-height or above and slim; Moon fleshy/not tall, generally bigger waxing than waning. Qualify this base using its domicile ruler, other major dignities and relevant tight aspects. A qualifying influence modifies rather than wholly replacing the main planet. Debility can diminish the body's characteristic, e.g. Mars lacking its usual muscle, without proving disease or deformity.

Treat age comparatively: Moon can mean younger, Saturn older; neither determines an infant/ancient person or an exact age. Sun/Venus/Mars can show comparable age. The chart does not establish race, and hair/skin/eye assumptions depend strongly on actual context; do not infer ethnicity or racial background. Avoid exact inches, measurements or an identifying tattoo/scar invented to appear clever.

Close aspects can qualify details, but source advice prefers confirming an actually reported feature instead of inventing hidden marks. A supplied Mars square can fit a reported scar; it does not justify announcing a scar as independently known. Uncomputed fixed stars, tight aspect precision or unavailable phase/particular dignity remain unknown. Main-role reception to its dignity hosts supplies qualifiers, not those hosts as extra people.

Physical description is a present-state question, not an event requiring future perfection. Do not reject description solely because no new aspect occurs or Moon is void. Nor does describing a future-partner role establish that a meeting or marriage will occur. Separate that event claim and its timing. Maintain proportionate uncertainty from approximate positions and the source's broad descriptive method.

Return only status, answer, analysis, evidence, unresolved: useful broad description first with material native eIDs/pages, conditional qualifications and limits; no invented exact identity, race, age or unsupported physical diagnosis.
";

const CONCISE_TAX_PROCEDURE: &str = "TAX BURDEN PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 231–232.
Preserve the actual taxpayer, pending/already-assessed bill, burden question and relevant deadline. Government is tenth and its coffers eleventh; taxpayer's purse is their own second. For a genuinely relayed question the relay principal takes first. For concerning another person use that person's actual house and turned second; do not turn the actual government into that person's boss. Keep authority, coffers and purse distinct even if rulers coincide.

These charts commonly show taxpayer money joining government or its coffers. Consider reception of BOTH government and coffers toward the purse, giving particular weight to whichever actually contacts it. A→B means A regards B, not B's response. The more the tax authority wants the money, the greater its demand may be; exaltation can show overestimation of what exists or is owed. Do not reverse that direction into the taxpayer loving the government.

The contrary reception matters: strong mutual reception can reduce demands even though the taxman likes the money. Keep every positive AND negative quality instead of a single weighted strength score. Domicile/exaltation are major, triplicity moderate, term/face minor. Chart affinity is a source-defined fiscal metaphor, not literal government emotion or independent evidence of an unlawful assessment.

Judge other forthcoming actual afflictions to the purse and its essential/accidental strength to assess capacity to withstand the bill. Good purse strength does not erase tax demand; low demand and ease of surviving it are different questions. An assessed bill is a stated observation, not a newly predicted unknown amount, and a qualitative chart burden does not create an exact currency number.

If timing of a supported demand/payment is actually asked, use native order, actual applying travel and justified contextual units. Do not treat any incidental contact as collection, tax seizure or prohibition without its connecting role. Astronomical hours are not civil dates. An explicit timeframe may have relevant sign-end testimony; missing candidates, fine order, future dignity and uncomputed techniques remain unknown.

No universal future-arrival aspect is required merely to judge how burdensome an already-assessed tax bill is. Do not silently reframe it as winning a grant, customer payment or general financial prophecy. Keep understood chart time and supplied reader place. Return status, answer, analysis, evidence, unresolved: useful proportionate burden judgment with material eIDs/pages, countervailing reception and purse evidence, limits and no invented monetary calculation.
";

const CONCISE_BET_PROCEDURE: &str = "BETTING AND PRIZE MONEY PROCEDURE — Frawley printed pp. 19–32, 44–77, 84–101, 127–136, 156, 158–163.
Preserve the actual bettor, wager/lottery/horse-prize type, selected proposition and requested net profit. Gambling profit is not fifth-house recreation merely because betting can be entertaining. A winner ends with more money, not simply a favoured team winning or a stake being refunded. Genuine relay principal takes first; another bettor retains their actual house. Conditional source rows are alternatives selected by the original question, not all operative simultaneously.

Ordinary bookmaker or private cash opponent is the bettor's seventh, their money eighth. Needed winnings must reach the bettor's ruler, available contextual Moon or pocket (second); which body applies does not change the assumed recipient. Establish a supported connection and relevant prevention/translation, then use the money's condition to qualify gain relative to stake, not invent an exact sum. A strong opponent without money-to-recipient testimony is not a win. Returning one's own money can be refund rather than profit.

Ordinary public lottery is gift/pennies-from-heaven, eleventh, not a contest by default. Only an explicitly competitive scheme to outwit the lottery company can invoke opponent/eighth. A jackpot-only claim needs exceptionally strong money testimony; minor gain is not automatically a jackpot. Do not equate winning a sports contest with receiving a wager's money.

Owned horse prize uses horse twelfth and its second, radical first for the principal's horse. Give first to horse profit in that branch rather than inventing a duplicate independent querent ruler. Strong benefit to recipient first/second by placement can support profit; otherwise examine horse money joining available Moon/pocket. House priority can remove the ordinary contextual Moon when it actually rules opponent/money. Retain coincident aliases without pretending they independently perfect an aspect.

Reception A→B means A regards B; preserve positive AND negative qualities. Attribute money condition and ability to its actual role. Inspect source-supported intervening contacts, not every incidental planet as a bookmaker or thief. An incomplete seven-day search cannot prove no possible win forever, and missing antiscia or precise order remains unknown. Do not chase a long chain until one looks profitable.

Timing follows established occurrence with actual applying travel and contextual units; astronomical search hours are not civil outcome dates. Respect the stated bet and horizon, not a recast event chart or an invented recommended wager. Return status, answer, analysis, evidence, unresolved: direct qualified net-profit judgment, material eIDs/pages, contrary testimony and honest calculation limits.
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
    let reference = match method {
        "contact" => component_contact_role_reference(consultation, &native)?,
        "new_job" | "movable_deal" | "relationship" | "money" | "property" | "rental"
        | "return_to_job" | "job_offer" | "investment" | "business_property" | "parcel"
        | "visit" | "trust" | "information" | "vehicle" | "person_description" | "tax" | "bet" => {
            component_family_role_reference(consultation, &native, method)?
        }
        _ => unreachable!("The concise procedure admits only implemented families"),
    };
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
/// unknowns. Quotation/provenance and coordinates are private once this native
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
    concise_reader_source(method)?;
    let (table, native) = verified_concise_chart(original, consultation, method)?;
    let selected = match method {
        "contact" => relevant_contact_ids(&table, &native, extra_demo_ids)?,
        "new_job" | "movable_deal" | "relationship" | "money" | "property" | "rental"
        | "return_to_job" | "job_offer" | "investment" | "business_property" | "parcel"
        | "visit" | "trust" | "information" | "vehicle" | "person_description" | "tax" | "bet" => {
            relevant_family_ids(&table, &native, consultation, method, extra_demo_ids)?
        }
        _ => unreachable!("The source procedure admits only implemented families"),
    };
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
            "{}|event|{}~{} {} t≈{}astro-h {signs} chartGap={}; {}\n",
            fact.id,
            row[1].as_str().context("No event endpoint")?,
            row[2].as_str().context("No event endpoint")?,
            row[3].as_str().context("No aspect")?,
            relevant_number(&row[5]),
            plain_relevant(&gap),
            relevant_motion(
                &row[7],
                fact.event
                    .as_ref()
                    .and_then(|event| event.perfection_motion.as_ref()),
            )?
        ));
    }
    text.push_str("</facts>\nLegend: H assigns a role, not the body's occupied house. → is regard, preserving every +/− quality; ~ endpoints are storage order, not the applicant or caller. Events are in computed astronomical order; fine prevention order remains approximate. chartGap and travel are not entryDepth; entryDepth is distance in the motion direction within the entered sign, and signEntries counts crossed sign boundaries. Reversals or rounding-sensitive boundaries leave entryDepth unknown. Motion endpoints/travel use hourly-bracket interpolation, rounded to 3 decimals (error ≤0.0005 in stated units); rounded zero is not stationarity. Astronomical hours are not earthly time. Only relevant source-selected facts are shown; omitted evidence or uncomputed techniques are unknown, not proof of absence.\n");
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

fn relevant_family_ids(
    table: &Value,
    native: &[crate::native_evidence::Fact],
    consultation: &Value,
    method: &str,
    extra_demo_ids: Option<&BTreeSet<String>>,
) -> Result<BTreeSet<String>> {
    let roles = table["role_reference"]["roles"]
        .as_array()
        .context("No family role rows")?;
    let role = |id: &str| roles.iter().find(|r| r[0] == id);
    let planet = |id: &str| role(id).and_then(|r| r[4].as_str());
    let actor = |id: &str| role(id).and_then(|r| r[1].as_str());
    let principal = actor("principal.person");
    let is_principal =
        |id: &str| principal.is_some() && actor(id).is_some() && actor(id) == principal;
    let mut recipients = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut planets = BTreeSet::from(["Moon".to_owned()]);
    let mut ids = BTreeSet::new();
    let mut conditional_pairs = None;
    for row in roles {
        // The speaker's anchor does not make them the applicant, contractor or
        // beneficiary. Keep its house binding without unrelated condition rows.
        if row[0] != "principal.person"
            && let Some(planet) = row[4].as_str()
        {
            planets.insert(planet.to_owned());
        }
        if let Some(id) = row[8].as_str() {
            ids.insert(id.to_owned());
        }
    }
    let mut recipient = |id: &str| {
        if let Some(p) = planet(id) {
            recipients.insert(p.to_owned());
        }
    };
    let mut target = |id: &str| {
        if let Some(p) = planet(id) {
            targets.insert(p.to_owned());
        }
    };
    match method {
        "new_job" | "return_to_job" | "job_offer" => {
            recipient("worker.person");
            if is_principal("worker.person") {
                recipient("principal.moon");
            }
            target("subject.primary");
            // Pocket and wages remain conditional condition/reception
            // references. A wages connection is not job acquisition.
        }
        "movable_deal" => match consultation["facet"].as_str().filter(|_| {
            !consultation["unknown"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row == "facet"))
        }) {
            Some("situation") => {
                // The object's present state is sufficient for quality. Its
                // first nearby occasions are supplied as possible changes,
                // not a requirement for completing a purchase.
                recipient("subject.primary");
            }
            Some("profit") => {
                recipient("deal.pocket");
                if is_principal("deal.pocket") {
                    recipient("principal.person");
                    recipient("principal.moon");
                } else if actor("deal.pocket") == actor("deal.actor")
                    && actor("deal.pocket").is_some()
                {
                    recipient("deal.actor");
                }
                target("deal.incoming_money");
            }
            facet => {
                recipient("deal.actor");
                if is_principal("deal.actor") {
                    recipient("principal.moon");
                }
                target("deal.counterparty");
                if facet.is_none() {
                    // An undeclared scope exposes conditional alternatives,
                    // never an expected facet or additional required fact.
                    recipient("deal.pocket");
                    target("deal.incoming_money");
                    if is_principal("deal.pocket") {
                        recipient("principal.person");
                        recipient("principal.moon");
                    }
                    let mut pairs = BTreeSet::new();
                    for (a, b) in [
                        ("deal.actor", "deal.counterparty"),
                        ("deal.pocket", "deal.incoming_money"),
                    ] {
                        if let (Some(a), Some(b)) = (planet(a), planet(b)) {
                            let mut pair = vec![a.to_owned(), b.to_owned()];
                            pair.sort();
                            pairs.insert(pair);
                        }
                    }
                    if is_principal("deal.actor")
                        && let (Some(a), Some(b)) =
                            (planet("principal.moon"), planet("deal.counterparty"))
                    {
                        let mut pair = vec![a.to_owned(), b.to_owned()];
                        pair.sort();
                        pairs.insert(pair);
                    }
                    let receivers = if is_principal("deal.pocket") {
                        vec!["principal.person", "principal.moon"]
                    } else if actor("deal.pocket").is_some()
                        && actor("deal.pocket") == actor("deal.actor")
                    {
                        vec!["deal.actor"]
                    } else {
                        Vec::new()
                    };
                    for receiver in receivers {
                        if let (Some(a), Some(b)) =
                            (planet(receiver), planet("deal.incoming_money"))
                        {
                            let mut pair = vec![a.to_owned(), b.to_owned()];
                            pair.sort();
                            pairs.insert(pair);
                        }
                    }
                    conditional_pairs = Some(pairs);
                }
            }
        },
        "vehicle" => {
            recipient("vehicle.undertaking");
            target("vehicle.eighth");
        }
        "person_description" => {
            recipient("description.person");
        }
        "tax" => {
            recipient("taxpayer.money");
            target("government.authority");
            target("government.coffers");
        }
        "bet" => {
            recipient("bet.player");
            recipient("bet.pocket");
            if is_principal("bet.player") {
                recipient("principal.moon");
            }
            for id in ["bet.winnings", "bet.lottery", "bet.horse_prize"] {
                target(id);
            }
        }
        "parcel" => {
            recipient("parcel.recipient");
            recipient("parcel.pocket");
            if is_principal("parcel.recipient") {
                recipient("principal.moon");
            }
            target("parcel.message");
            for row in roles.iter().filter(|row| {
                row[0]
                    .as_str()
                    .is_some_and(|id| id.starts_with("declared."))
            }) {
                if let Some(p) = row[4].as_str() {
                    recipients.insert(p.into());
                }
            }
        }
        "visit" => {
            recipient("principal.person");
            recipient("principal.moon");
            target("target.person");
        }
        "trust" => {
            recipient("target.person");
        }
        "information" => {
            recipient("principal.person");
            recipient("principal.moon");
            target("claim.information");
            target("claim.prediction");
        }
        "relationship" => {
            recipient("principal.person");
            recipient("principal.moon");
            target("target.partner");
            for row in roles.iter().filter(|row| {
                row[0]
                    .as_str()
                    .is_some_and(|id| id.starts_with("declared."))
            }) {
                if let Some(p) = row[4].as_str() {
                    targets.insert(p.into());
                }
            }
        }
        "investment" => {
            recipient("subject.primary");
        }
        "money" => {
            recipient("money.recipient");
            recipient("money.pocket");
            if is_principal("money.recipient") {
                recipient("principal.moon");
            }
            target("money.incoming");
        }
        "property" | "rental" | "business_property" => {
            recipient("deal.actor");
            if is_principal("deal.actor") {
                recipient("principal.moon");
            }
            for id in [
                "deal.counterparty",
                "deal.generic_opponent",
                "subject.primary",
                "property.price",
                "property.profit",
                "property.neighbours",
                "business.profit",
            ] {
                target(id);
            }
        }
        _ => anyhow::bail!("Unsupported relevant role family"),
    }
    // These bodies have explicit family-specific ancillary relevance even
    // without an assigned person alias. This does not assign natural roles.
    if method == "person_description"
        && let Some(main) = planet("description.person")
    {
        for fact in native {
            if let Some(edge) = fact.reception.as_ref().filter(|e| {
                e.guest_planet == main
                    && e.qualities
                        .iter()
                        .any(|q| q.polarity == crate::native_evidence::ReceptionPolarity::Positive)
            }) {
                planets.insert(edge.host_planet.clone());
            }
        }
    }
    if method == "information" {
        for fact in native {
            if let Some(edge) = fact.reception.as_ref().filter(|e| {
                e.guest_planet == "Moon"
                    && e.qualities.iter().any(|q| {
                        q.dignity.native_name() == "domicile"
                            && q.polarity == crate::native_evidence::ReceptionPolarity::Positive
                    })
            }) {
                planets.insert(edge.host_planet.clone());
            }
        }
    }
    if method == "relationship" {
        planets.extend(["Sun", "Venus", "Saturn"].map(str::to_owned));
    }
    if matches!(
        method,
        "property" | "rental" | "business_property" | "money" | "investment" | "bet"
    ) {
        let relevant_houses: BTreeSet<_> = roles.iter().filter_map(|r| r[3].as_u64()).collect();
        for fact in native.iter().filter(|f| f.kind == "position") {
            // Match only the verified calculator display grammar, never user
            // prose or a private expected role. Occupants can afflict/benefit
            // the actual property or money house independently of its ruler.
            if relevant_houses
                .iter()
                .any(|h| fact.detail.contains(&format!("house {h},")))
            {
                planets.extend(fact.planets.iter().cloned());
            }
        }
    }
    planets.extend(recipients.iter().cloned());
    planets.extend(targets.iter().cloned());
    // An unbound canonical actor is not missing information in the original
    // question. Keep the native house/condition/reception alternatives so
    // the reader can resolve the original words rather than manufacturing a
    // missing calculation by pruning the very ruler it needs.
    if roles.iter().any(|r| r[5] == false && !r[4].is_string()) {
        for fact in native.iter().filter(|f| {
            matches!(
                f.kind.as_str(),
                "house" | "position" | "condition" | "reception"
            )
        }) {
            ids.insert(fact.id.clone());
        }
    }
    let mut extra = extra_demo_ids.cloned().unwrap_or_default();
    if matches!(method, "vehicle" | "tax") {
        let subject = if method == "vehicle" {
            "vehicle.undertaking"
        } else {
            "taxpayer.money"
        };
        if let Some(vehicle) = planet(subject) {
            extra.extend(
                native
                    .iter()
                    .filter(|f| f.kind == "event" && f.planets.iter().any(|p| p == vehicle))
                    .map(|f| f.id.clone()),
            );
        }
    }
    relevant_bound_ids(
        native,
        ids,
        planets,
        recipients,
        targets,
        conditional_pairs,
        Some(&extra),
    )
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
    relevant_bound_ids(
        native,
        ids,
        planets,
        recipients,
        targets,
        None,
        extra_demo_ids,
    )
}

// The shared selection keeps one source-bounded occasion sequence. It does
// not choose a verdict, applicant, or causal meaning for an incidental contact.
fn relevant_bound_ids(
    native: &[crate::native_evidence::Fact],
    mut ids: BTreeSet<String>,
    planets: BTreeSet<String>,
    recipients: BTreeSet<String>,
    targets: BTreeSet<String>,
    conditional_pairs: Option<BTreeSet<Vec<String>>>,
    extra_demo_ids: Option<&BTreeSet<String>>,
) -> Result<BTreeSet<String>> {
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
            let supported_pair = if let Some(allowed) = &conditional_pairs {
                let mut pair = f.planets.clone();
                pair.sort();
                allowed.contains(&pair)
            } else {
                (recipients.contains(&f.planets[0]) && targets.contains(&f.planets[1]))
                    || (recipients.contains(&f.planets[1]) && targets.contains(&f.planets[0]))
            };
            !f.planets.iter().any(|p| p == "Moon") && supported_pair
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

fn relevant_perfection(body: &horary_ai_core::events::BodyMotion) -> String {
    const SIGNS: [&str; 12] = [
        "Aries",
        "Taurus",
        "Gemini",
        "Cancer",
        "Leo",
        "Virgo",
        "Libra",
        "Scorpio",
        "Sagittarius",
        "Capricorn",
        "Aquarius",
        "Pisces",
    ];
    let start = body.chart_longitude_degrees.rem_euclid(360.);
    let end = body.estimated_perfection_longitude_degrees.rem_euclid(360.);
    let offset = end.rem_euclid(30.);
    let mut text = format!(
        " perfect={}{}",
        relevant_number(&json!(offset)),
        SIGNS[(end / 30.).floor() as usize],
    );
    // Calculate from unrounded native motion, not the displayed chart gap or
    // total travel. Endpoints alone cannot certify an entry after a reversal.
    let start_sign = (start / 30.).floor();
    let end_sign = ((start + body.estimated_signed_travel_degrees) / 30.).floor();
    if start_sign != end_sign {
        if body.sampled_motion_reversal_observed
            || body.zero_mean_motion_interval_observed
            || offset.min(30. - offset) <= 0.0005
            || (body.estimated_signed_travel_degrees.abs() - body.estimated_path_distance_degrees)
                .abs()
                > 0.0005
        {
            text.push_str(" signEntries=? entryDepth=?");
        } else {
            let depth = if body.estimated_signed_travel_degrees > 0. {
                offset
            } else {
                30. - offset
            };
            text.push_str(&format!(
                " signEntries={} entryDepth={}°",
                (end_sign - start_sign).abs() as u64,
                relevant_number(&json!(depth)),
            ));
        }
    }
    text
}

fn relevant_motion(
    motion: &Value,
    native: Option<&horary_ai_core::events::PerfectionMotion>,
) -> Result<String> {
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
        if let Some(native) = native {
            let supplied = native
                .body_motion
                .iter()
                .find(|body| body.planet == name)
                .context("Supplied motion has no native body witness")?;
            text.push_str(&relevant_perfection(supplied));
        }
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
                concise_decimal(&json!(event.astronomical_hours)),
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
        "legend":"Reception direction is regarding→regarded, with every supplied quality retained; no weighting or verdict. Event endpoints are storage order, not applicant/caller order. Native gap is not travel. Degrees, astronomical hours and speeds in this display are rounded to three decimals, with error ≤0.0005 of their stated unit; full precision remains in the canonical review input. Rounded zero is not stationarity. Motion positions/travel are approximate sample-bracket interpolation; speeds are interval means, not certified exact speeds. Reversal flags cover samples only; intra-interval motion remains unresolved. Null motion was not supplied in this input. All native eIDs remain; unknown or omitted computation is not proof of absence."}),
    )
}

fn concise_event_motion(motion: &Value) -> Value {
    let bracket = motion["bracketHours"].as_array().map_or_else(
        || motion["bracketHours"].clone(),
        |hours| Value::Array(hours.iter().map(concise_decimal).collect()),
    );
    json!([
        motion["estimateMethod"],
        bracket,
        motion["intraIntervalMotionResolved"],
        motion["bodyMotion"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|body| json!([
                body["planet"],
                concise_decimal(&body["chartLongitudeDegrees"]),
                concise_decimal(&body["estimatedPerfectionLongitudeDegrees"]),
                concise_decimal(&body["estimatedSignedTravelDegrees"]),
                concise_decimal(&body["estimatedPathDistanceDegrees"]),
                concise_decimal(&body["initialIntervalMeanSpeedDegreesPerDay"]),
                concise_decimal(&body["perfectionBracketMeanSpeedDegreesPerDay"]),
                body["sampledMotionReversalObserved"],
                body["zeroMeanMotionIntervalObserved"]
            ]))
            .collect::<Vec<_>>()
    ])
}

/// Display-only precision. Native request validation and private review inputs
/// keep the original values; no rounded value is fed back to the calculator.
fn concise_decimal(value: &Value) -> Value {
    let Some(number) = value.as_f64().filter(|n| n.is_finite()) else {
        return value.clone();
    };
    let scaled = number * 1000.;
    if !scaled.is_finite() {
        return value.clone();
    }
    let rounded = scaled.round() / 1000.;
    if rounded.fract() == 0. && rounded.abs() < i64::MAX as f64 {
        json!(rounded as i64)
    } else {
        json!(rounded)
    }
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

    fn relevant_family_input(
        method: &str,
        consultation: Value,
    ) -> (Example, Vec<crate::native_evidence::Fact>) {
        let (mut input, mut chart, native) = declared(None);
        chart.as_object_mut().unwrap().remove("role_reference");
        input.set(
            "question",
            json!("The unchanged synthetic family question."),
        );
        input.set(
            "consultation",
            json!(serde_json::to_string(&consultation).unwrap()),
        );
        input.set(
            "chart_evidence",
            json!(serde_json::to_string(&chart).unwrap()),
        );
        let relevant = relevant_reader_inputs(&input, method).unwrap();
        assert_eq!(relevant.get("question"), input.get("question"));
        (relevant, native)
    }

    #[test]
    fn compact_mail_visitor_and_information_roles_follow_actual_declared_fields() {
        let (input, _) = relevant_family_input(
            "parcel",
            json!({"facts":[fact("sender","mother"),fact("people.mother.relationship","mother")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("parcel.sender(mother) = H10/"));
        assert!(chart.contains("parcel.message(mother) = H12/"));
        assert!(chart.contains("parcel.recipient(querent) = H1/"));
        assert!(chart.contains("parcel.pocket(querent) = H2/"));
        let (input, _) = relevant_family_input(
            "parcel",
            json!({"facts":[fact("sender","querent"),fact("people.iris.relationship","mother")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("parcel.message(querent) = H3/"));
        assert!(chart.contains("parcel.recipient(?) = —/?"));
        assert!(chart.contains("declared.iris(iris) = H10/"));
        assert!(chart.contains("declared.iris.pocket(iris) = H11/"));
        let (input, _) = relevant_family_input(
            "visit",
            json!({"facts":[fact("visitor","worker"),fact("people.worker.relationship","employee"),fact("expected","no")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("target.person(worker) = H6/"));
        assert!(
            chart.contains("arrival.ascendant(querent) = H1/")
                && chart.contains("arrival.home(querent) = H4/")
        );
        let consultation = input.get("consultation").unwrap().as_str().unwrap();
        assert!(consultation.contains("expected=no"));
        let (input, native) = relevant_family_input(
            "information",
            json!({"facts":[fact("claim","A reported public event")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        for house in [1, 3, 4, 7, 9, 10] {
            let fact = native
                .iter()
                .find(|f| f.kind == "house" && f.label == format!("House {house}"))
                .unwrap();
            assert!(relevant_chart_ids(chart).unwrap().contains(&fact.id));
        }
        assert!(
            chart.contains("claim.information(?) = H3/")
                && chart.contains("claim.prediction(?) = H9/")
        );
    }

    #[test]
    fn compact_safety_description_tax_and_bet_keep_distinct_source_roles() {
        let (input, native) = relevant_family_input(
            "vehicle",
            json!({"facts":[fact("subject.name","cargo voyage")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("vehicle.undertaking(?) = H1/Saturn"));
        assert!(!chart.contains("subject.primary(?) = H2/"));
        // Required unknown canonical roles retain the native alternatives.
        let (unbound, native_unbound) = relevant_family_input(
            "person_description",
            json!({"facts":[fact("subject.owner_id","max"),fact("subject.kind","person"),fact("people.max.relationship","unknown")],"unknown":[]}),
        );
        let unbound_ids =
            relevant_chart_ids(unbound.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        assert!(
            native_unbound
                .iter()
                .filter(|f| matches!(
                    f.kind.as_str(),
                    "house" | "position" | "condition" | "reception"
                ))
                .all(|f| unbound_ids.contains(&f.id))
        );

        let ids = relevant_chart_ids(chart).unwrap();
        for fact in native
            .iter()
            .filter(|f| f.kind == "event" && f.planets.iter().any(|p| p == "Saturn"))
        {
            assert!(
                ids.contains(&fact.id),
                "Later bounded harm candidate dropped: {}",
                fact.id
            );
        }
        let (input, _) = relevant_family_input(
            "person_description",
            json!({"facts":[fact("subject.kind","person_role"),fact("subject.owner_id","querent")],"unknown":[]}),
        );
        assert!(
            input
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("description.person(?) = H7/")
        );
        let (input, _) = relevant_family_input(
            "person_description",
            json!({"facts":[fact("subject.kind","person"),fact("subject.owner_id","father"),fact("people.father.relationship","father")],"unknown":[]}),
        );
        assert!(
            input
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("description.person(father) = H4/")
        );
        let (input, _) = relevant_family_input(
            "tax",
            json!({"facts":[fact("subject.owner_id","sister"),fact("people.sister.relationship","sibling")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(
            chart.contains("taxpayer.money(sister) = H4/")
                && chart.contains("government.authority(?) = H10/")
                && chart.contains("government.coffers(?) = H11/")
        );
        let (input, _) = relevant_family_input(
            "bet",
            json!({"facts":[fact("subject.owner_id","querent"),fact("context","ordinary bookmaker")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(
            chart.contains("bet.winnings(?) = H8/")
                && chart.contains("bet.lottery(?) = H11/")
                && chart.contains("bet.horse_prize(?) = H1/")
        );
        assert!(
            concise_reader_source("bet")
                .unwrap()
                .contains("Conditional source rows are alternatives")
        );
    }

    #[test]
    fn compact_next_families_actual_twenty_declared_components_are_native_verified() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        for method in [
            "return_to_job",
            "job_offer",
            "investment",
            "business_property",
            "parcel",
            "visit",
            "trust",
            "information",
            "vehicle",
            "person_description",
            "tax",
            "bet",
        ] {
            let bank = cases(&base.join("examples"), method).unwrap();
            let mut max_bytes = 0;
            let mut unknown = Vec::new();
            for case in bank {
                // Standard authored component assembly, not an extraction
                // journey: only its declared consultation reaches projection.
                let original = example(&case, "reading", &Book::fixture()).unwrap();
                let original_chart: Value =
                    serde_json::from_str(original.get("chart_evidence").unwrap().as_str().unwrap())
                        .unwrap();
                let compact = relevant_reader_inputs(&original, method)
                    .unwrap_or_else(|e| panic!("{}: {e:#}", case.id));
                assert_eq!(compact.get("question"), original.get("question"));
                let chart = compact.get("chart_evidence").unwrap().as_str().unwrap();
                let canonical = original_chart["facts"].as_str().unwrap();
                let ids = relevant_chart_ids(chart).unwrap();
                assert!(!ids.is_empty());
                assert!(
                    ids.iter()
                        .all(|id| canonical.lines().any(|l| l.starts_with(&format!("{id}\t"))))
                );
                assert!(!chart.contains("source_identity") && !chart.contains("expected"));
                if chart.contains("UNKNOWN") {
                    unknown.push(case.id);
                }
                max_bytes = max_bytes.max(chart.len());
            }
            assert!(
                concise_reader_source(method)
                    .unwrap()
                    .split_whitespace()
                    .count()
                    <= 450
            );
            eprintln!(
                "{method}: twenty authored components native-verified; max chart {max_bytes} bytes; unresolved {unknown:?}"
            );
        }
    }

    #[test]
    fn compact_return_job_turns_former_career_and_business_profit_is_tenth() {
        let consultation = json!({"facts":[fact("subject.owner_id","child"),fact("people.child.relationship","child")],"unknown":[]});
        let (input, _) = relevant_family_input("return_to_job", consultation.clone());
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("worker.person(child) = H5/"));
        assert!(chart.contains("subject.primary(?) = H2/"));
        let (input, _) = relevant_family_input("job_offer", consultation);
        assert!(
            input
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("subject.primary(?) = H10/")
        );
        let (input, _) = relevant_family_input(
            "business_property",
            json!({"facts":[fact("deal_actor","querent"),fact("deal_beneficiary","querent")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("business.profit(?) = H10/"));
        assert!(chart.contains("subject.primary(?) = H4/"));
        assert!(
            !chart.contains("property.price(") && !chart.contains("residential_profit_reference(")
        );
        let (input, _) = relevant_family_input(
            "investment",
            json!({"facts":[fact("subject.owner_id","sister"),fact("people.sister.relationship","sibling")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("investment.owner(sister) = H3/"));
        assert!(chart.contains("subject.primary(sister) = H4/"));
        assert!(!chart.contains("principal.moon("));
    }

    #[test]
    fn relationship_natural_references_keep_native_testimony_without_inventing_actor_sex() {
        let (input, native) = relevant_family_input(
            "relationship",
            json!({"facts":[fact("subject.name","Alex"),fact("baseline","ongoing")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        let ids = relevant_chart_ids(chart).unwrap();
        for fact in native.iter().filter(|f| {
            matches!(f.kind.as_str(), "position" | "condition")
                && f.planets
                    .iter()
                    .any(|p| matches!(p.as_str(), "Sun" | "Venus"))
        }) {
            assert!(
                ids.contains(&fact.id),
                "Missing natural-body testimony {}",
                fact.id
            );
        }
        for fact in native.iter().filter(|f| {
            f.kind == "reception"
                && f.planets
                    .iter()
                    .all(|p| matches!(p.as_str(), "Sun" | "Venus" | "Saturn" | "Moon"))
        }) {
            assert!(
                ids.contains(&fact.id),
                "Missing directed natural-body edge {}",
                fact.id
            );
        }
        assert!(chart.contains("natural.sun(?) = —/Sun optional"));
        assert!(chart.contains("natural.venus(?) = —/Venus optional"));
        let source = concise_reader_source("relationship").unwrap();
        assert!(
            source.contains("original question")
                && source.contains("not a mandatory typed gender field")
        );
        let (_, mut canonical, native) = declared(None);
        canonical.as_object_mut().unwrap().remove("role_reference");
        // A native fixture with H1/Saturn and H7/Moon leaves Sun/Venus free.
        // Claim Sun as H1 using only a native synthetic house row; no names
        // or answer labels are consulted by the role policy.
        let mut claimed = native.clone();
        let first = claimed
            .iter_mut()
            .find(|f| f.kind == "house" && f.label == "House 1")
            .unwrap();
        first.planets = vec!["Sun".into()];
        let roles = component_family_role_reference(
            &json!({"facts":[],"unknown":[]}),
            &claimed,
            "relationship",
        )
        .unwrap();
        assert!(
            !roles["role_rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["role_id"] == "natural.sun")
        );
    }

    #[test]
    fn compact_additional_family_actual_twenty_inputs_keep_native_domains() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        for method in ["relationship", "money", "property", "rental"] {
            let contexts =
                fs::read_to_string(base.join("review/contexts").join(format!("{method}.jsonl")))
                    .unwrap();
            let bank: Vec<Value> =
                fs::read_to_string(base.join("examples").join(format!("{method}.jsonl")))
                    .unwrap()
                    .lines()
                    .map(|l| serde_json::from_str(l).unwrap())
                    .collect();
            let mut max_bytes = 0;
            let mut unresolved = Vec::new();
            let mut count = 0;
            for line in contexts.lines() {
                let context: Value = serde_json::from_str(line).unwrap();
                let original_question =
                    bank.iter().find(|c| c["id"] == context["case_id"]).unwrap()["question"]
                        .clone();
                let input = dsrust::input! {question:original_question.clone(),consultation:serde_json::to_string(&context["consultation"]).unwrap(),chart_evidence:serde_json::to_string(&context["chart_evidence"]).unwrap()};
                let compact = relevant_reader_inputs(&input, method)
                    .unwrap_or_else(|e| panic!("{}: {e:#}", context["case_id"]));
                assert_eq!(compact.get("question"), Some(&original_question));
                let chart = compact.get("chart_evidence").unwrap().as_str().unwrap();
                let ids = relevant_chart_ids(chart).unwrap();
                assert!(!ids.is_empty());
                let canonical = context["chart_evidence"]["facts"].as_str().unwrap();
                assert!(
                    ids.iter()
                        .all(|id| canonical.lines().any(|l| l.starts_with(&format!("{id}\t"))))
                );
                assert!(!chart.contains("source_identity") && !chart.contains("expected"));
                if chart.contains("UNKNOWN") {
                    unresolved.push(context["case_id"].as_str().unwrap().to_owned());
                }
                max_bytes = max_bytes.max(chart.len());
                count += 1;
            }
            assert_eq!(count, 20);
            assert!(
                concise_reader_source(method)
                    .unwrap()
                    .split_whitespace()
                    .count()
                    <= 450
            );
            eprintln!(
                "{method}: {count} native-verified original contexts; max chart {max_bytes} bytes; unresolved {unresolved:?}"
            );
        }
    }

    #[test]
    fn compact_money_source_role_is_distinct_from_arrival_recipient() {
        for (source, recipient, relationship, source_house, money_house) in [
            ("job", "querent", "", 10, 11),
            ("customer", "querent", "", 7, 8),
            ("job", "child", "child", 2, 3),
            ("government", "child", "child", 10, 11),
        ] {
            let (input, _) = relevant_family_input(
                "money",
                json!({"facts":[fact("subject.owner_id",recipient),fact("money_source",source),fact("people.child.relationship",relationship)],"unknown":[]}),
            );
            let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
            assert!(chart.contains(&format!("money.source(?) = H{source_house}/")));
            assert!(chart.contains(&format!("money.incoming(?) = H{money_house}/")));
            assert_eq!(
                chart.contains("principal.moon("),
                recipient == "querent" && !chart.contains("Moon contextual=no")
            );
        }
        let (input, _) = relevant_family_input(
            "money",
            json!({"facts":[fact("subject.owner_id","querent"),fact("money_source","relative"),fact("sender","ivy")],"unknown":["people.ivy.relationship"]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("money.source(ivy) = —/?"));
        assert!(chart.contains("money.incoming(ivy) = —/?"));
        assert!(!chart.contains("money.incoming(ivy) = H8/"));
    }

    #[test]
    fn compact_property_party_and_relationship_roles_remain_conditional() {
        let (input, _) = relevant_family_input(
            "property",
            json!({"facts":[fact("deal_actor","querent"),fact("deal_party","unrelated buyer")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("deal.counterparty(unrelated buyer) = —/?"));
        assert!(chart.contains("deal.generic_opponent(?) = H7/"));
        assert!(chart.contains("subject.primary(?) = H4/"));
        assert!(chart.contains("property.price(?) = H10/"));
        assert!(chart.contains("property.profit(?) = H5/"));
        let (input, _) = relevant_family_input(
            "relationship",
            json!({"facts":[fact("subject.name","Noor"),fact("people.noor.relationship","neighbor")],"unknown":[]}),
        );
        let chart = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(chart.contains("target.partner(?) = H7/"));
        assert!(chart.contains("declared.noor(noor) = H3/"));
        assert!(chart.contains("natural.sun(?)") && chart.contains("natural.venus(?)"));
    }

    #[test]
    fn relevant_ready_families_preserve_declared_context_and_sparse_conditional_references() {
        for (method, consultation) in [
            (
                "new_job",
                json!({"method":"new_job","facts":[fact("subject.owner_id","querent"),fact("job_context","applied, no offer; a recruiter introduced me")],"unknown":[]}),
            ),
            (
                "movable_deal",
                json!({"method":"movable_deal","facts":[fact("deal_capacity","sell"),fact("deal_actor","agent"),fact("people.agent.relationship","employee"),fact("subject.owner_id","querent"),fact("deal_beneficiary","querent")],"unknown":[]}),
            ),
        ] {
            let (input, native) = relevant_family_input(method, consultation);
            let text = input.get("chart_evidence").unwrap().as_str().unwrap();
            assert!(relevant_chart_ids(text).unwrap().len() < native.len());
            assert!(text.contains("motion=? (not supplied)"));
            assert!(!text.contains("Δ="));
            assert!(text.contains("Astronomical hours are not earthly time"));
            assert!(text.contains("omitted evidence or uncomputed techniques are unknown"));
            for metadata in [
                "source_identity",
                "source_fields",
                "input_scope",
                "alias_authority",
                "expected",
            ] {
                assert!(!text.contains(metadata));
            }
            if method == "new_job" {
                assert!(
                    input
                        .get("consultation")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .contains("job_context=applied, no offer; a recruiter introduced me")
                );
                assert!(text.contains("worker.person(querent) = H1/Saturn"));
                assert!(text.contains("subject.primary(?) = H10/Mars"));
                for id in ["worker.pocket", "job.wages"] {
                    assert!(
                        text.lines()
                            .any(|line| line.starts_with(id) && line.ends_with(" optional"))
                    );
                }
            } else {
                assert!(text.contains("deal.actor(agent) = H6/Mercury"));
                assert!(text.contains("deal.counterparty(?) = H12/Jupiter"));
                assert!(text.contains("subject.primary(querent) = H2/Saturn optional"));
                assert!(text.contains("deal.pocket(querent) = H2/Saturn optional"));
                assert!(text.contains("No facet is declared"));
            }
        }
    }

    #[test]
    fn relevant_new_job_uses_external_tenth_and_never_transfers_worker_moon() {
        for (relationship, house, job_house) in [("child", 5, 10), ("mother", 10, 7)] {
            let (input, _) = relevant_family_input(
                "new_job",
                json!({"facts":[fact("subject.owner_id","iris"),fact("people.iris.relationship",relationship)],"unknown":[]}),
            );
            let text = input.get("chart_evidence").unwrap().as_str().unwrap();
            assert!(text.contains(&format!("worker.person(iris) = H{house}/")));
            assert!(text.contains(&format!("subject.primary(?) = H{job_house}/")));
            assert!(text.contains("Moon contextual=no"));
            assert!(!text.contains("principal.moon("));
        }
        let (input, _) = relevant_family_input(
            "new_job",
            json!({"facts":[fact("principal_mode","relay"),fact("principal_id","iris"),fact("subject.owner_id","iris")],"unknown":[]}),
        );
        let text = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(text.contains("worker.person(iris) = H1/Saturn"));
        assert!(text.contains("principal.moon(iris) = —/Moon optional"));
        let (input, _) = relevant_family_input(
            "new_job",
            json!({"facts":[fact("subject.owner_id","iris")],"unknown":["people.iris.relationship"]}),
        );
        let text = input.get("chart_evidence").unwrap().as_str().unwrap();
        assert!(text.contains("worker.person(iris) = —/?"));
        assert!(text.contains("subject.primary(?) = —/?"));
        assert!(!text.contains("worker.person(iris) = H7"));
    }

    #[test]
    fn relevant_family_occasions_follow_actual_scope_without_money_as_acquisition() {
        let event = |id: &str, a: &str, b: &str, hours: f64| crate::native_evidence::Fact {
            id: id.into(),
            kind: "event".into(),
            label: format!("{a} · {b}"),
            detail: "actual synthetic candidate".into(),
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
            event("e1", "Moon", "Sun", 1.),
            event("e2", "Moon", "Mercury", 2.),
            event("e3", "Saturn", "Jupiter", 3.),
            event("e4", "Saturn", "Mars", 6.),
            event("e5", "Saturn", "Jupiter", 9.),
            event("e6", "Mars", "Venus", 10.),
            event("e7", "Jupiter", "Mercury", 4.),
            event("e8", "Jupiter", "Venus", 5.),
            event("e9", "Saturn", "Venus", 11.),
            event("e10", "Venus", "Mercury", 7.),
        ];
        let row = |id: &str, actor: &str, planet: &str, optional: bool| {
            json!([id, actor, id, 1, planet, optional, null, null, null, [], []])
        };
        let job = json!({"role_reference":{"roles":[row("principal.person","querent","Saturn",false),row("worker.person","querent","Saturn",false),row("subject.primary","","Mars",false),row("worker.pocket","querent","Mercury",true),row("job.wages","","Jupiter",true),row("principal.moon","querent","Moon",true)]}});
        let ids = relevant_family_ids(&job, &native, &json!({}), "new_job", None).unwrap();
        assert_eq!(
            ids,
            BTreeSet::from(["e1", "e2", "e3", "e4"].map(str::to_owned))
        );
        // The earlier wages occasion remains possible prevention context. It
        // does not extend acquisition to the later wages or unrelated chain.
        let deal = json!({"role_reference":{"roles":[row("principal.person","querent","Saturn",false),row("deal.actor","agent","Saturn",false),row("deal.counterparty","buyer","Mars",false),row("subject.primary","owner","Jupiter",false),row("deal.incoming_money","buyer","Venus",false),row("deal.pocket","maya","Mercury",false)]}});
        let quality = relevant_family_ids(
            &deal,
            &native,
            &json!({"facet":"situation"}),
            "movable_deal",
            None,
        )
        .unwrap();
        assert_eq!(
            quality,
            BTreeSet::from(["e1", "e2", "e3", "e7"].map(str::to_owned))
        );
        let profit = relevant_family_ids(
            &deal,
            &native,
            &json!({"facet":"profit"}),
            "movable_deal",
            None,
        )
        .unwrap();
        assert_eq!(
            profit,
            BTreeSet::from(["e1", "e2", "e7", "e8", "e10"].map(str::to_owned))
        );
        let unresolved = relevant_family_ids(
            &deal,
            &native,
            &json!({"facet":"profit","unknown":["facet"]}),
            "movable_deal",
            None,
        )
        .unwrap();
        assert_eq!(
            unresolved,
            relevant_family_ids(&deal, &native, &json!({}), "movable_deal", None).unwrap()
        );
        assert!(!unresolved.contains("e9")); // Seller is not the distinct recipient.
    }

    #[test]
    fn relevant_family_cusp_position_identity_and_supplied_motion_stay_exact() {
        for method in ["new_job", "movable_deal"] {
            let (_, mut chart, native) = declared(None);
            let consultation = if method == "new_job" {
                json!({"method":method,"facts":[fact("subject.owner_id","querent")],"unknown":[]})
            } else {
                json!({"method":method,"facet":"profit","facts":[fact("deal_capacity","sell"),fact("deal_actor","querent"),fact("subject.owner_id","querent"),fact("deal_beneficiary","querent")],"unknown":[]})
            };
            chart["role_reference"] =
                component_family_role_reference(&consultation, &native, method).unwrap();
            let text = relevant_reader_chart(&chart, &consultation, method, None).unwrap();
            assert!(text.contains("Saturn at 10.464° Aries, house 3"));
            assert!(text.contains("principal.person(querent) = H1/Saturn"));
            assert!(text.contains("Δ="));
            for role in chart["role_reference"]["role_rows"].as_array().unwrap() {
                assert!(role["selected_id"].is_null());
                if let Some(id) = role["cusp_fact_id"].as_str() {
                    let fact = native.iter().find(|f| f.id == id).unwrap();
                    assert_eq!(fact.label, format!("House {}", role["significator_house"]));
                    assert_eq!(role["ruler_planet"], fact.planets[0]);
                }
            }
            chart["role_reference"]["source_identity"] = json!("foreign");
            assert!(relevant_reader_chart(&chart, &consultation, method, None).is_err());
        }
    }

    fn fact(field: &str, value: &str) -> Value {
        json!({"field":field,"value":value,"quote":value})
    }

    fn family_input(
        method: &str,
        consultation: Value,
    ) -> (Example, Vec<crate::native_evidence::Fact>) {
        let (mut input, mut chart, native) = declared(None);
        chart.as_object_mut().unwrap().remove("role_reference");
        input.set(
            "question",
            json!("A synthetic complete family question; no rubric selects its role."),
        );
        input.set(
            "consultation",
            json!(serde_json::to_string(&consultation).unwrap()),
        );
        input.set(
            "chart_evidence",
            json!(serde_json::to_string(&chart).unwrap()),
        );
        let projected = concise_reader_inputs(&input, method).unwrap();
        assert_eq!(projected.get("consultation"), input.get("consultation"));
        assert_eq!(projected.get("question"), input.get("question"));
        assert!(projected.get("expected").is_none());
        (projected, native)
    }

    fn family_row<'a>(reference: &'a Value, id: &str) -> &'a Value {
        reference["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["role_id"] == id)
            .unwrap()
    }

    #[test]
    fn concise_family_procedures_are_short_and_cover_the_declared_source_branches() {
        let job = concise_reader_source("new_job").unwrap();
        let deal = concise_reader_source("movable_deal").unwrap();
        for procedure in [job, deal] {
            assert!(procedure.split_whitespace().count() < 800);
            assert!(procedure.contains("127–136"));
            assert!(procedure.contains("status, answer, analysis, evidence, unresolved"));
            assert!(procedure.contains("positive AND negative"));
        }
        for witness in [
            "222–224",
            "tenth-house applicant",
            "not romantic feelings",
            "an aspect to wages does not establish acquisition",
            "Tenth-house fortunes, nodes",
        ] {
            assert!(job.contains(witness), "{witness}");
        }
        for witness in [
            "156–161",
            "167–172",
            "177",
            "whether the deal completes",
            "SHOULD I buy",
            "sale profit",
            "exact inventory-sale count",
        ] {
            assert!(deal.contains(witness), "{witness}");
        }
    }

    #[test]
    fn concise_new_job_uses_the_actual_worker_external_job_exception_and_relay() {
        let (_, _, native) = declared(None);
        for (relationship, worker_house, job_house) in
            [("child", 5, 10), ("mother", 10, 7), ("partner", 7, 10)]
        {
            let consultation = json!({"facts":[fact("subject.owner_id","iris"),fact("people.iris.relationship",relationship)],"unknown":[]});
            let view = component_family_role_reference(&consultation, &native, "new_job").unwrap();
            assert_eq!(family_row(&view, "worker.person")["actor_id"], "iris");
            assert_eq!(
                family_row(&view, "worker.person")["significator_house"],
                worker_house
            );
            assert_eq!(
                family_row(&view, "subject.primary")["significator_house"],
                job_house
            );
            assert_eq!(
                family_row(&view, "job.wages")["significator_house"],
                crate::native_evidence::turn(job_house, 2)
            );
            assert_eq!(view["moon"]["available_as_contextual_reference"], false);
        }
        let relay = json!({"facts":[fact("principal_mode","relay"),fact("principal_id","iris"),fact("subject.owner_id","iris"),fact("people.iris.relationship","child")],"unknown":[]});
        let view = component_family_role_reference(&relay, &native, "new_job").unwrap();
        assert_eq!(family_row(&view, "worker.person")["significator_house"], 1);
        assert_eq!(
            family_row(&view, "subject.primary")["significator_house"],
            10
        );
        assert_eq!(view["moon"]["available_as_contextual_reference"], true);
        assert!(
            view["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["role_id"] == "principal.moon" && row["actor_id"] == "iris")
        );
    }

    #[test]
    fn concise_new_job_unknown_and_moon_claims_do_not_invent_an_applicant() {
        let (_, _, mut native) = declared(None);
        let unknown = json!({"facts":[fact("subject.owner_id","iris")],"unknown":[]});
        let view = component_family_role_reference(&unknown, &native, "new_job").unwrap();
        for id in ["worker.person", "subject.primary"] {
            assert!(family_row(&view, id)["significator_house"].is_null());
            assert!(family_row(&view, id)["ruler_planet"].is_null());
        }
        assert_eq!(view["moon"]["available_as_contextual_reference"], false);
        let house10 = native
            .iter_mut()
            .find(|f| f.kind == "house" && f.label == "House 10")
            .unwrap();
        house10.planets = vec!["Moon".into()];
        let own = json!({"facts":[fact("subject.owner_id","querent")],"unknown":[]});
        let view = component_family_role_reference(&own, &native, "new_job").unwrap();
        assert_eq!(family_row(&view, "subject.primary")["ruler_planet"], "Moon");
        assert_eq!(view["moon"]["available_as_contextual_reference"], false);
        assert!(
            view["moon"]["house_claims"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["role_id"] == "subject.primary")
        );
    }

    #[test]
    fn concise_deal_preserves_named_parties_prospective_object_and_independent_profit_pocket() {
        let (_, _, native) = declared(None);
        let buy = json!({"facet":"situation","facts":[fact("deal_capacity","buy"),fact("deal_actor","querent"),fact("seller","nora"),fact("people.nora.relationship","friend"),fact("subject.owner_id","nora")],"unknown":[]});
        let view = component_family_role_reference(&buy, &native, "movable_deal").unwrap();
        assert_eq!(family_row(&view, "deal.counterparty")["actor_id"], "nora");
        assert_eq!(
            family_row(&view, "deal.counterparty")["significator_house"],
            11
        );
        assert_eq!(family_row(&view, "subject.primary")["actor_id"], "querent");
        assert_eq!(
            family_row(&view, "subject.primary")["significator_house"],
            2
        );
        assert!(
            !view["role_rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["role_id"] == "deal.pocket")
        );
        let sale = json!({"facet":"profit","facts":[fact("deal_capacity","sell"),fact("deal_actor","agent"),fact("people.agent.relationship","employee"),fact("subject.owner_id","querent"),fact("deal_beneficiary","querent")],"unknown":[]});
        let view = component_family_role_reference(&sale, &native, "movable_deal").unwrap();
        assert_eq!(family_row(&view, "deal.actor")["significator_house"], 6);
        assert_eq!(
            family_row(&view, "deal.counterparty")["significator_house"],
            12
        );
        assert_eq!(
            family_row(&view, "deal.incoming_money")["significator_house"],
            1
        );
        for id in ["subject.primary", "deal.pocket"] {
            assert_eq!(family_row(&view, id)["actor_id"], "querent");
            assert_eq!(family_row(&view, id)["significator_house"], 2);
        }
        let ruler = family_row(&view, "deal.pocket")["ruler_planet"]
            .as_str()
            .unwrap();
        let ids: Vec<_> = view["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["planet"] == ruler)
            .map(|r| r["role_id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"subject.primary") && ids.contains(&"deal.pocket"));
        assert!(
            family_row(&view, "deal.pocket")["source_fields"]
                .as_array()
                .unwrap()
                .contains(&json!("deal_beneficiary"))
        );
    }

    #[test]
    fn concise_deal_keeps_unselected_roles_conditional_and_unresolved_parties_unknown() {
        let (_, _, native) = declared(None);
        let facts = vec![
            fact("deal_capacity", "sell"),
            fact("deal_actor", "querent"),
            fact("subject.owner_id", "querent"),
        ];
        let undecided = json!({"facts":facts,"unknown":[]});
        let view = component_family_role_reference(&undecided, &native, "movable_deal").unwrap();
        for id in ["subject.primary", "deal.incoming_money", "deal.pocket"] {
            assert_eq!(family_row(&view, id)["optional"], true);
            assert!(
                family_row(&view, id)["authority"]
                    .as_str()
                    .unwrap()
                    .contains("unselected_facet")
            );
        }
        assert!(family_row(&view, "deal.pocket")["actor_id"].is_null());
        assert!(family_row(&view, "deal.pocket")["ruler_planet"].is_null());
        let mut completion = undecided.clone();
        completion["facet"] = json!("event");
        let view = component_family_role_reference(&completion, &native, "movable_deal").unwrap();
        assert!(
            view["role_rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| !matches!(
                    r["role_id"].as_str(),
                    Some("subject.primary" | "deal.incoming_money" | "deal.pocket")
                ))
        );
        let conflicting = json!({"facts":[fact("deal_capacity","buy"),fact("deal_actor","querent"),fact("deal_party","iris"),fact("deal_party","nora"),fact("seller","nora"),fact("people.nora.relationship","friend")],"unknown":[]});
        let view = component_family_role_reference(&conflicting, &native, "movable_deal").unwrap();
        assert!(family_row(&view, "deal.counterparty")["actor_id"].is_null());
        assert!(family_row(&view, "deal.counterparty")["ruler_planet"].is_null());
        let actor_conflict = json!({"facts":[fact("deal_capacity","sell"),fact("deal_actor","iris"),fact("deal_actor","nora"),fact("seller","querent")],"unknown":[]});
        let view =
            component_family_role_reference(&actor_conflict, &native, "movable_deal").unwrap();
        assert!(family_row(&view, "deal.actor")["actor_id"].is_null());
        assert!(
            component_family_role_reference(
                &json!({"method":"contact","facts":[]}),
                &native,
                "new_job"
            )
            .is_err()
        );
        assert!(
            component_family_role_reference(
                &json!({"facet":"timing","facts":[]}),
                &native,
                "movable_deal"
            )
            .is_err()
        );
    }

    #[test]
    fn concise_ready_families_keep_the_same_all_id_tables_without_filling_sparse_motion() {
        for (method, consultation) in [
            (
                "new_job",
                json!({"facts":[fact("subject.owner_id","querent"),fact("job_context","applied, no offer, a recruiter introduced me")],"unknown":[]}),
            ),
            (
                "movable_deal",
                json!({"facts":[fact("deal_capacity","sell"),fact("deal_actor","querent"),fact("subject.owner_id","querent"),fact("deal_beneficiary","querent")],"unknown":[]}),
            ),
        ] {
            let (projected, native) = family_input(method, consultation);
            let chart: Value =
                serde_json::from_str(projected.get("chart_evidence").unwrap().as_str().unwrap())
                    .unwrap();
            let mut ids = BTreeSet::new();
            for table in ["facts", "receptions", "events"] {
                for row in chart[table]["rows"].as_array().unwrap() {
                    assert!(ids.insert(row[0].as_str().unwrap()));
                }
            }
            assert_eq!(ids, native.iter().map(|f| f.id.as_str()).collect());
            assert_eq!(
                chart["events"]["columns"],
                json!([
                    "id",
                    "endpoint1",
                    "endpoint2",
                    "aspect",
                    "within_current_signs",
                    "astronomical_hours",
                    "native_current_angular_gap_or_detail",
                    "motion"
                ])
            );
            for row in chart["events"]["rows"].as_array().unwrap() {
                let fact = native.iter().find(|f| row[0] == f.id).unwrap();
                assert_eq!(row[1], fact.planets[0]);
                assert_eq!(row[2], fact.planets[1]);
                assert_eq!(
                    row[4],
                    json!(fact.event.as_ref().unwrap().within_current_signs)
                );
                assert!(row[7].is_null());
            }
            for row in chart["receptions"]["rows"].as_array().unwrap() {
                let edge = native
                    .iter()
                    .find(|f| row[0] == f.id)
                    .unwrap()
                    .reception
                    .as_ref()
                    .unwrap();
                assert_eq!(row[1], edge.guest_planet);
                assert_eq!(row[2], edge.host_planet);
                for (column, polarity) in [
                    (3, crate::native_evidence::ReceptionPolarity::Positive),
                    (4, crate::native_evidence::ReceptionPolarity::Negative),
                ] {
                    assert_eq!(
                        row[column],
                        json!(
                            edge.qualities
                                .iter()
                                .filter(|q| q.polarity == polarity)
                                .map(|q| q.dignity.native_name())
                                .collect::<Vec<_>>()
                        )
                    );
                }
            }
            let role_columns = chart["role_reference"]["role_columns"].as_array().unwrap();
            assert_eq!(role_columns.len(), 11);
            for row in chart["role_reference"]["roles"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r[3].is_number())
            {
                let cusp = native.iter().find(|f| f.id == row[8]).unwrap();
                assert_eq!(cusp.label, format!("House {}", row[3]));
                assert_eq!(row[4], cusp.planets[0]);
                assert!(row[7].is_null()); // A calculated reference is not an accepted selection.
            }
        }
    }

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
        let method = std::env::var("HORARY_RELEVANT_METHOD").unwrap_or_else(|_| "contact".into());
        let projected = relevant_reader_inputs(&original, &method).unwrap();
        if let Ok(expected) = std::env::var("HORARY_RELEVANT_EXPECTED_OUTPUT") {
            let previous: Value = serde_json::from_slice(&fs::read(expected).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(projected.rendered()).unwrap(),
                previous
            );
        }
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
    fn concise_projection_keeps_every_native_id_direction_quality_and_bounded_motion_display() {
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
            assert_eq!(
                row[5],
                concise_decimal(&supplied["candidate"]["astronomicalHours"])
            );
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
        assert!(concise_reader_source("unclassified").is_err());
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
                [5, 6],
                false,
                [
                    ["Venus", 350, 5, 15, 17, -0.2, 0.4, true, false],
                    ["Mars", 5, 4, -1, 1, -0.1, -0.2, false, true]
                ]
            ])
        );
    }

    #[test]
    fn perfection_depth_is_not_the_chart_gap_or_total_travel() {
        let mut body = horary_ai_core::events::BodyMotion {
            planet: "Moon".into(),
            chart_longitude_degrees: 172.404,
            estimated_perfection_longitude_degrees: 186.365,
            estimated_signed_travel_degrees: 13.961,
            estimated_path_distance_degrees: 13.961,
            initial_interval_mean_speed_degrees_per_day: 13.,
            perfection_bracket_mean_speed_degrees_per_day: 13.,
            sampled_motion_reversal_observed: false,
            zero_mean_motion_interval_observed: false,
        };
        let text = relevant_perfection(&body);
        assert!(text.contains("perfect=6.365Libra"));
        assert!(text.contains("signEntries=1 entryDepth=6.365°"));
        assert!(!text.contains("entryDepth=13.961"));
        body.chart_longitude_degrees = 350.;
        body.estimated_perfection_longitude_degrees = 5.;
        body.estimated_signed_travel_degrees = 15.;
        body.estimated_path_distance_degrees = 15.;
        assert!(relevant_perfection(&body).contains("perfect=5Aries signEntries=1 entryDepth=5°"));
        body.chart_longitude_degrees = 5.;
        body.estimated_perfection_longitude_degrees = 355.;
        body.estimated_signed_travel_degrees = -10.;
        body.estimated_path_distance_degrees = 10.;
        assert!(
            relevant_perfection(&body).contains("perfect=25Pisces signEntries=1 entryDepth=5°")
        );
        body.sampled_motion_reversal_observed = true;
        assert!(relevant_perfection(&body).contains("signEntries=? entryDepth=?"));
        assert_eq!(
            relevant_motion(&Value::Null, None).unwrap(),
            "motion=? (not supplied)"
        );
    }

    #[test]
    fn perfection_display_keeps_multiple_entries_and_boundary_uncertainty() {
        let mut body = horary_ai_core::events::BodyMotion {
            planet: "Moon".into(),
            chart_longitude_degrees: 172.404,
            estimated_perfection_longitude_degrees: 211.,
            estimated_signed_travel_degrees: 38.596,
            estimated_path_distance_degrees: 38.596,
            initial_interval_mean_speed_degrees_per_day: 13.,
            perfection_bracket_mean_speed_degrees_per_day: 13.,
            sampled_motion_reversal_observed: false,
            zero_mean_motion_interval_observed: false,
        };
        assert!(
            relevant_perfection(&body).contains("perfect=1Scorpio signEntries=2 entryDepth=1°")
        );
        body.estimated_perfection_longitude_degrees = 180.0001;
        body.estimated_signed_travel_degrees = 7.5961;
        body.estimated_path_distance_degrees = 7.5961;
        assert!(relevant_perfection(&body).contains("perfect=0Libra signEntries=? entryDepth=?"));
    }

    #[test]
    fn concise_numeric_display_is_bounded_without_changing_full_precision_or_flags() {
        let motion = json!({"estimateMethod":"sample_interval_linear_interpolation",
            "bracketHours":[10.123456789,11.987654321],"intraIntervalMotionResolved":false,
            "bodyMotion":[{"planet":"Venus","chartLongitudeDegrees":359.987654321,
                "estimatedPerfectionLongitudeDegrees":0.123456789,
                "estimatedSignedTravelDegrees":-0.000123456,"estimatedPathDistanceDegrees":0.0004321,
                "initialIntervalMeanSpeedDegreesPerDay":-0.0001,
                "perfectionBracketMeanSpeedDegreesPerDay":0.987654321,
                "sampledMotionReversalObserved":true,"zeroMeanMotionIntervalObserved":false}]});
        let original = motion.clone();
        let display = concise_event_motion(&motion);
        assert_eq!(motion, original);
        assert_eq!(display[0], original["estimateMethod"]);
        assert_eq!(display[2], false);
        assert_eq!(display[3][0][0], "Venus");
        assert_eq!(display[3][0][7], true);
        assert_eq!(display[3][0][8], false);
        for (column, field) in [
            "chartLongitudeDegrees",
            "estimatedPerfectionLongitudeDegrees",
            "estimatedSignedTravelDegrees",
            "estimatedPathDistanceDegrees",
            "initialIntervalMeanSpeedDegreesPerDay",
            "perfectionBracketMeanSpeedDegreesPerDay",
        ]
        .into_iter()
        .enumerate()
        {
            let value = original["bodyMotion"][0][field].as_f64().unwrap();
            let shown = display[3][0][column + 1].as_f64().unwrap();
            assert!((shown - value).abs() <= 0.0005);
            assert!((shown * 1000. - (shown * 1000.).round()).abs() < 1e-8);
        }
        for i in 0..2 {
            assert!(
                (display[1][i].as_f64().unwrap() - original["bracketHours"][i].as_f64().unwrap())
                    .abs()
                    <= 0.0005
            );
        }
        assert_eq!(display[3][0][5], 0);
        assert_eq!(display[3][0][8], false); // Rounded zero did not mint a station flag.
        assert_eq!(concise_decimal(&Value::Null), Value::Null);
        assert_eq!(concise_decimal(&json!("unknown")), json!("unknown"));
        assert!(
            serde_json::to_string(&display).unwrap().len()
                < serde_json::to_string(&original).unwrap().len()
        );
        let (input, _, _) = declared(Some("mother"));
        let compact = concise_reader_inputs(&input, "contact").unwrap();
        let chart: Value =
            serde_json::from_str(compact.get("chart_evidence").unwrap().as_str().unwrap()).unwrap();
        assert!(chart["legend"].as_str().unwrap().contains("error ≤0.0005"));
        assert!(
            chart["legend"]
                .as_str()
                .unwrap()
                .contains("Rounded zero is not stationarity")
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
