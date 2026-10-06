//! Source-bound review reference. Test-only: never reads private app data or
//! invokes a model. Regeneration is explicit; freshness is a normal CI test.
#![forbid(unsafe_code)]
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

const OVERVIEW: &str = include_str!("process_overview.md");
const SOURCES: &[&str] = &[
    ".gitattributes",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/model-manifest.json",
    "src-tauri/src/conversation.rs",
    "src-tauri/src/conversation_prompt.txt",
    "src-tauri/src/conversation_method.txt",
    "src-tauri/src/reading_method.rs",
    "src-tauri/src/voice.rs",
    "src-tauri/src/local_dictation.rs",
    "src-tauri/src/microphone_capture.rs",
    "src-tauri/src/native_llama_worker.rs",
    "src-tauri/src/native_location.rs",
    "src-tauri/native/location_bridge.m",
    "src-tauri/src/geocode.rs",
    "src-tauri/src/hf_cache.rs",
    "src-tauri/src/model_manifest.rs",
    "src-tauri/src/review_progress.rs",
    "crates/horary-ai-core/src/chart_input.rs",
    "crates/horary-ai-core/src/astronomy.rs",
    "crates/horary-ai-core/src/book_method.rs",
    "src/App.tsx",
    "src/ReadingDocument.tsx",
    "src-tauri/src/process_overview.md",
    "src-tauri/src/process_reference.rs",
];

fn excerpt<'a>(source: &'a str, start: &str, end: &str) -> Result<&'a str, String> {
    let offset = source
        .find(start)
        .ok_or_else(|| format!("Source anchor missing: {start}"))?;
    let rest = &source[offset..];
    let length = rest
        .find(end)
        .ok_or_else(|| format!("Source anchor missing: {end}"))?;
    Ok(rest[..length].trim_end())
}

fn generate(root: &Path) -> Result<Vec<(&'static str, String)>, String> {
    let reference = crate::conversation::process_examples()?;
    let mut manifest = Vec::new();
    for path in SOURCES {
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
        manifest.push(json!({"path":path,"sha256":format!("{:x}",Sha256::digest(bytes))}));
    }
    let manifest = json!({"purpose":"Fingerprint of source reviewed by this generated reference; no private data or model output.","sources":manifest});
    let pretty = |value: &Value| serde_json::to_string_pretty(value).map_err(|e| e.to_string());
    let mut document = OVERVIEW.to_owned();
    document.push_str("\n## Exact live prompt material\n\nThese strings are taken from the functions that build the model request. The companion [prompt examples](llm-process/prompt-examples.json) contains complete message arrays and response schemas for unresolved place, device place, direct audio, stated place after clarification, first chart, significators and testimony. Its inputs are synthetic; it is not a record of a model run. [Source extracts](llm-process/runtime-excerpts.md) show how each request is assembled and executed. [Source fingerprints](llm-process/source-manifest.json) make this edition auditable.\n\n### Actual system message\n\n```text\n");
    document.push_str(
        reference["systemPrompt"]
            .as_str()
            .ok_or("Missing system message")?,
    );
    document.push_str("\n```\n\n### Actual direct-audio instruction\n\n`<CURRENT ACTION SCHEMA>` below marks the dynamic schema inserted into this exact instruction by `direct_audio_prompt`. A full concrete example is in the companion JSON.\n\n```text\n");
    document.push_str(
        reference["directAudioInstruction"]
            .as_str()
            .ok_or("Missing audio instruction")?,
    );
    document.push_str("\n```\n\n### Actual optional Gemma transcription instruction\n\nThis is used only by the explicit comparison route, not Auto's fallback.\n\n```text\n");
    document.push_str(
        reference["transcriptionPrompt"]
            .as_str()
            .ok_or("Missing transcript instruction")?,
    );
    document.push_str("\n```\n\n### Actual current-stage instructions\n\nThese are extracted from the native update built for each executable fixture. The schemas beside them in the companion JSON show the permitted actions.\n\n");
    for (key, label) in [
        ("place_unresolved", "No place yet"),
        ("device_place_available", "A usable device place"),
        (
            "chart_cast_before_roles",
            "Chart cast; method stages pending",
        ),
    ] {
        let messages = reference["examples"][key]["messages"]
            .as_array()
            .ok_or("Missing fixture messages")?;
        let content = messages
            .last()
            .and_then(|m| m["content"].as_str())
            .ok_or("Missing native update")?;
        let instruction = content
            .split_once("Current step:\n")
            .ok_or("Missing stage instruction")?
            .1;
        document.push_str(&format!("#### {label}\n\n```text\n{instruction}\n```\n\n"));
    }
    document.push_str("The exact system/rule catalog above is the model's book guidance. The longer review background is [conversation_method.txt](../src-tauri/src/conversation_method.txt); it is not an additional model message.\n");

    let conversation = std::fs::read_to_string(root.join("src-tauri/src/conversation.rs"))
        .map_err(|e| e.to_string())?;
    let mut extracts = String::from("# Exact application source used by the process map\n\nGenerated verbatim from the runtime and renderer. This is source, not a transcript or model reasoning. The [main process map](../LLM_PROCESS.md) supplies the task classifications and review questions.\n\n");
    for (label, start, end) in [
        (
            "Device location and context",
            "fn device_place(",
            "fn note(",
        ),
        (
            "State-dependent action schema",
            "fn schema(",
            "impl ConversationState",
        ),
        (
            "Time, chart, method validation and stored interpretation",
            "fn execute(",
            "/// Keep verified evidence",
        ),
        (
            "Actual prompt assembly",
            "struct PromptThread",
            "// A failed inference",
        ),
        (
            "Retry, direct-audio prompt and action parsing",
            "fn generate_action(",
            "enum TurnInput",
        ),
        (
            "Complete orchestration loop",
            "fn run(",
            "#[tauri::command]\npub fn conversation_snapshot",
        ),
        (
            "Fixed native closing",
            "fn finish_working_reading(",
            "fn execute(",
        ),
    ] {
        extracts.push_str(&format!(
            "## {label}\n\nSource: `src-tauri/src/conversation.rs`.\n\n```rust\n{}\n```\n\n",
            excerpt(&conversation, start, end)?
        ));
    }
    let renderer =
        std::fs::read_to_string(root.join("src/ReadingDocument.tsx")).map_err(|e| e.to_string())?;
    extracts.push_str("## Visible passage versus proposed interpretation\n\nSource: `src/ReadingDocument.tsx`.\n\n```tsx\n");
    extracts.push_str(
        &renderer[renderer
            .find("export function ReadingPassage")
            .ok_or("Missing renderer anchor")?..],
    );
    extracts.push_str("\n```\n");
    let voice =
        std::fs::read_to_string(root.join("src-tauri/src/voice.rs")).map_err(|e| e.to_string())?;
    extracts.push_str(
        "\n## Actual speech route selection\n\nSource: `src-tauri/src/voice.rs`.\n\n```rust\n",
    );
    extracts.push_str(excerpt(
        &voice,
        "pub async fn voice_finish(",
        "// The owned recorder",
    )?);
    extracts.push_str("\n```\n");
    let speech = std::fs::read_to_string(root.join("src-tauri/src/local_dictation.rs"))
        .map_err(|e| e.to_string())?;
    extracts.push_str("\n## On-device transcription and its waits\n\nSource: `src-tauri/src/local_dictation.rs`.\n\n```rust\n");
    extracts.push_str(excerpt(
        &speech,
        "fn claim_authorization_request(",
        "#[cfg(any(target_os = \"macos\", test))]",
    )?);
    extracts.push_str("\n```\n");
    Ok(vec![
        ("docs/LLM_PROCESS.md", document),
        (
            "docs/llm-process/prompt-examples.json",
            format!("{}\n", pretty(&reference)?),
        ),
        ("docs/llm-process/runtime-excerpts.md", extracts),
        (
            "docs/llm-process/source-manifest.json",
            format!("{}\n", pretty(&manifest)?),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }
    #[test]
    fn checked_in_reference_is_current() {
        for (name, expected) in generate(root()).expect("Generate reference from runtime") {
            let current = std::fs::read_to_string(root().join(name)).unwrap_or_default();
            assert!(current==expected,"{name} is stale. Run cargo test --manifest-path src-tauri/Cargo.toml --locked process_reference::tests::regenerate -- --ignored --nocapture");
        }
    }
    #[test]
    #[ignore = "Explicitly regenerate the source-bound review reference, without a model or app data."]
    fn regenerate() {
        for (name, contents) in generate(root()).expect("Generate reference from runtime") {
            let path = root().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
            eprintln!("Updated {name}");
        }
    }
}
