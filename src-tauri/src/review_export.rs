#![forbid(unsafe_code)]
use std::{io::Write, path::Path};
use tauri_plugin_dialog::DialogExt;

fn validate(content: &str) -> Result<(), String> {
    if content.len() > 32 * 1024 * 1024 {
        return Err("Review export exceeds 32 MB.".into());
    }
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if value["version"] != 1
        || !matches!(
            value["format"].as_str(),
            Some("horary-reading-review" | "horary-review-notes")
        )
    {
        return Err("Unrecognized review export format.".into());
    }
    Ok(())
}

fn write_export(path: &Path, content: &str) -> Result<(), String> {
    validate(content)?;
    let parent = path.parent().ok_or("Export needs a destination folder.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn export_review(
    app: tauri::AppHandle,
    filename: String,
    content: String,
) -> Result<bool, String> {
    validate(&content)?;
    if !filename.ends_with(".json") || filename.len() > 100 || filename.contains(['/', '\\']) {
        return Err("Invalid review filename.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(&filename)
            .add_filter("Horary review", &["json"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        write_export(&path.into_path().map_err(|e| e.to_string())?, &content)?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_replaces_only_the_selected_file_after_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reading.json");
        std::fs::write(&path, "previous export").unwrap();
        assert!(write_export(&path, "not JSON").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous export");
        let content =
            r#"{"format":"horary-reading-review","version":1,"question":"Where is the ring?"}"#;
        write_export(&path, content).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        assert!(write_export(&dir.path().join("missing/file.json"), content).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
