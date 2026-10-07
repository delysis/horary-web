//! Authored domain/controller regressions, not model or astrologer qualification.
use super::*;

fn resolved<T>(value: T) -> Slot<T> {
    Slot::Resolved {
        observation: Observation {
            value,
            evidence: Evidence::Migration {
                detail: "Authored regression facts".into(),
            },
        },
    }
}
fn case(method: Method, owner: &str) -> Consultation {
    let mut case = Consultation {
        question: resolved("The actual original question".into()),
        frame: resolved(Frame {
            method,
            facet: Facet::Event,
        }),
        subject: resolved(Subject {
            name: "Subject".into(),
            kind: subject_kinds(method)[0].into(),
            owner_id: owner.into(),
            source_quote: "Authored subject".into(),
        }),
        ..Default::default()
    };
    case.people.insert(
        "daughter".into(),
        Person {
            id: "daughter".into(),
            label: "Daughter".into(),
            relationship: "child".into(),
            source_quote: "my daughter".into(),
        },
    );
    case.people.insert(
        "friend".into(),
        Person {
            id: "friend".into(),
            label: "Friend".into(),
            relationship: "friend".into(),
            source_quote: "my friend".into(),
        },
    );
    case.people.insert(
        "mother".into(),
        Person {
            id: "mother".into(),
            label: "Mother".into(),
            relationship: "mother".into(),
            source_quote: "my mother".into(),
        },
    );
    case
}
fn options(case: &Consultation) -> crate::horary_role_options::Options {
    crate::horary_role_options::build_for(
        case,
        matter(case.method().unwrap()),
        &case.people.values().cloned().collect::<Vec<_>>(),
        case.subject.resolved().unwrap(),
    )
}
fn primary(case: &Consultation) -> Option<u8> {
    options(case)
        .choices
        .iter()
        .find(|c| c.id == "subject.primary")
        .and_then(|c| c.house)
}

#[test]
fn own_object_and_daughters_object_have_different_conditional_contracts() {
    let own = case(Method::LostObject, "querent");
    assert!(own
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Description)));
    assert_eq!(
        options(&own).compare,
        vec!["subject.primary", "subject.alternative_fourth"]
    );
    let daughter = case(Method::LostObject, "daughter");
    assert!(!daughter
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Description)));
    assert!(options(&daughter).compare.is_empty());
    assert_eq!(primary(&daughter), Some(6));
    let mut relay = daughter;
    relay
        .facts
        .insert(Field::PrincipalMode, resolved("relay".into()));
    relay
        .facts
        .insert(Field::PrincipalId, resolved("daughter".into()));
    assert!(relay
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Description)));
    assert_eq!(options(&relay).compare.len(), 2);
}
#[test]
fn animal_kind_and_external_job_exceptions_override_blanket_turning() {
    let mut animal = case(Method::LostAnimal, "daughter");
    animal
        .facts
        .insert(Field::AnimalKind, resolved("small_kind".into()));
    assert_eq!(primary(&animal), Some(6));
    animal
        .facts
        .insert(Field::AnimalKind, resolved("large_kind".into()));
    assert_eq!(primary(&animal), Some(12));
    let mut job = case(Method::NewJob, "friend");
    if let Slot::Resolved { observation } = &mut job.subject {
        observation.value.kind = "job".into();
    }
    assert_eq!(primary(&job), Some(10));
    job.frame = resolved(Frame {
        method: Method::ExistingJob,
        facet: Facet::Situation,
    });
    assert_eq!(primary(&job), Some(8));
    job.frame = resolved(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    if let Slot::Resolved { observation } = &mut job.subject {
        observation.value.owner_id = "mother".into();
    }
    assert_eq!(primary(&job), Some(7));
}
#[test]
fn conditional_applicability_is_recomputed_after_a_source_correction() {
    let mut money = case(Method::Money, "querent");
    money
        .facts
        .insert(Field::MoneySource, resolved("government".into()));
    assert!(money
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Discretionary)));
    let mut patch = control(Intent::Correct);
    patch.updates = vec![Update {
        field: Field::MoneySource,
        value: "job".into(),
        quote: "my job".into(),
        mode: UpdateMode::Correct,
    }];
    money
        .apply(&patch, 2, "Actually it is from my job", false)
        .unwrap();
    assert!(!money
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Discretionary)));
    assert!(!money.facts.contains_key(&Field::Discretionary));
    assert_eq!(primary(&money), Some(11));
}
#[test]
fn role_reclassification_changes_requirements_without_dropping_known_context() {
    let mut work = case(Method::NewJob, "querent");
    work.facts
        .insert(Field::EventPlace, resolved("Bozeman".into()));
    let mut patch = control(Intent::Correct);
    patch.frame = Some(Frame {
        method: Method::Money,
        facet: Facet::Event,
    });
    patch.subject = Some(Subject {
        name: "Pay".into(),
        kind: "money".into(),
        owner_id: "querent".into(),
        source_quote: "pay".into(),
    });
    work.apply(
        &patch,
        2,
        "Actually I am asking whether the pay will arrive",
        false,
    )
    .unwrap();
    assert!(work
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::MoneySource)));
    assert_eq!(work.text(Field::EventPlace), Some("Bozeman"));
}
#[test]
fn an_unresolved_override_never_silently_becomes_a_device_default() {
    let mut work = case(Method::NewJob, "querent");
    work.facts.insert(
        Field::QuestionTime,
        Slot::Proposed {
            observation: Observation {
                value: "yesterday about three".into(),
                evidence: Evidence::User {
                    turn: 1,
                    quote: "yesterday about three".into(),
                },
            },
        },
    );
    assert!(work
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::ChartMoment));
    work.facts.insert(
        Field::ReaderPlace,
        Slot::Unavailable {
            reason: "I don't know the earlier reader location".into(),
            evidence: Evidence::User {
                turn: 1,
                quote: "I don't know".into(),
            },
        },
    );
    assert!(work
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::ChartPlace && n.state == "unavailable"));
}
#[test]
fn a_person_can_say_they_do_not_know_an_owner_or_relationship() {
    let mut work = case(Method::MovableDeal, "");
    work.requested = Some(RequirementKey::Owner);
    let mut patch = control(Intent::Clarify);
    patch.unavailable_quote = "I don't know".into();
    work.apply(&patch, 2, "I don't know", false).unwrap();
    assert!(work
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Owner && n.state == "unavailable"));
    assert!(work.subject.resolved().unwrap().owner_id.is_empty());
}
#[test]
fn recognition_does_not_repeat_the_private_change_history() {
    let mut work = case(Method::NewJob, "querent");
    work.apply(&control(Intent::Resume), 2, "Continue", false)
        .unwrap();
    assert!(!work.changes.is_empty());
    assert!(work.recognition_snapshot().changes.is_empty());
    assert_eq!(
        work.recognition_snapshot().question.resolved(),
        work.question.resolved()
    );
}

#[test]
fn transaction_roles_include_parties_separately_from_goods() {
    let mut sale = case(Method::MovableDeal, "friend");
    sale.facts
        .insert(Field::DealCapacity, resolved("sell".into()));
    sale.facts.insert(Field::Seller, resolved("friend".into()));
    let choices = options(&sale);
    assert_eq!(primary(&sale), Some(12));
    assert_eq!(
        choices
            .choices
            .iter()
            .find(|c| c.id == "deal.counterparty")
            .unwrap()
            .house,
        Some(5)
    );
    assert!(choices
        .required_groups
        .contains(&vec!["deal.counterparty".into()]));
    sale.facts
        .insert(Field::DealParty, resolved("daughter".into()));
    let choices = options(&sale);
    assert!(!choices.choices.iter().any(|c| c.id == "deal.counterparty"));
    assert_eq!(
        choices
            .choices
            .iter()
            .find(|c| c.id == "daughter.self")
            .unwrap()
            .house,
        Some(5)
    );
}

#[test]
fn work_capacity_resolves_the_role_without_an_extra_personal_relationship() {
    let mut work = case(Method::WorkPerson, "friend");
    work.people.get_mut("friend").unwrap().relationship = "unknown".into();
    work.facts
        .insert(Field::WorkCapacity, resolved("boss".into()));
    assert!(!work
        .plan(None)
        .needs
        .iter()
        .any(|n| matches!(n.key, RequirementKey::PersonRelationship(_))));
    assert!(options(&work).missing.is_empty());
    assert_eq!(primary(&work), Some(10));
    work.facts
        .insert(Field::WorkCapacity, resolved("colleague".into()));
    assert_eq!(primary(&work), Some(7));
}

#[test]
fn an_unnamed_future_partner_does_not_erase_the_genuine_relay_principal() {
    let mut relationship = case(Method::Relationship, "");
    relationship
        .facts
        .insert(Field::PrincipalMode, resolved("relay".into()));
    relationship
        .facts
        .insert(Field::PrincipalId, resolved("daughter".into()));
    relationship
        .facts
        .insert(Field::Baseline, resolved("hoped_for".into()));
    let choices = options(&relationship);
    assert!(choices.missing.is_empty());
    assert_eq!(
        choices
            .choices
            .iter()
            .find(|c| c.id == "querent.self")
            .unwrap()
            .label,
        "Daughter"
    );
    assert_eq!(primary(&relationship), Some(7));
}

#[test]
fn a_reading_handoff_excludes_inapplicable_facts_and_background_people() {
    let mut money = case(Method::Money, "querent");
    money
        .facts
        .insert(Field::MoneySource, resolved("job".into()));
    money
        .facts
        .insert(Field::Discretionary, resolved("owed".into()));
    let anchor = Anchor {
        timestamp_ms: 1789387200000.,
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
    };
    let ready = ReadyReading::prepare(&money, anchor).unwrap();
    assert!(ready.people.is_empty());
    assert!(!ready.inputs.contains_key(&Field::Discretionary));
    assert_eq!(money.text(Field::Discretionary), Some("owed"));
    assert_eq!(money.people.len(), 3);
}

#[test]
fn a_revised_method_does_not_inherit_an_obsolete_readers_information_need() {
    let mut work = case(Method::NewJob, "querent");
    work.require_information(
        RequirementKey::Field(Field::Priorities),
        "The old program needs priorities".into(),
    );
    let mut patch = control(Intent::Correct);
    patch.frame = Some(Frame {
        method: Method::ExistingJob,
        facet: Facet::Event,
    });
    work.apply(
        &patch,
        2,
        "Actually this is about keeping my current job",
        false,
    )
    .unwrap();
    assert!(work.additional.is_empty());
    assert!(!work
        .plan(None)
        .needs
        .iter()
        .any(|n| n.key == RequirementKey::Field(Field::Priorities)));
}

#[test]
fn a_source_span_rejection_identifies_the_bad_quote_without_normalising_it() {
    let mut relationship = case(Method::Relationship, "");
    let before = serde_json::to_value(&relationship).unwrap();
    let mut patch = control(Intent::Clarify);
    patch.updates = vec![Update {
        field: Field::Context,
        value: "Getting married".into(),
        quote: "marry".into(),
        mode: UpdateMode::Supply,
    }];
    let error = relationship
        .apply(&patch, 2, "Getting married", false)
        .unwrap_err();
    assert!(error.contains("\"marry\""));
    assert_eq!(serde_json::to_value(&relationship).unwrap(), before);
}

#[test]
fn a_purchase_does_not_quietly_use_a_sellers_title_as_the_buyers_role_frame() {
    let mut property = case(Method::Property, "friend");
    property
        .facts
        .insert(Field::DealCapacity, resolved("buy".into()));
    let anchor = Anchor {
        timestamp_ms: 1789387200000.,
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
    };
    assert_eq!(
        property.plan(Some(&anchor)).limitation.unwrap().code,
        "deal_party_frame_needs_review"
    );
    assert!(ReadyReading::prepare(&property, anchor).is_err());
}

#[test]
fn a_reader_place_declaration_is_distinct_from_a_venue_and_a_question() {
    let observed =
        reader_place_statement("I'm asking from Woodbridge, Virginia, United States.").unwrap();
    assert_eq!(observed.field, Field::ReaderPlace);
    assert_eq!(observed.value, "Woodbridge, Virginia, United States");
    assert!(reader_place_statement("The fair is in Bozeman, Montana.").is_none());
    assert!(reader_place_statement("I'm asking from London; does that change things?").is_none());
}
