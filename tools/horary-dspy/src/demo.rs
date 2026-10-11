//! Compact saved demonstrations, never the inputs of a live oracle call.
//! Raw teacher responses remain in the provider artifacts; this is a derived
//! prompt example with exact native rows retained for every cited fact.
use anyhow::{Context, Result, ensure};
use dsrust::Example;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;

const OMIT: &[&str] = &[
    "source_text",
    "requirements",
    "catalogue",
    "__score",
    "__feedback",
    "__passed",
    "__review_source",
];
const HEADER: &str = "ID\tKIND\tLABEL\tCALCULATED TESTIMONY";
const SUBSET: &str = "Demonstration only: native facts retain all cited IDs, planet positions, house cusps, Moon rows and calculation boundaries. Omitted rows are not evidence of absence in the original chart.";

/// Return a separate, compact demonstration while preserving its declared
/// input/label split. Unknown citations and malformed chart rows fail instead
/// of supplying an example with unsupported evidence.
pub fn compact(demonstration: &Example) -> Result<Example> {
    demonstration.inputs()?;
    let ids = Regex::new(r"\be[0-9]+\b")?;
    let moon = Regex::new(r"\bMoon\b")?;
    let mut cited = BTreeSet::new();
    for field in ["evidence", "analysis", "answer"] {
        if let Some(value) = demonstration.get(field) {
            let text = value
                .as_str()
                .with_context(|| format!("Demonstration {field} must be text"))?;
            cited.extend(ids.find_iter(text).map(|found| found.as_str().to_owned()));
        }
    }
    ensure!(
        cited.is_empty() || demonstration.get("chart_evidence").is_some(),
        "Demonstration cites native evidence without an actual chart"
    );
    let mut fields = Vec::new();
    let mut inputs = Vec::new();
    for (name, value) in demonstration.fields() {
        if OMIT.contains(&name) {
            continue;
        }
        let value = if name == "chart_evidence" {
            project(value, &cited, &moon)?
        } else {
            value.clone()
        };
        if demonstration.is_input(name) {
            inputs.push(name.to_owned());
        }
        fields.push((name.to_owned(), value));
    }
    Ok(Example::new(fields).with_inputs(inputs))
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix('e')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn foundational(kind: &str, label: &str, detail: &str, moon: &Regex) -> bool {
    matches!(kind, "position" | "house" | "moon" | "boundary")
        || moon.is_match(label)
        || moon.is_match(detail)
}

fn project(value: &Value, cited: &BTreeSet<String>, moon: &Regex) -> Result<Value> {
    let encoded = value.is_string();
    let mut chart = if let Some(text) = value.as_str() {
        serde_json::from_str(text).context("Demonstration chart_evidence is not JSON")?
    } else {
        value.clone()
    };
    let chart = chart
        .as_object_mut()
        .context("Demonstration chart_evidence must be an object")?;
    ensure!(
        chart.contains_key("facts") != chart.contains_key("native_facts"),
        "Demonstration chart needs one actual native fact representation"
    );
    let mut actual = BTreeSet::new();
    if let Some(facts) = chart.get_mut("facts") {
        let table = facts
            .as_str()
            .context("Demonstration facts must be the native TSV table")?;
        let mut lines = table.split_inclusive('\n');
        let header = lines.next().context("Demonstration fact table is empty")?;
        ensure!(
            header.trim_end_matches('\n') == HEADER,
            "Demonstration native fact header differs"
        );
        let mut retained = header.to_owned();
        for line in lines {
            let row = line.trim_end_matches('\n');
            let columns = row.splitn(4, '\t').collect::<Vec<_>>();
            ensure!(
                columns.len() == 4 && valid_id(columns[0]),
                "Malformed demonstration native fact row"
            );
            let id = columns[0];
            ensure!(
                actual.insert(id.to_owned()),
                "Duplicate native fact ID {id}"
            );
            if cited.contains(id) || foundational(columns[1], columns[2], columns[3], moon) {
                retained.push_str(line);
            }
        }
        *facts = Value::String(retained);
    } else {
        let facts = chart
            .get_mut("native_facts")
            .context("No demonstration native facts")?;
        let rows = facts
            .as_array()
            .context("Demonstration native_facts must be an array")?;
        let mut retained = Vec::new();
        for row in rows {
            let text = |name| {
                row.get(name)
                    .and_then(Value::as_str)
                    .with_context(|| format!("Demonstration native fact lacks {name}"))
            };
            let id = text("id")?;
            let kind = text("kind")?;
            let label = text("label")?;
            let detail = text("detail")?;
            ensure!(valid_id(id), "Invalid native fact ID {id}");
            ensure!(
                actual.insert(id.to_owned()),
                "Duplicate native fact ID {id}"
            );
            let has_moon = row
                .get("planets")
                .and_then(Value::as_array)
                .is_some_and(|planets| planets.iter().any(|planet| planet == "Moon"));
            if cited.contains(id) || foundational(kind, label, detail, moon) || has_moon {
                retained.push(row.clone());
            }
        }
        *facts = Value::Array(retained);
    }
    for id in cited {
        ensure!(
            actual.contains(id),
            "Demonstration cites absent native fact {id}"
        );
    }
    chart.insert(
        "demonstration_projection".into(),
        Value::String(SUBSET.into()),
    );
    let chart = Value::Object(chart.clone());
    if encoded {
        Ok(Value::String(serde_json::to_string(&chart)?))
    } else {
        Ok(chart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chart() -> Value {
        json!({
            "anchor": {"utc":"2031-04-17T11:30:00Z","latitude":38.657,"longitude":-77.249},
            "search_limitations":"Synthetic finite coverage; absence is unknown.",
            "moon":{"available":false,"isVoid":null},
            "event_context":[{"place":"A stated event venue"}],
            "facts": format!("{HEADER}\n{}", [
                "e0\tposition\tSaturn\tSaturn at 4.125° Cancer; source detail unchanged.\n",
                "e1\thouse\tHouse 1\tCusp 12.500° Capricorn; source detail unchanged.\n",
                "e2\tcondition\tCalculated testimony\tVenus: literal adverse testimony.\n",
                "e3\treception\tMars → Venus\tMars regards Venus: exact original direction.\n",
                "e4\tevent\tMars · Venus\tA cited event with literal timing limits.\n",
                "e5\tcondition\tCalculated testimony\tMoon: literal source condition.\n",
                "e6\tevent\tMoon · Saturn\tExact lunar event order remains.\n",
                "e7\tmoon\tThe Moon's next contact\tCoverage unknown.\n",
                "e8\tboundary\tWhat this chart can establish\tExact original calculation boundary.\n",
                "e9\treception\tJupiter → Saturn\tUncited, non-lunar detail.\n",
            ].concat())
        })
    }

    fn reading(chart: Value) -> Example {
        dsrust::example! {
            question:"Will this synthetic matter proceed?",
            consultation:"Only actual sourced inputs.",
            chart_evidence:serde_json::to_string(&chart).unwrap(),
            source_text:"Repeated primary source context.",
            requirements:"Repeated contract.",
            catalogue:"Repeated method index.",
            status:"answer",
            evidence:"e2",
            analysis:"e3 gives the actual directed reception.",
            answer:"The occasion is limited by e4.",
            unresolved:"",
            __score:1.0,
            __feedback:"Private independent judgment.",
            __passed:true,
            __review_source:"Private classification-review primary text."
        }
        .with_inputs([
            "question",
            "consultation",
            "chart_evidence",
            "source_text",
            "requirements",
            "catalogue",
        ])
    }

    fn projected(example: &Example) -> Value {
        serde_json::from_str(example.get("chart_evidence").unwrap().as_str().unwrap()).unwrap()
    }

    #[test]
    fn removes_only_named_context_and_grades_without_changing_raw_demo_or_split() {
        let raw = reading(chart());
        let before = raw.clone();
        let compacted = compact(&raw).unwrap();
        assert_eq!(raw, before);
        for field in OMIT {
            assert!(compacted.get(field).is_none());
        }
        for field in [
            "question",
            "consultation",
            "status",
            "answer",
            "analysis",
            "evidence",
            "unresolved",
        ] {
            assert_eq!(compacted.get(field), raw.get(field));
        }
        assert_eq!(compacted.inputs().unwrap().len(), 3);
        assert!(!compacted.is_input("answer"));
        assert_eq!(
            compacted.labels().unwrap().get("status"),
            Some(&json!("answer"))
        );
    }

    #[test]
    fn retains_all_output_citations_foundations_moon_and_exact_chart_metadata() {
        let source = chart();
        let compacted = projected(&compact(&reading(source.clone())).unwrap());
        let actual = source["facts"].as_str().unwrap();
        let kept = compacted["facts"].as_str().unwrap();
        for line in actual
            .split_inclusive('\n')
            .filter(|line| !line.starts_with("e9\t"))
        {
            assert!(kept.contains(line));
        }
        assert!(!kept.contains("e9\t"));
        for key in ["anchor", "search_limitations", "moon", "event_context"] {
            assert_eq!(compacted[key], source[key]);
        }
        assert_eq!(compacted["demonstration_projection"], SUBSET);
    }

    #[test]
    fn any_absent_citation_fails_without_altering_the_original() {
        for field in ["evidence", "analysis", "answer"] {
            let mut raw = reading(chart());
            raw.set(field, json!("This claims e999."));
            let original = raw.clone();
            assert!(
                compact(&raw)
                    .unwrap_err()
                    .to_string()
                    .contains("absent native fact e999")
            );
            assert_eq!(raw, original);
        }
    }

    #[test]
    fn duplicate_and_malformed_uncited_rows_are_not_silently_discarded() {
        for row in [
            "e9\treception\tOther\tDuplicate.\n",
            "not-an-id\tcondition\tOther\tMalformed.\n",
            "e10\tcondition\tNo detail\n",
        ] {
            let mut source = chart();
            source["facts"] = json!(format!("{}{row}", source["facts"].as_str().unwrap()));
            assert!(compact(&reading(source)).is_err());
        }
        let mut source = chart();
        source["facts"] = json!("Not the native header\n");
        assert!(compact(&reading(source)).is_err());
    }

    #[test]
    fn structured_native_rows_preserve_exact_typed_details_and_unknown_metadata() {
        let rows = json!([
            {"id":"e0","kind":"position","label":"Saturn","detail":"Exact 4.125° position.","planets":["Saturn"]},
            {"id":"e1","kind":"house","label":"House 1","detail":"Exact cusp.","planets":["Saturn"]},
            {"id":"e2","kind":"condition","label":"Calculated testimony","detail":"Exact condition.","planets":["Venus"],"condition_facet":"motion"},
            {"id":"e3","kind":"reception","label":"Mars → Venus","detail":"Exact direction.","planets":["Mars","Venus"],"reception":{"guestPlanet":"Mars","hostPlanet":"Venus","qualities":[{"dignity":"domicile","polarity":"positive"}]}},
            {"id":"e4","kind":"event","label":"Mars · Venus","detail":"Exact occasion.","event":{"withinCurrentSigns":false,"astronomicalHours":107.77218518861261}},
            {"id":"e5","kind":"condition","label":"Calculated testimony","detail":"Exact supplied motion.","planets":["Moon"]},
            {"id":"e6","kind":"boundary","label":"Calculation limits","detail":"Exact limit."},
            {"id":"e7","kind":"reception","label":"Jupiter → Saturn","detail":"Uncited.","planets":["Jupiter","Saturn"]}
        ]);
        let source = json!({"native_facts":rows,"anchor":{"utc":"2031-04-17T11:30:00Z"},"limitations":"Exact limits.","event_search":{"available":false,"events":[]},"calculated_testimony":["Unchanged ancillary data."]});
        let compacted = projected(&compact(&reading(source.clone())).unwrap());
        assert_eq!(
            compacted["native_facts"],
            json!(rows.as_array().unwrap()[..7])
        );
        for key in [
            "anchor",
            "limitations",
            "event_search",
            "calculated_testimony",
        ] {
            assert_eq!(compacted[key], source[key]);
        }
        let mut object_demo = reading(source.clone());
        object_demo.set("chart_evidence", source);
        assert!(
            compact(&object_demo)
                .unwrap()
                .get("chart_evidence")
                .unwrap()
                .is_object()
        );
    }

    #[test]
    fn extraction_demo_has_no_chart_or_new_facts_and_still_loses_private_context() {
        let raw = dsrust::example! {question:"A synthetic source.",context:"Actual device.",requirements:"Repeated source.",facts:"[]",inferences:"None.",__score:1.0}.with_inputs(["question","context","requirements"]);
        let compacted = compact(&raw).unwrap();
        assert_eq!(compacted.get("facts"), raw.get("facts"));
        assert_eq!(compacted.get("inferences"), raw.get("inferences"));
        assert!(compacted.get("chart_evidence").is_none());
        assert!(compacted.get("__score").is_none());
        assert_eq!(compacted.inputs().unwrap().len(), 2);
    }

    #[test]
    fn malformed_chart_and_undeclared_inputs_fail_instead_of_guessing() {
        let mut raw = reading(chart());
        raw.set("chart_evidence", json!("Not JSON"));
        assert!(compact(&raw).is_err());
        raw.set(
            "chart_evidence",
            json!({"anchor":{"utc":"2031-04-17T11:30:00Z"}}),
        );
        assert!(compact(&raw).is_err());
        let undeclared = dsrust::example! {question:"No declared inputs."};
        assert!(compact(&undeclared).is_err());
        let no_chart = dsrust::example! {question:"An actual question.",answer:"Unsupported e999."}
            .with_inputs(["question"]);
        assert!(compact(&no_chart).is_err());
    }

    #[test]
    fn structured_cited_rows_still_require_actual_details_and_unique_ids() {
        for rows in [
            json!([{"id":"e2","kind":"condition","label":"No detail"}]),
            json!([
                {"id":"e2","kind":"condition","label":"First","detail":"Exact first detail."},
                {"id":"e2","kind":"condition","label":"Second","detail":"Conflicting duplicate."}
            ]),
        ] {
            let mut raw = reading(json!({"native_facts":rows}));
            raw.set("evidence", json!("e2"));
            raw.set("analysis", json!("A direct analysis."));
            raw.set("answer", json!("A direct answer."));
            assert!(compact(&raw).is_err());
        }
    }

    #[test]
    fn current_native_fact_table_projects_without_reconstructing_any_detail() {
        let at = chrono::DateTime::parse_from_rfc3339("2031-04-17T11:30:00Z").unwrap();
        let chart = horary_ai_core::astronomy::chart(at.timestamp_millis() as f64, 38.657, -77.249)
            .unwrap();
        let facts = crate::native_evidence::facts(Some(&chart));
        let selected = facts
            .iter()
            .find(|fact| fact.kind == "condition")
            .unwrap()
            .id
            .clone();
        let table = crate::catalogue::compact_native_facts(&facts);
        let source = json!({"facts":table,"anchor":{"utc":at.to_rfc3339()},"moon":chart["derived"]["voidOfCourseMoon"],"search_limitations":chart["calculationNote"]});
        let mut raw = reading(source);
        for name in ["evidence", "analysis", "answer"] {
            raw.set(name, json!(format!("Actual evidence {selected}.")));
        }
        let compacted = projected(&compact(&raw).unwrap());
        let retained = compacted["facts"].as_str().unwrap();
        let moon = Regex::new(r"\bMoon\b").unwrap();
        for fact in facts {
            let line = format!(
                "{}\t{}\t{}\t{}\n",
                fact.id, fact.kind, fact.label, fact.detail
            );
            let expected =
                fact.id == selected || foundational(&fact.kind, &fact.label, &fact.detail, &moon);
            assert_eq!(retained.contains(&line), expected, "{}", fact.id);
        }
    }

    #[test]
    fn uncited_bulk_shrinks_without_changing_utf8_details_and_compaction_is_idempotent() {
        let mut source = chart();
        let mut table = source["facts"].as_str().unwrap().to_owned();
        for id in 100..300 {
            table.push_str(&format!(
                "e{id}\treception\tJupiter → Saturn\t{}\n",
                "Synthetic uncited detail. ".repeat(20)
            ));
        }
        table.push_str("e300\tcondition\tVénus\tÉtat exact, 4.125°; literal UTF-8 remains.\n");
        source["facts"] = json!(table);
        let mut raw = reading(source);
        raw.set("evidence", json!("e2; e300"));
        let compacted = compact(&raw).unwrap();
        let kept = projected(&compacted);
        assert!(
            kept["facts"]
                .as_str()
                .unwrap()
                .contains("e300\tcondition\tVénus\tÉtat exact, 4.125°; literal UTF-8 remains.\n")
        );
        assert!(
            compacted
                .get("chart_evidence")
                .unwrap()
                .as_str()
                .unwrap()
                .len()
                * 10
                < raw.get("chart_evidence").unwrap().as_str().unwrap().len()
        );
        assert_eq!(compact(&compacted).unwrap(), compacted);
    }
}
