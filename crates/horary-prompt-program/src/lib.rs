//! Typed prompt candidates shared by the evaluator and the optimization runner.
//! A candidate can change teaching, never facts, schemas, guards or gold answers.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub mod comparison;

pub fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

/// Protected quotations may occur between editable lessons. Method-specific
/// teaching appended after a quotation remains available to the optimizer.
pub struct GuideParts<'a> {
    pub teaching: Vec<&'a str>,
    pub quoted_source: Vec<&'a str>,
}

fn book_ranges(guide: &str) -> Result<Vec<(usize, usize)>, String> {
    const OPEN: &str = "<book_extracts>";
    const CLOSE: &str = "</book_extracts>";
    let mut ranges = Vec::new();
    let mut offset = 0;
    loop {
        let remaining = &guide[offset..];
        let open = remaining.find(OPEN);
        let close = remaining.find(CLOSE);
        let Some(start) = open else {
            if close.is_some() {
                return Err("Unbalanced quoted book block".into());
            }
            return Ok(ranges);
        };
        if close.is_some_and(|end| end < start) {
            return Err("Unbalanced quoted book block".into());
        }
        let content = offset + start + OPEN.len();
        let end = guide[content..]
            .find(CLOSE)
            .ok_or("Unclosed quoted book block")?
            + content;
        if guide[content..end].contains(OPEN) {
            return Err("Nested quoted book blocks are not supported".into());
        }
        let end = end + CLOSE.len();
        ranges.push((offset + start, end));
        offset = end;
    }
}

pub fn guide_parts(guide: &str) -> Result<GuideParts<'_>, String> {
    let ranges = book_ranges(guide)?;
    let mut parts = GuideParts {
        teaching: Vec::new(),
        quoted_source: Vec::new(),
    };
    let mut copied = 0;
    for (start, end) in ranges {
        parts.teaching.push(&guide[copied..start]);
        parts.quoted_source.push(&guide[start..end]);
        copied = end;
    }
    parts.teaching.push(&guide[copied..]);
    Ok(parts)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub case_id: String,
    pub file: String,
    pub json_pointer: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub stage: String,
    pub recognition_phase: Option<String>,
    pub method: Option<String>,
}

impl Selector {
    fn matches(&self, signature: &Signature<'_>) -> bool {
        self.stage == signature.stage
            && self
                .recognition_phase
                .as_deref()
                .is_none_or(|phase| signature.recognition_phase == Some(phase))
            && self
                .method
                .as_deref()
                .is_none_or(|method| signature.method == Some(method))
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.stage == other.stage
            && (self.recognition_phase.is_none()
                || other.recognition_phase.is_none()
                || self.recognition_phase == other.recognition_phase)
            && (self.method.is_none() || other.method.is_none() || self.method == other.method)
    }
}

/// These values come from the application's actual typed stage and original
/// input, including when native validation asks the model to repair an answer.
#[derive(Clone, Copy, Debug)]
pub struct Signature<'a> {
    pub stage: &'a str,
    pub recognition_phase: Option<&'a str>,
    pub method: Option<&'a str>,
}

pub fn original_input(mut input: &serde_json::Value) -> &serde_json::Value {
    while let Some(original) = input.get("original_input") {
        input = original;
    }
    input
}

/// Resolve selector metadata from accepted application state, never from the
/// rejected model answer which happens to be beside it in a repair request.
pub fn signature<'a>(stage: &'a str, input: &'a serde_json::Value) -> Signature<'a> {
    let input = original_input(input);
    let frame = &input["consultation"]["frame"];
    let method = (frame["state"] == "resolved")
        .then(|| frame["observation"]["value"]["method"].as_str())
        .flatten()
        .or_else(|| input["reading_request"]["binding"]["frame"]["method"].as_str());
    Signature {
        stage,
        recognition_phase: input["recognition_phase"].as_str(),
        method,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub stage: String,
    pub recognition_phase: Option<String>,
    pub method: Option<String>,
    pub expected_guide_sha256: String,
    pub replacement_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<Edit>,
}

/// Short, exact teaching edits avoid making the writer regenerate long source
/// quotations. They are applied once to the hashed original guide.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
}

impl Override {
    pub fn selector(&self) -> Selector {
        Selector {
            stage: self.stage.clone(),
            recognition_phase: self.recognition_phase.clone(),
            method: self.method.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub version: u32,
    pub id: String,
    pub baseline_manifest_sha256: String,
    pub overrides: Vec<Override>,
    pub rationale: String,
    pub evidence: Vec<Evidence>,
    pub training_case_ids: Vec<String>,
    pub holdout_case_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Applied {
    pub candidate_id: String,
    pub selector: Selector,
    pub original_guide_sha256: String,
    pub replacement_guide_sha256: String,
}

fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

const STAGES: &[&str] = &[
    "intake",
    "conversation",
    "place",
    "moment",
    "significators",
    "condition",
    "reception",
    "contacts",
    "timing",
    "location",
    "judgment",
    "explanation",
];
const PHASES: &[&str] = &[
    "classify_question",
    "complete_selected_program",
    "update_selected_program",
];

impl Program {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let program: Self = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        program.validate()?;
        Ok(program)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.id.is_empty() || self.id.len() > 120 {
            return Err("A prompt program needs version 1 and a bounded nonempty id".into());
        }
        if !sha(&self.baseline_manifest_sha256) || self.overrides.is_empty() {
            return Err("A prompt program needs its baseline manifest hash and an override".into());
        }
        let training: BTreeSet<_> = self.training_case_ids.iter().collect();
        let holdout: BTreeSet<_> = self.holdout_case_ids.iter().collect();
        if training.is_empty()
            || holdout.is_empty()
            || training.len() != self.training_case_ids.len()
            || holdout.len() != self.holdout_case_ids.len()
            || !training.is_disjoint(&holdout)
        {
            return Err(
                "Training and reserved validation ids must be nonempty, unique and disjoint".into(),
            );
        }
        for (index, replacement) in self.overrides.iter().enumerate() {
            if !STAGES.contains(&replacement.stage.as_str())
                || replacement.recognition_phase.as_ref().is_some_and(|phase| {
                    replacement.stage != "intake" || !PHASES.contains(&phase.as_str())
                })
                || replacement.method.as_ref().is_some_and(|method| {
                    method.is_empty()
                        || !method.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                })
                || !sha(&replacement.expected_guide_sha256)
            {
                return Err(format!(
                    "Invalid typed selector or guide hash at override {index}"
                ));
            }
            let full = !replacement.replacement_text.is_empty();
            let edits = !replacement.edits.is_empty();
            if full == edits
                || full
                    && (replacement.replacement_text.trim().len() < 80
                        || replacement.replacement_text.len() > 100_000
                        || replacement.replacement_text.contains("<|")
                        || replacement.replacement_text.contains("|>"))
                || replacement.edits.len() > 8
                || replacement.edits.iter().any(|edit| {
                    edit.old_text.len() < 10
                        || edit.old_text.len() > 16_000
                        || edit.new_text.len() > 16_000
                        || edit.new_text.contains("<|")
                        || edit.new_text.contains("|>")
                })
            {
                return Err(format!("Invalid teaching text at override {index}"));
            }
            if self.overrides[..index]
                .iter()
                .any(|earlier| earlier.selector().overlaps(&replacement.selector()))
            {
                return Err(format!("Overlapping selectors at override {index}"));
            }
        }
        if self.evidence.is_empty()
            || self.evidence.iter().any(|item| {
                !training.contains(&item.case_id)
                    || !sha(&item.sha256)
                    || item.file.is_empty()
                    || (!item.json_pointer.is_empty() && !item.json_pointer.starts_with('/'))
            })
        {
            return Err(
                "Candidate evidence must cite hashed training records, never reserved validation"
                    .into(),
            );
        }
        Ok(())
    }

    /// Return None only for an inapplicable selector. An applicable candidate
    /// based on different source is an error, not a silent fallback to baseline.
    pub fn apply(
        &self,
        signature: Signature<'_>,
        guide: &str,
    ) -> Result<Option<(String, Applied)>, String> {
        let Some(replacement) = self
            .overrides
            .iter()
            .find(|replacement| replacement.selector().matches(&signature))
        else {
            return Ok(None);
        };
        let original = digest(guide);
        if original != replacement.expected_guide_sha256 {
            return Err(format!(
                "Candidate {} is stale for {}: expected guide {}, actual {}",
                self.id, signature.stage, replacement.expected_guide_sha256, original
            ));
        }
        let protected = book_ranges(guide)?;
        let text = if replacement.edits.is_empty() {
            replacement.replacement_text.clone()
        } else {
            // Always locate edits in the original, never let one edit create
            // the target of a later edit. Disjoint ranges make order irrelevant.
            let mut ranges = Vec::new();
            for edit in &replacement.edits {
                let matches: Vec<_> = guide
                    .match_indices(&edit.old_text)
                    .filter(|(start, _)| {
                        let end = start + edit.old_text.len();
                        !protected
                            .iter()
                            .any(|(left, right)| *start < *right && end > *left)
                    })
                    .collect();
                if matches.len() != 1 {
                    return Err(
                        "Each teaching edit must match exactly once outside quoted book blocks"
                            .into(),
                    );
                }
                let start = matches[0].0;
                ranges.push((start, start + edit.old_text.len(), &edit.new_text));
            }
            ranges.sort_by_key(|range| range.0);
            if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return Err("Teaching edits overlap".into());
            }
            let mut text = String::new();
            let mut copied = 0;
            for (start, end, inserted) in ranges {
                text.push_str(&guide[copied..start]);
                text.push_str(inserted);
                copied = end;
            }
            text.push_str(&guide[copied..]);
            text
        };
        if guide_parts(guide)?.quoted_source != guide_parts(&text)?.quoted_source {
            return Err(
                "A teaching candidate must preserve every quoted book block exactly".into(),
            );
        }
        if text == guide {
            return Err("A prompt candidate must change the teaching text".into());
        }
        Ok(Some((
            text.clone(),
            Applied {
                candidate_id: self.id.clone(),
                selector: replacement.selector(),
                original_guide_sha256: original,
                replacement_guide_sha256: digest(&text),
            },
        )))
    }
}

/// Replace only the stable system teaching in the real request. In particular,
/// leave native rejection feedback, user words and the worksheet schema intact.
pub fn apply_messages(
    program: &Program,
    signature: Signature<'_>,
    messages: &mut serde_json::Value,
) -> Result<Option<Applied>, String> {
    let first = messages
        .as_array_mut()
        .and_then(|messages| messages.first_mut())
        .ok_or("A trial needs the actual application message sequence")?;
    if first["role"] != "system" {
        return Err("A trial may replace only the first system teaching message".into());
    }
    let guide = first["content"]
        .as_str()
        .ok_or("System teaching must be text")?;
    let Some((replacement, receipt)) = program.apply(signature, guide)? else {
        return Ok(None);
    };
    first["content"] = serde_json::Value::String(replacement);
    Ok(Some(receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Program {
        Program {
            version: 1,
            id: "trial-1".into(),
            baseline_manifest_sha256: digest("manifest"),
            overrides: vec![Override {
                stage: "intake".into(),
                recognition_phase: Some("classify_question".into()),
                method: None,
                expected_guide_sha256: digest("original guide"),
                replacement_text: "Teach the private classifier to identify the actual question, preserve its words and leave factual extraction to the selected method. Return the supplied typed contract only.".into(),
                edits: vec![],
            }],
            rationale: "A stage boundary was repeatedly ignored".into(),
            evidence: vec![Evidence {
                case_id: "train".into(),
                file: "cases/train/calls/0001-result.json".into(),
                json_pointer: "/result/Ok/content".into(),
                sha256: digest("trace"),
            }],
            training_case_ids: vec!["train".into()],
            holdout_case_ids: vec!["validation".into()],
        }
    }

    #[test]
    fn only_matching_typed_signature_can_change_teaching() {
        let program = program();
        program.validate().unwrap();
        let signature = Signature {
            stage: "intake",
            recognition_phase: Some("classify_question"),
            method: None,
        };
        assert!(program
            .apply(signature, "original guide")
            .unwrap()
            .is_some());
        assert!(program
            .apply(
                Signature {
                    recognition_phase: Some("update_selected_program"),
                    ..signature
                },
                "original guide"
            )
            .unwrap()
            .is_none());
        assert!(program
            .apply(
                Signature {
                    stage: "conversation",
                    ..signature
                },
                "original guide"
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn stale_candidate_cannot_silently_be_reported_as_tested() {
        assert!(program()
            .apply(
                Signature {
                    stage: "intake",
                    recognition_phase: Some("classify_question"),
                    method: None
                },
                "changed guide"
            )
            .unwrap_err()
            .contains("stale"));
    }

    #[test]
    fn an_unchanged_prompt_cannot_count_as_an_applied_candidate() {
        let mut p = program();
        let scope = Signature {
            stage: "intake",
            recognition_phase: Some("classify_question"),
            method: None,
        };
        p.overrides[0].replacement_text = "original guide".into();
        assert!(p
            .apply(scope, "original guide")
            .unwrap_err()
            .contains("must change"));
        p.overrides[0].replacement_text.clear();
        p.overrides[0].edits = vec![Edit {
            old_text: "original".into(),
            new_text: "original".into(),
        }];
        assert!(p
            .apply(scope, "original guide")
            .unwrap_err()
            .contains("must change"));
    }

    #[test]
    fn overlap_and_training_leakage_are_rejected() {
        let mut p = program();
        p.overrides.push(Override {
            recognition_phase: None,
            ..p.overrides[0].clone()
        });
        assert!(p.validate().unwrap_err().contains("Overlapping"));
        p.overrides.pop();
        p.evidence[0].case_id = "validation".into();
        assert!(p.validate().unwrap_err().contains("reserved validation"));
        p.evidence[0].case_id = "train".into();
        p.holdout_case_ids.push("train".into());
        assert!(p.validate().unwrap_err().contains("disjoint"));
    }

    #[test]
    fn code_and_fact_patches_are_not_candidate_fields() {
        let mut value = serde_json::to_value(program()).unwrap();
        value["native_guard_patch"] = serde_json::json!("skip rejection");
        assert!(Program::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn a_trial_preserves_schema_facts_and_native_repair_feedback() {
        let mut messages = serde_json::json!([
            {"role":"system","content":"original guide"},
            {"role":"user","content":"actual input and immutable schema"},
            {"role":"assistant","content":"rejected proposal"},
            {"role":"user","content":"native rejection with the specific correction"}
        ]);
        let remainder = messages.as_array().unwrap()[1..].to_vec();
        let receipt = apply_messages(
            &program(),
            Signature {
                stage: "intake",
                recognition_phase: Some("classify_question"),
                method: None,
            },
            &mut messages,
        )
        .unwrap()
        .unwrap();
        assert_eq!(receipt.original_guide_sha256, digest("original guide"));
        assert_eq!(messages.as_array().unwrap()[1..], remainder);
        assert_eq!(
            messages[0]["content"],
            program().overrides[0].replacement_text
        );
    }

    #[test]
    fn repairs_keep_the_original_signature_instead_of_the_rejected_guess() {
        let input = serde_json::json!({
            "original_input": {
                "recognition_phase":"complete_selected_program",
                "consultation":{"frame":{"state":"resolved","observation":{"value":{"method":"relationship"}}}}
            },
            "previous_worksheet":{"frame":{"method":"lost_object"}},
            "recognition_phase":"wrong guess"
        });
        let scope = signature("intake", &input);
        assert_eq!(scope.method, Some("relationship"));
        assert_eq!(scope.recognition_phase, Some("complete_selected_program"));
    }

    #[test]
    fn method_appendices_remain_editable_without_exposing_any_source_block() {
        let guide="Original teaching.\n<book_extracts>Source one</book_extracts>\nMethod appendix: use the seller.\n<book_extracts>Source two</book_extracts>\nFinal lesson.";
        let mut p = program();
        p.overrides[0].expected_guide_sha256 = digest(guide);
        p.overrides[0].replacement_text.clear();
        p.overrides[0].edits = vec![Edit {
            old_text: "Method appendix: use the seller.".into(),
            new_text: "Method appendix: distinguish seller and intermediary.".into(),
        }];
        p.validate().unwrap();
        let scope = Signature {
            stage: "intake",
            recognition_phase: Some("classify_question"),
            method: None,
        };
        let (text, _) = p.apply(scope, guide).unwrap().unwrap();
        assert_eq!(
            guide_parts(guide).unwrap().quoted_source,
            guide_parts(&text).unwrap().quoted_source
        );
        assert!(text.contains("distinguish seller and intermediary"));
        p.overrides[0].replacement_text = text.replace("Source two", "Changed source");
        p.overrides[0].edits.clear();
        assert!(p.apply(scope, guide).is_err());
        for malformed in [
            "</book_extracts>",
            "<book_extracts>unclosed",
            "<book_extracts><book_extracts>x</book_extracts>",
        ] {
            assert!(guide_parts(malformed).is_err());
        }
    }

    #[test]
    fn adding_a_fake_book_block_cannot_evade_source_protection() {
        let guide="Teaching before a source block. <book_extracts>Original source</book_extracts> Teaching after it.";
        let mut p = program();
        p.overrides[0].expected_guide_sha256 = digest(guide);
        p.overrides[0].replacement_text.clear();
        p.overrides[0].edits = vec![Edit {
            old_text: "Teaching after it.".into(),
            new_text: "<book_extracts>Fabricated source</book_extracts>".into(),
        }];
        let scope = Signature {
            stage: "intake",
            recognition_phase: Some("classify_question"),
            method: None,
        };
        assert!(p.apply(scope, guide).is_err());
    }

    #[test]
    fn short_exact_edits_leave_source_quotations_immutable() {
        let guide="Teaching: return the required object.\n<book_extracts>Exact source quotation</book_extracts>";
        let mut p = program();
        p.overrides[0].expected_guide_sha256 = digest(guide);
        p.overrides[0].replacement_text.clear();
        p.overrides[0].edits = vec![Edit {
            old_text: "return the required object".into(),
            new_text: "return the required JSON object only".into(),
        }];
        p.validate().unwrap();
        let scope = Signature {
            stage: "intake",
            recognition_phase: Some("classify_question"),
            method: None,
        };
        let (result, _) = p.apply(scope, guide).unwrap().unwrap();
        assert!(result.ends_with("<book_extracts>Exact source quotation</book_extracts>"));
        p.overrides[0].edits[0].old_text = "Exact source quotation".into();
        assert!(p.apply(scope, guide).is_err());
        p.overrides[0].edits.clear();
        p.overrides[0].replacement_text="A long rewritten teaching string which tries to delete the book extracts entirely and should not be accepted in a trial.".into();
        assert!(p.apply(scope, guide).is_err());
    }
}
