//! Bounded local progress journal. No question text, audio or coordinates.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub at_ms: u64,
    pub event: String,
    pub detail: String,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<Inference>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inference {
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub elapsed_ms: u64,
    pub tokens_per_second: f64,
}

pub fn record(dir: &Path, event: &str, detail: &str, elapsed_ms: u64) -> std::io::Result<Progress> {
    record_with_inference(dir, event, detail, elapsed_ms, None)
}

pub fn record_with_inference(
    dir: &Path,
    event: &str,
    detail: &str,
    elapsed_ms: u64,
    inference: Option<Inference>,
) -> std::io::Result<Progress> {
    let item = Progress {
        at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        event: event.into(),
        detail: detail.into(),
        elapsed_ms,
        inference,
    };
    std::fs::create_dir_all(dir)?;
    let path = dir.join("reading-progress.jsonl");
    // Rotate only this reproducible diagnostic journal; the conversation is separate.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 4 * 1024 * 1024) {
        let old = dir.join("reading-progress.previous.jsonl");
        if old.exists() {
            std::fs::remove_file(&old)?;
        }
        std::fs::rename(&path, old)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_vec(
        &serde_json::json!({"build":env!("HORARY_BUILD_GIT_SHA"),"progress":item}),
    )?;
    file.write_all(&line)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(item)
}

#[cfg(test)]
mod tests {
    #[test]
    fn journal_is_valid_per_line_and_preserves_reading_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("conversation.json"), "private reading").unwrap();
        super::record(dir.path(), "cast", "The chart found its moment.", 17).unwrap();
        super::record(dir.path(), "written", "The first passage unfolded.", 25).unwrap();
        let text = std::fs::read_to_string(dir.path().join("reading-progress.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("conversation.json")).unwrap(),
            "private reading"
        );
    }
}
