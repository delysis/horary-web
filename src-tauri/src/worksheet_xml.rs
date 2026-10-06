//! XML encoding of the same typed stage contract. Never executes text as tools.
#![forbid(unsafe_code)]
use quick_xml::{events::Event, Reader};
use serde_json::Value;

pub fn instruction() -> &'static str {
    "Return exactly one <worksheet> element. Object fields use their contract names as child elements. Arrays contain <item> elements. Strings are ordinary prose (escape & and <). Enums use their exact words. Numbers are decimal. A null field contains NULL. An empty array is an empty element. No attributes, comments, Markdown fences, external entities, or extra text. Use only this task's fields; never copy fields from a different task. The contract describes values, not a request to output JSON."
}

pub fn prompt(
    stage: crate::horary_lessons::Stage,
    matter: crate::horary_lessons::Matter,
    input: &Value,
    schema: &Value,
) -> Result<String, String> {
    let json = crate::horary_pipeline::prompt(stage, matter, input, schema)?;
    let mut messages: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let lesson=messages[0]["content"].as_str().ok_or("Missing lesson")?.replace("Return one JSON worksheet matching the supplied response schema, without fences or extra commentary.",instruction());
    let shape = shape("worksheet", &crate::horary_pipeline::schema(stage, &[]));
    messages[0]["content"]=Value::String(format!("{lesson}\n<format_example>\nThis illustrates ONLY this task's field structure. Replace the placeholders with the actual input's findings; do not copy placeholder conclusions.\n{shape}\n</format_example>"));
    Ok(messages.to_string())
}

fn shape(name: &str, schema: &Value) -> String {
    let contents = match schema["type"].as_str() {
        Some("object") => schema["properties"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, s)| shape(key, s))
            .collect(),
        Some("array") => String::new(),
        Some("string") => schema["enum"]
            .as_array()
            .and_then(|values| {
                values
                    .iter()
                    .find(|v| *v == "unestablished")
                    .or_else(|| values.first())
            })
            .and_then(Value::as_str)
            .unwrap_or("write this field from the actual input")
            .into(),
        _ => {
            if schema["type"]
                .as_array()
                .is_some_and(|types| types.iter().any(|t| t == "null"))
            {
                "NULL".into()
            } else {
                "1".into()
            }
        }
    };
    format!("<{name}>{contents}</{name}>")
}

struct Frame {
    name: String,
    contract: Value,
    children: Vec<(String, Value)>,
    text: String,
}
fn value(frame: Frame) -> Result<Value, String> {
    let types: Vec<_> = if let Some(t) = frame.contract["type"].as_str() {
        vec![t]
    } else {
        frame.contract["type"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    if types.contains(&"object") {
        if !frame.text.trim().is_empty() {
            return Err("An object cannot also contain prose.".into());
        }
        let mut values = serde_json::Map::new();
        for (key, value) in frame.children {
            if values.insert(key, value).is_some() {
                return Err("Duplicate worksheet field.".into());
            }
        }
        return Ok(Value::Object(values));
    }
    if types.contains(&"array") {
        if !frame.text.trim().is_empty() || frame.children.iter().any(|(key, _)| key != "item") {
            return Err("Array elements must be named item.".into());
        }
        return Ok(Value::Array(
            frame.children.into_iter().map(|(_, v)| v).collect(),
        ));
    }
    if !frame.children.is_empty() {
        return Err("A scalar cannot contain nested fields.".into());
    }
    let text = frame.text.trim();
    if text == "NULL" && types.contains(&"null") {
        return Ok(Value::Null);
    }
    if types.contains(&"string") {
        return Ok(Value::String(text.into()));
    }
    if types.contains(&"integer") {
        return text
            .parse::<u64>()
            .map(Value::from)
            .map_err(|_| "Invalid worksheet integer.".into());
    }
    if types.contains(&"number") {
        let n = text
            .parse::<f64>()
            .map_err(|_| "Invalid worksheet number")?;
        return serde_json::Number::from_f64(n)
            .map(Value::Number)
            .ok_or("Invalid worksheet number.".into());
    }
    Err("Unsupported worksheet value.".into())
}

pub fn parse(raw: &str, contract: &Value) -> Result<Value, String> {
    if raw.len() > 24000 {
        return Err("Worksheet XML exceeds its bounds.".into());
    }
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Frame> = Vec::new();
    let mut result = None;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let empty = matches!(&event, Event::Empty(_));
                let e = match event {
                    Event::Start(e) | Event::Empty(e) => e,
                    _ => unreachable!(),
                };
                if e.attributes().next().is_some() {
                    return Err("Worksheet attributes are not permitted.".into());
                }
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|e| e.to_string())?
                    .to_string();
                let schema = if let Some(parent) = stack.last() {
                    if parent.contract["type"] == "array" && name == "item" {
                        parent.contract["items"].clone()
                    } else {
                        parent.contract["properties"]
                            .get(&name)
                            .cloned()
                            .ok_or("Unexpected worksheet field")?
                    }
                } else {
                    if name != "worksheet" || result.is_some() {
                        return Err("Expected one worksheet element.".into());
                    }
                    contract.clone()
                };
                if stack.len() >= 8 {
                    return Err("Worksheet nesting exceeds its bounds.".into());
                }
                let frame = Frame {
                    name,
                    contract: schema,
                    children: Vec::new(),
                    text: String::new(),
                };
                if empty {
                    let name = frame.name.clone();
                    let v = value(frame)?;
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push((name, v));
                    } else {
                        result = Some(v);
                    }
                } else {
                    stack.push(frame);
                }
            }
            Event::Text(e) => {
                let decoded = e.decode().map_err(|e| e.to_string())?;
                let text = quick_xml::escape::unescape(&decoded).map_err(|e| e.to_string())?;
                if let Some(frame) = stack.last_mut() {
                    frame.text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err("Prose outside the worksheet.".into());
                }
            }
            Event::GeneralRef(e) => {
                // Entities are handled as separate events by current quick-xml.
                let name = e.decode().map_err(|e| e.to_string())?;
                let decoded = match name.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => return Err("Only standard text entities are permitted.".into()),
                };
                stack
                    .last_mut()
                    .ok_or("Entity outside worksheet")?
                    .text
                    .push_str(decoded);
            }
            Event::End(e) => {
                let frame = stack.pop().ok_or("Unexpected closing field")?;
                if frame.name.as_bytes() != e.name().as_ref() {
                    return Err("Mismatched worksheet fields.".into());
                }
                let name = frame.name.clone();
                let v = value(frame)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push((name, v));
                } else {
                    result = Some(v);
                }
            }
            Event::Eof => break,
            _ => return Err("Unexpected XML construct.".into()),
        }
    }
    if !stack.is_empty() {
        return Err("Incomplete worksheet.".into());
    }
    result.ok_or("No worksheet was returned.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn typed_xml_accepts_prose_arrays_and_null_but_rejects_ambiguous_authority() {
        let contract = json!({"type":"object","properties":{"prose":{"type":"string"},"number":{"type":["number","null"]},"facts":{"type":"array","items":{"type":"string"}}}});
        let v=parse("<worksheet><prose>A &amp; B</prose><number>NULL</number><facts><item>e1</item></facts></worksheet>",&contract).unwrap();
        assert_eq!(v, json!({"prose":"A & B","number":null,"facts":["e1"]}));
        assert!(parse("<!DOCTYPE worksheet><worksheet/>", &contract).is_err());
        assert!(parse(
            "<worksheet><prose>one</prose><prose>two</prose></worksheet>",
            &contract
        )
        .is_err());
        assert!(parse("<worksheet><unknown>value</unknown></worksheet>", &contract).is_err());
    }

    #[test]
    #[ignore = "Real Gemma worksheet comparison; writes every output and validation result to HORARY_WORKSHEET_EVIDENCE"]
    #[cfg(feature = "native-llama")]
    fn compare_real_typed_worksheets() {
        use crate::{
            horary_lessons::{Matter, Stage},
            horary_pipeline::{self as pipeline, Brief},
            native_llama_worker::{
                generate_native, generate_native_batch, start_native_llama_from_path,
                stop_native_llama, NativeGenerateOptions, NativeLlamaState,
            },
            reading_method::Fact,
        };
        use serde_json::json;
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let evidence = std::path::PathBuf::from(
            std::env::var_os("HORARY_WORKSHEET_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&evidence).expect("Preserve earlier trials");
        let state = NativeLlamaState::default();
        start_native_llama_from_path(
            &state,
            "worksheet-comparison".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(
                json!({"modelId":"worksheet-comparison","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let prior = Brief {
            question: "Will I get married in the next year?".into(),
            matter: Matter::Relationship,
            question_kind: "event".into(),
            horizon: "within the next year".into(),
            ..Default::default()
        };
        let facts = [
            Fact {
                id: "e1".into(), kind: "reception".into(), label: "Mars → Venus".into(),
                detail: "Mars regards Venus by domicile (major positive). This is Mars's regard for Venus, not the reverse.".into(),
                planets: vec!["Mars".into(), "Venus".into()], event: None,
            },
            Fact {
                id: "e2".into(), kind: "reception".into(), label: "Venus → Mars".into(),
                detail: "Venus regards Mars by detriment (major negative). This is Venus's regard for Mars, not the reverse.".into(),
                planets: vec!["Venus".into(), "Mars".into()], event: None,
            },
            Fact {
                id: "e3".into(), kind: "boundary".into(), label: "Calculation boundary".into(),
                detail: "Hourly contact search covers seven days. No relevant candidate was found. Absence cannot establish a one-year negative forecast.".into(),
                planets: vec![], event: None,
            },
        ];
        let cases = vec![
            (
                "city_reply_preserves_question",
                Stage::Intake,
                json!({"retained_brief":prior,"canonical_question":prior.question,"chart_exists":false,"latest_words":"Woodbridge, Virginia, United States.","last_reader_question":"Where should this question be cast?","spoken_input":false}),
                Vec::new(),
            ),
            (
                "literal_place_ids",
                Stage::Place,
                json!({"brief":{"place_request":"London, United Kingdom"},"candidates":[{"id":"candidate-uk","name":"London","country":"United Kingdom","timezone":"Europe/London"},{"id":"candidate-ca","name":"London","country":"Canada","timezone":"America/Toronto"}]}),
                Vec::new(),
            ),
            (
                "historical_civil_time",
                Stage::Moment,
                json!({"requested_question_moment":"The question was understood on 2026-01-14 at 14:30.","question_context":"Historical question, not a reported event date.","selected_timezone":"Europe/London","current_local_clock":"2026-10-05T18:00","existing_chart_moment":null}),
                Vec::new(),
            ),
            (
                "directed_reception",
                Stage::Reception,
                json!({"brief":{"question":"Does this person want a relationship with me?","matter":"relationship"},"roles":[{"label":"You","planet":"Mars"},{"label":"The other person","planet":"Venus"}],"facts":facts[..2]}),
                facts[..2].to_vec(),
            ),
            (
                "short_window_not_year_verdict",
                Stage::Judgment,
                json!({"brief":prior,"roles":[{"label":"You","planet":"Mars"},{"label":"Prospective partner","planet":"Venus"}],"condition":{"summary":"Not supplied for this fixture."},"reception":{"summary":"Not supplied for this fixture; no motive is established."},"contacts":{"basis":"no_candidate_covered","candidate_ids":[]},"timing":{"timing_status":"unestablished"},"facts":[facts[2]]}),
                vec![facts[2].clone()],
            ),
        ];
        let mut records = Vec::new();
        for (name, stage, input, facts) in cases {
            let schema = pipeline::schema(stage, &facts);
            let json_prompt =
                pipeline::prompt(stage, Matter::Relationship, &input, &schema).unwrap();
            let xml_prompt = prompt(stage, Matter::Relationship, &input, &schema).unwrap();
            let wall = std::time::Instant::now();
            let constrained = generate_native(
                &state,
                json_prompt.clone(),
                NativeGenerateOptions {
                    max_tokens: 1100,
                    temperature: 0.,
                    response_schema: Some(schema.to_string()),
                    cache_lesson: true,
                    ..Default::default()
                },
            );
            let constrained_wall = wall.elapsed().as_millis();
            let wall = std::time::Instant::now();
            let batch = generate_native_batch(
                &state,
                vec![(json_prompt.clone(), 1100), (xml_prompt.clone(), 1100)],
                NativeGenerateOptions {
                    temperature: 0.,
                    cache_lesson: true,
                    ..Default::default()
                },
            );
            let pair_wall = wall.elapsed().as_millis();
            let mut outputs = vec![(
                "json_constrained",
                constrained,
                constrained_wall,
                json_prompt.clone(),
            )];
            match batch {
                Ok(values) => {
                    outputs.push(("json", Ok(values[0].clone()), pair_wall, json_prompt));
                    outputs.push(("xml", Ok(values[1].clone()), pair_wall, xml_prompt.clone()));
                }
                Err(e) => {
                    outputs.push((
                        "json",
                        Err(crate::llama::LlamaError {
                            message: e.message.clone(),
                        }),
                        pair_wall,
                        json_prompt,
                    ));
                    outputs.push(("xml", Err(e), pair_wall, xml_prompt.clone()));
                }
            }
            for (format, result, wall, prompt) in outputs {
                let parsed = result
                    .as_ref()
                    .map_err(|e| e.message.clone())
                    .and_then(|r| {
                        if format == "xml" {
                            parse(&r.content, &schema)
                        } else {
                            pipeline::decode_json(&r.content)
                        }
                    });
                let validation = parsed
                    .as_ref()
                    .map_err(|e| e.clone())
                    .and_then(|v| pipeline::validate(stage, v, &facts));
                let correct = validation.is_ok()
                    && parsed.as_ref().is_ok_and(|v| match name {
                        "city_reply_preserves_question" => {
                            v["question"] == prior.question
                                && v["horizon"] == prior.horizon
                                && v["place_request"]
                                    .as_str()
                                    .is_some_and(|s| s.contains("Woodbridge"))
                        }
                        "literal_place_ids" => {
                            v["mode"] == "select" && v["place_id"] == "candidate-uk"
                        }
                        "historical_civil_time" => {
                            v["mode"] == "explicit" && v["local_time"] == "2026-01-14T14:30"
                        }
                        "directed_reception" => v["checks"]["direction"]["state"] == "supported",
                        "short_window_not_year_verdict" => {
                            v["verdict"] == "unresolved" || v["verdict"] == "mixed"
                        }
                        _ => false,
                    });
                let record = json!({"case":name,"stage":stage,"format":format,"input":input,"prompt":serde_json::from_str::<Value>(&prompt).unwrap(),"promptSha256":crate::horary_lessons::digest(&prompt),"schema":schema,"result":result.as_ref().map_err(|e|e.message.as_str()),"parsed":parsed,"validation":validation,"semanticChecksPass":correct,"wallMs":wall,"authorship":"Actual local Gemma output over explicitly authored facts; reception prose still needs manual SME review"});
                std::fs::write(
                    evidence.join(format!("trial-{:02}.json", records.len())),
                    serde_json::to_vec_pretty(&record).unwrap(),
                )
                .unwrap();
                eprintln!("WORKSHEET {name} {format} pass={correct}");
                records.push(record);
            }
        }
        stop_native_llama(&state).unwrap();
        std::fs::write(
            evidence.join("summary.json"),
            serde_json::to_vec_pretty(&records).unwrap(),
        )
        .unwrap();
    }
}
