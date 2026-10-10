//! Living review map generated from the actual lesson builder and scheduler.
#![forbid(unsafe_code)]
use crate::horary_lessons::{self as lessons, Matter, Stage};
use serde_json::json;
use std::path::Path;

const OVERVIEW: &str = include_str!("process_overview.md");
const SOURCES: &[&str] = &[
    ".gitattributes",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/model-manifest.json",
    "src-tauri/native-llama-runtime.json",
    "src-tauri/src/native_llama.rs",
    "scripts/check-release-assets.mjs",
    "src-tauri/src/worksheet_xml.rs",
    "src-tauri/src/conversation.rs",
    "src-tauri/src/horary_pipeline.rs",
    "src-tauri/src/horary_conversation.rs",
    "src-tauri/src/horary_contract.rs",
    "src-tauri/src/horary_executor.rs",
    "src-tauri/src/horary_step.rs",
    "src-tauri/src/horary_role_options.rs",
    "src-tauri/src/reading_contracts.rs",
    "src-tauri/src/recognition_programs.rs",
    "src-tauri/src/elicitation_eval.rs",
    "src-tauri/src/reading_neural_eval.rs",
    "src-tauri/src/deal_training_bank.rs",
    "src-tauri/src/hosted_gemma_eval.rs",
    "src-tauri/src/reading_eval.rs",
    "src-tauri/test-fixtures/readings/core-rubrics.json",
    "src-tauri/test-fixtures/readings/specialist-rubrics.json",
    "src-tauri/test-fixtures/readings/deal-action-training-rubrics-20261010.json",
    "src-tauri/test-fixtures/elicitation/core.json",
    "src-tauri/test-fixtures/elicitation/specialist.json",
    "src-tauri/test-fixtures/elicitation/edge.json",
    "docs/ELICITATION_EVALUATION.md",
    "docs/PROMPT_OPTIMIZATION.md",
    "docs/ELICITATION_FINDINGS.md",
    "docs/llm-process/discovery-145-union.json",
    "docs/llm-process/adversarial-fixture-provenance.json",
    "docs/llm-process/optimization-trial-1.json",
    "docs/llm-process/production-replays.json",
    "crates/horary-prompt-program/Cargo.toml",
    "crates/horary-prompt-program/Cargo.lock",
    "crates/horary-prompt-program/src/lib.rs",
    "crates/horary-prompt-program/src/comparison.rs",
    "tools/horary-loop/Cargo.toml",
    "tools/horary-loop/Cargo.lock",
    "tools/horary-loop/README.md",
    "tools/horary-loop/src/lib.rs",
    "tools/horary-loop/src/main.rs",
    "tools/horary-loop/src/types.rs",
    "tools/horary-loop/src/packet.rs",
    "tools/horary-loop/src/refs.rs",
    "tools/horary-loop/src/campaign.rs",
    "tools/horary-loop/src/catalogue.rs",
    "tools/horary-loop/src/comparison.rs",
    "tools/horary-loop/src/schema_check.rs",
    "tools/horary-loop/src/review_events.rs",
    "tools/horary-loop/src/store.rs",
    "tools/horary-loop/src/bin/horary-experiment.rs",
    "tools/horary-loop/src/bin/horary-optimize.rs",
    "tools/horary-loop/src/bin/horary-catalogue.rs",
    "tools/horary-gepa/Cargo.toml",
    "tools/horary-gepa/Cargo.lock",
    "tools/horary-gepa/README.md",
    "tools/horary-gepa/src/lib.rs",
    "tools/horary-gepa/src/controls.rs",
    "tools/horary-gepa/src/campaign.rs",
    "tools/horary-gepa/src/corpus.rs",
    "tools/horary-gepa/src/reading.rs",
    "tools/horary-gepa/src/main.rs",
    "tools/horary-gepa/src/executable.rs",
    "tools/horary-gepa/src/journal.rs",
    "tools/horary-gepa/src/native.rs",
    "tools/horary-gepa/src/teacher.rs",
    "tools/horary-gepa/src/metric.rs",
    "tools/horary-gepa/src/review.rs",
    "tools/horary-gepa/src/recovery.rs",
    "tools/horary-gepa/src/status.rs",
    "src-tauri/src/reading_contract_overview.md",
    "src-tauri/src/reading_contract_tests.rs",
    "src-tauri/src/horary_recovery_tests.rs",
    "src-tauri/src/horary_step_tests.rs",
    "src-tauri/src/horary_lessons.rs",
    "src-tauri/src/tool_formats.rs",
    "src-tauri/src/reading_store.rs",
    "src-tauri/src/reading_method.rs",
    "src-tauri/src/voice.rs",
    "src-tauri/src/wake_listening.rs",
    "src-tauri/src/lib.rs",
    "src-tauri/Info.plist",
    "src-tauri/src/local_dictation.rs",
    "src-tauri/src/microphone_capture.rs",
    "src-tauri/src/native_llama_worker.rs",
    "src-tauri/src/native_location.rs",
    "src-tauri/native/location_bridge.m",
    "src-tauri/native/location_bridge_test.inc",
    "src-tauri/native/permissions_bridge.m",
    "src-tauri/src/permissions.rs",
    "src-tauri/build.rs",
    "src-tauri/src/geocode.rs",
    "src-tauri/src/hf_cache.rs",
    "src-tauri/src/imported_models.rs",
    "src-tauri/src/llama.rs",
    "src-tauri/src/model_manifest.rs",
    "src-tauri/src/review_progress.rs",
    "crates/horary-ai-core/src/chart_input.rs",
    "crates/horary-ai-core/src/astronomy.rs",
    "crates/horary-ai-core/src/events.rs",
    "crates/horary-ai-core/src/book_method.rs",
    "src/App.tsx",
    "src/App.css",
    "src/ReadingDocument.tsx",
    "src/ReadingHistory.tsx",
    "src-tauri/src/process_overview.md",
    "src-tauri/src/process_reference.rs",
];

fn generate(root: &Path) -> Result<Vec<(&'static str, String)>, String> {
    let examples = crate::horary_pipeline::process_examples()?;
    let case_count = crate::elicitation_eval::catalogue_size(root)?;
    let mut paths: Vec<String> = SOURCES.iter().map(|p| (*p).into()).collect();
    for entry in
        std::fs::read_dir(root.join("src-tauri/src/horary_prompts")).map_err(|e| e.to_string())?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_file() {
            paths.push(
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    for entry in std::fs::read_dir(root.join("src-tauri/test-fixtures/elicitation"))
        .map_err(|e| e.to_string())?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            paths.push(
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    paths.sort();
    paths.dedup();
    let manifest = paths
        .iter()
        .map(|path| {
            std::fs::read_to_string(root.join(path))
                .map(|source| json!({"path":path,"sha256":lessons::digest(&source)}))
                .map_err(|e| format!("{path}: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (preamble, review) = OVERVIEW
        .split_once("## What Eileen should examine")
        .ok_or("Missing review section")?;
    let mut document = preamble.to_string();
    document.push_str("\nThe application is now governed by the [executable reading catalogue](READING_CONTRACTS.md). It generates the recognition contract, conditional fact reminders, readiness gate and frozen reading request. The earlier [elicitation design](ELICITATION_DESIGN.md) is its research record.\n\n");
    document.push_str("\n## The completion state machine\n\n```mermaid\nflowchart TB\n");
    for (from, to, reason) in crate::horary_step::Phase::EDGES {
        document.push_str(&format!("  {from} -->|{reason}| {to}\n"));
    }
    document.push_str("```\n\nThese transition labels come from the Rust state catalog. `horary_step.rs` owns the completion permit and durable job journal. `horary_executor.rs` sends both single and batch results through one acceptance path. `horary_contract.rs` validates shapes and domain checks. `horary_role_options.rs` binds named roles, computes turned houses and derives rulers. `horary_pipeline.rs` assembles dependencies and the document. The normal regression suite checks their invariants.\n\nA JSON-shaped response is a proposal. No stage becomes complete until all native checks accept its required data. A request for user information leaves it awaiting input. Rejection returns to the same stage, with the unchanged original input and latest rejected proposal, until accepted data arrives or execution is cancelled/interrupted. There is no two-attempt abandonment. Repair prompts retain the cached lesson and present the original input, the rejected assistant answer, then focused native feedback. The rejected proposal is never accepted by being included in that dialogue. Selected recognition lessons omit unrelated typed examples while the update vocabulary still permits explicit reclassification. User replies are retained with the waiting stage; a changed input supersedes the old job rather than pretending it completed.\n\nA completion permit is bound to its stage and input fingerprint. Work identity also fingerprints the current native validation code, lesson, contract and chart revision. Saved data is revalidated before reuse. All batch results are recorded before any one case is repaired, so valid siblings survive cancellation. Reloaded unfinished work becomes paused; it is never inferred complete. Original outputs and rejections remain in the private receipts.\n\nFor place/moment explanations, the selected native chart context supplies the actual time, zone and place even before an interpretation exists. The current follow-up words are always included. If an interpretation or passage does not exist, the controller explains that it is unfinished rather than dispatching an actor with empty context or asking the person for chart data. Explicit continue/cast commands preserve the current matter and resume it.\n\n");
    document.push_str(&format!("\n## Voice turns\n\n```mermaid\nflowchart TB\n  launch[\"Launch: request microphone, speech and location consent\"] --> access{{\"Microphone available?\"}}\n  access -->|No| writing[\"Conditional text fallback; native microphone recovery\"]\n  writing --> reading\n  access -->|Yes| focus{{\"Active window; no speech, work or history\"}}\n  focus --> wake[\"On-device streaming speech recognition\"]\n  wake --> addressed{{\"Oracle or expected reply?\"}}\n  addressed -->|No| discard[\"Discard ambient hypothesis; renew task after {} ms\"]\n  discard --> wake\n  addressed -->|Yes| words[\"Preserve addressed words; light listening mark\"]\n  words --> pause[\"{} ms of quiet and unchanged words\"]\n  pause --> final{{\"Final native recognition result?\"}}\n  final -->|Yes| receipt[\"Release microphone; one-use receipt\"]\n  final -->|No| fallback[\"Stop; retain manual microphone fallback\"]\n  receipt --> reading[\"Private clipboard, conversational reader and specialist judgments\"]\n  reading --> reply[\"Installed voice speaks; await completion\"]\n  reply --> expected[\"{} ms expected-reply window, then Oracle\"]\n  expected --> focus\n  orb[\"Manual microphone mark\"] --> wav[\"Bounded in-memory WAV\"]\n  wav --> route[\"On-device dictation, direct Gemma audio, or transcription comparison\"]\n  route --> receipt\n```\n\nTiming values above are emitted from the native listener's constants. Callback and PCM queues are bounded. Ambient and partial recognition never enter a reading, model prompt, file or log. Wake recognition is macOS-only and requires local language assets and permission. No claim of native recognition accuracy follows from controller tests.\n", crate::wake_listening::RENEW_MS, crate::wake_listening::SILENCE_MS, crate::wake_listening::FOLLOW_UP_MS));
    document.push_str("\n## The judgment process\n\n```mermaid\nflowchart TB\n  words[\"Spoken question\"]\n  chart[\"N: calculate chart and derive rulers\"]\n  retained_step[\"N: selected prior worksheet and evidence\"]\n");
    for stage in Stage::ALL {
        let kind = match stage.kind() {
            "classification" => "C",
            "explanation" | "conversation" => "W",
            _ => "J",
        };
        let suffix = match stage {
            Stage::Place => " / native default",
            Stage::Moment => " / native default",
            Stage::Location => " / lost matters only",
            Stage::Timing => " / currently native unestablished",
            Stage::Judgment => " + W: answer the question",
            _ => "",
        };
        document.push_str(&format!(
            "  {}[\"{}: {}{}\"]\n",
            stage.name(),
            kind,
            stage.title(),
            suffix
        ));
        for dependency in stage.dependencies() {
            document.push_str(&format!("  {dependency} --> {}\n", stage.name()));
        }
        let key = lessons::key(stage, Matter::Other);
        document.push_str(&format!(
            "  click {} href \"#lesson-{}\" \"Inspect its actual lesson\"\n",
            stage.name(),
            key
        ));
    }
    document.push_str("  place --> chart\n  moment --> chart\n  intake --> consultation_clipboard[\"N: accepted facts, missing inputs and boundaries\"]\n  judgment --> consultation_clipboard\n  explanation --> consultation_clipboard\n  consultation_clipboard --> conversation\n  conversation --> document[\"Model reply + unfolding chart and evidence\"]\n  document --> words\n```\n\nThe model conducts the conversation. Rust holds the private clipboard, validates extracted observations and computes remaining prerequisites. A missing fact or method boundary becomes a reminder to the conversational reader, never a canned reply. The reader can select one reminder to pursue, without changing accepted facts or authorizing judgment. Specialized explanation and judgment findings return to that same reader. Its complete prompt and live constrained reminder IDs are exported below.\n\nA new matter is archived into a separate leaf before downstream work. Condition, reception and contact mechanics, plus location when applicable, are submitted as **one native generation batch** with separate prompts and saved prefixes. Contact selection does not need the other worksheets: the final judgment combines those independent findings.\n\n");
    document.push_str(r#"## Elicitation: classify, then complete the selected program

```mermaid
flowchart TD
  words["Current words, actual clock and device context"] --> classify["C: recognize intent and tentative question type"]
  words --> acquire["N: acquire missing device coordinates in parallel"]
  classify --> checked{"Native patch accepted?"}
  checked -->|No| repair["Same task with original input and exact rejection"]
  repair --> classify
  checked -->|New concrete method| extract["C: focused method lesson, SAME words"]
  extract --> validate{"Native patch accepted?"}
  validate -->|No| retry["Same focused task and explicit rejection"]
  retry --> extract
  validate -->|Changed method only| extract
  validate -->|Verified method and accepted facts| facts["Canonical sourced facts"]
  checked -->|Existing method or unresolved concern| facts
  acquire --> facts
  facts --> plan["N: conditional requirements from the executable contract"]
  plan -->|Genuine gap| guru["W: conversational reader receives a private reminder"]
  guru --> answer["User supplies, corrects, explains or declines"]
  answer --> words
  plan -->|Enough facts| anchor["N: verified reader place and understood moment"]
  anchor --> permit["Typed reading permit"]
  permit --> judgment["Question-specific specialist judgment"]
```

Initial classification cannot establish people, subjects or facts. The focused pass verifies a tentative method and answer type before extracting facts from the same words. Its schema fixes question to null and intent to clarify; the accepted question remains intact. A corrected method is returned alone and runs its own lesson before extracting facts. A same-method facet correction can accompany observations. Subsequent turns already use the selected lesson. All passes share the application's native acceptance, repair and receipt path. An ordinary resolved fact cannot reappear merely because an earlier result requested it. Native time/place validation can retain a more specific unresolved anchor reason. Supplied event cities are independently qualified by the offline geocoder; their provenance retains the original words and they never become the chart's reader place.

## Place and moment: independence and genuine dependencies

```mermaid
flowchart TB
  intake["Retained question and explicit overrides"] --> placecheck["N: assess place and lookup stated city"]
  intake --> timecheck["N: assess moment sufficiency"]
  placecheck --> device{"Usable device place, no override?"}
  device -->|Yes| place["Selected device coordinates and zone"]
  device -->|No| geocode["Offline candidates or ask for city"]
  geocode --> unique{"One resolved candidate?"}
  unique -->|Yes| place
  unique -->|No| choose["C: distinguish candidates or clarify"]
  choose --> place
  timecheck --> supplied{"Explicit earlier or corrected question moment?"}
  supplied -->|No| recorded["N: receipt instant or existing chart instant"]
  supplied -->|Yes| civil["C: parse civil date and time"]
  place -->|Selected time zone and native local clock| civil
  civil --> resolve["N: validate civil time, DST gap or overlap"]
  resolve --> ambiguous{"Needs clarification?"}
  ambiguous -->|Yes| ask["Private place/time reminder to conversational reader"]
  geocode -->|No useful candidate| ask
  ambiguous -->|No| moment["Verified instant"]
  recorded --> moment
  place --> cast["N: cast after both results"]
  moment --> cast
  intake --> event["Event place and time: retained contextual observations"]
  event --> interpretation["Question-specific judgment; inquire if relevant"]
```

The clipboard presents chart_context and event_context separately. Device location can supply the reader anchor without establishing an event venue. Ask about the venue when it matters to the selected question, rather than requiring it universally. Mentioning London or yesterday as the location/time of a lost object does not change the chart place or moment. A city-only clarification preserves the original question. A relative historical time needs the native clock in the **chosen place's** zone, not the model's guessed date. Nonexistent civil times fail; repeated civil times require an occurrence choice.

## The cache and native batch boundary

```mermaid
flowchart LR
  fixed["Fixed, task-specific lesson"] --> key["SHA256 lesson key in one live model owner"]
  key --> hit{"Saved native prefix present?"}
  hit -->|No| prefill["Prefill fixed system message once"]
  prefill --> bank["Bounded in-memory saved prefix bank"]
  hit -->|Yes| bank
  bank --> verify["Verify exact token prefix and live ownership"]
  verify --> tasks["1 to 4 independent case prompts"]
  changing["Changing question, facts and output contract"] --> tasks
  tasks --> batch["Native generate_batch: distinct KV sequences, one weight copy"]
  batch --> check["Parse each worksheet; check schema, facts and native completion rules"]
  check -->|Valid| receipt["Keep original output, checks, source IDs and metrics"]
  check -->|Invalid| repair["Retain failure; retry that same task until checked data or cancellation"]
  check -->|Needs user information| wait["Keep the task unfinished; deliver replies to it"]
  wait -->|User reply| retry["Generate only this unfinished task with its saved lesson"]
  repair --> retry
  retry --> check
```

The bank contains only fixed teaching messages, not private question inputs or audio. It is bounded to one eighth of physical memory, at most 4 GiB. Eviction, owner restart, changed lesson text, changed model or template can require another prefill; an absolute once-ever guarantee would be false. The ordinary batch API accepts an authenticated saved prefix **per case**. The constrained API has one constraint program for the whole batch and no supplied per-case-prefix field in the current pin. Single text tasks use constrained JSON; independent analysis tasks use ordinary cached batching and native validation. This boundary is visible rather than hidden behind an apparent cache-hit claim.

"#);
    document.push_str(&format!("\nThe [scenario evaluation guide](ELICITATION_EVALUATION.md) documents the executable {case_count}-case bank, separate first-turn and continuation grades, and full private traces. The case catalogue is never inserted into the model's prompt. The [optimization loop](PROMPT_OPTIMIZATION.md) uses typed teaching candidates, the existing Codex login for review/writing, and paired native trials; [its first measured trial](llm-process/optimization-trial-1.json) was rejected on reserved validation.\n\n"));
    document.push_str("\n## Stage inventory and reviewable outputs\n\n| Task | Kind | Must have first | Public checks |\n|---|---|---|---|\n");
    for stage in Stage::ALL {
        document.push_str(&format!(
            "| [{}](#lesson-{}) | {} | {} | {} |\n",
            stage.title(),
            lessons::key(stage, Matter::Other),
            stage.kind(),
            stage.dependencies().join(", "),
            stage.checks().join(", ")
        ));
    }
    document.push_str("\n## What Eileen should examine");
    document.push_str(review);
    document.push_str("\n## Exact live lessons and contracts\n\nThe complete request examples are in [prompt-examples.json](llm-process/prompt-examples.json). They are captured by an authored fixture driving the real scheduler. [Runtime source](llm-process/runtime-excerpts.md) and [source fingerprints](llm-process/source-manifest.json) make the implementation inspectable. Evidence-ID enums in each contract are specific to the current stage's supplied facts. No whole chart or full chat history is inserted into every task.\n\n");
    let mut guides = Vec::new();
    for stage in Stage::ALL {
        let matters: &[Matter] = if stage == Stage::Significators {
            &[Matter::Relationship, Matter::LostObject, Matter::Other]
        } else {
            &[Matter::Other]
        };
        for matter in matters {
            let key = lessons::key(stage, *matter);
            let guide =
                crate::horary_contract::guide_for(stage, *matter, &serde_json::Value::Null)?;
            document.push_str(&format!("<a id=\"lesson-{key}\"></a>\n\n### {} · {key}\n\nGuide SHA256: `{}`\n\n<details><summary>Exact teaching prompt, worked cases and Frawley passages</summary>\n\n```text\n{guide}\n```\n\n</details>\n\n",stage.title(),lessons::digest(&guide)));
            let contract = if stage == Stage::Significators {
                examples["examples"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|example| example["stage"] == "significators")
                    .map(|example| example["responseSchema"].clone())
                    .ok_or("Missing live role-selector example")?
            } else {
                crate::horary_step::response_schema(stage, *matter, &[])
            };
            document.push_str(&format!("<details><summary>Output contract (role IDs use the captured marriage fixture; other facts empty)</summary>\n\n```json\n{}\n```\n\n</details>\n\n",serde_json::to_string_pretty(&contract).map_err(|e|e.to_string())?));
            guides.push(json!({"key":key,"stage":stage,"matter":matter,"guide":guide,"guideSha256":lessons::digest(&guide),"contractWithNoFacts":contract}));
        }
    }
    let mut extracts =
        String::from("# Actual runtime source\n\nGenerated verbatim; not a model transcript.\n\n");
    for path in [
        "src-tauri/src/horary_pipeline.rs",
        "src-tauri/src/horary_conversation.rs",
        "src-tauri/src/horary_contract.rs",
        "src-tauri/src/horary_executor.rs",
        "src-tauri/src/horary_step.rs",
        "src-tauri/src/horary_role_options.rs",
        "src-tauri/src/horary_lessons.rs",
        "src-tauri/src/conversation.rs",
        "src-tauri/src/native_llama_worker.rs",
        "src-tauri/src/tool_formats.rs",
    ] {
        let source = std::fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?;
        extracts.push_str(&format!("## {path}\n\n```rust\n{source}\n```\n\n"));
    }
    let reference = json!({"authorship":"Generated exact live prompts plus explicitly authored scheduler fixture; no model invoked","fixture":examples,"lessons":guides,"bookOcrSha256":lessons::BOOK_OCR_SHA256});
    let manifest = json!({"purpose":"Source fingerprint; no private app data","sources":manifest});
    Ok(vec![
        (
            "docs/READING_CONTRACTS.md",
            crate::reading_contracts::documentation(),
        ),
        (
            "docs/llm-process/reading-catalogue.json",
            format!("{}\n", serde_json::to_string_pretty(&json!({
                "authorship":"Generated from the production Rust catalogue; not model output or expert qualification",
                "version":crate::reading_contracts::VERSION,
                "contracts":crate::reading_contracts::CATALOGUE,
                "fields":crate::reading_contracts::Field::ALL.iter().map(|field| json!({
                    "id":field.name(), "question":crate::reading_contracts::field_prompt(*field),
                    "labels":crate::reading_contracts::allowed_values(*field)
                })).collect::<Vec<_>>()
            })).map_err(|e|e.to_string())?),
        ),
        ("docs/LLM_PROCESS.md", format!("{}\n", document.trim_end())),
        (
            "docs/llm-process/prompt-examples.json",
            format!(
                "{}\n",
                serde_json::to_string_pretty(&reference).map_err(|e| e.to_string())?
            ),
        ),
        (
            "docs/llm-process/runtime-excerpts.md",
            format!("{}\n", extracts.trim_end()),
        ),
        (
            "docs/llm-process/source-manifest.json",
            format!(
                "{}\n",
                serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
            ),
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
        for (name, expected) in generate(root()).expect("Generate actual process reference") {
            let current = std::fs::read_to_string(root().join(name)).unwrap_or_default();
            assert!(current==expected,"{name} is stale. Run cargo test --manifest-path src-tauri/Cargo.toml --locked --lib process_reference::tests::regenerate -- --ignored --nocapture");
        }
    }
    #[test]
    #[ignore = "Explicitly regenerate the living process reference without a model or app data"]
    fn regenerate() {
        for (name, contents) in generate(root()).expect("Generate actual process reference") {
            let path = root().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
            eprintln!("Updated {name}");
        }
    }
}
