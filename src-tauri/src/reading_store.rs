//! Whole-reading snapshots. Opening a new leaf never discards its predecessor.
#![forbid(unsafe_code)]
use crate::conversation::{Message, Session};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReading {
    pub id: String,
    pub title: String,
    pub saved_at_ms: u64,
    pub has_chart: bool,
}

fn identity() -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok(format!("{}-{}", now.as_nanos(), std::process::id()))
}

pub fn has_content(session: &Session) -> bool {
    !session.messages.is_empty()
        || !session.question.is_empty()
        || session.chart.is_some()
        || !session.sections.is_empty()
        || !session.revisions.is_empty()
}

fn saved_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() > 60
        || id.split('-').count() != 2
        || !id
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("That saved reading is unavailable.".into());
    }
    Ok(dir.join("readings").join(format!("{id}.json")))
}

pub fn read(path: &Path) -> Result<Session, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("This saved reading is too large to open safely.".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Could not read the saved reading: {e}"))
}

pub fn write(path: &Path, session: &Session, new_file: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(session).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("This reading is full. Its saved copy is kept.".into());
    }
    let parent = path
        .parent()
        .ok_or("The reading has no storage directory.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    if new_file {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    }
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn keep(dir: &Path, previous: &Session) -> Result<Vec<SavedReading>, String> {
    let mut saved = previous.saved_readings.clone();
    if !has_content(previous) {
        return Ok(saved);
    }
    let id = identity()?;
    let title = if previous.question.trim().is_empty() {
        previous
            .messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| m.text.trim())
            .unwrap_or("Unfinished reading")
    } else {
        previous.question.trim()
    };
    let summary = SavedReading {
        id: id.clone(),
        title: title.chars().take(120).collect(),
        saved_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64,
        has_chart: previous.chart.is_some(),
    };
    write(&saved_path(dir, &id)?, previous, true)?;
    saved.push(summary);
    Ok(saved)
}

pub fn fresh(dir: &Path, previous: &Session, question: Option<&str>) -> Result<Session, String> {
    let question = question.unwrap_or("").trim();
    if question.len() > 8000 {
        return Err("The new question is too long.".into());
    }
    let mut next = Session {
        reading_id: identity()?,
        snapshot_id: previous.snapshot_id,
        saved_readings: keep(dir, previous)?,
        question: question.into(),
        ..Default::default()
    };
    if !question.is_empty() {
        next.messages.push(Message {
            role: "user".into(),
            text: question.into(),
        });
    }
    Ok(next)
}

pub fn reopen(dir: &Path, previous: &Session, id: &str) -> Result<Session, String> {
    if !previous.saved_readings.iter().any(|saved| saved.id == id) {
        return Err("That saved reading is unavailable.".into());
    }
    // Validate the target before saving or changing the current leaf.
    let mut next = read(&saved_path(dir, id)?)?;
    next.saved_readings = keep(dir, previous)?;
    next.reading_id = identity()?;
    next.snapshot_id = previous.snapshot_id;
    next.status.clear();
    next.busy = false;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_and_archiving_preserves_recorded_float_bits() {
        let dir = tempfile::tempdir().unwrap();
        let expected = [
            0.9857229285553331_f64,
            13.855763302433843,
            216.67872924814046,
            9.247724793621359,
        ];
        let original = Session {
            chart: Some(serde_json::json!({"samples":expected})),
            audit: vec![serde_json::json!({"samples":expected})],
            ..Default::default()
        };
        let path = dir.path().join("conversation.json");
        write(&path, &original, false).unwrap();
        let loaded = read(&path).unwrap();
        let fresh = fresh(dir.path(), &loaded, None).unwrap();
        let archived = read(&saved_path(dir.path(), &fresh.saved_readings[0].id).unwrap()).unwrap();
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(
                archived.chart.as_ref().unwrap()["samples"][index]
                    .as_f64()
                    .unwrap()
                    .to_bits(),
                value.to_bits()
            );
            assert_eq!(
                archived.audit[0]["samples"][index]
                    .as_f64()
                    .unwrap()
                    .to_bits(),
                value.to_bits()
            );
        }
    }
    #[test]
    fn fresh_leaf_preserves_full_conversation_and_reopens_it_without_old_scope() {
        let dir = tempfile::tempdir().unwrap();
        let original = Session {
            reading_id: "old-scope".into(),
            snapshot_id: 31,
            messages: vec![Message {
                role: "user".into(),
                text: "Where is my ring?".into(),
            }],
            audit: vec![serde_json::json!({"failure":"kept for review"})],
            ..Default::default()
        };
        write(&dir.path().join("conversation.json"), &original, false).unwrap();
        let next = fresh(dir.path(), &original, None).unwrap();
        assert!(
            next.messages.is_empty()
                && next.chart.is_none()
                && next.audit.is_empty()
                && next.progress.is_empty()
        );
        assert_ne!(next.reading_id, original.reading_id);
        assert_eq!(next.snapshot_id, 31);
        assert_eq!(next.saved_readings.len(), 1);
        let saved = read(&saved_path(dir.path(), &next.saved_readings[0].id).unwrap()).unwrap();
        assert_eq!(saved.messages[0].text, original.messages[0].text);
        assert_eq!(saved.audit, original.audit);
        let restored = reopen(dir.path(), &next, &next.saved_readings[0].id).unwrap();
        assert_eq!(restored.messages[0].text, original.messages[0].text);
        assert_ne!(restored.reading_id, original.reading_id);
        assert_ne!(restored.reading_id, next.reading_id);
        let empty_again = fresh(dir.path(), &next, None).unwrap();
        assert_eq!(
            empty_again.saved_readings.len(),
            1,
            "Empty launches create no extra saved readings"
        );
    }
    #[test]
    fn corrupt_or_unlisted_saved_reading_leaves_current_work_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let current = Session {
            messages: vec![Message {
                role: "user".into(),
                text: "Current words".into(),
            }],
            saved_readings: vec![SavedReading {
                id: "1-2".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        write(&dir.path().join("conversation.json"), &current, false).unwrap();
        std::fs::create_dir_all(dir.path().join("readings")).unwrap();
        std::fs::write(saved_path(dir.path(), "1-2").unwrap(), "broken").unwrap();
        let before = std::fs::read(dir.path().join("conversation.json")).unwrap();
        assert!(reopen(dir.path(), &current, "../conversation").is_err());
        assert!(reopen(dir.path(), &current, "1-2").is_err());
        assert_eq!(
            std::fs::read(dir.path().join("conversation.json")).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("readings"))
                .unwrap()
                .count(),
            1
        );
    }
}
