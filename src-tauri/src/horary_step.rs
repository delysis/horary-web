//! The completion boundary for every model step. An accepted control request is
//! not data. Only `CheckedData`, constructed here after native checks, completes
//! a step. Parsing, schema checks, derivation and repair share this boundary.
#![forbid(unsafe_code)]
use crate::{
    horary_contract,
    horary_lessons::{Matter, Stage},
    reading_method::{Fact, Role},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputRequest {
    pub field: String,
    pub question: String,
    pub reason: String,
}

/// A persisted wait belongs to a particular unfinished stage, not to the last
/// conversational intent. The user's next reply is delivered to that stage.
#[derive(Clone, Deserialize, Serialize)]
pub struct PendingInput {
    pub stage: Stage,
    pub request: InputRequest,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    Running,
    Repairing { error: String },
    AwaitingUser,
    Paused { error: String },
    Complete { record_index: usize },
    Superseded { by: String },
}

impl Phase {
    pub const EDGES: &'static [(&'static str, &'static str, &'static str)] = &[
        ("prepared", "running", "input prerequisites present"),
        ("running", "repairing", "parse, schema or native rejection"),
        ("repairing", "running", "retry the same original task"),
        ("running", "awaiting_user", "explicit information request"),
        ("awaiting_user", "running", "reply to the waiting task"),
        ("running", "complete", "bound CheckedData permit"),
        ("running", "paused", "cancellation or backend interruption"),
        ("repairing", "paused", "cancellation between attempts"),
        ("paused", "running", "resume unfinished work"),
        (
            "awaiting_user",
            "superseded",
            "changed task input; no completion implied",
        ),
        ("repairing", "superseded", "changed task input"),
        ("paused", "superseded", "changed task input"),
        ("prepared", "complete", "revalidated saved data permit"),
        ("repairing", "complete", "revalidated saved data permit"),
        ("awaiting_user", "complete", "revalidated saved data permit"),
        ("paused", "complete", "revalidated saved data permit"),
        ("complete", "complete", "revalidated saved data permit"),
    ];
    pub fn name(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Running => "running",
            Self::Repairing { .. } => "repairing",
            Self::AwaitingUser => "awaiting_user",
            Self::Paused { .. } => "paused",
            Self::Complete { .. } => "complete",
            Self::Superseded { .. } => "superseded",
        }
    }
    fn allows(&self, next: &Phase) -> bool {
        Self::EDGES
            .iter()
            .any(|(from, to, _)| *from == self.name() && *to == next.name())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub key: String,
    pub stage: Stage,
    pub revision: u64,
    pub input_sha256: String,
    pub attempts: u64,
    phase: Phase,
}
impl Job {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }
    fn transition(&mut self, next: Phase) -> Result<(), String> {
        if !self.phase.allows(&next) {
            return Err(format!(
                "Invalid step transition {} to {}",
                self.phase.name(),
                next.name()
            ));
        }
        self.phase = next;
        Ok(())
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub jobs: Vec<Job>,
    pub active: Vec<String>,
    pub pending: Vec<PendingInput>,
}

pub struct CheckedData {
    stage: Stage,
    input_sha256: String,
    worksheet: Value,
    roles: Vec<Role>,
    turn: Option<Box<crate::reading_contracts::Turn>>,
}
impl CheckedData {
    pub fn turn(&self) -> Option<&crate::reading_contracts::Turn> {
        self.turn.as_deref()
    }
    pub fn worksheet(&self) -> &Value {
        &self.worksheet
    }
    pub fn roles(&self) -> &[Role] {
        &self.roles
    }
}

pub enum Checked {
    Data(CheckedData),
    NeedsInput { request: InputRequest },
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty() && self.active.is_empty() && self.pending.is_empty()
    }
    pub fn begin(
        &mut self,
        key: String,
        stage: Stage,
        revision: u64,
        input_sha256: String,
    ) -> Result<(), String> {
        if self.active.len() >= 4 || self.active.contains(&key) {
            return Err("The step is already running or the native batch is full.".into());
        }
        let index = self.jobs.iter().position(|job| job.key == key);
        if index.is_none() {
            for job in self.jobs.iter_mut().filter(|job| {
                job.stage == stage
                    && job.revision == revision
                    && matches!(
                        job.phase,
                        Phase::AwaitingUser | Phase::Repairing { .. } | Phase::Paused { .. }
                    )
            }) {
                job.transition(Phase::Superseded { by: key.clone() })?;
            }
        }
        let job = match index {
            Some(index) => &mut self.jobs[index],
            None => {
                self.jobs.push(Job {
                    key: key.clone(),
                    stage,
                    revision,
                    input_sha256,
                    attempts: 0,
                    phase: Phase::Prepared,
                });
                self.jobs.last_mut().expect("The inserted job exists")
            }
        };
        if !matches!(
            job.phase,
            Phase::Prepared | Phase::Repairing { .. } | Phase::Paused { .. } | Phase::AwaitingUser
        ) {
            return Err("Only unfinished work can run.".into());
        }
        job.attempts = job
            .attempts
            .checked_add(1)
            .ok_or("Step attempt counter exhausted")?;
        job.transition(Phase::Running)?;
        self.active.push(key);
        Ok(())
    }

    fn running(&mut self, key: &str) -> Result<&mut Job, String> {
        if !self.active.iter().any(|active| active == key) {
            return Err("No step owns this result".into());
        }
        self.jobs
            .iter_mut()
            .find(|job| job.key == key && job.phase == Phase::Running)
            .ok_or_else(|| "The result does not belong to a running step.".into())
    }

    pub fn reject(&mut self, key: &str, error: String) -> Result<(), String> {
        self.running(key)?.transition(Phase::Repairing { error })?;
        self.active.retain(|active| active != key);
        Ok(())
    }

    pub fn wait(&mut self, key: &str, request: InputRequest) -> Result<(), String> {
        let job = self.running(key)?;
        let stage = job.stage;
        job.transition(Phase::AwaitingUser)?;
        self.pending.retain(|pending| pending.stage != stage);
        self.pending.push(PendingInput { stage, request });
        self.active.retain(|active| active != key);
        Ok(())
    }

    pub fn finish(
        &mut self,
        key: &str,
        permit: &CheckedData,
        record_index: usize,
    ) -> Result<(), String> {
        let job = self.running(key)?;
        if permit.stage != job.stage || permit.input_sha256 != job.input_sha256 {
            return Err("The completion permit belongs to different work.".into());
        }
        let stage = job.stage;
        job.transition(Phase::Complete { record_index })?;
        self.pending.retain(|pending| pending.stage != stage);
        self.active.retain(|active| active != key);
        Ok(())
    }

    pub fn reuse(
        &mut self,
        key: String,
        revision: u64,
        permit: &CheckedData,
        record_index: usize,
    ) -> Result<(), String> {
        if !self.active.is_empty() {
            return Err("Do not reuse a result while other steps are running.".into());
        }
        if let Some(job) = self.jobs.iter_mut().find(|job| job.key == key) {
            if job.stage != permit.stage
                || job.input_sha256 != permit.input_sha256
                || job.revision != revision
            {
                return Err("The saved result belongs to different work.".into());
            }
            job.transition(Phase::Complete { record_index })?;
        } else {
            self.jobs.push(Job {
                key,
                stage: permit.stage,
                revision,
                input_sha256: permit.input_sha256.clone(),
                attempts: 0,
                phase: Phase::Complete { record_index },
            });
        }
        self.pending.retain(|pending| pending.stage != permit.stage);
        Ok(())
    }

    pub fn pause(&mut self, error: String) {
        self.active.clear();
        for job in self
            .jobs
            .iter_mut()
            .filter(|job| matches!(job.phase, Phase::Running | Phase::Repairing { .. }))
        {
            // Includes cancellation between rejection and the next generation.
            // Completed work and genuine user waits remain intact.
            let _ = job.transition(Phase::Paused {
                error: error.clone(),
            });
        }
    }
}

pub fn information_fields(stage: Stage) -> &'static [&'static str] {
    match stage {
        Stage::Significators => &["subject_relationship", "ownership", "context"],
        Stage::Condition | Stage::Reception => &["context"],
        Stage::Contacts | Stage::Judgment | Stage::Explanation => &["context", "scope"],
        Stage::Location => &["ownership", "context"],
        _ => &[],
    }
}

pub fn validation_authority() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        crate::horary_lessons::digest(&format!(
            "{}\n{}\n{}\n{}\n{}",
            include_str!("horary_step.rs"),
            include_str!("horary_contract.rs"),
            include_str!("reading_method.rs"),
            include_str!("horary_role_options.rs"),
            include_str!("reading_contracts.rs")
        ))
    })
}

pub fn request_schema(stage: Stage) -> Value {
    json!({"type":"object","properties":{"request_input":{"type":"object","properties":{
        "field":{"type":"string","enum":information_fields(stage)},
        "question":{"type":"string","maxLength":180},
        "reason":{"type":"string","maxLength":240}
    },"required":["field","question","reason"],"additionalProperties":false}},"required":["request_input"],"additionalProperties":false})
}

fn request_schema_for(stage: Stage, input: &Value) -> Value {
    let mut schema = request_schema(stage);
    if original_input(input)["reading_request"].is_object() {
        let mut fields = information_fields(stage)
            .iter()
            .map(|f| Value::String((*f).into()))
            .collect::<Vec<_>>();
        fields.extend(
            crate::reading_contracts::Field::ALL
                .iter()
                .map(|f| Value::String(f.name().into())),
        );
        fields.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        fields.dedup();
        schema["properties"]["request_input"]["properties"]["field"]["enum"] = Value::Array(fields);
    }
    schema
}

#[cfg(test)]
pub fn response_schema(stage: Stage, matter: Matter, facts: &[Fact]) -> Value {
    let data = horary_contract::schema_for(stage, matter, facts);
    if information_fields(stage).is_empty() {
        data
    } else {
        json!({"oneOf":[data,request_schema(stage)]})
    }
}

pub fn response_schema_for(stage: Stage, matter: Matter, input: &Value, facts: &[Fact]) -> Value {
    if stage == Stage::Intake {
        let case = serde_json::from_value::<crate::reading_contracts::Consultation>(
            original_input(input)["consultation"].clone(),
        )
        .ok();
        return crate::reading_contracts::turn_schema(case.as_ref());
    }
    if stage != Stage::Significators {
        let data = horary_contract::schema_for(stage, matter, facts);
        return if information_fields(stage).is_empty() {
            data
        } else {
            json!({"oneOf":[data,request_schema_for(stage,input)]})
        };
    }
    let options: Result<crate::horary_role_options::Options, _> =
        serde_json::from_value(original_input(input)["native_role_options"].clone());
    match options {
        Ok(options) if options.missing.is_empty() => {
            json!({"oneOf":[crate::horary_role_options::contract(&options),request_schema_for(stage,input)]})
        }
        _ => request_schema_for(stage, input),
    }
}

/// Input gates are separate from model-output repair. An omitted internal
/// artifact is the controller's job; it must not ask a user for chart data.
pub fn prerequisites(stage: Stage, input: &Value) -> Result<(), String> {
    let input = original_input(input);
    let roles = input
        .get("roles")
        .or_else(|| input["question"].get("roles"));
    match stage {
        Stage::Significators
            if !input["house_rulers_and_positions"]
                .as_array()
                .is_some_and(|facts| facts.iter().any(|f| f["kind"] == "house")) =>
        {
            Err("The controller must supply calculated house rulers before assigning roles.".into())
        }
        Stage::Significators if !input["native_role_options"].is_object() => {
            Err("The controller must supply the native role-option table.".into())
        }
        Stage::Condition | Stage::Reception | Stage::Contacts | Stage::Location
            if roles
                .and_then(Value::as_array)
                .is_none_or(|roles| roles.is_empty()) =>
        {
            Err("The controller must resolve roles before weighing testimony.".into())
        }
        Stage::Judgment
            if ["condition", "reception", "contacts"].iter().any(|field| {
                !input[*field]["checks"].is_object() || input[*field].get("request_input").is_some()
            }) =>
        {
            Err("The controller must resolve all required testimony before judgment.".into())
        }
        Stage::Explanation
            if input["follow_up_words"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty()) =>
        {
            Err("An explanation needs the person's actual follow-up words.".into())
        }
        Stage::Explanation
            if input["prior_worksheet"].is_null() && input["chart_context"].is_null() =>
        {
            Err(
                "The controller must supply the selected reading step or native chart context."
                    .into(),
            )
        }
        _ => Ok(()),
    }
}

pub fn original_input(mut input: &Value) -> &Value {
    // Repair always carries the same original input and only the latest rejected
    // proposal. Old saved repairs may be nested; do not propagate that nesting.
    while let Some(original) = input.get("original_input") {
        input = original;
    }
    input
}

pub fn check(
    stage: Stage,
    matter: Matter,
    value: &Value,
    input: &Value,
    facts: &[Fact],
) -> Result<Checked, String> {
    if stage == Stage::Intake {
        let input = original_input(input);
        horary_contract::validate_shape(value, &response_schema_for(stage, matter, input, facts))?;
        let turn: crate::reading_contracts::Turn =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let mut case: crate::reading_contracts::Consultation =
            serde_json::from_value(input["consultation"].clone()).unwrap_or_default();
        case.apply(
            &turn,
            0,
            input["latest_words"].as_str().unwrap_or(""),
            input["spoken_input"] == true,
        )?;
        let words = if input["spoken_input"] == true {
            turn.heard.as_str()
        } else {
            input["latest_words"].as_str().unwrap_or("")
        };
        if words.trim().to_ascii_lowercase().starts_with("how many ")
            && matches!(
                turn.intent,
                crate::reading_contracts::Intent::Read
                    | crate::reading_contracts::Intent::NewQuestion
                    | crate::reading_contracts::Intent::Correct
            )
            && case
                .frame
                .resolved()
                .is_some_and(|frame| frame.facet != crate::reading_contracts::Facet::Quantity)
        {
            return Err("The supplied question explicitly asks HOW MANY. Keep facet=quantity and its count goal; do not substitute an event prediction.".into());
        }
        if case.question.resolved().is_none()
            && case.subject.resolved().is_some()
            && case.method().is_some()
        {
            return Err("The original consultation has no question yet. Preserve the actual question from latest_words (or heard for audio); null cannot stand for an unsaved question. A rejected previous_worksheet is not retained context.".into());
        }
        if case.question.resolved().is_some()
            && case.subject.resolved().is_some()
            && case.method().is_none()
            && !matches!(
                turn.intent,
                crate::reading_contracts::Intent::Explain
                    | crate::reading_contracts::Intent::Pause
                    | crate::reading_contracts::Intent::Restore
            )
        {
            return Err("Select the reading method from the retained concern, or explicitly mark it unclassified. A blank frame cannot complete classification.".into());
        }
        if turn.intent == crate::reading_contracts::Intent::Restore
            && !input["available_revisions"]
                .as_array()
                .is_some_and(|revisions| {
                    revisions
                        .iter()
                        .any(|r| r["number"].as_u64() == turn.restore_revision)
                })
        {
            return Err("Restore must select an existing revision.".into());
        }
        return Ok(Checked::Data(CheckedData {
            stage,
            input_sha256: crate::horary_lessons::digest(&input.to_string()),
            worksheet: value.clone(),
            roles: Vec::new(),
            turn: Some(Box::new(turn)),
        }));
    }
    if value.get("request_input").is_some() {
        horary_contract::validate_shape(value, &request_schema_for(stage, input))?;
        let request: InputRequest =
            serde_json::from_value(value["request_input"].clone()).map_err(|e| e.to_string())?;
        if request.question.trim().is_empty() || request.reason.trim().is_empty() {
            return Err(
                "A request must name the missing context and ask one specific question.".into(),
            );
        }
        let ready = &original_input(input)["reading_request"];
        if ready.is_object() {
            let subject = &ready["subject"];
            if request.field == "ownership"
                && subject["owner_id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty())
                || request.field == "subject_relationship"
                    && subject["owner_id"].as_str().is_some_and(|id| {
                        id == "querent"
                            || ready["people"][id]["relationship"]
                                .as_str()
                                .is_some_and(|r| r != "unknown")
                    })
            {
                return Err("This fact is already resolved in reading_request. Use it; do not ask the person to repeat known ownership or capacity.".into());
            }
        }
        return Ok(Checked::NeedsInput { request });
    }
    let input = original_input(input);
    if stage != Stage::Significators {
        horary_contract::validate_for(stage, matter, value, facts)?;
    }
    let roles = if stage == Stage::Significators {
        let options: crate::horary_role_options::Options =
            serde_json::from_value(input["native_role_options"].clone())
                .map_err(|e| e.to_string())?;
        horary_contract::validate_shape(value, &crate::horary_role_options::contract(&options))?;
        crate::horary_role_options::resolve(&options, value, facts)?
    } else {
        Vec::new()
    };
    if stage == Stage::Explanation && !input["chart_context"].is_null() {
        let required = if input["focus"] == "place" {
            "chart.place"
        } else {
            "chart.moment"
        };
        let evidence = &value["checks"]["evidence_used"];
        if evidence["state"] != "supported"
            || !evidence["evidence"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == required))
        {
            return Err(format!("The explanation must use the supplied native {required} fact; a missing interpretation does not mean a missing chart."));
        }
    }
    match stage {
        Stage::Place
            if value["mode"] == "select"
                && !input["candidates"].as_array().is_some_and(|candidates| {
                    candidates.iter().any(|p| p["id"] == value["place_id"])
                }) =>
        {
            return Err("Choose a supplied place candidate ID.".into());
        }
        Stage::Place
            if value["mode"] == "lookup"
                && value["query"].as_str().is_none_or(|s| s.trim().is_empty()) =>
        {
            return Err("A place lookup needs a stated location.".into())
        }
        Stage::Place | Stage::Moment if value["mode"] == "ask" => {
            let question = value["clarification"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("Ask the specific missing place or question moment.")?;
            return Ok(Checked::NeedsInput {
                request: InputRequest {
                    field: if stage == Stage::Place {
                        "chart_place"
                    } else {
                        "chart_moment"
                    }
                    .into(),
                    question: question.into(),
                    reason: value["basis"].as_str().unwrap_or("").into(),
                },
            });
        }
        Stage::Moment if value["mode"] == "explicit" => {
            if input["explicit_occurrence"]
                .as_str()
                .is_some_and(|choice| value["occurrence"].as_str() != Some(choice))
            {
                return Err("Use the person's explicitly selected earlier/later occurrence; do not replace it.".into());
            }
            let zone = input["selected_timezone"]
                .as_str()
                .ok_or("The controller must supply a selected time zone")?;
            let local = value["local_time"]
                .as_str()
                .ok_or("Supply a civil date and time")?;
            let occurrence = value["occurrence"].as_str().unwrap_or("");
            if let Err(error) =
                horary_ai_core::chart_input::resolve_chart_time(local, zone, occurrence)
            {
                if error.contains("occurs twice") || error.contains("does not exist") {
                    return Ok(Checked::NeedsInput { request: InputRequest {
                        field: if error.contains("occurs twice"){"time_occurrence"}else{"chart_moment"}.into(),
                        question: if error.contains("occurs twice") {
                            "That clock time happened twice. Do you mean the earlier occurrence or the later one?"
                        } else { "The clocks skipped that time. What time before or after the change should I use?" }.into(),
                        reason: error,
                    } });
                }
                return Err(error);
            }
        }
        _ => {}
    }
    Ok(Checked::Data(CheckedData {
        stage,
        input_sha256: crate::horary_lessons::digest(&input.to_string()),
        worksheet: value.clone(),
        roles,
        turn: None,
    }))
}

#[cfg(test)]
#[path = "horary_step_tests.rs"]
mod invariants;
