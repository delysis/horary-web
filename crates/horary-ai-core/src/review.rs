//! Version-one local review records. Preserve the captured context verbatim.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Note {
    id: String,
    created_at: String,
    category: String,
    note: String,
    context: Context,
}

#[derive(Debug, Deserialize, Serialize)]
struct Context {
    question: String,
    chart: Value,
    interpretation: Value,
    build: String,
    #[serde(
        default,
        rename = "methodStep",
        skip_serializing_if = "Option::is_none"
    )]
    method_step: Option<Value>,
}

fn parse(stored: &str) -> Result<Vec<Note>, String> {
    let notes: Vec<Note> =
        serde_json::from_str(stored).map_err(|e| format!("Cannot read saved review notes: {e}"))?;
    for note in &notes {
        if note.id.trim().is_empty()
            || note.note.trim().is_empty()
            || note.created_at.trim().is_empty()
        {
            return Err(
                "A saved review note is incomplete; the original data has been preserved.".into(),
            );
        }
    }
    Ok(notes)
}

pub fn read_notes_json(stored: &str) -> Result<String, String> {
    serde_json::to_string(&parse(stored)?).map_err(|e| e.to_string())
}

pub fn append_note_json(stored: &str, new_note: &str) -> Result<String, String> {
    let mut notes = parse(stored)?;
    let mut incoming = parse(&format!("[{new_note}]"))?;
    let note = incoming.pop().ok_or("Review note is required.")?;
    if notes.iter().any(|saved| saved.id == note.id) {
        return Err("This review note has already been saved.".into());
    }
    notes.push(note);
    serde_json::to_string(&notes).map_err(|e| e.to_string())
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = read_review_notes_json)]
    pub fn read(stored: &str) -> Result<String, JsValue> {
        super::read_notes_json(stored).map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = append_review_note_json)]
    pub fn append(stored: &str, note: &str) -> Result<String, JsValue> {
        super::append_note_json(stored, note).map_err(|e| JsValue::from_str(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn note(id: &str) -> String {
        json!({"id": id, "createdAt": "2026-09-10T22:00:00Z", "category": "Astrology", "note": "Use house six here.", "context": {"question": "Where is the cat?", "chart": {"houses": [6]}, "interpretation": null, "build": "test"}}).to_string()
    }

    #[test]
    fn preserves_earlier_notes_and_the_original_chart() {
        let first = append_note_json("[]", &note("one")).unwrap();
        let both = append_note_json(&first, &note("two")).unwrap();
        let parsed: Value = serde_json::from_str(&both).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["context"]["chart"], json!({"houses": [6]}));
        assert!(append_note_json(&both, &note("one")).is_err());
    }

    #[test]
    fn rejects_corrupt_storage_without_replacing_it() {
        for saved in ["broken", "{}", "[{}]", "[null]"] {
            assert!(read_notes_json(saved).is_err());
            assert!(append_note_json(saved, &note("one")).is_err());
        }
    }
}
