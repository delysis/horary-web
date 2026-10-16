//! Boundary tests through DsRust's real Module, ChatAdapter and ChatModel.
//! Scripted replies establish routing and provenance, never reading quality.

use std::sync::Arc;

use dsrust::{DummyLM, Example, Module, lm::dummy::Asked};
use serde_json::{Value, json};

use crate::{
    book::Book,
    catalogue::{Case, Chart, Device, Expected, Fact, Place},
    journey::{self, Journey},
};

const LABEL_ID: &str = "CASE_ID_MUST_NEVER_PRIME_THE_STUDENT";
const GOLD_FACT: &str = "GOLD_FACT_MUST_NEVER_PRIME_THE_STUDENT";
const GOLD_RULE: &str = "GOLD_RUBRIC_MUST_NEVER_PRIME_THE_STUDENT";
const GOLD_LIMIT: &str = "GOLD_PROHIBITION_MUST_NEVER_PRIME_THE_STUDENT";
const GOLD_FACET: &str = "GOLD_FACET_MUST_NEVER_PRIME_THE_STUDENT";
const CHART_SEED: &str = "1901-02-03T04:05:06Z";

fn case() -> Case {
    Case {
        id: LABEL_ID.into(),
        method: "movable_deal".into(),
        split: "train".into(),
        variant: "explicit".into(),
        question: "Will I sell my spare bicycle to a stranger in Springfield on Friday?".into(),
        history: Vec::new(),
        device: Device {
            observed_at: "2026-10-08T16:00:00Z".into(),
            time_zone: "America/New_York".into(),
            place: Place {
                name: "Woodbridge, Virginia, United States".into(),
                latitude: 38.657,
                longitude: -77.249,
            },
        },
        // Poison labels deliberately disagree with the scripted observation.
        // If these cross the oracle boundary, the request assertions catch it.
        expected: Expected {
            facet: GOLD_FACET.into(),
            facts: vec![Fact {
                field: "subject.name".into(),
                value: GOLD_FACT.into(),
                quote: "my spare bicycle".into(),
            }],
            missing: Vec::new(),
            reading_status: "answer".into(),
            must_explain: vec![GOLD_RULE.into()],
            must_not: vec![GOLD_LIMIT.into()],
            source_pages: vec![172],
        },
        // An authored component seed is not this journey's chart authority.
        chart: Chart {
            utc: CHART_SEED.into(),
            latitude: -44.125,
            longitude: 139.75,
        },
    }
}

fn classify(method: &str, branch: &str) -> Example {
    dsrust::example! {
        method: method,
        facet: "event",
        branch: branch,
        principal: "querent",
        clarification: ""
    }
}

fn facts() -> Vec<Fact> {
    [
        ("subject.name", "bicycle", "bicycle"),
        ("subject.owner_id", "querent", "my spare bicycle"),
        ("deal_actor", "querent", "I sell"),
        ("deal_capacity", "sell", "sell"),
        ("event_place", "Springfield", "Springfield"),
        ("event_time", "2026-10-09", "Friday"),
    ]
    .into_iter()
    .map(|(field, value, quote)| Fact {
        field: field.into(),
        value: value.into(),
        quote: quote.into(),
    })
    .collect()
}

fn extract(facts: &[Fact], missing: &[&str]) -> Example {
    dsrust::example! {
        facts: serde_json::to_string(facts).unwrap(),
        missing: serde_json::to_string(missing).unwrap(),
        inferences: "",
        clarification: if missing.is_empty() { "" } else { "Who owns the bicycle?" }
    }
}

fn reading() -> Example {
    dsrust::example! {
        status: "answer",
        answer: "Scripted mechanics answer; this is not a qualified horary judgment.",
        analysis: "Scripted mechanics only.",
        evidence: "",
        unresolved: ""
    }
}

fn review(classification: &str, reading: &str, passed: bool) -> Example {
    dsrust::example! {
        score: 1.0,
        passed: passed,
        classification: classification,
        extraction: "pass",
        reading: reading,
        answer_quality: "pass",
        feedback: "Scripted review: only transport and score boundaries are under test."
    }
}

fn program(case: &Case, student: Arc<DummyLM>, judge: Arc<DummyLM>) -> Journey {
    Journey::new(
        "movable_deal",
        std::slice::from_ref(case),
        Arc::new(Book::fixture()),
        student,
        judge,
    )
    .unwrap()
}

fn student() -> Arc<DummyLM> {
    Arc::new(DummyLM::new([
        classify("movable_deal", "pure_completion"),
        extract(&facts(), &[]),
        reading(),
    ]))
}

/// Read the actual ChatAdapter user payload, rather than reconstructing the
/// application's intended input or inspecting a separate test-only route.
fn input_field<'a>(request: &'a Asked, name: &str) -> &'a str {
    let marker = format!("[[ ## {name} ## ]]\n");
    let value = request
        .last_message()
        .split_once(&marker)
        .unwrap_or_else(|| panic!("Request has no {name} input"))
        .1;
    value
        .split_once("\n\n[[ ## ")
        .or_else(|| value.split_once("\n\nRespond with"))
        .map_or(value, |(value, _)| value)
}

fn json_input(request: &Asked, name: &str) -> Value {
    serde_json::from_str(input_field(request, name)).unwrap()
}

#[tokio::test]
async fn journey_student_requests_exclude_labels_and_authored_chart_seeds() {
    let case = case();
    let student = student();
    let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
    let program = program(&case, student.clone(), judge.clone());

    // Even an accidental full labelled Example must not prime the student.
    let result = program.forward(journey::example(&case)).await.unwrap();
    assert_eq!(result.get("__passed"), Some(&json!(true)));
    let requests = student.asked();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert!(
            !request.json_mode,
            "The student uses ordinary unconstrained text"
        );
        let all = format!("{}\n{:?}", request.system, request.turns);
        for forbidden in [
            LABEL_ID, GOLD_FACT, GOLD_RULE, GOLD_LIMIT, GOLD_FACET, CHART_SEED,
        ] {
            assert!(
                !all.contains(forbidden),
                "Student saw label/seed {forbidden}"
            );
        }
        assert_eq!(input_field(request, "question"), case.question);
    }
    let context = json_input(&requests[1], "context");
    assert_eq!(context["accepted_facts"], json!([]));
    assert_eq!(
        context["device"],
        serde_json::to_value(&case.device).unwrap()
    );
    let consultation = json_input(&requests[2], "consultation");
    assert_eq!(
        consultation["facts"],
        serde_json::to_value(facts()).unwrap()
    );
    assert!(!consultation.to_string().contains(GOLD_FACT));
    assert_eq!(student.remaining(), 0);
    let reviewer = judge.asked();
    assert_eq!(reviewer.len(), 1);
    assert!(reviewer[0].last_message().contains(GOLD_RULE));
    assert!(reviewer[0].last_message().contains(GOLD_FACT));
    assert!(!reviewer[0].json_mode);
}

#[tokio::test]
async fn journey_chart_is_calculated_from_device_not_case_or_event_context() {
    let case = case();
    let student = student();
    let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
    let program = program(&case, student.clone(), judge);
    program
        .forward(journey::example(&case).inputs().unwrap())
        .await
        .unwrap();

    let requests = student.asked();
    let evidence = json_input(&requests[2], "chart_evidence");
    let actual_at = chrono::DateTime::parse_from_rfc3339(&case.device.observed_at).unwrap();
    let used_at =
        chrono::DateTime::parse_from_rfc3339(evidence["anchor"]["utc"].as_str().unwrap()).unwrap();
    assert_eq!(used_at, actual_at);
    assert_eq!(
        evidence["anchor"]["place"]["latitude"],
        json!(case.device.place.latitude)
    );
    assert_eq!(
        evidence["anchor"]["place"]["longitude"],
        json!(case.device.place.longitude)
    );
    assert_ne!(
        evidence["anchor"]["place"]["latitude"],
        json!(case.chart.latitude)
    );
    assert_eq!(
        evidence["event_context"][1]["native_day"]["date"],
        "2026-10-09"
    );
    assert_ne!(
        evidence["event_context"][0]["resolved"]["latitude"],
        evidence["anchor"]["place"]["latitude"]
    );

    // Anchor metadata alone would not catch a chart secretly calculated from
    // Case.chart; compare the actual native evidence supplied to the reader.
    let chart = horary_ai_core::astronomy::chart(
        actual_at.timestamp_millis() as f64,
        case.device.place.latitude,
        case.device.place.longitude,
    )
    .unwrap();
    let native = crate::native_evidence::facts(Some(&chart));
    assert!(!native.is_empty());
    assert_eq!(
        evidence["facts"],
        json!(crate::catalogue::compact_native_facts(&native))
    );
    let observations = program.observations.lock().unwrap();
    assert_eq!(
        observations[0]["stages"]["full_native_facts"],
        serde_json::to_value(&native).unwrap()
    );
}

#[tokio::test]
async fn journey_wrong_classification_routes_to_its_actual_method_without_gold_correction() {
    let case = case();
    let student = Arc::new(DummyLM::new([
        classify("money", "wages_and_client_payments"),
        extract(&facts(), &[]),
        reading(),
    ]));
    let judge = Arc::new(DummyLM::new([review("fail", "pass", false)]));
    let program = program(&case, student.clone(), judge);
    let mut trace = Vec::new();
    let result = program
        .forward_traced(journey::example(&case).inputs().unwrap(), &mut trace)
        .await
        .unwrap();

    assert_eq!(result.get("__passed"), Some(&json!(false)));
    assert!(result.get("__score").unwrap().as_f64().unwrap() < 1.0);
    assert_eq!(
        trace
            .iter()
            .map(|step| step.predictor.as_str())
            .collect::<Vec<_>>(),
        ["classification", "extraction.money", "reading.money"]
    );
    let requests = student.asked();
    assert!(
        requests[1]
            .system
            .contains("Extract the situational inputs for money")
    );
    assert!(input_field(&requests[1], "requirements").starts_with("Method: money."));
    assert!(requests[2].system.contains("Judge a money question"));
    let consultation = json_input(&requests[2], "consultation");
    assert_eq!(consultation["method"], "money");
    assert_eq!(consultation["branch"], "wages_and_client_payments");
    assert!(input_field(&requests[2], "source_text").contains("printed_page=\"156\""));
    assert!(!input_field(&requests[2], "source_text").contains("printed_page=\"172\""));
    let observations = program.observations.lock().unwrap();
    assert_eq!(
        observations[0]["stages"]["classification"]["method"],
        "money"
    );
    assert_eq!(observations[0]["passed"], false);
}

#[tokio::test]
async fn journey_contradictory_judge_pass_cannot_promote_unobserved_reading() {
    let case = case();
    let student = student();
    let judge = Arc::new(DummyLM::new([review("pass", "unobserved", true)]));
    let program = program(&case, student.clone(), judge);
    let result = program
        .forward(journey::example(&case).inputs().unwrap())
        .await
        .unwrap();

    assert_eq!(student.call_count(), 3, "The reading was actually invoked");
    assert_eq!(result.get("status"), Some(&json!("answer")));
    assert_eq!(result.get("__passed"), Some(&json!(false)));
    assert!(result.get("__score").unwrap().as_f64().unwrap() < 1.0);
    let observations = program.observations.lock().unwrap();
    assert_eq!(
        observations[0]["review"]["passed"], true,
        "Preserve the original contradictory review"
    );
    assert_eq!(observations[0]["review"]["reading"], "unobserved");
    assert_eq!(observations[0]["passed"], false);
}

#[tokio::test]
async fn journey_nonliteral_fact_withholds_chart_and_reading_even_if_judge_passes() {
    let case = case();
    let mut facts = facts();
    facts[0].quote = "words the person never said".into();
    let student = Arc::new(DummyLM::new([
        classify("movable_deal", "pure_completion"),
        extract(&facts, &[]),
    ]));
    let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
    let program = program(&case, student.clone(), judge);
    let result = program
        .forward(journey::example(&case).inputs().unwrap())
        .await
        .unwrap();

    assert_eq!(student.call_count(), 2);
    assert_eq!(result.get("status"), Some(&json!("need_information")));
    assert_eq!(result.get("__passed"), Some(&json!(false)));
    assert_eq!(result.get("__score"), Some(&json!(0.0)));
    let observations = program.observations.lock().unwrap();
    assert!(observations[0]["stages"].get("chart").is_none());
    assert!(observations[0]["stages"].get("reading").is_none());
    assert!(
        observations[0]["native_failures"]
            .to_string()
            .contains("nonliteral quote")
    );
}

#[tokio::test]
async fn journey_conflicting_or_empty_fact_paths_withhold_chart_and_reader() {
    for empty in [false, true] {
        let case = case();
        let mut observed = facts();
        let mut conflict = observed[1].clone();
        conflict.value = "another-person".into();
        if empty {
            conflict.field = "   ".into();
        }
        observed.push(conflict);
        let student = Arc::new(DummyLM::new([
            classify("movable_deal", "pure_completion"),
            extract(&observed, &[]),
        ]));
        let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
        let program = program(&case, student.clone(), judge);
        let result = program
            .forward(journey::example(&case).inputs().unwrap())
            .await
            .unwrap();
        assert_eq!(student.call_count(), 2);
        assert_eq!(result.get("__passed"), Some(&json!(false)));
        let records = program.observations.lock().unwrap();
        assert!(records[0]["stages"].get("chart").is_none());
        assert!(records[0]["stages"].get("reading").is_none());
        assert!(
            records[0]["native_failures"]
                .to_string()
                .contains("empty or duplicate canonical path")
        );
    }
}

#[tokio::test]
async fn journey_unknown_method_or_branch_is_observed_rejection_without_downstream_work() {
    for (method, branch) in [
        ("money", "not_a_book_branch"),
        ("not_a_catalogue_method", "pure_completion"),
    ] {
        let case = case();
        let student = Arc::new(DummyLM::new([classify(method, branch)]));
        let judge = Arc::new(DummyLM::new([dsrust::example! {
            score: 0.0,
            passed: false,
            classification: "fail",
            extraction: "unobserved",
            reading: "unobserved",
            answer_quality: "unobserved",
            feedback: "The recorded classification selects an unsupported method or branch; no later stage ran."
        }]));
        let program = program(&case, student.clone(), judge.clone());
        let mut trace = Vec::new();
        let result = program
            .forward_traced(journey::example(&case).inputs().unwrap(), &mut trace)
            .await
            .expect("A parsed classification rejection must reach independent feedback");

        assert_eq!(student.call_count(), 1);
        assert_eq!(judge.call_count(), 1);
        assert_eq!(result.get("__passed"), Some(&json!(false)));
        assert_eq!(result.get("__score"), Some(&json!(0.0)));
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].predictor, "classification");
        let observed_by_judge = json_input(&judge.asked()[0], "prediction");
        assert_eq!(observed_by_judge["classification"]["method"], method);
        assert_eq!(observed_by_judge["classification"]["branch"], branch);
        for withheld in ["extraction", "chart", "full_native_facts", "reading"] {
            assert!(observed_by_judge.get(withheld).is_none());
        }
        let observations = program.observations.lock().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0]["stages"], observed_by_judge);
        assert!(
            !observations[0]["native_failures"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn journey_model_error_is_not_an_observed_semantic_zero() {
    let case = case();
    let student = Arc::new(DummyLM::new([]));
    let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
    let program = program(&case, student.clone(), judge.clone());
    let error = program
        .forward(journey::example(&case).inputs().unwrap())
        .await
        .expect_err("An LM failure has no settled classification to grade");

    assert!(format!("{error:#}").contains("no answer left"));
    assert!(student.call_count() >= 1);
    assert_eq!(judge.call_count(), 0);
    assert!(program.observations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn journey_checks_citations_in_answer_analysis_and_unresolved() {
    for field in ["answer", "analysis", "unresolved"] {
        let case = case();
        let mut reply = reading();
        reply.set(field, json!("Invented native fact e999999"));
        let student = Arc::new(DummyLM::new([
            classify("movable_deal", "pure_completion"),
            extract(&facts(), &[]),
            reply,
        ]));
        // A mistaken all-pass independent answer cannot hide an invented ID.
        let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
        let program = program(&case, student, judge);
        let result = program
            .forward(journey::example(&case).inputs().unwrap())
            .await
            .unwrap();
        assert_eq!(result.get("__passed"), Some(&json!(false)));
        let observations = program.observations.lock().unwrap();
        assert!(
            observations[0]["native_failures"]
                .to_string()
                .contains(&format!("e999999 in {field}"))
        );
        assert_eq!(
            observations[0]["review"]["passed"], true,
            "Keep the raw disagreement"
        );
    }
}

#[tokio::test]
async fn journey_adapter_parse_error_is_not_an_observed_classification_rejection() {
    let case = case();
    // DummyLM emits the real ChatAdapter wire. The missing output fields are
    // therefore rejected by the port's parser, before a classification exists.
    let student = Arc::new(DummyLM::new([]).with_fallback(dsrust::example! { method: "money" }));
    let judge = Arc::new(DummyLM::new([review("pass", "pass", true)]));
    let program = program(&case, student.clone(), judge.clone());
    let error = program
        .forward(journey::example(&case).inputs().unwrap())
        .await
        .expect_err("An incomplete wire response is not a parsed semantic judgment");

    assert!(
        format!("{error:#}").contains("facet"),
        "The actual missing signature output must remain in the parser error: {error:#}"
    );
    assert!(student.call_count() >= 1);
    assert_eq!(judge.call_count(), 0);
    assert!(program.observations.lock().unwrap().is_empty());
}
