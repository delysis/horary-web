//! A reading component owns both branch selection and source-grounded judgment.
//! Actual classified journeys already supply a branch and bypass the selector.
use crate::{
    book::Book,
    catalogue,
    optimize::{plain, text},
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::{
    Example, Module, Predict, SignatureSpec,
    example::Prediction,
    lm::DynChatModel,
    module::{NamedPredictor, TraceStep, relabel},
    signature::Signature,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub const VERSION: u32 = 2;

/// Authored adaptation of the current native NewJob teaching to this reader's
/// own declared inputs and five-field output contract. No native worksheet or
/// fixture-specific conclusion is imported as authority.
pub fn new_job_reader_baseline() -> Result<String> {
    let mut instruction = signatures::reading("new_job")?.instructions;
    instruction.push_str("\n\n");
    instruction.push_str(include_str!("../prompts/new_job_reader.md"));
    Ok(instruction)
}

/// The declared family union plus the common page supplied by every existing
/// NewJob branch allocation. No case label chooses these primary passages.
pub fn new_job_family_source(book: &Book) -> Result<String> {
    catalogue::family_reader_source(book, "new_job")
}

/// Other families start with their existing typed reading instruction and
/// ordinary output contract. GEPA supplies the learned family teaching from
/// source-reviewed training feedback, not an imported native worksheet.
pub fn direct_reader_baseline(method: &str) -> Result<String> {
    ensure!(
        !matches!(method, "wish" | "unclassified"),
        "Select a substantive reading family"
    );
    if method == "new_job" {
        return new_job_reader_baseline();
    }
    let mut instruction = signatures::reading(method)?.instructions;
    instruction.push_str("\n\nReturn ONLY the requested status, answer, analysis, evidence and unresolved fields in their requested field markers, beginning with status. Use answer, need_information or method_limit for status according to the actual evidence. Never copy question, consultation, chart_evidence or source_text. Do not output a native worksheet or a hidden reasoning transcript. Answer the person's actual question directly, then give concise reviewable reasons with the supplied native evidence IDs and honest limitations. The supplied family source covers all its branches; choose the appropriate procedure from actual circumstances, not an expected label.");
    Ok(instruction)
}

#[derive(dsrust::Signature)]
/// Select the book's judgment branch within the supplied reading family using only the person's original question and accepted consultation. Return one exact branch ID from the taxonomy. Select the requested outcome, not an incidental deadline or an unrequested reframing. Do not invent absent situational inputs. If no branch is supportable, return unknown. This selects source context; it does not answer the question.
pub struct SelectReadingBranch {
    #[input]
    pub question: String,
    #[input]
    pub consultation: String,
    /// Declared family and its source-defined branch IDs and judgment purposes.
    #[input]
    pub branches: String,
    #[output]
    pub branch: String,
}

pub fn selector_signature() -> Signature {
    SelectReadingBranch::signature()
}
pub fn selector_name(method: &str) -> String {
    format!("reading_branch.{method}")
}
pub fn taxonomy(method: &str) -> Result<String> {
    let card = catalogue::coverage_card(method)?;
    let branches = card["judgment_subbranches"]
        .as_array()
        .context("No source branches")?;
    Ok(serde_json::to_string(
        &json!({"method":method,"branches":branches.iter().map(|b|json!({"id":b["id"],"judgment":b["decisive_judgment"]})).collect::<Vec<_>>()}),
    )?)
}

pub fn selector_input(input: &Example, method: &str) -> Result<Example> {
    Ok(
        dsrust::input! {question:input.get("question").and_then(Value::as_str).context("No original reading question")?,consultation:input.get("consultation").and_then(Value::as_str).context("No accepted consultation")?,branches:taxonomy(method)?},
    )
}

/// Prepare the exact reader request for an observed valid branch. Budget
/// preflight may enumerate these alternatives, but cannot claim it selected one.
pub fn reader_input(input: &Example, method: &str, branch: &str, book: &Book) -> Result<Example> {
    let mut consultation: Value = serde_json::from_str(
        input
            .get("consultation")
            .and_then(Value::as_str)
            .context("No accepted consultation")?,
    )?;
    let accepted = consultation
        .as_object_mut()
        .context("Accepted consultation must be an object")?;
    ensure!(
        accepted
            .get("method")
            .is_none_or(|value| value.as_str() == Some(method)),
        "Accepted consultation family differs"
    );
    ensure!(
        accepted
            .get("branch")
            .is_none_or(|value| value.as_str() == Some(branch)),
        "Cannot replace an observed consultation branch"
    );
    let source = catalogue::branch_source(book, method, branch, true)?;
    accepted.insert("method".into(), json!(method));
    accepted.insert("branch".into(), json!(branch));
    Ok(
        dsrust::input! {question:input.get("question").and_then(Value::as_str).context("No original reading question")?,consultation:serde_json::to_string(&consultation)?,chart_evidence:input.get("chart_evidence").and_then(Value::as_str).context("No native chart evidence")?,source_text:source},
    )
}

pub struct ReadingProgram {
    pub(crate) selector: Predict,
    pub(crate) reader: Predict,
    method: String,
    book: Arc<Book>,
}
impl ReadingProgram {
    pub fn new(method: &str, book: Arc<Book>, model: Arc<dyn DynChatModel>) -> Result<Self> {
        ensure!(
            signatures::METHODS.contains(&method) && !matches!(method, "wish" | "unclassified"),
            "Select a substantive reading family"
        );
        Ok(Self {
            selector: plain(selector_signature(), model.clone()),
            reader: plain(signatures::reading(method)?, model),
            method: method.into(),
            book,
        })
    }
    fn rejection(&self, error: String, branch: Option<&str>) -> Prediction {
        let mut prediction = Prediction::new(
            dsrust::example! {status:"need_information",answer:"",analysis:"",evidence:"",unresolved:error},
            "Native reading branch guard withheld judgment",
        );
        prediction.example.set("__native_failures", json!([error]));
        prediction.example.set("__reading_branch", json!(branch));
        prediction
    }
    async fn ask(&self, input: Example, trace: &mut Vec<TraceStep>) -> Result<Prediction> {
        let input = input.inputs()?;
        let consultation_text = input
            .get("consultation")
            .and_then(Value::as_str)
            .context("No accepted consultation")?;
        let consultation: Value = serde_json::from_str(consultation_text)
            .context("Accepted consultation must be JSON")?;
        let accepted = consultation
            .as_object()
            .context("Accepted consultation must be an object")?;
        if let Some(method) = accepted.get("method")
            && method.as_str() != Some(self.method.as_str())
        {
            return Ok(self.rejection(
                "Accepted consultation family differs from the declared reader".into(),
                None,
            ));
        }
        let branch = if let Some(branch) = accepted.get("branch") {
            let Some(branch) = branch.as_str() else {
                return Ok(self.rejection("Accepted branch is not an exact text ID".into(), None));
            };
            branch.to_owned()
        } else {
            let from = trace.len();
            let selected = self
                .selector
                .forward_traced(selector_input(&input, &self.method)?, trace)
                .await?;
            relabel(trace, from, &selector_name(&self.method));
            text(&selected, "branch")?.to_owned()
        };
        // A received but unsupported branch is an observed program failure,
        // not transport uncertainty and not authority to guess a different one.
        if !catalogue::coverage_card(&self.method)?["judgment_subbranches"]
            .as_array()
            .context("No source branches")?
            .iter()
            .any(|b| b["id"] == branch)
        {
            return Ok(self.rejection(
                format!("Unknown {} reading branch {branch}", self.method),
                Some(&branch),
            ));
        }
        let from = trace.len();
        let mut prediction = self
            .reader
            .forward_traced(
                reader_input(&input, &self.method, &branch, &self.book)?,
                trace,
            )
            .await?;
        relabel(trace, from, &format!("reading.{}", self.method));
        prediction.example.set("__reading_branch", json!(branch));
        Ok(prediction)
    }
}
impl Module for ReadingProgram {
    fn forward<'a>(
        &'a self,
        input: Example,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(async move { self.ask(input, &mut Vec::new()).await })
    }
    fn forward_traced<'a>(
        &'a self,
        input: Example,
        trace: &'a mut Vec<TraceStep>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Prediction>> + Send + 'a>> {
        Box::pin(self.ask(input, trace))
    }
    fn named_predictors(&mut self) -> Vec<NamedPredictor<'_>> {
        let mut predictors = self.selector.named_predictors();
        for p in &mut predictors {
            p.name = selector_name(&self.method);
        }
        let mut reader = self.reader.named_predictors();
        for p in &mut reader {
            p.name = format!("reading.{}", self.method);
        }
        predictors.extend(reader);
        predictors
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dsrust::DummyLM;

    pub(crate) fn input(consultation: Value) -> Example {
        dsrust::example! {question:"Will this synthetic sale complete?",consultation:serde_json::to_string(&consultation).unwrap(),chart_evidence:"{\"facts\":\"ID\\tKIND\\tLABEL\\tCALCULATED TESTIMONY\\ne1\\tboundary\\tLimits\\tSynthetic calculation is incomplete.\\n\",\"limitations\":[\"UNKNOWN NATIVE COMPUTATION\"]}",source_text:"BROAD SOURCE MUST NOT REACH THE STUDENT",expected:"PRIVATE GOLD MUST NOT REACH THE STUDENT",case_id:"private-identity",__review_source:"FULL FAMILY REVIEW CONTEXT"}.with_inputs(["question","consultation","chart_evidence","source_text"])
    }
    fn reply() -> Example {
        dsrust::example! {status:"method_limit",answer:"Synthetic answer only.",analysis:"Synthetic e1 observation.",evidence:"e1",unresolved:"UNKNOWN NATIVE COMPUTATION"}
    }
    #[test]
    fn adapted_new_job_baseline_preserves_its_real_function_contract() {
        let text = new_job_reader_baseline().unwrap();
        assert!(text.contains("triplicity is moderate"));
        assert!(text.contains("job → applicant ruler and job → Moon-as-worker distinct"));
        assert!(text.contains("retrogradation must be interpreted for the planet"));
        assert!(text.contains("identity-versus-worker conflict"));
        assert!(text.contains("status, answer, analysis, evidence, unresolved"));
        assert!(text.contains("hypothetical examples are teaching, never this chart's facts"));
        assert!(!text.contains("<output_fields>verdict"));
        assert!(!text.contains("dspy-new_job-first-"));
    }
    #[test]
    fn direct_contact_baseline_preserves_its_typed_contract_and_excludes_job_teaching() {
        let text = direct_reader_baseline("contact").unwrap();
        assert!(text.contains("Judge a contact question"));
        assert!(text.contains("status, answer, analysis, evidence and unresolved"));
        assert!(!text.contains("Moon-as-worker"));
        assert!(!text.contains("dspy-contact-first-"));
        assert!(direct_reader_baseline("wish").is_err());
    }
    #[test]
    fn generic_family_allocation_keeps_new_job_input_byte_identical() {
        let book = Book::fixture();
        let old = format!(
            "{}{}",
            catalogue::method_source(&book, "new_job", true).unwrap(),
            book.pages(&[225].into_iter().collect()).unwrap()
        );
        assert_eq!(new_job_family_source(&book).unwrap(), old);
        assert_eq!(
            direct_reader_baseline("new_job").unwrap(),
            new_job_reader_baseline().unwrap()
        );
    }
    #[tokio::test]
    async fn actual_selector_and_reader_trace_match_both_optimizable_predictors() {
        let model = Arc::new(DummyLM::new([
            dsrust::example! {branch:"pure_completion"},
            reply(),
        ]));
        let mut program =
            ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone()).unwrap();
        let input = input(
            json!({"facts":[],"unknown":["unknown fact"],"device":{"clock":"actual supplied clock"}}),
        );
        let expected_chart = input.get("chart_evidence").unwrap().clone();
        let mut trace = Vec::new();
        let output = program
            .forward_traced(input.inputs().unwrap(), &mut trace)
            .await
            .unwrap();
        assert_eq!(
            output.get("__reading_branch"),
            Some(&json!("pure_completion"))
        );
        assert_eq!(
            trace
                .iter()
                .map(|t| t.predictor.as_str())
                .collect::<Vec<_>>(),
            vec!["reading_branch.movable_deal", "reading.movable_deal"]
        );
        assert_eq!(
            program
                .named_predictors()
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>(),
            vec!["reading_branch.movable_deal", "reading.movable_deal"]
        );
        assert_eq!(trace[1].inputs.get("chart_evidence"), Some(&expected_chart));
        let received: Value = serde_json::from_str(
            trace[1]
                .inputs
                .get("consultation")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(received["unknown"], json!(["unknown fact"]));
        assert_eq!(received["device"]["clock"], "actual supplied clock");
        for request in model.asked() {
            let rendered = format!("{} {:?}", request.system, request.turns);
            for private in [
                "PRIVATE GOLD",
                "private-identity",
                "FULL FAMILY REVIEW",
                "BROAD SOURCE",
            ] {
                assert!(!rendered.contains(private));
            }
        }
        assert_eq!(
            trace[1]
                .inputs
                .get("source_text")
                .unwrap()
                .as_str()
                .unwrap(),
            catalogue::branch_source(&Book::fixture(), "movable_deal", "pure_completion", true)
                .unwrap()
        );
        let state = program.dump_state();
        assert_eq!(state.predictors.len(), 2);
        assert_eq!(
            state
                .get("reading.movable_deal")
                .unwrap()
                .signature
                .fields
                .len(),
            9
        );
    }
    #[tokio::test]
    async fn actual_classified_branch_needs_one_reader_call_and_never_reselects() {
        let model = Arc::new(DummyLM::new([reply()]));
        let program =
            ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone()).unwrap();
        let mut trace = Vec::new();
        program
            .forward_traced(
                input(json!({"method":"movable_deal","branch":"pure_completion","facts":[]})),
                &mut trace,
            )
            .await
            .unwrap();
        assert_eq!(model.asked().len(), 1);
        assert_eq!(trace[0].predictor, "reading.movable_deal");
    }
    #[tokio::test]
    async fn unsupported_received_branch_is_observed_failure_and_withholds_reader() {
        let model = Arc::new(DummyLM::new([dsrust::example! {branch:"made_up"}]));
        let program =
            ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone()).unwrap();
        let mut trace = Vec::new();
        let output = program
            .forward_traced(input(json!({"facts":[]})), &mut trace)
            .await
            .unwrap();
        assert_eq!(model.asked().len(), 1);
        assert_eq!(trace.len(), 1);
        assert_eq!(output.get("status"), Some(&json!("need_information")));
        assert!(
            output.get("__native_failures").unwrap()[0]
                .as_str()
                .unwrap()
                .contains("made_up")
        );
        assert_eq!(output.get("analysis"), Some(&json!("")));
    }
    #[tokio::test]
    async fn bad_observed_branch_or_family_is_not_reinterpreted_by_a_selector() {
        for consultation in [
            json!({"branch":"unknown"}),
            json!({"branch":5}),
            json!({"method":"money","branch":"pure_completion"}),
        ] {
            let model = Arc::new(DummyLM::new([]));
            let program =
                ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone())
                    .unwrap();
            let output = program.forward(input(consultation)).await.unwrap();
            assert!(output.get("__native_failures").is_some());
            assert!(model.asked().is_empty());
        }
    }
    #[tokio::test]
    async fn missing_transport_or_adapter_reply_remains_unobserved_error() {
        let model = Arc::new(DummyLM::new([]));
        let program =
            ReadingProgram::new("movable_deal", Arc::new(Book::fixture()), model.clone()).unwrap();
        assert!(program.forward(input(json!({"facts":[]}))).await.is_err());
        assert_eq!(model.asked().len(), 1);
    }
    #[test]
    fn all_family_branches_use_exact_existing_foundations_and_cross_chapter_context() {
        let book = Book::fixture();
        let mut count = 0;
        for method in signatures::METHODS
            .iter()
            .filter(|m| !matches!(**m, "wish" | "unclassified"))
        {
            let input = input(json!({"facts":[]}));
            let selector = selector_input(&input, method).unwrap();
            assert_eq!(selector.fields().count(), 3);
            for branch in catalogue::coverage_card(method).unwrap()["judgment_subbranches"]
                .as_array()
                .unwrap()
            {
                let branch = branch["id"].as_str().unwrap();
                let prepared = reader_input(&input, method, branch, &book).unwrap();
                assert_eq!(
                    prepared.get("source_text").unwrap().as_str().unwrap(),
                    catalogue::branch_source(&book, method, branch, true).unwrap()
                );
                count += 1;
            }
        }
        assert_eq!(count, 119);
    }
    #[test]
    fn preparation_cannot_replace_an_actual_branch_or_family() {
        let book = Book::fixture();
        assert!(
            reader_input(
                &input(json!({"branch":"unknown"})),
                "movable_deal",
                "pure_completion",
                &book
            )
            .is_err()
        );
        assert!(
            reader_input(
                &input(json!({"method":"money"})),
                "movable_deal",
                "pure_completion",
                &book
            )
            .is_err()
        );
    }
}
