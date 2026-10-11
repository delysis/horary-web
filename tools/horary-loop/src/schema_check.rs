//! Check the exact submitted output schema before expanding short evidence refs.
#![forbid(unsafe_code)]
use crate::store::Result;
use serde_json::Value;

pub fn validate(schema: &Value, value: &Value) -> Result<()> {
    visit(schema, value, "$", 0)
}
fn matches_type(kind: &str, value: &Value) -> Result<bool> {
    Ok(match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value
                    .as_f64()
                    .is_some_and(|n| n.is_finite() && n.fract() == 0.0)
        }
        _ => return Err(format!("Unsupported schema type {kind}")),
    })
}
fn visit(schema: &Value, value: &Value, path: &str, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err("Output schema recursion exceeds 64 levels".into());
    }
    if schema == &Value::Bool(true) {
        return Ok(());
    }
    if schema == &Value::Bool(false) {
        return Err(format!("Output schema forbids {path}"));
    }
    let fields = schema
        .as_object()
        .ok_or("Output schema must be an object or boolean")?;
    for key in fields.keys() {
        if ![
            "$schema",
            "title",
            "description",
            "type",
            "properties",
            "required",
            "additionalProperties",
            "items",
            "enum",
            "minLength",
            "maxLength",
            "minItems",
            "maxItems",
            "minimum",
            "maximum",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("Unsupported output schema keyword {key}"));
        }
    }
    if let Some(kinds) = fields.get("type") {
        let valid = if let Some(kind) = kinds.as_str() {
            matches_type(kind, value)?
        } else {
            let mut valid = false;
            for kind in kinds.as_array().ok_or("Invalid schema type union")? {
                valid |= matches_type(kind.as_str().ok_or("Invalid schema type name")?, value)?;
            }
            valid
        };
        if !valid {
            return Err(format!("Output type does not match schema at {path}"));
        }
    }
    if let Some(choices) = fields.get("enum") {
        if !choices
            .as_array()
            .ok_or("Invalid schema enum")?
            .contains(value)
        {
            return Err(format!("Output enum does not match schema at {path}"));
        }
    }
    if let Some(object) = value.as_object() {
        let properties = fields.get("properties").and_then(Value::as_object);
        if let Some(required) = fields.get("required") {
            for key in required.as_array().ok_or("Invalid schema required list")? {
                let key = key.as_str().ok_or("Invalid required property")?;
                if !object.contains_key(key) {
                    return Err(format!("Missing output property {path}/{key}"));
                }
            }
        }
        for (key, field) in object {
            if let Some(property) = properties.and_then(|p| p.get(key)) {
                visit(property, field, &format!("{path}/{key}"), depth + 1)?;
            } else if let Some(additional) = fields.get("additionalProperties") {
                visit(additional, field, &format!("{path}/{key}"), depth + 1)?;
            }
        }
    }
    if let Some(items) = value.as_array() {
        for (keyword, invalid) in [("minItems", false), ("maxItems", true)] {
            if let Some(bound) = fields.get(keyword) {
                let bound = bound.as_u64().ok_or("Invalid array size bound")?;
                if if invalid {
                    items.len() as u64 > bound
                } else {
                    (items.len() as u64) < bound
                } {
                    return Err(format!("Output {keyword} violates schema at {path}"));
                }
            }
        }
        if let Some(item_schema) = fields.get("items") {
            for (index, item) in items.iter().enumerate() {
                visit(item_schema, item, &format!("{path}/{index}"), depth + 1)?;
            }
        }
    }
    if let Some(text) = value.as_str() {
        for (keyword, maximum) in [("minLength", false), ("maxLength", true)] {
            if let Some(bound) = fields.get(keyword) {
                let bound = bound.as_u64().ok_or("Invalid string size bound")?;
                let length = text.chars().count() as u64;
                if if maximum {
                    length > bound
                } else {
                    length < bound
                } {
                    return Err(format!("Output {keyword} violates schema at {path}"));
                }
            }
        }
    }
    if let Some(number) = value.as_f64() {
        for (keyword, maximum) in [("minimum", false), ("maximum", true)] {
            if let Some(bound) = fields.get(keyword) {
                let bound = bound.as_f64().ok_or("Invalid numeric bound")?;
                if if maximum {
                    number > bound
                } else {
                    number < bound
                } {
                    return Err(format!("Output {keyword} violates schema at {path}"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn original_required_keys_refs_bounds_and_extra_fields_are_checked() {
        let schema = json!({"type":"object","required":["evidence_refs"],"properties":{"evidence_refs":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":2}},"additionalProperties":false});
        validate(&schema, &json!({"evidence_refs":["r1"]})).unwrap();
        for value in [
            json!({"evidence":[]}),
            json!({"evidence_refs":[]}),
            json!({"evidence_refs":["r1","r2","r3"]}),
            json!({"evidence_refs":[1]}),
            json!({"evidence_refs":["r1"],"extra":true}),
        ] {
            assert!(validate(&schema, &value).is_err());
        }
        assert!(validate(&json!({"oneOf":[]}), &Value::Null).is_err());
    }
}
