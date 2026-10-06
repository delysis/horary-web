use super::*;
fn permit(marker: &str) -> CheckedData {
    let options = crate::horary_role_options::build(
        Matter::Work,
        &[],
        &crate::horary_role_options::Subject {
            name: "Job".into(),
            kind: "job".into(),
            owner_id: "querent".into(),
            source_quote: String::new(),
        },
    );
    let input = json!({"native_role_options":options,"marker":marker});
    let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
    let facts = crate::reading_method::facts(Some(&chart));
    let value = json!({"selections":[{"id":"querent.self","reason":"The native first represents the person asking."},{"id":"subject.primary","reason":"The native tenth represents this job."}],"summary":"Authored invariant fixture.","unknowns":[]});
    let Checked::Data(data) =
        check(Stage::Significators, Matter::Work, &value, &input, &facts).unwrap()
    else {
        panic!("Fixture must produce data")
    };
    data
}
#[test]
fn a_completion_permit_cannot_complete_another_input_or_stage() {
    let data = permit("one");
    let foreign = permit("two");
    let mut journal = Journal::default();
    journal
        .begin(
            "job".into(),
            Stage::Significators,
            1,
            data.input_sha256.clone(),
        )
        .unwrap();
    assert!(journal.finish("job", &foreign, 0).is_err());
    assert_eq!(journal.jobs[0].phase(), &Phase::Running);
    journal.finish("job", &data, 0).unwrap();
    assert!(journal
        .begin(
            "job".into(),
            Stage::Significators,
            1,
            data.input_sha256.clone()
        )
        .is_err());
    let mut wrong = Journal::default();
    wrong
        .begin(
            "other".into(),
            Stage::Condition,
            1,
            data.input_sha256.clone(),
        )
        .unwrap();
    assert!(wrong.finish("other", &data, 0).is_err());
}
#[test]
fn every_analysis_stage_has_an_internal_context_gate_before_generation() {
    for stage in [
        Stage::Significators,
        Stage::Condition,
        Stage::Reception,
        Stage::Contacts,
        Stage::Location,
        Stage::Judgment,
        Stage::Explanation,
    ] {
        assert!(
            prerequisites(stage, &json!({})).is_err(),
            "{} must not dispatch without its own input",
            stage.name()
        );
    }
    assert!(prerequisites(
        Stage::Judgment,
        &json!({"condition":{"request_input":{}},"reception":{},"contacts":{}})
    )
    .is_err());
}
#[test]
fn four_independent_steps_can_run_but_a_fifth_cannot_enter_the_batch() {
    let mut journal = Journal::default();
    for (index, stage) in [
        Stage::Condition,
        Stage::Reception,
        Stage::Contacts,
        Stage::Location,
    ]
    .into_iter()
    .enumerate()
    {
        journal
            .begin(index.to_string(), stage, 1, "input".into())
            .unwrap();
    }
    assert!(journal
        .begin("fifth".into(), Stage::Judgment, 1, "input".into())
        .is_err());
    journal.pause("Interrupted".into());
    assert!(journal.active.is_empty());
    assert!(journal
        .jobs
        .iter()
        .all(|job| matches!(job.phase(), Phase::Paused { .. })));
}
#[test]
fn superseded_or_completed_work_cannot_restart_as_a_generation() {
    assert!(!Phase::Superseded { by: "new".into() }.allows(&Phase::Complete { record_index: 0 }));
    assert!(!Phase::Complete { record_index: 0 }.allows(&Phase::Running));
}
