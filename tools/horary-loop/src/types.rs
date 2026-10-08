#![forbid(unsafe_code)]
use crate::store::{FileRef, Result};
use horary_prompt_program::{Evidence, Program};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    Training,
    ReservedValidation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SplitCase {
    pub id: String,
    pub method: String,
    pub mode: String,
    pub bank: String,
    pub partition: Partition,
    pub fixture_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Split {
    pub version: u32,
    pub policy: String,
    pub qualification: String,
    pub fixture_files: Vec<FileRef>,
    pub cases: Vec<SplitCase>,
}

/// Observability is derived from the native receipt, never a scripted answer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FollowUpProvenance {
    pub version: u32,
    pub user_turn_submitted: bool,
    pub assistant_reply_observed: bool,
    pub source: Evidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScoreState {
    Scored,
    NotApplicable,
    Unobserved,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub state: ScoreState,
    pub score: Option<u8>,
    pub reason: String,
    pub evidence: Vec<Evidence>,
}
impl Dimension {
    pub fn validate(&self, inquiry: bool) -> Result<()> {
        if self.reason.trim().is_empty() {
            return Err("Every rubric dimension needs a reason".into());
        }
        match self.state {
            ScoreState::Scored
                if self.score.is_some_and(|s| s <= 2) && !self.evidence.is_empty() =>
            {
                Ok(())
            }
            ScoreState::NotApplicable
                if inquiry && self.score.is_none() && !self.evidence.is_empty() =>
            {
                Ok(())
            }
            ScoreState::Unobserved if self.score.is_none() && !self.evidence.is_empty() => Ok(()),
            _ => Err("Rubric score/state/evidence mismatch; only inquiry may be N/A".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rubric {
    pub concern_actor: Dimension,
    pub evidence_honesty: Dimension,
    pub useful_inquiry: Dimension,
    pub natural_phrasing: Dimension,
    pub continuity: Dimension,
}
impl Rubric {
    pub fn dimensions(&self) -> [(&Dimension, bool); 5] {
        [
            (&self.concern_actor, false),
            (&self.evidence_honesty, false),
            (&self.useful_inquiry, true),
            (&self.natural_phrasing, false),
            (&self.continuity, false),
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Classification,
    MissingFact,
    RedundantInquiry,
    ActorOwnership,
    SourceGrounding,
    ChartAnchor,
    FormatSchema,
    NativeBoundary,
    RepairLoop,
    Conversation,
    UnsupportedClaim,
    ExecutionInterruption,
    Infrastructure,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub failure_class: FailureClass,
    pub stage: Option<String>,
    pub recognition_phase: Option<String>,
    pub method: Option<String>,
    pub summary: String,
    pub repair_owner: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseReview {
    pub case_id: String,
    pub native_semantic_pass: bool,
    pub native_journey_pass: Option<bool>,
    pub first_turn: Rubric,
    pub follow_up: Option<Rubric>,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub id: String,
    pub failure_class: FailureClass,
    pub stage: Option<String>,
    pub case_ids: Vec<String>,
    pub summary: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeOutput {
    pub version: u32,
    pub reviews: Vec<CaseReview>,
    pub clusters: Vec<Cluster>,
    pub qualification: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeOutput {
    pub candidate: Option<Program>,
    pub repair_required: Vec<Finding>,
    pub abstain_reason: Option<String>,
}

fn object(properties: Value) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .expect("schema properties")
        .keys()
        .cloned()
        .collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn text() -> Value {
    json!({"type":"string"})
}
fn optional_text() -> Value {
    json!({"type":["string","null"]})
}
fn list(item: Value) -> Value {
    json!({"type":"array","items":item})
}
fn nullable(mut item: Value) -> Value {
    item["type"] = json!(["object", "null"]);
    item
}
fn evidence_schema() -> Value {
    object(json!({"case_id":text(),"file":text(),"json_pointer":text(),"sha256":text()}))
}
fn dimension_schema() -> Value {
    object(
        json!({"state":{"type":"string","enum":["scored","not_applicable","unobserved"]},
        "score":{"type":["integer","null"],"enum":[null,0,1,2]},
        "reason":text(),"evidence":list(evidence_schema())}),
    )
}
fn rubric_schema() -> Value {
    object(
        json!({"concern_actor":dimension_schema(),"evidence_honesty":dimension_schema(),
        "useful_inquiry":dimension_schema(),"natural_phrasing":dimension_schema(),"continuity":dimension_schema()}),
    )
}
fn failure_schema() -> Value {
    json!({"type":"string","enum":["classification","missing_fact","redundant_inquiry","actor_ownership", "source_grounding","chart_anchor","format_schema","native_boundary","repair_loop","conversation", "unsupported_claim","execution_interruption","infrastructure"]})
}
fn finding_schema() -> Value {
    object(
        json!({"failure_class":failure_schema(),"stage":optional_text(),
        "recognition_phase":optional_text(),"method":optional_text(),"summary":text(),
        "repair_owner":{"type":"string","enum":["prompt","native_code","infrastructure","none"]},
        "evidence":list(evidence_schema())}),
    )
}
pub fn judge_schema() -> Value {
    object(json!({"version":{"type":"integer","enum":[1]},
        "reviews":list(object(json!({"case_id":text(),"native_semantic_pass":{"type":"boolean"},
            "native_journey_pass":{"type":["boolean","null"]},"first_turn":rubric_schema(),
            "follow_up":nullable(rubric_schema()),"findings":list(finding_schema())}))),
        "clusters":list(object(json!({"id":text(),"failure_class":failure_schema(),"stage":optional_text(),
            "case_ids":list(text()),"summary":text(),"evidence":list(evidence_schema())}))),
        "qualification":text()}))
}
pub fn propose_schema() -> Value {
    let candidate = object(json!({"version":{"type":"integer","enum":[1]},"id":text(),
        "baseline_manifest_sha256":text(),"overrides":list(object(json!({"stage":text(),
            "recognition_phase":optional_text(),"method":optional_text(),"expected_guide_sha256":text(),
            "replacement_text":text(),"edits":list(object(json!({"old_text":text(),"new_text":text()})))}))),"rationale":text(),"evidence":list(evidence_schema()),
        "training_case_ids":list(text()),"holdout_case_ids":list(text())}));
    object(
        json!({"candidate":nullable(candidate),"repair_required":list(finding_schema()),"abstain_reason":optional_text()}),
    )
}

pub fn distinct_ids(ids: &[String]) -> Result<BTreeSet<&String>> {
    let set: BTreeSet<_> = ids.iter().collect();
    if set.len() != ids.len() {
        return Err("Duplicate case IDs in result".into());
    }
    Ok(set)
}
