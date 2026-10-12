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
        (
            "repairing",
            "paused",
            "cancellation or repeated native rejection",
        ),
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
    calendar_normalizations: Vec<Value>,
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
    pub fn calendar_normalizations(&self) -> &[Value] {
        &self.calendar_normalizations
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

    /// A repeated rejection pauses only its owner. Other paid batch results
    /// must still enter their running jobs and retain their own records.
    pub fn pause_job(&mut self, key: &str, error: String) -> Result<(), String> {
        let job = self
            .jobs
            .iter_mut()
            .find(|job| job.key == key)
            .ok_or("No step owns this pause")?;
        if !matches!(job.phase, Phase::Running | Phase::Repairing { .. }) {
            return Err("Only running or repairing work can pause".into());
        }
        job.transition(Phase::Paused { error })?;
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
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            include_str!("horary_step.rs"),
            include_str!("horary_contract.rs"),
            include_str!("reading_method.rs"),
            include_str!("horary_role_options.rs"),
            include_str!("reading_contracts.rs"),
            include_str!("message_programs.rs"),
            include_str!("../../crates/horary-ai-core/src/chart_input.rs")
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
    if stage == Stage::Conversation {
        return crate::horary_conversation::schema(original_input(input));
    }
    if stage == Stage::Intake {
        let case = serde_json::from_value::<crate::reading_contracts::Consultation>(
            original_input(input)["consultation"].clone(),
        )
        .ok();
        let mut schema = crate::reading_contracts::turn_schema(case.as_ref());
        if original_input(input)["recognition_phase"] == "classify_question" {
            // Classification selects the next lesson; it cannot establish
            // facts that lesson has not yet checked.
            schema["properties"]["subject"] = json!({"type":"null"});
            // This phase cannot extract people. Showing the whole person
            // item contract beside maxItems=0 primed unconstrained models to
            // populate it anyway; the only legal value remains [].
            schema["properties"]["people"] = json!({"type":"array","maxItems":0});
            // No observations may be emitted, so do not prefill the extractor's
            // field/value alternatives in this unrelated classification call.
            schema["properties"]["updates"] = json!({"type":"array","maxItems":0});
        } else if original_input(input)["recognition_phase"] == "complete_selected_program" {
            // Verify the tentative classification, then collect that method's
            // facts. The question and already accepted observations retain
            // their authority; the provisional frame is refinable.
            let fields = schema["properties"]
                .as_object_mut()
                .expect("Turn schema fields");
            fields.insert("intent".into(), json!({"type":"string","enum":["clarify"]}));
            fields.insert("question".into(), json!({"type":"null"}));
            // The selected lesson may refine a provisional classification.
            // A different method must be returned alone and receive its own
            // lesson before it can establish observations.
            fields.insert("heard".into(), json!({"type":"string","maxLength":0}));
            fields.insert(
                "unavailable_quote".into(),
                json!({"type":"string","maxLength":0}),
            );
            fields.insert("restore_revision".into(), json!({"type":"null"}));
            if let Some(method) = case
                .as_ref()
                .and_then(crate::reading_contracts::Consultation::method)
            {
                // A changed method can still be returned with subject=null.
                // Its own program must run before its subject types or other
                // observations can be accepted. This enum also governs raw
                // batch outputs and saved worksheets through native checking.
                let subject = &mut fields.get_mut("subject").expect("Turn subject schema")["oneOf"]
                    [1]["properties"];
                subject["kind"]["enum"] =
                    json!(crate::reading_contracts::contract(method).subject_kinds);
                if method == crate::reading_contracts::Method::Relationship {
                    subject["name"]["description"] = json!("An unspecified prospective spouse may be named Prospective partner. Preserve an explicitly identified target's actual name.");
                    subject["kind"]["description"] = json!("person is either an identified target or the unnamed prospective partner in this Relationship question. No spouse identity is invented.");
                    subject["owner_id"]["description"] = json!("For an identified target use that person's actual ID. For an unnamed prospective partner use an empty string; querent would identify the wrong person.");
                }
            }
        }
        return schema;
    }
    if stage != Stage::Significators {
        let data = if stage == Stage::Condition {
            horary_contract::condition_schema(original_input(input), facts)
        } else {
            horary_contract::schema_for(stage, matter, facts)
        };
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

/// The schema constrains decoding, while this boundary also governs raw batch
/// outputs and saved worksheets. Returns whether an authorized provisional
/// frame must be replaced before validating its particular fact patch.
fn recognition_scope(
    turn: &crate::reading_contracts::Turn,
    case: &crate::reading_contracts::Consultation,
    input: &Value,
) -> Result<bool, String> {
    use crate::reading_contracts::{Facet, Intent};
    let input = original_input(input);
    if turn.intent == Intent::Resume
        && case
            .question
            .resolved()
            .is_none_or(|question| question.trim().is_empty())
    {
        return Err("There is no understood consultation to resume. An earlier consultation described for the first time is still a new question here: classify the actual latest question with intent=read, then complete its selected program.".into());
    }
    if input["recognition_phase"] == "classify_question" || turn.intent == Intent::NewQuestion {
        if turn.subject.is_some() || !turn.people.is_empty() || !turn.updates.is_empty() {
            return Err("Classification cannot establish subject, people or observations. Return subject=null, people=[], updates=[]; the selected program will extract them from the same words.".into());
        }
        return Ok(false);
    }
    if input["recognition_phase"] != "complete_selected_program" {
        return Ok(false);
    }
    if turn.intent != Intent::Clarify
        || turn.question.is_some()
        || !turn.heard.is_empty()
        || !turn.unavailable_quote.is_empty()
        || turn.restore_revision.is_some()
    {
        return Err("Focused completion can confirm or refine the provisional frame and supply sourced facts, never replace the question or issue a control command. Use intent=clarify, question=null, heard='', unavailable_quote='', restore_revision=null.".into());
    }
    let prior = case.frame.resolved();
    let literal_count = case.question.resolved().is_some_and(|question| {
        let question = question
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        // "How much does he love me?" asks about feeling, not a numerical tally.
        // Only unambiguous count predicates override a provisional facet here.
        ["how many ", "what number ", "what is the number of "]
            .iter()
            .any(|prefix| question.starts_with(prefix))
    });
    if literal_count
        && turn
            .frame
            .as_ref()
            .or(prior)
            .is_some_and(|frame| frame.facet != Facet::Quantity)
    {
        return Err("Focused completion cannot replace the retained quantity goal with event, profit or another facet. Preserve quantity; a different question needs the person's consent in the conversational intake, before focused completion.".into());
    }
    let Some(frame) = &turn.frame else {
        return Ok(false);
    };
    if prior.is_none_or(|prior| prior.method != frame.method)
        && (turn.subject.is_some() || !turn.people.is_empty() || !turn.updates.is_empty())
    {
        return Err(format!(
            "A different selected method is a frame-only refinement. You proposed {{\"method\":\"{}\",\"facet\":\"{}\"}}. To request that reroute, keep this proposed frame and return subject=null, people=[], updates=[]. The accompanying facts are premature and remain unsaved. The proposed frame is still an unverified hypothesis: its own lesson must verify it against the original question before accepting facts. Do not restore the old tentative method merely to satisfy this scope rule.",
            frame.method.name(), frame.facet.name()
        ));
    }
    Ok(prior.is_none_or(|prior| prior != frame))
}

pub fn check(
    stage: Stage,
    matter: Matter,
    value: &Value,
    input: &Value,
    facts: &[Fact],
) -> Result<Checked, String> {
    if stage == Stage::Conversation {
        let input = original_input(input);
        crate::horary_conversation::validate_ask(input, value)?;
        horary_contract::validate_shape(value, &crate::horary_conversation::schema(input))?;
        if value["reply"].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err("The reader must give a nonempty conversational reply.".into());
        }
        return Ok(Checked::Data(CheckedData {
            stage,
            input_sha256: crate::horary_lessons::digest(&input.to_string()),
            worksheet: value.clone(),
            roles: Vec::new(),
            turn: None,
            calendar_normalizations: Vec::new(),
        }));
    }
    if stage == Stage::Intake {
        let input = original_input(input);
        horary_contract::validate_shape(value, &response_schema_for(stage, matter, input, facts))?;
        let mut turn: crate::reading_contracts::Turn =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let mut case: crate::reading_contracts::Consultation =
            serde_json::from_value(input["consultation"].clone()).unwrap_or_default();
        let scope_refined = recognition_scope(&turn, &case, input)?;
        let method_only_refinement = turn
            .frame
            .as_ref()
            .is_some_and(|frame| case.method() != Some(frame.method))
            && turn.subject.is_none()
            && turn.people.is_empty()
            && turn.updates.is_empty();
        if input["recognition_phase"] != "classify_question"
            && !method_only_refinement
            && matches!(
                turn.intent,
                crate::reading_contracts::Intent::Clarify
                    | crate::reading_contracts::Intent::Correct
            )
        {
            let source_words = if input["spoken_input"] == true {
                turn.heard.as_str()
            } else {
                input["latest_words"].as_str().unwrap_or("")
            };
            crate::reading_contracts::validate_anchor_completion(&case, &turn, source_words)?;
        }
        if scope_refined {
            // Native phase authority allows correcting a provisional frame;
            // it does not turn clarification into a user-authorized rewrite
            // of the question, its subject, or previously accepted facts.
            case.frame = crate::reading_contracts::Slot::Missing;
        }
        let mut calendar_normalizations = normalize_event_dates(&mut turn, &case, input)?;
        for receipt in &mut calendar_normalizations {
            receipt["source_input_sha256"] =
                json!(crate::horary_lessons::digest(&input.to_string()));
            receipt["worksheet_sha256"] = json!(crate::horary_lessons::digest(&value.to_string()));
        }
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
            && case.method().is_some()
            && (case.subject.resolved().is_some()
                || input["recognition_phase"] == "classify_question"
                    && case.method().is_some_and(|method| {
                        !matches!(
                            method,
                            crate::reading_contracts::Method::Wish
                                | crate::reading_contracts::Method::Unclassified
                        )
                    })
                || matches!(
                    turn.intent,
                    crate::reading_contracts::Intent::Read
                        | crate::reading_contracts::Intent::NewQuestion
                ))
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
            calendar_normalizations,
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
    if stage == Stage::Condition {
        if let Err(error) =
            horary_contract::validate_shape(value, &horary_contract::condition_schema(input, facts))
        {
            let error = analytical_check_shape_feedback(stage, value, error);
            let repair = condition_coverage(value, input, facts)
                .err()
                .unwrap_or_else(|| condition_repair_limits(input, facts));
            return Err(format!("{error}\n{repair}"));
        }
    }
    if stage != Stage::Significators {
        horary_contract::validate_for(stage, matter, value, facts).map_err(|error| {
            let error = analytical_check_shape_feedback(stage, value, error);
            if stage == Stage::Condition {
                let repair = condition_coverage(value, input, facts)
                    .err()
                    .unwrap_or_else(|| condition_repair_limits(input, facts));
                format!("{error}\n{repair}")
            } else {
                error
            }
        })?;
    }
    if stage == Stage::Condition {
        condition_coverage(value, input, facts)?;
    }
    crate::message_programs::check(stage, input, value, facts)?;
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
        calendar_normalizations: Vec::new(),
    }))
}

/// Derive a current-turn event date before the same atomic fact application.
/// The raw worksheet stays raw; the checked patch and audit expose the native
/// normalization. Neither a retained quote nor a historical chart borrows this
/// turn's clock. All other fields, quote proofs and conflict modes are untouched.
fn normalize_event_dates(
    turn: &mut crate::reading_contracts::Turn,
    case: &crate::reading_contracts::Consultation,
    input: &Value,
) -> Result<Vec<Value>, String> {
    use crate::reading_contracts::{Field, RequirementKey, UpdateMode};
    use horary_ai_core::chart_input::{event_day_value, local_clock, resolve_event_day};

    let mut receipts = Vec::new();
    let (Some(instant), Some(zone), Some(clock)) = (
        input["receipt_instant_ms"].as_f64(),
        input["current_timezone"].as_str(),
        input["current_local_clock"].as_str(),
    ) else {
        return Ok(receipts);
    };
    if local_clock(instant, zone).ok().as_deref() != Some(clock)
        || case.facts.contains_key(&Field::QuestionTime)
        || turn
            .updates
            .iter()
            .any(|update| update.field == Field::QuestionTime)
    {
        return Ok(receipts);
    }
    let words = if input["spoken_input"] == true {
        turn.heard.as_str()
    } else {
        input["latest_words"].as_str().unwrap_or("")
    };
    let question = case
        .question
        .resolved()
        .map(String::as_str)
        .or(turn.question.as_deref())
        .unwrap_or("");
    let pending_event_time = case.requested == Some(RequirementKey::Field(Field::EventTime));
    for update in &mut turn.updates {
        if update.field != Field::EventTime || update.mode == UpdateMode::Unavailable {
            continue;
        }
        let Some(day) = resolve_event_day(
            &update.quote,
            words,
            question,
            pending_event_time,
            instant,
            zone,
        )?
        else {
            continue;
        };
        let normalized = event_day_value(&update.value, &day).ok_or_else(|| {
            format!("The exact current source establishes event date {}. Preserve one date component and all supplied event circumstances; do not combine competing dates or remove an hour. This date does not change question_time.", day.date)
        })?;
        receipts.push(
            json!({"field":update.field,"mode":update.mode,"quote":update.quote,
            "original_value":update.value,"normalized_value":normalized,"native_day":day}),
        );
        update.value = normalized;
    }
    Ok(receipts)
}

/// Explain a rejected nested check without changing or accepting its shape.
fn analytical_check_shape_feedback(stage: Stage, value: &Value, error: String) -> String {
    const FIELDS: [&str; 3] = ["state", "evidence", "finding"];
    for key in stage.checks() {
        if !error.starts_with(&format!("$.checks.{key}: missing fields ")) {
            continue;
        }
        let nested_check = value["checks"][key].as_object().is_some_and(|record| {
            record.iter().any(|(field, child)| {
                !FIELDS.contains(&field.as_str())
                    && child
                        .as_object()
                        .is_some_and(|fields| FIELDS.iter().all(|name| fields.contains_key(*name)))
            })
        });
        if nested_check {
            return format!("{error}\nRepair checks.{key}: this check is ONE direct state/evidence/finding object. Combine role-specific distinctions in its existing finding string, with actual allowed IDs in the evidence array; do not add per-role child maps. Shape illustration only, with placeholders rather than a verdict or supplied fact IDs: {{\"state\":\"<allowed state>\",\"evidence\":[\"<actual allowed ID>\"],\"finding\":\"<combine role-specific distinctions here>\"}}. Preserve every required check and repair the whole rejected worksheet.");
        }
    }
    error
}

/// Require coverage of supplied native testimony, not a particular verdict or
/// an assertion that the model's interpretation of that testimony is correct.
fn condition_coverage(value: &Value, input: &Value, facts: &[Fact]) -> Result<(), String> {
    use horary_ai_core::book_method::ConditionFacet;
    let roles = input
        .get("roles")
        .or_else(|| input["question"].get("roles"))
        .and_then(Value::as_array)
        .ok_or("Condition review needs the original selected roles.")?;
    let planets: std::collections::BTreeSet<_> = roles
        .iter()
        .filter_map(|role| role["planet"].as_str())
        .collect();
    let mut missing = Vec::new();
    for planet in planets {
        for (facet, check) in [
            (ConditionFacet::Essential, "own_dignity"),
            (ConditionFacet::HouseCapacity, "ability_to_act"),
        ] {
            let available: Vec<_> = facts
                .iter()
                .filter(|fact| {
                    fact.kind == "condition"
                        && fact.condition_facet == Some(facet)
                        && fact.planets.iter().any(|p| p == planet)
                })
                .collect();
            if available.is_empty() {
                continue; // Missing calculation is unknown, never fabricated.
            }
            let cited = value["checks"][check]["evidence"].as_array();
            if !available.iter().any(|fact| {
                cited.is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(fact.id.as_str())))
            }) {
                let testimony = available
                    .iter()
                    .map(|fact| format!("{}: {}", fact.id, fact.detail))
                    .collect::<Vec<_>>()
                    .join("; ");
                missing.push(format!("Condition review omits {planet} from {check}. Required native testimony (choose a supplied ID for this facet): {testimony}."));
            }
        }
        for fact in facts.iter().filter(|fact| {
            fact.kind == "condition"
                && fact
                    .condition_facet
                    .is_some_and(ConditionFacet::requires_context_review)
                && fact.planets.iter().any(|p| p == planet)
        }) {
            let cited = ["ability_to_act", "context_exceptions"]
                .iter()
                .any(|check| {
                    value["checks"][*check]["evidence"]
                        .as_array()
                        .is_some_and(|ids| {
                            ids.iter().any(|id| id.as_str() == Some(fact.id.as_str()))
                        })
                });
            if !cited {
                missing.push(format!("Condition review omits {planet}'s required contextual testimony from ability_to_act or context_exceptions: {}: {}. Weigh it or explain its contextual relevance; no particular state or verdict is required.", fact.id, fact.detail));
            }
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("{}\n{}\nPreserve correctly sourced checks and repair the whole rejected worksheet. Do not ask the person for already supplied chart facts. Coverage does not require a positive state or establish interpretation correctness.", missing.join("\n"), condition_repair_limits(input, facts)))
    }
}

fn condition_repair_limits(input: &Value, facts: &[Fact]) -> String {
    let contract = horary_contract::condition_schema(input, facts);
    let limits = Stage::Condition
        .checks()
        .iter()
        .map(|key| {
            format!(
                "{key}: {}",
                contract["properties"]["checks"]["properties"][*key]["properties"]["evidence"]
                    ["maxItems"]
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("Allowed states: supported, contradicted, unestablished, not_relevant. Evidence maxima for this input ({limits}); use current IDs only. Optional supporting facts must fit these limits without replacing required native testimony, including supplied retrogradation or Sun-opposition context.")
}

#[cfg(test)]
#[path = "horary_step_tests.rs"]
mod invariants;

#[cfg(test)]
mod analytical_shape_repair_tests {
    use super::*;

    fn reception() -> (Value, Value, Vec<Fact>) {
        let facts = crate::reading_method::facts(Some(&json!({
            "derived":{"receptions":[
                {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"domicile","polarity":"positive"}
            ]}
        })));
        let id = &facts
            .iter()
            .find(|fact| fact.kind == "reception")
            .unwrap()
            .id;
        let input = json!({"roles":[
            {"label":"First synthetic party","planet":"Venus","house":null},
            {"label":"Second synthetic party","planet":"Mars","house":null}
        ],"facts":facts});
        let value = json!({"checks":{
            "direction":{"state":"supported","evidence":[id],"finding":"The supplied fact gives Venus's regard for Mars; the reverse is not supplied."},
            "strength_and_quality":{"state":"supported","evidence":[id],"finding":"The supplied domicile quality gives positive regard in this direction."},
            "contextual_motive":{"state":"unestablished","evidence":[],"finding":"No motive follows from this synthetic fact alone."}
        },"summary":"Synthetic directed-reception worksheet; no event prediction.","unknowns":[]});
        (input, value, facts)
    }

    #[test]
    fn nested_reception_check_is_rejected_with_direct_record_repair_guidance() {
        let (input, corrected, facts) = reception();
        let mut malformed = corrected.clone();
        malformed["checks"]["direction"] = json!({"role_a_to_b":corrected["checks"]["direction"]});
        let before = malformed.clone();
        let original =
            horary_contract::validate_for(Stage::Reception, Matter::Other, &malformed, &facts)
                .unwrap_err();
        let error = check(Stage::Reception, Matter::Other, &malformed, &input, &facts)
            .err()
            .unwrap();
        assert!(error.starts_with(&original));
        assert!(error.contains("$.checks.direction: missing fields"));
        assert!(error.contains("Repair checks.direction"));
        assert!(error.contains("ONE direct state/evidence/finding object"));
        assert!(error.contains("existing finding string"));
        assert!(error.contains("actual allowed IDs in the evidence array"));
        assert!(error.contains("do not add per-role child maps"));
        assert!(error.contains("Shape illustration only"));
        assert!(error.contains("\"state\":\"<allowed state>\""));
        assert!(!error.contains("\"supported\""));
        assert!(!error.contains(&format!("\"{}\"", facts[0].id)));
        assert_eq!(
            malformed, before,
            "A rejection must never normalize the map"
        );
        let Checked::Data(data) =
            check(Stage::Reception, Matter::Other, &corrected, &input, &facts).unwrap()
        else {
            panic!("The existing direct contract must still complete the step");
        };
        assert_eq!(data.worksheet(), &corrected);
        // Merely adding the direct fields cannot make a per-role child legal.
        malformed = corrected.clone();
        malformed["checks"]["direction"]["role_a_to_b"] = corrected["checks"]["direction"].clone();
        assert!(check(Stage::Reception, Matter::Other, &malformed, &input, &facts).is_err());
    }

    #[test]
    fn unrelated_reception_errors_keep_their_exact_existing_feedback() {
        let (input, corrected, facts) = reception();
        let mut cases = Vec::new();
        let mut missing = corrected.clone();
        missing["checks"]["direction"]
            .as_object_mut()
            .unwrap()
            .remove("finding");
        cases.push(missing);
        let mut extra = corrected.clone();
        extra["checks"]["direction"]["metadata"] = json!({"note":"Unrelated extra object"});
        cases.push(extra);
        let mut wrong_state = corrected.clone();
        wrong_state["checks"]["direction"]["state"] = json!("strong");
        cases.push(wrong_state);
        let mut foreign = corrected.clone();
        foreign["checks"]["direction"]["evidence"] = json!(["not-supplied"]);
        cases.push(foreign);
        let mut other_path = corrected.clone();
        other_path["checks"]["direction"] = json!({"role_a_to_b":corrected["checks"]["direction"]});
        other_path.as_object_mut().unwrap().remove("summary");
        cases.push(other_path);
        let mut oversized = corrected.clone();
        oversized["checks"]["direction"] = json!({"role_a_to_b":corrected["checks"]["direction"]});
        oversized["checks"]["strength_and_quality"]["finding"] = json!("a".repeat(8001));
        cases.push(oversized);
        for value in cases {
            let original =
                horary_contract::validate_for(Stage::Reception, Matter::Other, &value, &facts)
                    .unwrap_err();
            assert_eq!(
                check(Stage::Reception, Matter::Other, &value, &input, &facts)
                    .err()
                    .unwrap(),
                original
            );
        }
        let mut unsupported = corrected;
        unsupported["checks"]["direction"]["evidence"] = json!([]);
        let original =
            horary_contract::validate_for(Stage::Reception, Matter::Other, &unsupported, &facts)
                .unwrap_err();
        assert_eq!(
            check(
                Stage::Reception,
                Matter::Other,
                &unsupported,
                &input,
                &facts
            )
            .err()
            .unwrap(),
            original
        );
    }
}

#[cfg(test)]
mod recognition_boundaries {
    use super::*;
    use crate::reading_contracts::{
        self, Consultation, Evidence, Facet, Frame, Intent, Method, Observation, Slot,
    };

    fn retained(question: &str, method: Method, facet: Facet) -> Consultation {
        Consultation {
            question: Slot::Resolved {
                observation: Observation {
                    value: question.into(),
                    evidence: Evidence::User {
                        turn: 0,
                        quote: question.into(),
                    },
                },
            },
            frame: Slot::Resolved {
                observation: Observation {
                    value: Frame { method, facet },
                    evidence: Evidence::User {
                        turn: 0,
                        quote: question.into(),
                    },
                },
            },
            ..Consultation::default()
        }
    }

    fn input(case: &Consultation, phase: &str, words: &str) -> Value {
        json!({"consultation":case,"recognition_phase":phase,"latest_words":words,"spoken_input":false})
    }

    fn run(turn: &reading_contracts::Turn, input: &Value) -> Result<Checked, String> {
        check(
            Stage::Intake,
            Matter::Other,
            &serde_json::to_value(turn).unwrap(),
            input,
            &[],
        )
    }

    fn calendar_input(case: &Consultation, words: &str) -> Value {
        let mut source = input(case, "complete_selected_program", words);
        source["receipt_instant_ms"] = json!(horary_ai_core::chart_input::resolve_chart_time(
            "2032-03-02T10:00",
            "Europe/London",
            ""
        )
        .unwrap());
        source["current_local_clock"] = json!("2032-03-02T10:00");
        source["current_timezone"] = json!("Europe/London");
        source
    }

    fn calendar_patch(value: &str, quote: &str) -> reading_contracts::Turn {
        let mut turn = reading_contracts::control(Intent::Clarify);
        turn.updates.push(reading_contracts::Update {
            field: reading_contracts::Field::EventTime,
            value: value.into(),
            quote: quote.into(),
            mode: reading_contracts::UpdateMode::Supply,
        });
        turn
    }

    #[test]
    fn native_event_date_acceptance_preserves_raw_proposal_quote_and_hour_with_clock_proof() {
        let words = "Will my boat sell Friday at 3 PM?";
        let mut case = retained(words, Method::MovableDeal, Facet::Event);
        let source = calendar_input(&case, words);
        let turn = calendar_patch("2032-03-04 at 3 PM", "Friday at 3 PM");
        let Checked::Data(data) = run(&turn, &source).unwrap() else {
            panic!("Expected data")
        };
        assert_eq!(
            data.worksheet()["updates"][0]["value"],
            "2032-03-04 at 3 PM"
        );
        let patch = data.turn().unwrap();
        assert_eq!(patch.updates[0].value, "2032-03-05 at 3 PM");
        assert_eq!(patch.updates[0].quote, "Friday at 3 PM");
        assert_eq!(
            data.calendar_normalizations()[0]["original_value"],
            "2032-03-04 at 3 PM"
        );
        assert_eq!(
            data.calendar_normalizations()[0]["native_day"]["local_clock"],
            source["current_local_clock"]
        );
        case.apply(patch, 7, words, false).unwrap();
        assert_eq!(
            case.text(reading_contracts::Field::EventTime),
            Some("2032-03-05 at 3 PM")
        );
        let Slot::Resolved { observation } = &case.facts[&reading_contracts::Field::EventTime]
        else {
            panic!("Expected resolved fact")
        };
        assert_eq!(
            observation.evidence,
            Evidence::User {
                turn: 7,
                quote: "Friday at 3 PM".into()
            }
        );
        assert!(case.text(reading_contracts::Field::QuestionTime).is_none());
        assert!(case
            .text(reading_contracts::Field::TimeOccurrence)
            .is_none());
    }

    #[test]
    fn native_event_dates_keep_conflicts_and_explicit_corrections_atomic() {
        let words = "Will my boat sell Friday?";
        let mut case = retained(words, Method::MovableDeal, Facet::Event);
        case.facts.insert(
            reading_contracts::Field::EventTime,
            Slot::Resolved {
                observation: Observation {
                    value: "2032-03-12".into(),
                    evidence: Evidence::User {
                        turn: 1,
                        quote: "March 12".into(),
                    },
                },
            },
        );
        let source = calendar_input(&case, words);
        let mut turn = calendar_patch("Friday", "Friday");
        let Checked::Data(data) = run(&turn, &source).unwrap() else {
            panic!("Expected data")
        };
        case.apply(data.turn().unwrap(), 2, words, false).unwrap();
        let Slot::Conflicting { observations } = &case.facts[&reading_contracts::Field::EventTime]
        else {
            panic!("Expected conflict")
        };
        assert_eq!(
            observations
                .iter()
                .map(|o| o.value.as_str())
                .collect::<Vec<_>>(),
            ["2032-03-12", "2032-03-05"]
        );
        turn.updates[0].mode = reading_contracts::UpdateMode::Correct;
        let Checked::Data(data) = run(&turn, &calendar_input(&case, words)).unwrap() else {
            panic!("Expected data")
        };
        case.apply(data.turn().unwrap(), 3, words, false).unwrap();
        assert_eq!(
            case.text(reading_contracts::Field::EventTime),
            Some("2032-03-05")
        );

        let before = serde_json::to_value(&case).unwrap();
        turn.updates[0].quote = "Friday...".into();
        assert!(run(&turn, &calendar_input(&case, words))
            .err()
            .unwrap()
            .contains("does not occur"));
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
    }

    #[test]
    fn native_event_dates_do_not_borrow_retained_historical_or_missing_clocks() {
        let question = "Will my boat sell Friday?";
        let mut case = retained(question, Method::MovableDeal, Facet::Event);
        let turn = calendar_patch("Friday", "Friday");
        let Checked::Data(data) =
            run(&turn, &calendar_input(&case, "The boat belongs to me.")).unwrap()
        else {
            panic!("Expected data")
        };
        assert_eq!(data.turn().unwrap().updates[0].value, "Friday");
        assert!(data.calendar_normalizations().is_empty());
        for (field, value) in [
            ("current_timezone", json!("unknown")),
            ("current_local_clock", json!("2032-03-03T10:00")),
            ("receipt_instant_ms", Value::Null),
        ] {
            let mut source = calendar_input(&case, question);
            source[field] = value;
            let Checked::Data(data) = run(&turn, &source).unwrap() else {
                panic!("Expected data")
            };
            assert_eq!(data.turn().unwrap().updates[0].value, "Friday");
            assert!(data.calendar_normalizations().is_empty());
        }
        case.facts.insert(
            reading_contracts::Field::QuestionTime,
            Slot::Resolved {
                observation: Observation {
                    value: "2031-04-10T11:00".into(),
                    evidence: Evidence::User {
                        turn: 1,
                        quote: "April 10, 2031 at 11 AM".into(),
                    },
                },
            },
        );
        let Checked::Data(data) = run(&turn, &calendar_input(&case, question)).unwrap() else {
            panic!("Expected data")
        };
        assert_eq!(data.turn().unwrap().updates[0].value, "Friday");
        assert!(data.calendar_normalizations().is_empty());
        assert_eq!(
            case.text(reading_contracts::Field::QuestionTime),
            Some("2031-04-10T11:00")
        );
    }

    #[test]
    fn native_event_dates_preserve_authoritative_absolute_and_ambiguous_source_dates() {
        for (words, value, quote) in [
            (
                "Will my boat sell Friday, March 12?",
                "2032-03-12 at 3 PM",
                "Friday, March 12",
            ),
            (
                "Will my boat sell Friday or Saturday?",
                "Friday or Saturday",
                "Friday or Saturday",
            ),
            (
                "Will my boat sell next Friday?",
                "next Friday",
                "next Friday",
            ),
        ] {
            let case = retained(words, Method::MovableDeal, Facet::Event);
            let Checked::Data(data) =
                run(&calendar_patch(value, quote), &calendar_input(&case, words)).unwrap()
            else {
                panic!("Expected data")
            };
            assert_eq!(data.turn().unwrap().updates[0].value, value);
            assert!(data.calendar_normalizations().is_empty());
        }
    }

    fn subject() -> crate::horary_role_options::Subject {
        crate::horary_role_options::Subject {
            name: "myself".into(),
            kind: "person".into(),
            owner_id: "querent".into(),
            source_quote: "myself".into(),
        }
    }

    #[test]
    fn classification_schema_and_native_scope_do_not_establish_facts() {
        let case = Consultation::default();
        let source = input(&case, "classify_question", "Can I trust myself?");
        let repaired = json!({"original_input":{"original_input":source},"previous_worksheet":{"recognition_phase":"complete_selected_program"}});
        let schema = response_schema_for(Stage::Intake, Matter::Other, &repaired, &[]);
        assert_eq!(schema["properties"]["subject"], json!({"type":"null"}));
        assert_eq!(schema["properties"]["people"]["maxItems"], 0);
        assert_eq!(schema["properties"]["updates"]["maxItems"], 0);
        let mut turn = reading_contracts::control(Intent::Read);
        turn.question = Some("Can I trust myself?".into());
        turn.frame = Some(Frame {
            method: Method::Trust,
            facet: Facet::Situation,
        });
        assert!(run(&turn, &repaired).is_ok());
        for intent in [
            Intent::Read,
            Intent::NewQuestion,
            Intent::Clarify,
            Intent::Correct,
        ] {
            let mut missing_question = turn.clone();
            missing_question.question = None;
            missing_question.intent = intent;
            assert!(
                run(&missing_question, &repaired)
                    .err()
                    .unwrap()
                    .contains("unsaved question"),
                "A model-selected intent cannot bypass the canonical prerequisite"
            );
        }
        turn.subject = Some(subject());
        assert!(recognition_scope(&turn, &case, &repaired)
            .unwrap_err()
            .contains("Classification cannot establish"));
        assert!(
            run(&turn, &repaired).is_err(),
            "Unconstrained batch output must obey the same boundary"
        );
        turn.subject = None;
        turn.updates.push(reading_contracts::Update {
            field: reading_contracts::Field::Context,
            value: "trusting myself".into(),
            quote: "trust myself".into(),
            mode: reading_contracts::UpdateMode::Supply,
        });
        assert!(recognition_scope(&turn, &case, &repaired).is_err());
        assert!(run(&turn, &repaired).is_err());
    }

    #[test]
    fn a_new_question_inside_an_existing_program_is_also_classification_only() {
        let case = retained("Can I trust myself?", Method::Trust, Facet::Situation);
        let source = input(&case, "conversation_intake", "Where is my ring?");
        let mut turn = reading_contracts::control(Intent::NewQuestion);
        turn.question = Some("Where is my ring?".into());
        turn.frame = Some(Frame {
            method: Method::LostObject,
            facet: Facet::Location,
        });
        assert!(run(&turn, &source).is_ok());
        turn.subject = Some(crate::horary_role_options::Subject {
            name: "ring".into(),
            kind: "movable".into(),
            owner_id: "querent".into(),
            source_quote: "my ring".into(),
        });
        assert!(run(&turn, &source)
            .err()
            .unwrap()
            .contains("Classification cannot establish"));
    }

    #[test]
    fn focused_completion_can_refine_a_facet_without_rewriting_the_question() {
        let words = "Can I trust myself with the keys?";
        let case = retained(words, Method::Trust, Facet::Safety);
        let source = input(&case, "complete_selected_program", words);
        let mut turn = reading_contracts::control(Intent::Clarify);
        turn.frame = Some(Frame {
            method: Method::Trust,
            facet: Facet::Situation,
        });
        turn.subject = Some(subject());
        assert!(
            run(&turn, &source).is_ok(),
            "A supported focused refinement must not create a conflicting frame"
        );
        turn.question = Some("A different concern".into());
        assert!(recognition_scope(&turn, &case, &source).is_err());
        assert!(run(&turn, &source).is_err());
        turn.question = None;
        turn.intent = Intent::NewQuestion;
        assert!(run(&turn, &source).is_err());
    }

    #[test]
    fn focused_decoder_excludes_foreign_subject_kinds_and_preserves_a_real_gap() {
        let words = "Will I get married within the next year?";
        let mut case = retained(words, Method::Relationship, Facet::Timing);
        let source = input(&case, "complete_selected_program", words);
        let schema = response_schema_for(Stage::Intake, Matter::Other, &source, &[]);
        let mut turn = reading_contracts::control(Intent::Clarify);
        turn.frame = Some(Frame {
            method: Method::Relationship,
            facet: Facet::Event,
        });
        turn.subject = Some(crate::horary_role_options::Subject {
            name: "prospective partner".into(),
            kind: "person".into(),
            owner_id: String::new(),
            source_quote: "get married".into(),
        });
        let value = serde_json::to_value(&turn).unwrap();
        horary_contract::validate_shape(&value, &schema).unwrap();
        for kind in ["person_role", "other"] {
            let mut foreign = value.clone();
            foreign["subject"]["kind"] = json!(kind);
            assert!(horary_contract::validate_shape(&foreign, &schema).is_err());
        }
        assert!(run(&turn, &source).is_ok());
        assert!(recognition_scope(&turn, &case, &source).unwrap());
        case.frame = Slot::Missing;
        case.apply(&turn, 1, words, false).unwrap();
        let anchor = reading_contracts::Anchor {
            timestamp_ms: 1791388800000.,
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
        };
        let plan = case.plan(Some(&anchor));
        assert_eq!(
            plan.needs.iter().map(|need| &need.key).collect::<Vec<_>>(),
            vec![&reading_contracts::RequirementKey::Field(
                reading_contracts::Field::Baseline
            )]
        );
        assert!(case.subject.resolved().unwrap().owner_id.is_empty());
    }

    #[test]
    fn a_frame_only_refinement_selects_a_new_subject_contract() {
        let words = "Will I get the librarian job? There is no offer yet.";
        let mut case = retained(words, Method::Relationship, Facet::Event);
        let source = input(&case, "complete_selected_program", words);
        let mut refinement = reading_contracts::control(Intent::Clarify);
        refinement.frame = Some(Frame {
            method: Method::NewJob,
            facet: Facet::Event,
        });
        assert!(run(&refinement, &source).is_ok());
        let before = response_schema_for(Stage::Intake, Matter::Other, &source, &[]);
        assert_eq!(
            before["properties"]["subject"]["oneOf"][1]["properties"]["kind"]["enum"],
            json!(["person"])
        );
        case.frame = Slot::Missing;
        case.apply(&refinement, 1, words, false).unwrap();
        let next = input(&case, "complete_selected_program", words);
        let after = response_schema_for(Stage::Intake, Matter::Other, &next, &[]);
        assert_eq!(
            after["properties"]["subject"]["oneOf"][1]["properties"]["kind"]["enum"],
            json!(["job"])
        );
        let mut extraction = reading_contracts::control(Intent::Clarify);
        extraction.subject = Some(crate::horary_role_options::Subject {
            name: "librarian job".into(),
            kind: "job".into(),
            owner_id: "querent".into(),
            source_quote: "librarian job".into(),
        });
        assert!(run(&extraction, &source).is_err());
        assert!(run(&extraction, &next).is_ok());
    }

    #[test]
    fn a_different_method_receives_its_own_lesson_before_facts_are_accepted() {
        let words = "Will I get the librarian job? There is no offer yet.";
        let case = retained(words, Method::JobOffer, Facet::Event);
        let source = input(&case, "complete_selected_program", words);
        let mut turn = reading_contracts::control(Intent::Clarify);
        turn.frame = Some(Frame {
            method: Method::NewJob,
            facet: Facet::Event,
        });
        assert!(run(&turn, &source).is_ok());
        turn.subject = Some(crate::horary_role_options::Subject {
            name: "librarian job".into(),
            kind: "job".into(),
            owner_id: "querent".into(),
            source_quote: "librarian job".into(),
        });
        assert!(run(&turn, &source).err().unwrap().contains("frame-only"));
    }

    #[test]
    fn a_how_much_feeling_question_is_not_forced_into_an_exact_count() {
        let words = "How much does my husband love me?";
        let case = retained(words, Method::Relationship, Facet::Situation);
        let source = input(&case, "complete_selected_program", words);
        let mut turn = reading_contracts::control(Intent::Clarify);
        assert!(run(&turn, &source).is_ok());

        let mistaken = retained(words, Method::Relationship, Facet::Quantity);
        turn.frame = Some(Frame {
            method: Method::Relationship,
            facet: Facet::Situation,
        });
        assert!(run(&turn, &input(&mistaken, "complete_selected_program", words)).is_ok());
    }

    #[test]
    fn literal_count_is_preserved_but_a_mistaken_quantity_classification_is_refinable() {
        let words = "How many fish will I sell?";
        let case = retained(words, Method::MovableDeal, Facet::Quantity);
        let source = input(&case, "complete_selected_program", words);
        let mut turn = reading_contracts::control(Intent::Clarify);
        turn.frame = Some(Frame {
            method: Method::Undertaking,
            facet: Facet::Profit,
        });
        assert!(run(&turn, &source).err().unwrap().contains("quantity goal"));
        let misclassified = retained(words, Method::MovableDeal, Facet::Event);
        turn.frame = None;
        assert!(
            recognition_scope(
                &turn,
                &misclassified,
                &input(&misclassified, "complete_selected_program", words)
            )
            .is_err(),
            "A null refinement must not retain an already misclassified literal count"
        );
        let agreed = "Will I sell the fish?";
        let case = retained(agreed, Method::MovableDeal, Facet::Quantity);
        let source = input(&case, "complete_selected_program", agreed);
        turn.frame = Some(Frame {
            method: Method::MovableDeal,
            facet: Facet::Event,
        });
        assert!(run(&turn, &source).is_ok(), "A mistaken classifier facet cannot overrule the actual retained predicate or an already agreed reframing");
    }

    #[test]
    fn resume_requires_a_previously_established_question_but_not_complete_inputs() {
        let words = "Use my earlier question in London on January 14 at 2:30 PM. Will I marry within a year?";
        let empty = Consultation::default();
        let source = input(&empty, "classify_question", words);
        let mut turn = reading_contracts::control(Intent::Resume);
        turn.question = Some(words.into());
        turn.frame = Some(Frame {
            method: Method::Relationship,
            facet: Facet::Event,
        });
        assert!(run(&turn, &source).err().unwrap().contains("to resume"));
        let established = retained(
            "Will I marry within a year?",
            Method::Relationship,
            Facet::Event,
        );
        assert!(
            !established.understood(),
            "This question still lacks a focused subject and baseline"
        );
        turn = reading_contracts::control(Intent::Resume);
        assert!(run(
            &turn,
            &input(&established, "conversation_intake", "Continue.")
        )
        .is_ok());
    }
}
