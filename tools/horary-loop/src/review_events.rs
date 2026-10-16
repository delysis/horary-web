//! Capability warnings are receipts, never permission to use tools.
#![forbid(unsafe_code)]
use crate::store::{self, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    pub line: usize,
    pub kind: String,
    pub message: String,
}
#[derive(Default)]
pub struct Audit {
    pub warnings: Vec<Warning>,
    pub violation: Option<String>,
    pub completed_turns: usize,
}

fn capability_warning(message: &str) -> Option<&'static str> {
    const UNSTABLE: &str = "Under-development features enabled: skip_host_skill_discovery. Under-development features are incomplete and may behave unpredictably. To suppress this warning, set `suppress_unstable_features_warning = true` in ";
    const CODE_MODE: &str = "Code Mode is unavailable because code-mode host is disabled. Code mode will fail closed; enable `features.code_mode_host` and install `codex-code-mode-host`.";
    if message == CODE_MODE {
        return Some("code_mode_host_disabled");
    }
    if let Some(path) = message
        .strip_prefix(UNSTABLE)
        .and_then(|s| s.strip_suffix("/config.toml."))
    {
        if path.starts_with('/') && !path.contains(['\n', '\r', '`']) && !path.is_empty() {
            return Some("unstable_skill_discovery");
        }
    }
    None
}

pub fn inspect(path: &Path) -> Result<Audit> {
    let bytes = store::read(path)?;
    let source =
        std::str::from_utf8(&bytes).map_err(|e| format!("Event stream is not UTF-8: {e}"))?;
    let mut audit = Audit::default();
    let mut started = false;
    let mut active = false;
    for (line, text) in source.lines().enumerate() {
        let event: Value = serde_json::from_str(text)
            .map_err(|e| format!("Invalid event JSON at line {}: {e}", line + 1))?;
        let failure = match event["type"].as_str() {
            Some("thread.started") if !started => None,
            Some("turn.started") if !active => {
                started = true;
                active = true;
                None
            }
            Some("turn.completed") if active => {
                active = false;
                audit.completed_turns += 1;
                None
            }
            Some("item.started" | "item.updated" | "item.completed") => {
                match event.pointer("/item/type").and_then(Value::as_str) {
                    Some("agent_message" | "reasoning") if active => None,
                    Some("error") if !started => {
                        let message = event.pointer("/item/message").and_then(Value::as_str).unwrap_or("");
                        if let Some(kind) = capability_warning(message) {
                            audit.warnings.push(Warning { line: line + 1, kind: kind.into(), message: message.into() });
                            None
                        } else {
                            Some("Unknown pre-turn startup error; output is not accepted".to_owned())
                        }
                    }
                    Some(kind) => Some(format!("Self-contained reviewer attempted disallowed item {kind}; output is not accepted")),
                    None => Some("Event item is missing its type; output is not accepted".into()),
                }
            }
            Some(kind) => Some(format!(
                "Unexpected or out-of-order event {kind}; output is not accepted"
            )),
            None => Some("Event is missing its type; output is not accepted".into()),
        };
        if audit.violation.is_none() {
            audit.violation = failure;
        }
    }
    if audit.violation.is_none() && (active || audit.completed_turns == 0) {
        audit.violation = Some("No completed review turn; saved output is not accepted".into());
    }
    Ok(audit)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn audit(events: &str) -> Audit {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, events).unwrap();
        inspect(&path).unwrap()
    }
    fn event(kind: &str, message: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type":"item.completed","item":{"type":kind,"message":message}})
        )
    }
    #[test]
    fn known_warnings_are_recorded_only_before_the_turn() {
        let warning = event("error", "Code Mode is unavailable because code-mode host is disabled. Code mode will fail closed; enable `features.code_mode_host` and install `codex-code-mode-host`.");
        let start = "{\"type\":\"turn.started\"}\n";
        let end = "{\"type\":\"turn.completed\"}\n";
        let result = audit(&format!("{warning}{start}{end}"));
        assert!(result.violation.is_none());
        assert_eq!(result.warnings.len(), 1);
        let unstable = event("error", "Under-development features enabled: skip_host_skill_discovery. Under-development features are incomplete and may behave unpredictably. To suppress this warning, set `suppress_unstable_features_warning = true` in /Users/george/.codex/config.toml.");
        let result = audit(&format!("{unstable}{warning}{start}{end}"));
        assert!(result.violation.is_none());
        assert_eq!(result.warnings.len(), 2);
        assert!(audit(&format!("{start}{warning}{end}")).violation.is_some());
        assert!(audit(&format!(
            "{}{start}{end}",
            event("error", "Something unknown failed")
        ))
        .violation
        .is_some());
    }
    #[test]
    fn tools_unknown_events_runtime_errors_and_incomplete_turns_fail_closed() {
        for item in ["command_execution", "mcp_tool_call", "web_search", "error"] {
            assert!(audit(&format!(
                "{{\"type\":\"turn.started\"}}\n{}{{\"type\":\"turn.completed\"}}\n",
                event(item, "failure")
            ))
            .violation
            .is_some());
        }
        for events in [
            "{\"type\":\"turn.failed\"}\n",
            "{\"type\":\"error\"}\n",
            "{\"type\":\"new_kind\"}\n",
            "{\"type\":\"turn.started\"}\n",
            "{\"type\":\"turn.completed\"}\n",
        ] {
            assert!(audit(events).violation.is_some());
        }
    }
}
