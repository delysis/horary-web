use super::*;

fn condition_fixture(planets: &[&str]) -> (Value, Value, Vec<Fact>) {
    use horary_ai_core::book_method::ConditionFacet;
    let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
    let facts = crate::reading_method::facts(Some(&chart));
    let roles: Vec<_> = planets.iter().map(|planet|
        json!({"label":planet,"house":null,"planet":planet,"reason":"Authored selected role"})).collect();
    let input = json!({"question":{"roles":roles},"facts":facts});
    let ids = |facet| {
        facts
            .iter()
            .filter(|fact| {
                fact.condition_facet == Some(facet)
                    && fact
                        .planets
                        .iter()
                        .any(|planet| planets.contains(&planet.as_str()))
            })
            .map(|fact| fact.id.clone())
            .collect::<Vec<_>>()
    };
    let value = json!({"checks":{
        "own_dignity":{"state":"unestablished","evidence":ids(ConditionFacet::Essential),
            "finding":"Authored coverage only; interpretation is unqualified."},
        "ability_to_act":{"state":"unestablished","evidence":ids(ConditionFacet::HouseCapacity),
            "finding":"Authored coverage includes supplied neutral or weak capacity."},
        "context_exceptions":{"state":"not_relevant","evidence":[],
            "finding":"No exception asserted by this authored coverage fixture."}},
        "summary":"Authored native coverage regression; not an oracle reading.","unknowns":[]});
    (input, value, facts)
}

#[test]
fn condition_coverage_rejects_each_omitted_selected_actor_facet() {
    use horary_ai_core::book_method::ConditionFacet;
    let (input, complete, facts) = condition_fixture(&["Jupiter", "Mercury"]);
    for (facet, key) in [
        (ConditionFacet::Essential, "own_dignity"),
        (ConditionFacet::HouseCapacity, "ability_to_act"),
    ] {
        let mut omitted = complete.clone();
        let id = &facts
            .iter()
            .find(|fact| fact.condition_facet == Some(facet) && fact.planets == ["Mercury"])
            .unwrap()
            .id;
        omitted["checks"][key]["evidence"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item != id);
        let error = check(Stage::Condition, Matter::Work, &omitted, &input, &facts)
            .err()
            .expect("The job's omitted testimony must reject");
        assert!(error.contains(&format!("omits Mercury from {key}")));
    }
    assert!(matches!(
        check(Stage::Condition, Matter::Work, &complete, &input, &facts).unwrap(),
        Checked::Data(_)
    ));
}

#[test]
fn condition_coverage_cannot_substitute_a_different_native_facet() {
    use horary_ai_core::book_method::ConditionFacet;
    let (input, mut value, facts) = condition_fixture(&["Mercury"]);
    let solar = facts
        .iter()
        .find(|fact| {
            fact.condition_facet == Some(ConditionFacet::Solar) && fact.planets == ["Mercury"]
        })
        .unwrap();
    value["checks"]["own_dignity"]["evidence"] = json!([solar.id]);
    assert!(
        check(Stage::Condition, Matter::Work, &value, &input, &facts)
            .err()
            .unwrap()
            .contains("omits Mercury from own_dignity")
    );
}

#[test]
fn condition_coverage_shared_rulers_reuse_proof_without_forcing_a_state() {
    let (input, value, facts) = condition_fixture(&["Jupiter", "Mercury", "Mercury"]);
    assert_eq!(
        value["checks"]["own_dignity"]["evidence"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for state in ["supported", "contradicted", "unestablished", "not_relevant"] {
        let mut proposal = value.clone();
        proposal["checks"]["own_dignity"]["state"] = json!(state);
        assert!(check(Stage::Condition, Matter::Work, &proposal, &input, &facts).is_ok());
    }
}

#[test]
fn condition_coverage_uses_original_input_and_does_not_invent_missing_calculations() {
    use horary_ai_core::book_method::ConditionFacet;
    let (mut input, mut value, mut facts) = condition_fixture(&["Jupiter", "Mercury"]);
    facts.retain(|fact| {
        !(fact.condition_facet == Some(ConditionFacet::Essential) && fact.planets == ["Mercury"])
    });
    value["checks"]["own_dignity"]["evidence"] = json!([facts
        .iter()
        .find(
            |fact| fact.condition_facet == Some(ConditionFacet::Essential)
                && fact.planets == ["Jupiter"]
        )
        .unwrap()
        .id]);
    input["facts"] = json!(facts);
    let repair = json!({"original_input":input,"previous_worksheet":{"question":{"roles":[]}}});
    assert!(check(Stage::Condition, Matter::Work, &value, &repair, &facts).is_ok());
    value["checks"]["ability_to_act"]["evidence"] = json!([facts
        .iter()
        .find(
            |fact| fact.condition_facet == Some(ConditionFacet::HouseCapacity)
                && fact.planets == ["Jupiter"]
        )
        .unwrap()
        .id]);
    assert!(
        check(Stage::Condition, Matter::Work, &value, &repair, &facts)
            .err()
            .unwrap()
            .contains("omits Mercury from ability_to_act")
    );
}

#[test]
fn condition_coverage_seven_planets_fit_without_broadening_other_stage_contracts() {
    let planets = [
        "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
    ];
    let (input, value, facts) = condition_fixture(&planets);
    assert_eq!(
        value["checks"]["own_dignity"]["evidence"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert!(check(Stage::Condition, Matter::Other, &value, &input, &facts).is_ok());
    let schema = horary_contract::schema(Stage::Reception, &facts);
    let ids = value["checks"]["own_dignity"]["evidence"].clone();
    assert_eq!(
        schema["properties"]["checks"]["properties"]["direction"]["properties"]["evidence"]
            ["maxItems"],
        6
    );
    let reception = json!({"checks":{
        "direction":{"state":"unestablished","evidence":ids,"finding":"Authored excess-evidence rejection."},
        "strength_and_quality":{"state":"unestablished","evidence":[],"finding":"Unestablished."},
        "contextual_motive":{"state":"not_relevant","evidence":[],"finding":"Not relevant."}},
        "summary":"Authored reception shape.","unknowns":[]});
    assert!(horary_contract::validate_shape(&reception, &schema).is_err());
}
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
