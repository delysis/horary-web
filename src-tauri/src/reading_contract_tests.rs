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

#[test]
fn a_reader_declaration_after_the_question_keeps_its_exact_clause() {
    let declaration = "I'm asking from Springfield.";
    let observed = reader_place_statement(&format!(
        "I'm single. Will I marry within a year? {declaration}"
    ))
    .unwrap();
    assert_eq!(observed.value, "Springfield");
    assert_eq!(observed.quote, declaration);
    assert_eq!(
        reader_place_statement("Will I marry? I’m asking from St. Louis, Missouri.")
            .unwrap()
            .value,
        "St. Louis, Missouri"
    );
    for words in [
        "Bob said, \"Will I marry? I'm asking from Bozeman.\"",
        "Bob said, 'Will I marry? I'm asking from Bozeman.'",
        "Bob said, ‘Will I marry? I’m asking from Bozeman.’",
        "The fair is in Springfield. Where will Bob be?",
        "Will I marry? I'm asking from here.",
        "Will I marry? I'm asking from my device's current location.",
        "Will I marry? I'm asking from London. I'm asking from Paris.",
    ] {
        assert!(
            reader_place_statement(words).is_none(),
            "Not an unambiguous reader declaration: {words}"
        );
    }
}

#[test]
fn reported_reader_speech_is_deferred_but_plural_possessives_are_not_quotes() {
    for words in [
        "Bob said:\nI'm asking from Bozeman.",
        "Bob wrote:\nWill my books sell? I'm asking from Bozeman.",
        "I told him:\nI'm asking from London.",
    ] {
        assert!(
            reader_place_statement(words).is_none(),
            "Reported speech: {words}"
        );
    }
    let observed =
        reader_place_statement("Will my parents' business sell? I'm asking from Springfield.")
            .unwrap();
    assert_eq!(observed.value, "Springfield");
    assert_eq!(observed.quote, "I'm asking from Springfield.");
    assert_eq!(
        reader_place_statement("Will my books sell? I’m asking from D’Iberville, Mississippi.")
            .unwrap()
            .value,
        "D’Iberville, Mississippi"
    );
}

fn regression_anchor() -> Anchor {
    Anchor {
        timestamp_ms: 1791388800000.,
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
    }
}

#[test]
fn an_article_does_not_turn_a_goods_name_into_ownership_evidence() {
    for (name, quote) in [
        ("tulips", "the tulips"),
        ("fish", "the fish"),
        ("Fish", "those fish"),
    ] {
        let words = format!("Will my friend Bob sell {quote}? Bob is the seller.");
        let mut patch = control(Intent::Read);
        patch.question = Some(words.clone());
        patch.frame = Some(Frame {
            method: Method::MovableDeal,
            facet: Facet::Event,
        });
        patch.people.push(Person {
            id: "bob".into(),
            label: "Bob".into(),
            relationship: "friend".into(),
            source_quote: "my friend Bob".into(),
        });
        patch.subject = Some(Subject {
            name: name.into(),
            kind: "movable".into(),
            owner_id: "bob".into(),
            source_quote: quote.into(),
        });
        patch.updates = vec![
            Update {
                field: Field::DealCapacity,
                value: "sell".into(),
                quote: "sell".into(),
                mode: UpdateMode::Supply,
            },
            Update {
                field: Field::Seller,
                value: "bob".into(),
                quote: "Bob is the seller".into(),
                mode: UpdateMode::Supply,
            },
        ];
        let mut case = Consultation::default();
        let before = serde_json::to_value(&case).unwrap();
        assert!(case
            .apply(&patch, 1, &words, false)
            .unwrap_err()
            .contains("name alone"));
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
        patch.subject.as_mut().unwrap().owner_id.clear();
        case.apply(&patch, 1, &words, false).unwrap();
        assert!(case
            .plan(Some(&regression_anchor()))
            .needs
            .iter()
            .any(|need| need.key == RequirementKey::Owner));
    }
}

#[test]
fn a_future_marriage_partner_is_a_source_bound_role_without_an_invented_person() {
    for (words, quote) in [
        (
            "What will my future husband look like?",
            "my future husband",
        ),
        (
            "Please describe my future wife's general appearance.",
            "my future wife's",
        ),
        (
            "Could you describe my future marriage partner?",
            "my future marriage partner",
        ),
        (
            "What will the person I will marry look like?",
            "the person I will marry",
        ),
        (
            "I don't know what my future husband will look like; could you describe him?",
            "my future husband",
        ),
    ] {
        let mut consultation = Consultation::default();
        let mut patch = control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame {
            method: Method::PersonDescription,
            facet: Facet::Description,
        });
        patch.subject = Some(Subject {
            name: crate::horary_role_options::FUTURE_MARRIAGE_PARTNER.into(),
            kind: "person_role".into(),
            owner_id: "querent".into(),
            source_quote: quote.into(),
        });
        consultation.apply(&patch, 1, words, false).unwrap();
        assert!(
            consultation.people.is_empty(),
            "An unnamed spouse is not a biographical person"
        );
        assert_eq!(consultation.subject.resolved().unwrap().source_quote, quote);
        let plan = consultation.plan(Some(&regression_anchor()));
        assert!(plan.needs.is_empty());
        assert_eq!(
            plan.limitation.unwrap().code,
            "judgment_program_needs_review"
        );
        assert!(ReadyReading::prepare(&consultation, regression_anchor()).is_err());
        assert_eq!(primary(&consultation), Some(7));
        assert!(options(&consultation).missing.is_empty());
    }
}

#[test]
fn a_future_marriage_role_binds_its_actual_principal_and_requires_unknown_capacity() {
    let words = "Please describe my daughter Sam's future marriage partner.";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::PersonDescription,
        facet: Facet::Description,
    });
    patch.people.push(Person {
        id: "sam".into(),
        label: "Sam".into(),
        relationship: "child".into(),
        source_quote: "my daughter Sam".into(),
    });
    patch.subject = Some(Subject {
        name: crate::horary_role_options::FUTURE_MARRIAGE_PARTNER.into(),
        kind: "person_role".into(),
        owner_id: "sam".into(),
        source_quote: "Sam's future marriage partner".into(),
    });
    let mut consultation = Consultation::default();
    consultation.apply(&patch, 1, words, false).unwrap();
    assert_eq!(consultation.people.len(), 1);
    assert!(consultation.people.contains_key("sam"));
    assert!(consultation
        .plan(Some(&regression_anchor()))
        .needs
        .is_empty());
    assert_eq!(
        primary(&consultation),
        Some(11),
        "Seventh from the daughter's fifth"
    );
    consultation
        .facts
        .insert(Field::PrincipalMode, resolved("relay".into()));
    consultation
        .facts
        .insert(Field::PrincipalId, resolved("sam".into()));
    assert_eq!(
        primary(&consultation),
        Some(7),
        "A genuinely relayed principal is first-house"
    );

    let words = "Please describe Sam's future spouse.";
    patch.question = Some(words.into());
    patch.people[0].relationship = "unknown".into();
    patch.people[0].source_quote = "Sam".into();
    patch.subject.as_mut().unwrap().source_quote = "Sam's future spouse".into();
    let mut unknown = Consultation::default();
    unknown.apply(&patch, 1, words, false).unwrap();
    assert!(unknown
        .plan(Some(&regression_anchor()))
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::PersonRelationship("sam".into())));
    assert_eq!(
        primary(&unknown),
        None,
        "No default seventh for an unbound principal"
    );

    let words = "Could you describe the future marriage partner?";
    patch.question = Some(words.into());
    patch.people.clear();
    patch.subject.as_mut().unwrap().owner_id.clear();
    patch.subject.as_mut().unwrap().source_quote = "future marriage partner".into();
    let mut unbound = Consultation::default();
    unbound.apply(&patch, 1, words, false).unwrap();
    assert!(unbound
        .plan(Some(&regression_anchor()))
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::Owner));
    assert_eq!(primary(&unbound), None);
}

#[test]
fn person_roles_reject_named_targets_negation_wrong_principals_and_other_future_roles() {
    for (words, quote, owner) in [
        (
            "What will my future husband Alex look like?",
            "my future husband",
            "querent",
        ),
        (
            "Describe Alex, my future husband.",
            "my future husband",
            "querent",
        ),
        (
            "Alex is my future husband. Describe him.",
            "my future husband",
            "querent",
        ),
        (
            "My future husband is Alex. Describe him.",
            "my future husband",
            "querent",
        ),
        (
            "Alex is my future husband. Describe him.",
            "my future husband",
            "",
        ),
        (
            "Describe Sam, not my future husband.",
            "my future husband",
            "querent",
        ),
        (
            "I don't mean my future husband; describe Sam.",
            "my future husband",
            "querent",
        ),
        (
            "I'm not asking about my future husband; describe Sam.",
            "my future husband",
            "querent",
        ),
        (
            "What will my future business partner look like?",
            "my future business partner",
            "querent",
        ),
        (
            "What will my future child look like?",
            "my future child",
            "querent",
        ),
        (
            "What will my future partner look like?",
            "my future partner",
            "querent",
        ),
        (
            "What will Sam's future husband look like?",
            "Sam's future husband",
            "querent",
        ),
    ] {
        let mut consultation = Consultation::default();
        let mut patch = control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame {
            method: Method::PersonDescription,
            facet: Facet::Description,
        });
        patch.subject = Some(Subject {
            name: crate::horary_role_options::FUTURE_MARRIAGE_PARTNER.into(),
            kind: "person_role".into(),
            owner_id: owner.into(),
            source_quote: quote.into(),
        });
        let before = serde_json::to_value(&consultation).unwrap();
        assert!(
            consultation.apply(&patch, 1, words, false).is_err(),
            "Not an unnamed marriage role: {words}"
        );
        assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    }
    let words = "What will my future husband look like?";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::PersonDescription,
        facet: Facet::Description,
    });
    patch.subject = Some(Subject {
        name: "Future business partner".into(),
        kind: "person_role".into(),
        owner_id: "querent".into(),
        source_quote: "my future husband".into(),
    });
    assert!(Consultation::default()
        .apply(&patch, 1, words, false)
        .is_err());
    patch.subject.as_mut().unwrap().name =
        crate::horary_role_options::FUTURE_MARRIAGE_PARTNER.into();
    patch.frame.as_mut().unwrap().method = Method::Relationship;
    assert!(Consultation::default()
        .apply(&patch, 1, words, false)
        .is_err());
    assert!(CATALOGUE
        .iter()
        .all(|card| card.subject_kinds.contains(&"person_role")
            == (card.method == Method::PersonDescription)));
    let schema = turn_schema(None);
    assert!(
        schema["properties"]["subject"]["oneOf"][1]["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "person_role")
    );
}

#[test]
fn a_known_person_cannot_be_replaced_silently_by_a_future_marriage_role() {
    let mut consultation = case(Method::PersonDescription, "daughter");
    let before = serde_json::to_value(&consultation).unwrap();
    let mut patch = control(Intent::Clarify);
    patch.subject = Some(Subject {
        name: crate::horary_role_options::FUTURE_MARRIAGE_PARTNER.into(),
        kind: "person_role".into(),
        owner_id: "querent".into(),
        source_quote: "my future husband".into(),
    });
    assert!(consultation
        .apply(&patch, 2, "Describe my future husband.", false)
        .is_err());
    assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    assert_eq!(consultation.subject.resolved().unwrap().kind, "person");
    assert_eq!(
        consultation.subject.resolved().unwrap().owner_id,
        "daughter"
    );
}

#[test]
fn a_pending_generic_pet_can_be_identified_without_changing_an_existing_animal_or_owner() {
    let initial = |name: &str| {
        let words = format!("When will my pet {name} return? My friend Jo is helping.");
        let mut consultation = Consultation::default();
        let mut patch = control(Intent::Read);
        patch.question = Some(words.clone());
        patch.frame = Some(Frame {
            method: Method::LostAnimal,
            facet: Facet::Timing,
        });
        patch.people.push(Person {
            id: "jo".into(),
            label: "Jo".into(),
            relationship: "friend".into(),
            source_quote: "My friend Jo".into(),
        });
        patch.subject = Some(Subject {
            name: name.into(),
            kind: "animal".into(),
            owner_id: "querent".into(),
            source_quote: "my pet".into(),
        });
        consultation.apply(&patch, 1, &words, false).unwrap();
        consultation.requested = Some(RequirementKey::Field(Field::AnimalKind));
        consultation
    };
    let words = "The pet is my cat Moss, a tabby cat.";
    let mut patch = control(Intent::Clarify);
    patch.subject = Some(Subject {
        name: "Moss".into(),
        kind: "small_animal".into(),
        owner_id: "querent".into(),
        source_quote: "my cat Moss".into(),
    });
    patch.updates = vec![
        Update {
            field: Field::AnimalKind,
            value: "small_kind".into(),
            quote: "cat".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::Description,
            value: "tabby cat".into(),
            quote: "tabby cat".into(),
            mode: UpdateMode::Supply,
        },
    ];
    let mut consultation = initial("pet");
    let question = consultation.question.resolved().unwrap().clone();
    consultation.apply(&patch, 2, words, false).unwrap();
    assert_eq!(consultation.question.resolved().unwrap(), &question);
    assert_eq!(consultation.subject.resolved().unwrap().name, "Moss");
    assert_eq!(
        consultation.subject.resolved().unwrap().kind,
        "small_animal"
    );
    assert_eq!(consultation.subject.resolved().unwrap().owner_id, "querent");
    assert_eq!(consultation.text(Field::Description), Some("tabby cat"));
    assert!(ReadyReading::prepare(&consultation, regression_anchor()).is_ok());

    for (old_name, new_name, owner) in [("Moss", "Luna", "querent"), ("pet", "Moss", "jo")] {
        let mut consultation = initial(old_name);
        let before = serde_json::to_value(&consultation).unwrap();
        patch.subject.as_mut().unwrap().name = new_name.into();
        patch.subject.as_mut().unwrap().owner_id = owner.into();
        patch.subject.as_mut().unwrap().source_quote = format!("my cat {new_name}");
        let words = format!("The pet is my cat {new_name}, a tabby cat.");
        assert!(consultation.apply(&patch, 2, &words, false).is_err());
        assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    }
}

#[test]
fn a_bare_sale_fragment_does_not_establish_ownership_or_overwrite_supplied_ownership() {
    let words = "Will my friend Bob sell the tulips at Friday's market? He is the seller.";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::MovableDeal,
        facet: Facet::Event,
    });
    patch.people.push(Person {
        id: "bob_1".into(),
        label: "Bob".into(),
        relationship: "friend".into(),
        source_quote: "my friend Bob".into(),
    });
    patch.subject = Some(Subject {
        name: "tulips".into(),
        kind: "movable".into(),
        owner_id: "bob_1".into(),
        source_quote: "sell the tulips".into(),
    });
    let mut consultation = Consultation::default();
    let before = serde_json::to_value(&consultation).unwrap();
    assert!(consultation.apply(&patch, 1, words, false).is_err());
    assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    patch.subject.as_mut().unwrap().source_quote = words.into();
    assert!(
        consultation.apply(&patch, 1, words, false).is_err(),
        "Quoting the whole sale question still does not establish title"
    );
    assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    patch.subject.as_mut().unwrap().owner_id.clear();
    consultation.apply(&patch, 1, words, false).unwrap();
    assert!(consultation
        .plan(Some(&regression_anchor()))
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::Owner));

    let mut supply = control(Intent::Clarify);
    supply.subject = Some(Subject {
        name: "tulips".into(),
        kind: "movable".into(),
        owner_id: "querent".into(),
        source_quote: "The tulips belong to me".into(),
    });
    consultation
        .apply(&supply, 2, "The tulips belong to me", false)
        .unwrap();
    supply.subject.as_mut().unwrap().source_quote = "sell the tulips".into();
    consultation
        .apply(&supply, 3, "Bob will sell the tulips for me", false)
        .unwrap();
    assert_eq!(consultation.subject.resolved().unwrap().owner_id, "querent");

    let mut purchase = control(Intent::Read);
    purchase.question = Some("Should I buy the tulips?".into());
    purchase.frame = Some(Frame {
        method: Method::MovableDeal,
        facet: Facet::Event,
    });
    purchase.subject = Some(Subject {
        name: "tulips".into(),
        kind: "movable".into(),
        owner_id: "querent".into(),
        source_quote: "buy the tulips".into(),
    });
    let mut prospective = Consultation::default();
    prospective
        .apply(&purchase, 1, "Should I buy the tulips?", false)
        .unwrap();
    assert_eq!(prospective.subject.resolved().unwrap().owner_id, "querent");
}

#[test]
fn both_named_relationship_subjects_bind_alex_before_handoff() {
    for (words, name, quote, baseline, baseline_quote) in [
        (
            "Will my partner Alex and I go through with the wedding we have already arranged?",
            "Alex",
            "my partner Alex",
            "arranged_wedding",
            "the wedding we have already arranged",
        ),
        (
            "My partner Alex and I have been together six years. How are things between us?",
            "relationship",
            "How are things between us?",
            "ongoing",
            "have been together six years",
        ),
    ] {
        let mut patch = control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame {
            method: Method::Relationship,
            facet: if baseline == "ongoing" {
                Facet::Situation
            } else {
                Facet::Event
            },
        });
        let person_quote = if words.starts_with("My") {
            "My partner Alex"
        } else {
            "my partner Alex"
        };
        patch.people.push(Person {
            id: "alex".into(),
            label: "Alex".into(),
            relationship: "partner".into(),
            source_quote: person_quote.into(),
        });
        patch.subject = Some(Subject {
            name: name.into(),
            kind: "person".into(),
            owner_id: String::new(),
            source_quote: quote.into(),
        });
        patch.updates.push(Update {
            field: Field::Baseline,
            value: baseline.into(),
            quote: baseline_quote.into(),
            mode: UpdateMode::Supply,
        });
        let mut case = Consultation::default();
        assert!(case
            .apply(&patch, 1, words, false)
            .unwrap_err()
            .contains("subject.owner_id"));
        assert!(case.question.resolved().is_none());
        let subject = patch.subject.as_mut().unwrap();
        subject.name = "Alex".into();
        subject.owner_id = "alex".into();
        subject.source_quote = person_quote.into();
        case.apply(&patch, 1, words, false).unwrap();
        let ready = ReadyReading::prepare(&case, regression_anchor()).unwrap();
        assert!(
            ready.people.contains_key("alex"),
            "Named person survives the frozen reading inputs"
        );
        let roles = options(&case);
        assert!(roles
            .choices
            .iter()
            .any(|choice| choice.label == "Alex" && choice.house == Some(7)));
        assert!(!roles
            .choices
            .iter()
            .any(|choice| choice.basis.contains("unnamed partner")));
    }
}

#[test]
fn a_saved_unbound_named_person_can_be_repaired_without_changing_the_question() {
    let mut case = case(Method::Relationship, "");
    case.facts
        .insert(Field::Baseline, resolved("ongoing".into()));
    case.people.insert(
        "alex".into(),
        Person {
            id: "alex".into(),
            label: "Alex".into(),
            relationship: "partner".into(),
            source_quote: "my partner Alex".into(),
        },
    );
    if let Slot::Resolved { observation } = &mut case.subject {
        observation.value.name = "relationship".into();
    }
    assert!(ReadyReading::prepare(&case, regression_anchor()).is_err());
    let original = case.question.resolved().cloned();
    let mut patch = control(Intent::Clarify);
    patch.subject = Some(Subject {
        name: "Alex".into(),
        kind: "person".into(),
        owner_id: "alex".into(),
        source_quote: "my partner Alex".into(),
    });
    case.apply(&patch, 2, "I mean my partner Alex", false)
        .unwrap();
    assert_eq!(case.question.resolved(), original.as_ref());
    assert!(ReadyReading::prepare(&case, regression_anchor())
        .unwrap()
        .people
        .contains_key("alex"));
}

#[test]
fn an_unspecified_pet_has_no_small_or_large_animal_claim_until_the_species_arrives() {
    let words = "My pet escaped this morning. Will I find the animal again?";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::LostAnimal,
        facet: Facet::Event,
    });
    patch.subject = Some(Subject {
        name: "pet".into(),
        kind: "small_animal".into(),
        owner_id: "querent".into(),
        source_quote: "My pet".into(),
    });
    let mut case = Consultation::default();
    assert!(case
        .apply(&patch, 1, words, false)
        .unwrap_err()
        .contains("kind is unresolved"));
    patch.subject.as_mut().unwrap().kind = "animal".into();
    case.apply(&patch, 1, words, false).unwrap();
    assert!(case
        .plan(Some(&regression_anchor()))
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::Field(Field::AnimalKind)));
    assert!(!options(&case).missing.is_empty());
    assert!(!options(&case)
        .choices
        .iter()
        .any(|choice| choice.id == "subject.primary"));
    case.requested = Some(RequirementKey::Field(Field::AnimalKind));
    let mut answer = control(Intent::Clarify);
    answer.updates.push(Update {
        field: Field::AnimalKind,
        value: "small_kind".into(),
        quote: "cat".into(),
        mode: UpdateMode::Supply,
    });
    case.apply(&answer, 3, "The pet is my cat Moss, a tabby cat.", false)
        .unwrap();
    assert_eq!(primary(&case), Some(6));
    assert!(ReadyReading::prepare(&case, regression_anchor()).is_ok());
    let schema = turn_schema(None);
    assert!(
        schema["properties"]["subject"]["oneOf"][1]["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("animal"))
    );
    patch.frame.as_mut().unwrap().method = Method::LostObject;
    assert!(
        Consultation::default()
            .apply(&patch, 1, words, false)
            .is_err(),
        "Generic animal is confined to the animal method"
    );
}

#[test]
fn event_chronology_is_not_a_repeated_civil_time_choice() {
    for quote in ["this morning", "earlier"] {
        let mut case = case(Method::NewJob, "querent");
        let before = serde_json::to_value(&case).unwrap();
        let mut patch = control(Intent::Clarify);
        patch.updates.push(Update {
            field: Field::TimeOccurrence,
            value: "earlier".into(),
            quote: quote.into(),
            mode: UpdateMode::Supply,
        });
        assert!(case
            .apply(&patch, 2, &format!("The interview was {quote}"), false)
            .unwrap_err()
            .contains("clock-overlap"));
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
    }
}

#[test]
fn a_pending_overlap_or_explicit_repeated_chart_time_can_select_an_occurrence() {
    let mut consultation = case(Method::NewJob, "querent");
    consultation.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
    let mut patch = control(Intent::Clarify);
    patch.updates.push(Update {
        field: Field::TimeOccurrence,
        value: "later".into(),
        quote: "later".into(),
        mode: UpdateMode::Supply,
    });
    consultation
        .apply(&patch, 2, "The later occurrence", false)
        .unwrap();
    assert_eq!(consultation.text(Field::TimeOccurrence), Some("later"));
    let mut fresh = case(Method::NewJob, "querent");
    patch.updates = vec![
        Update {
            field: Field::TimeOccurrence,
            value: "earlier".into(),
            quote: "first occurrence".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::QuestionTime,
            value: "2026-11-01T01:30".into(),
            quote: "2026-11-01T01:30".into(),
            mode: UpdateMode::Supply,
        },
    ];
    fresh
        .apply(
            &patch,
            2,
            "Cast my question for 2026-11-01T01:30, the first occurrence.",
            false,
        )
        .unwrap();
    assert_eq!(fresh.text(Field::TimeOccurrence), Some("earlier"));
    fresh.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
    patch.updates = vec![Update {
        field: Field::TimeOccurrence,
        value: "later".into(),
        quote: "first occurrence".into(),
        mode: UpdateMode::Supply,
    }];
    assert!(
        fresh
            .apply(&patch, 3, "The first occurrence", false)
            .is_err(),
        "Selected label must match the person's choice"
    );
}

#[test]
fn a_pending_chart_moment_accepts_the_second_clock_occurrence_without_an_occurrence_keyword() {
    let mut consultation = case(Method::NewJob, "querent");
    consultation
        .facts
        .insert(Field::QuestionTime, resolved("2025-11-02T01:30".into()));
    consultation.requested = Some(RequirementKey::ChartMoment);
    let mut patch = control(Intent::Clarify);
    patch.updates.push(Update {
        field: Field::TimeOccurrence,
        value: "later".into(),
        quote: "second 1:30 AM".into(),
        mode: UpdateMode::Supply,
    });
    consultation
        .apply(
            &patch,
            2,
            "The second 1:30 AM, after the clocks moved back.",
            false,
        )
        .unwrap();
    assert_eq!(consultation.text(Field::TimeOccurrence), Some("later"));
    consultation.requested = Some(RequirementKey::ChartMoment);
    patch.updates[0].value = "earlier".into();
    patch.updates[0].quote = "earlier".into();
    assert!(consultation
        .apply(&patch, 3, "The fair was earlier this morning", false)
        .is_err());
}

#[test]
fn unknown_people_still_need_an_exact_source_quote() {
    let mut case = Consultation::default();
    let mut patch = control(Intent::Clarify);
    patch.people.push(Person {
        id: "bob".into(),
        label: "Bob".into(),
        relationship: "unknown".into(),
        source_quote: "Robert".into(),
    });
    assert!(case
        .apply(&patch, 1, "Bob", false)
        .unwrap_err()
        .contains("exact span"));
    patch.people[0].source_quote = "Bob".into();
    case.apply(&patch, 1, "Bob", false).unwrap();
    assert_eq!(case.people["bob"].relationship, "unknown");
}

#[test]
fn source_backed_neighbor_and_sibling_inference_keeps_negation_and_target_scope() {
    for (relationship, label, quote) in [
        ("neighbor", "Pat", "Pat lives next door to me"),
        ("sibling", "Nina", "Nina and I share the same parents"),
        (
            "sibling",
            "Sam",
            "Sam and I have the same mother and father",
        ),
    ] {
        let mut case = Consultation::default();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: label.to_ascii_lowercase(),
            label: label.into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        case.apply(&patch, 1, quote, false).unwrap();
        assert_eq!(
            case.people[&label.to_ascii_lowercase()].relationship,
            relationship
        );
    }
    for (relationship, quote, source) in [
        (
            "neighbor",
            "lives next door to me",
            "Pat no longer lives next door to me",
        ),
        ("neighbor", "my neighbor", "Pat is not my neighbor"),
        (
            "neighbor",
            "Pat lives next door to my mother",
            "Pat lives next door to my mother",
        ),
        (
            "sibling",
            "We do not share the same parents",
            "We do not share the same parents",
        ),
        (
            "sibling",
            "Pat and Nina share the same parents",
            "Pat and Nina share the same parents",
        ),
        (
            "sibling",
            "have the same mother and father",
            "Pat and I do not have the same mother and father",
        ),
        (
            "sibling",
            "Pat and Nina have the same mother and father",
            "Pat and Nina have the same mother and father",
        ),
    ] {
        let mut case = Consultation::default();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: "pat".into(),
            label: "Pat".into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        assert!(
            case.apply(&patch, 1, source, false).is_err(),
            "Unsupported relationship: {source}"
        );
    }
}

#[test]
fn literal_relationships_keep_the_speaker_target_and_negation_beyond_four_modifiers() {
    for (relationship, label, quote, source) in [
        ("friend", "Jo", "friend", "Can I trust my friend Jo?"),
        (
            "sibling",
            "Sister",
            "my sister",
            "Where is my sister's watch?",
        ),
        (
            "sibling",
            "Ava",
            "my sister Ava's watch",
            "Where is my sister Ava's watch?",
        ),
        (
            "friend",
            "Jo",
            "friend",
            "I cannot trust my friend Jo to keep a secret.",
        ),
    ] {
        let mut consultation = Consultation::default();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: label.to_ascii_lowercase(),
            label: label.into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        consultation.apply(&patch, 1, source, false).unwrap();
    }
    for (relationship, quote, source) in [
        ("neighbor", "neighbor", "Pat is my mother's neighbor"),
        ("friend", "friend", "Pat is my sister's friend"),
        ("sibling", "my sister", "Pat is my sister's friend"),
        (
            "neighbor",
            "neighbor",
            "Pat is not actually really my next door neighbor",
        ),
        (
            "sibling",
            "Pat and Nina have the same parents, but I do not know them",
            "Pat and Nina have the same parents, but I do not know them",
        ),
        ("friend", "friend", "My friend Jo does not know Pat"),
    ] {
        let mut consultation = Consultation::default();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: "pat".into(),
            label: "Pat".into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        assert!(
            consultation.apply(&patch, 1, source, false).is_err(),
            "Unsupported personal relationship: {source}"
        );
    }
    let mut consultation = Consultation::default();
    consultation.people.insert(
        "pat".into(),
        Person {
            id: "pat".into(),
            label: "Pat".into(),
            relationship: "unknown".into(),
            source_quote: "Pat".into(),
        },
    );
    consultation.requested = Some(RequirementKey::PersonRelationship("pat".into()));
    let mut patch = control(Intent::Clarify);
    patch.people.push(Person {
        id: "pat".into(),
        label: "Pat".into(),
        relationship: "neighbor".into(),
        source_quote: "my neighbor".into(),
    });
    consultation
        .apply(&patch, 2, "He is my neighbor", false)
        .unwrap();
    assert_eq!(consultation.people["pat"].relationship, "neighbor");
}

#[test]
fn a_pending_personal_capacity_accepts_anaphora_but_not_a_different_named_actor() {
    let consult = || {
        let mut consultation = Consultation::default();
        consultation.people.insert(
            "bob".into(),
            Person {
                id: "bob".into(),
                label: "Bob".into(),
                relationship: "unknown".into(),
                source_quote: "Bob".into(),
            },
        );
        consultation.requested = Some(RequirementKey::PersonRelationship("bob".into()));
        consultation
    };
    for (relationship, quote, source) in [
        ("friend", "friend", "He is my friend"),
        ("friend", "friend", "He is my friend from school"),
        ("sibling", "brother", "My brother"),
        ("sibling", "brother", "My brother in Arlington"),
        ("sibling", "brother", "Bob is my brother"),
        ("friend", "friend", "My friend Bob"),
        (
            "neighbor",
            "lives next door to me",
            "He lives next door to me",
        ),
        (
            "sibling",
            "have the same mother and father",
            "We have the same mother and father",
        ),
    ] {
        let mut consultation = consult();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: "bob".into(),
            label: "Bob".into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        consultation.apply(&patch, 2, source, false).unwrap();
        assert_eq!(consultation.people["bob"].relationship, relationship);
    }
    for (relationship, quote, source) in [
        ("friend", "friend", "Pat is my friend"),
        ("sibling", "brother", "Pat is my brother"),
        ("sibling", "brother", "My brother Pat"),
        ("sibling", "brother", "My brother, Pat"),
        (
            "neighbor",
            "lives next door to me",
            "Pat lives next door to me",
        ),
        (
            "sibling",
            "have the same mother and father",
            "Pat and I have the same mother and father",
        ),
        ("friend", "friend", "He is not really my friend"),
    ] {
        let mut consultation = consult();
        let mut patch = control(Intent::Clarify);
        patch.people.push(Person {
            id: "bob".into(),
            label: "Bob".into(),
            relationship: relationship.into(),
            source_quote: quote.into(),
        });
        assert!(
            consultation.apply(&patch, 2, source, false).is_err(),
            "Bob must not inherit an unrelated capacity: {source}"
        );
        assert_eq!(consultation.people["bob"].relationship, "unknown");
        assert_eq!(
            consultation.requested,
            Some(RequirementKey::PersonRelationship("bob".into()))
        );
    }
}

#[test]
fn a_seller_and_a_spouse_are_not_evidence_for_ownership_or_a_relationship_baseline() {
    let words = "How many fish will Bob sell at the market on Friday?";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::MovableDeal,
        facet: Facet::Quantity,
    });
    patch.people = vec![Person {
        id: "bob".into(),
        label: "Bob".into(),
        relationship: "unknown".into(),
        source_quote: "Bob".into(),
    }];
    patch.subject = Some(Subject {
        name: "Fish".into(),
        kind: "movable".into(),
        owner_id: "bob".into(),
        source_quote: "fish".into(),
    });
    let mut case = Consultation::default();
    assert!(case
        .apply(&patch, 1, words, false)
        .unwrap_err()
        .contains("name alone"));
    assert!(
        case.question.resolved().is_none(),
        "Rejected extraction is atomic"
    );
    patch.subject.as_mut().unwrap().owner_id.clear();
    case.apply(&patch, 1, words, false).unwrap();
    let mut reply = control(Intent::Clarify);
    reply.updates.push(Update {
        field: Field::Baseline,
        value: "ongoing".into(),
        quote: "Bob is my husband".into(),
        mode: UpdateMode::Supply,
    });
    assert!(case
        .apply(&reply, 2, "Bob is my husband. They are his fish.", false)
        .unwrap_err()
        .contains("baseline"));
    assert!(case.text(Field::Baseline).is_none());
}

#[test]
fn an_object_labels_capitalization_does_not_make_an_owner_clarification_a_correction() {
    let mut case = case(Method::MovableDeal, "");
    if let Slot::Resolved { observation } = &mut case.subject {
        observation.value.name = "fish".into();
    }
    let mut patch = control(Intent::Clarify);
    patch.subject = Some(Subject {
        name: "Fish".into(),
        kind: "movable".into(),
        owner_id: "friend".into(),
        source_quote: "my friend's fish".into(),
    });
    case.apply(&patch, 2, "They are my friend's fish.", false)
        .unwrap();
    assert_eq!(case.subject.resolved().unwrap().owner_id, "friend");
    patch.subject.as_mut().unwrap().owner_id = "daughter".into();
    patch.subject.as_mut().unwrap().source_quote = "my daughter's fish".into();
    assert!(
        case.apply(&patch, 3, "They are my daughter's fish.", false)
            .is_err(),
        "Known ownership still requires explicit correction"
    );
}

#[test]
fn recognition_teaching_keeps_other_question_types_out_of_a_selected_sale() {
    let sale = recognition_guide(Some(&case(Method::MovableDeal, "")));
    assert!(sale.contains("(movable_deal)"));
    assert!(sale.contains("Selling a thing or working a stall does not prove ownership"));
    assert!(sale.contains("seller as the actual person's ID or querent"));
    assert!(!sale.contains("hoped_for"));
    assert!(!sale.contains("I'm single. Will I get married"));
    assert!(!sale.contains("money_source=customer"));
    let relationship = recognition_guide(Some(&case(Method::Relationship, "")));
    assert!(relationship.contains("hoped_for"));
    assert!(relationship.contains("baseline is absent in A"));
    assert!(!relationship.contains("Will Bob sell his books at the fair?"));
    // Reclassification remains expressible in the shared patch vocabulary.
    let schema = turn_schema(Some(&case(Method::MovableDeal, "")));
    assert!(schema.to_string().contains("baseline"));
}

#[test]
fn retained_event_words_cannot_become_new_chart_overrides_on_an_unrelated_reply() {
    let mut case = case(Method::MovableDeal, "querent");
    case.question = resolved("Will my books sell at the fair in Bozeman tomorrow at three?".into());
    let before = serde_json::to_value(&case).unwrap();
    for (field, value) in [
        (Field::ReaderPlace, "Bozeman"),
        (Field::QuestionTime, "tomorrow at three"),
    ] {
        let mut patch = control(Intent::Clarify);
        patch.updates = vec![Update {
            field,
            value: value.into(),
            quote: value.into(),
            mode: UpdateMode::Supply,
        }];
        assert!(case
            .apply(&patch, 2, "They are my books.", false)
            .unwrap_err()
            .contains("Remove this override"));
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
    }
    // A real answer to the reader's place question still works, including a
    // city mentioned earlier as an event venue. Its CURRENT source matters.
    case.requested = Some(RequirementKey::ChartPlace);
    let mut patch = control(Intent::Clarify);
    patch.updates = vec![Update {
        field: Field::ReaderPlace,
        value: "Bozeman".into(),
        quote: "Bozeman".into(),
        mode: UpdateMode::Supply,
    }];
    case.apply(&patch, 3, "Bozeman", false).unwrap();
    assert_eq!(case.text(Field::ReaderPlace), Some("Bozeman"));
}

#[test]
fn investment_ownership_can_use_a_long_name_equal_to_its_explicit_owning_quote() {
    let words = "Will the shares I own in Northstar Ltd increase in value over the next six months? These are my own holdings; I want to know about their value, not whether someone will pay me a debt.";
    let quote = "shares I own in Northstar Ltd";
    let mut patch = control(Intent::Read);
    patch.question = Some(words.into());
    patch.frame = Some(Frame {
        method: Method::Investment,
        facet: Facet::Profit,
    });
    patch.subject = Some(Subject {
        name: quote.into(),
        kind: "movable".into(),
        owner_id: "querent".into(),
        source_quote: quote.into(),
    });
    let mut consultation = Consultation::default();
    consultation.apply(&patch, 1, words, false).unwrap();
    assert_eq!(consultation.subject.resolved().unwrap().owner_id, "querent");
    assert!(consultation
        .plan(Some(&regression_anchor()))
        .needs
        .is_empty());

    // The focused program may recover the same source-bound holding later.
    let mut retained = Consultation::default();
    patch.subject = None;
    retained.apply(&patch, 1, words, false).unwrap();
    let mut focus = control(Intent::Clarify);
    focus.subject = consultation.subject.resolved().cloned();
    retained
        .apply(&focus, 2, "That same holding.", false)
        .unwrap();
    assert_eq!(retained.subject.resolved().unwrap().owner_id, "querent");
}

#[test]
fn investment_owning_quote_exception_preserves_scope_negation_and_atomicity() {
    for (quote, words) in [
        ("shares I own", "These are not shares I own."),
        ("units I own", "These aren't units I own."),
        ("I own no shares", "I own no shares in the fund."),
        (
            "shares I own",
            "Marta said ‘Let us discuss portfolios. The shares I own are thriving.’",
        ),
        ("I own those shares", "I might say I own those shares."),
        ("I own shares", "It is false that I own shares in the fund."),
        ("I own shares", "I wonder whether I own shares in the fund."),
        ("I own shares", "Do I own shares in the fund?"),
    ] {
        let mut patch = control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame {
            method: Method::Investment,
            facet: Facet::Profit,
        });
        patch.subject = Some(Subject {
            name: quote.into(),
            kind: "movable".into(),
            owner_id: "querent".into(),
            source_quote: quote.into(),
        });
        let mut consultation = Consultation::default();
        let before = serde_json::to_value(&consultation).unwrap();
        assert!(
            consultation.apply(&patch, 1, words, false).is_err(),
            "{words}"
        );
        assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    }
}

#[test]
fn the_common_here_now_reader_statement_preserves_the_device_anchor() {
    let words = "I'm single. Will I get married within a year? I'm asking from here, now.";
    assert!(
        reader_place_statement(words).is_none(),
        "A deictic declaration must not manufacture a reader_place=here, now override"
    );
}

#[test]
fn complete_deictic_reader_phrases_do_not_become_geocoding_queries() {
    for place in [
        "here",
        "here, now",
        "here now",
        "right here",
        "right here, now",
        "right here right now",
        "here at the moment",
        "where I am",
        "where I am now",
        "where I am right now",
        "where I currently am",
        "my current location",
        "my current-location",
        "my present location",
        "my device's current location",
        "this device",
    ] {
        let words = format!("Will I marry? I'm asking from {place}.");
        assert!(reader_place_statement(&words).is_none(), "{words}");
    }
}

#[test]
fn here_names_and_explicit_reader_declarations_keep_the_literal_source_span() {
    for place in [
        "Hereford, England",
        "Here, Kansas",
        "Here, KS",
        "Springfield, Virginia",
    ] {
        let statement = format!("I'm asking from {place}.");
        let words = format!("Will I marry within a year? {statement}");
        let update = reader_place_statement(&words)
            .unwrap_or_else(|| panic!("Explicit place was lost: {words}"));
        assert_eq!(update.field, Field::ReaderPlace);
        assert_eq!(update.value, place);
        assert_eq!(update.quote, statement);
        assert_eq!(update.mode, UpdateMode::Supply);
    }
}

#[test]
fn deictic_exclusion_does_not_turn_a_venue_or_reported_reader_into_the_anchor() {
    for words in [
        "Will Bob sell his fish at the fair? The fair is in Hereford, England.",
        "Will we meet? The venue is Here, Kansas, tomorrow.",
        "I'm asking from here, now. The market is in Hereford, England.",
        "Bob said: I'm asking from Here, Kansas.",
        "Bob said ‘I'm asking from Hereford, England.’ Will I hear from Bob?",
    ] {
        assert!(reader_place_statement(words).is_none(), "{words}");
    }
}

#[test]
fn canonical_field_schema_uses_every_catalogue_closed_label() {
    let schema = turn_schema(None);
    for field in Field::ALL {
        let labels = allowed_values(*field);
        if labels.is_empty() {
            continue;
        }
        let mut patch = control(Intent::Clarify);
        patch.updates.push(Update {
            field: *field,
            value: labels[0].into(),
            quote: "Exact source words".into(),
            mode: UpdateMode::Supply,
        });
        for label in labels {
            patch.updates[0].value = (*label).into();
            crate::horary_contract::validate_shape(&serde_json::to_value(&patch).unwrap(), &schema)
                .unwrap();
        }
        patch.updates[0].value = "not_a_canonical_value".into();
        let error =
            crate::horary_contract::validate_shape(&serde_json::to_value(&patch).unwrap(), &schema)
                .unwrap_err();
        assert!(error.contains("$.updates[0].value"), "{field:?}: {error}");
        for label in labels {
            assert!(error.contains(label), "{field:?}: {error}");
        }
    }
}

#[test]
fn canonical_field_mode_precedes_value_and_unavailable_reasons_remain_text() {
    let schema = turn_schema(None);
    for branch in schema["properties"]["updates"]["items"]["oneOf"]
        .as_array()
        .unwrap()
    {
        let fields: Vec<_> = branch["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(fields, ["field", "mode", "value", "quote"]);
    }
    let mut patch = control(Intent::Clarify);
    patch.updates.push(Update {
        field: Field::TimeOccurrence,
        value: "I cannot remember which occurrence".into(),
        quote: "I cannot remember".into(),
        mode: UpdateMode::Unavailable,
    });
    crate::horary_contract::validate_shape(&serde_json::to_value(&patch).unwrap(), &schema)
        .unwrap();
}

#[test]
fn canonical_field_second_clock_choice_saves_later_with_exact_quote() {
    let words = "The second 1:30 AM, after the clocks moved back.";
    for key in [
        RequirementKey::Field(Field::TimeOccurrence),
        RequirementKey::ChartMoment,
    ] {
        let mut consultation = case(Method::NewJob, "querent");
        consultation
            .facts
            .insert(Field::QuestionTime, resolved("2025-11-02T01:30".into()));
        consultation.requested = Some(key);
        let mut patch = control(Intent::Clarify);
        patch.updates.push(Update {
            field: Field::TimeOccurrence,
            value: "later".into(),
            quote: words.into(),
            mode: UpdateMode::Supply,
        });
        crate::horary_contract::validate_shape(
            &serde_json::to_value(&patch).unwrap(),
            &turn_schema(Some(&consultation)),
        )
        .unwrap();
        consultation.apply(&patch, 2, words, false).unwrap();
        assert_eq!(consultation.text(Field::TimeOccurrence), Some("later"));
        assert_eq!(
            consultation.text(Field::QuestionTime),
            Some("2025-11-02T01:30")
        );
        assert!(
            matches!(&consultation.facts[&Field::TimeOccurrence], Slot::Resolved { observation }
            if matches!(&observation.evidence, Evidence::User { turn: 2, quote } if quote == words))
        );
    }
}

#[test]
fn canonical_field_second_value_gets_label_feedback_before_provenance() {
    let mut consultation = case(Method::NewJob, "querent");
    consultation.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
    let before = serde_json::to_value(&consultation).unwrap();
    let mut patch = control(Intent::Clarify);
    patch.updates.push(Update {
        field: Field::TimeOccurrence,
        value: "second".into(),
        quote: "Not an actual quotation".into(),
        mode: UpdateMode::Supply,
    });
    let error = consultation
        .apply(
            &patch,
            2,
            "The second 1:30 AM, after the clocks moved back.",
            false,
        )
        .unwrap_err();
    assert!(
        error.contains("time_occurrence value \"second\" is not canonical"),
        "{error}"
    );
    assert!(error.contains("earlier, later"), "{error}");
    assert!(error.contains("second/later maps to later"), "{error}");
    assert!(!error.contains("Remove this entry"), "{error}");
    assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    let shape_error = crate::horary_contract::validate_shape(
        &serde_json::to_value(&patch).unwrap(),
        &turn_schema(Some(&consultation)),
    )
    .unwrap_err();
    assert!(shape_error.contains("$.updates[0].value"), "{shape_error}");
    assert!(
        shape_error.contains("earlier") && shape_error.contains("later"),
        "{shape_error}"
    );
}

#[test]
fn canonical_field_labels_never_replace_source_or_clock_context_proof() {
    for (quote, words, pending) in [
        ("second 1:30 AM", "The first 1:30 AM", true),
        ("first 1:30 AM", "The first 1:30 AM", true),
        (
            "second 1:30 AM",
            "The fair was at the second 1:30 AM",
            false,
        ),
    ] {
        let mut consultation = case(Method::NewJob, "querent");
        if pending {
            consultation.requested = Some(RequirementKey::Field(Field::TimeOccurrence));
        }
        let before = serde_json::to_value(&consultation).unwrap();
        let mut patch = control(Intent::Clarify);
        patch.updates.push(Update {
            field: Field::TimeOccurrence,
            value: "later".into(),
            quote: quote.into(),
            mode: UpdateMode::Supply,
        });
        assert!(consultation.apply(&patch, 2, words, false).is_err());
        assert_eq!(serde_json::to_value(&consultation).unwrap(), before);
    }
}

#[test]
fn canonical_field_open_context_and_horizon_remain_source_bound_text() {
    let mut consultation = case(Method::NewJob, "querent");
    let mut patch = control(Intent::Clarify);
    patch.updates = vec![
        Update {
            field: Field::Context,
            value: "The role involves evening rehearsals".into(),
            quote: "The role involves evening rehearsals".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::Horizon,
            value: "within nine months".into(),
            quote: "within nine months".into(),
            mode: UpdateMode::Supply,
        },
    ];
    crate::horary_contract::validate_shape(
        &serde_json::to_value(&patch).unwrap(),
        &turn_schema(Some(&consultation)),
    )
    .unwrap();
    consultation
        .apply(
            &patch,
            2,
            "The role involves evening rehearsals; I mean within nine months.",
            false,
        )
        .unwrap();
    assert_eq!(
        consultation.text(Field::Context),
        Some("The role involves evening rehearsals")
    );
    assert_eq!(
        consultation.text(Field::Horizon),
        Some("within nine months")
    );
}

#[test]
fn canonical_field_guide_labels_are_scoped_to_the_selected_program() {
    let consultation = case(Method::Relationship, "");
    for phase in ["complete_selected_program", "update_selected_program"] {
        let guide = recognition_guide_for(Some(&consultation), phase);
        assert!(guide.contains("time_occurrence: earlier | later"));
        assert!(guide.contains("baseline: hoped_for | ongoing | arranged_wedding"));
        assert!(guide.contains("second or later maps to value=later"));
        assert!(!guide.contains("medical_task: diagnosis"));
    }
    assert!(!recognition_guide_for(None, "classify_question").contains("CANONICAL UPDATE LABELS"));
}

#[test]
fn canonical_field_classifier_still_forbids_people_and_updates() {
    let schema = crate::horary_step::response_schema_for(
        crate::horary_lessons::Stage::Intake,
        Matter::Other,
        &json!({"recognition_phase":"classify_question"}),
        &[],
    );
    let mut value = serde_json::to_value(control(Intent::Read)).unwrap();
    crate::horary_contract::validate_shape(&value, &schema).unwrap();
    assert!(schema["properties"]["updates"]["items"].is_null());
    value["people"] =
        json!([{"id":"jo","label":"Jo","relationship":"friend","source_quote":"my friend Jo"}]);
    let error = crate::horary_contract::validate_shape(&value, &schema).unwrap_err();
    assert!(
        error.contains("$.people") && error.contains("must be []"),
        "{error}"
    );
    value["people"] = json!([]);
    value["updates"] = json!([{"field":"time_occurrence","mode":"supply","value":"later","quote":"second 1:30 AM"}]);
    let error = crate::horary_contract::validate_shape(&value, &schema).unwrap_err();
    assert!(
        error.contains("$.updates") && error.contains("must be []"),
        "{error}"
    );
}

fn historical_case(words: &str) -> Consultation {
    let mut consultation = case(Method::Relationship, "");
    consultation.question = Slot::Resolved {
        observation: Observation {
            value: "Will I marry within a year?".into(),
            evidence: Evidence::User {
                turn: 1,
                quote: words.into(),
            },
        },
    };
    consultation.subject = resolved(Subject {
        name: "prospective partner".into(),
        kind: "person".into(),
        owner_id: "".into(),
        source_quote: "Will I marry within a year?".into(),
    });
    consultation
        .facts
        .insert(Field::Baseline, resolved("hoped_for".into()));
    consultation
}
fn device_anchor_for_obligation() -> Anchor {
    Anchor {
        timestamp_ms: 1791388800000.,
        latitude: 38.657,
        longitude: -77.249,
        timezone: "America/New_York".into(),
    }
}
const OMITTED_GAP_ANCHOR: &str = "I'm single. Will I marry within a year? Use the question I understood in New York City, United States, on March 8, 2026 at 2:30 AM.";

#[test]
fn anchor_obligation_supplied_gap_components_require_private_repair() {
    let consultation = historical_case(OMITTED_GAP_ANCHOR);
    let mut omitted = control(Intent::Clarify);
    omitted.updates.push(Update {
        field: Field::Horizon,
        value: "within a year".into(),
        quote: "within a year".into(),
        mode: UpdateMode::Supply,
    });
    let error =
        validate_anchor_completion(&consultation, &omitted, OMITTED_GAP_ANCHOR).unwrap_err();
    assert!(error.contains("reader_place, question_time"), "{error}");
    assert!(error.contains("March 8, 2026 at 2:30 AM"), "{error}");
    assert!(
        error.contains("do not ask for information already present"),
        "{error}"
    );
    assert!(
        ReadyReading::prepare(&consultation, device_anchor_for_obligation()).is_err(),
        "No default-device permit after omitted source anchors"
    );
}

#[test]
fn anchor_obligation_repaired_fields_keep_exact_source_and_reach_time_validation() {
    let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
    let mut patch = control(Intent::Clarify);
    patch.updates = vec![
        Update {
            field: Field::ReaderPlace,
            value: "New York City, United States".into(),
            quote: "New York City, United States".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::QuestionTime,
            value: "2026-03-08T02:30".into(),
            quote: "March 8, 2026 at 2:30 AM".into(),
            mode: UpdateMode::Supply,
        },
    ];
    validate_anchor_completion(&consultation, &patch, OMITTED_GAP_ANCHOR).unwrap();
    consultation
        .apply(&patch, 1, OMITTED_GAP_ANCHOR, false)
        .unwrap();
    assert_eq!(
        consultation.text(Field::ReaderPlace),
        Some("New York City, United States")
    );
    assert_eq!(
        consultation.text(Field::QuestionTime),
        Some("2026-03-08T02:30")
    );
    assert!(horary_ai_core::chart_input::resolve_chart_time(
        "2026-03-08T02:30",
        "America/New_York",
        ""
    )
    .is_err());
    assert!(
        matches!(&consultation.facts[&Field::QuestionTime], Slot::Resolved { observation }
        if matches!(&observation.evidence,Evidence::User { quote, .. } if quote == "March 8, 2026 at 2:30 AM"))
    );
}

#[test]
fn anchor_obligation_partial_history_keeps_missing_fields_for_the_guru() {
    let words = "Will I marry within a year? Use my earlier consultation.";
    let mut consultation = historical_case(words);
    let patch = control(Intent::Clarify);
    validate_anchor_completion(&consultation, &patch, words).unwrap();
    consultation.apply(&patch, 2, words, false).unwrap();
    let plan = consultation.plan(Some(&device_anchor_for_obligation()));
    for field in [Field::ReaderPlace, Field::QuestionTime] {
        assert!(plan
            .needs
            .iter()
            .any(|need| need.key == RequirementKey::Field(field)));
    }
    assert!(ReadyReading::prepare(&consultation, device_anchor_for_obligation()).is_err());
}

#[test]
fn anchor_obligation_defaults_event_context_negation_and_reports_remain_unaffected() {
    for words in [
        "I'm single. Will I marry within a year? I'm asking from here, now.",
        "Will I marry within a year? The fair is in New York on March 8, 2026 at 2:30 AM.",
        "Will I marry within a year? Do not use my earlier consultation in New York on March 8, 2026 at 2:30 AM.",
        "Will I marry within a year? Mira said: 'Use my earlier consultation in New York on March 8, 2026 at 2:30 AM.'",
        "Will I marry within a year?\nMira said:\nUse my earlier consultation in New York on March 8, 2026 at 2:30 AM.",
    ] {
        let consultation = historical_case(words);
        validate_anchor_completion(&consultation, &control(Intent::Clarify), words).unwrap();
        assert!(ReadyReading::prepare(&consultation, device_anchor_for_obligation()).is_ok(), "{words}");
    }
}

#[test]
fn anchor_obligation_missing_original_fields_can_recover_only_the_original_directive() {
    let source = "Will I marry within a year? The fair is in Paris tomorrow. Use my earlier consultation in London on January 14, 2030 at 14:30.";
    let mut consultation = historical_case(source);
    let mut patch = control(Intent::Clarify);
    patch.updates = vec![
        Update {
            field: Field::ReaderPlace,
            value: "London".into(),
            quote: "London".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::QuestionTime,
            value: "2030-01-14T14:30".into(),
            quote: "January 14, 2030 at 14:30".into(),
            mode: UpdateMode::Supply,
        },
    ];
    validate_anchor_completion(&consultation, &patch, "Continue that earlier reading.").unwrap();
    consultation
        .apply(&patch, 2, "Continue that earlier reading.", false)
        .unwrap();
    assert!(
        matches!(&consultation.facts[&Field::ReaderPlace], Slot::Resolved { observation }
        if matches!(&observation.evidence, Evidence::RetainedQuestion { quote } if quote == "London"))
    );
    let mut bad = historical_case(source);
    patch.updates[0].value = "Paris".into();
    patch.updates[0].quote = "Paris".into();
    assert!(bad
        .apply(&patch, 2, "Continue that earlier reading.", false)
        .is_err());
}

#[test]
fn anchor_obligation_current_partial_instruction_reopens_unrelated_old_values() {
    let mut consultation = historical_case("Will I marry within a year?");
    consultation
        .facts
        .insert(Field::ReaderPlace, resolved("Woodbridge".into()));
    consultation
        .facts
        .insert(Field::QuestionTime, resolved("2026-10-07T12:00".into()));
    consultation
        .apply(
            &control(Intent::Clarify),
            2,
            "Use my earlier consultation.",
            false,
        )
        .unwrap();
    assert_eq!(consultation.text(Field::ReaderPlace), None);
    assert_eq!(consultation.text(Field::QuestionTime), None);
    assert!(ReadyReading::prepare(&consultation, device_anchor_for_obligation()).is_err());
}

#[test]
fn anchor_obligation_scoped_check_preserves_classifier_controls_and_frame_only_refinement() {
    use crate::horary_lessons::Stage;
    let consultation = historical_case(OMITTED_GAP_ANCHOR);
    let input = |phase| json!({"recognition_phase":phase,"consultation":consultation,"latest_words":OMITTED_GAP_ANCHOR});
    let mut focused = control(Intent::Clarify);
    focused.frame = Some(Frame {
        method: Method::Relationship,
        facet: Facet::Event,
    });
    let error = crate::horary_step::check(
        Stage::Intake,
        Matter::Other,
        &serde_json::to_value(&focused).unwrap(),
        &input("complete_selected_program"),
        &[],
    )
    .err()
    .expect("historical omission must be rejected");
    assert!(
        error.contains("historical chart-anchor information was omitted"),
        "{error}"
    );
    let classify = control(Intent::Read);
    crate::horary_step::check(
        Stage::Intake,
        Matter::Other,
        &serde_json::to_value(classify).unwrap(),
        &input("classify_question"),
        &[],
    )
    .unwrap();
    let mut pause_input = input("update_selected_program");
    pause_input["latest_words"] = json!("Pause.");
    crate::horary_step::check(
        Stage::Intake,
        Matter::Other,
        &serde_json::to_value(control(Intent::Pause)).unwrap(),
        &pause_input,
        &[],
    )
    .unwrap();
    let mut provisional = input("complete_selected_program");
    provisional["consultation"]["subject"] = json!({"state":"missing"});
    focused.frame = Some(Frame {
        method: Method::NewJob,
        facet: Facet::Event,
    });
    crate::horary_step::check(
        Stage::Intake,
        Matter::Other,
        &serde_json::to_value(focused).unwrap(),
        &provisional,
        &[],
    )
    .unwrap();
}

#[test]
fn anchor_obligation_use_device_cancels_only_retained_place_even_without_change_history() {
    let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
    let patch = control(Intent::Clarify);
    consultation
        .apply(&patch, 1, OMITTED_GAP_ANCHOR, false)
        .unwrap();
    consultation
        .apply(
            &control(Intent::UseDevice),
            2,
            "Use the device's location instead.",
            false,
        )
        .unwrap();
    let snapshot = consultation.recognition_snapshot();
    assert!(snapshot.changes.is_empty());
    let saved: Consultation =
        serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
    assert!(saved.device_reader_place);
    let plan = saved.plan(Some(&device_anchor_for_obligation()));
    assert!(!plan
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::Field(Field::ReaderPlace)));
    assert!(plan
        .needs
        .iter()
        .any(|need| need.key == RequirementKey::Field(Field::QuestionTime)));
    let error =
        validate_anchor_completion(&saved, &control(Intent::Clarify), "Continue.").unwrap_err();
    assert!(!error.contains("reader_place,"), "{error}");
    assert!(error.contains("question_time"), "{error}");
    let mut recover_time = control(Intent::Clarify);
    recover_time.updates.push(Update {
        field: Field::QuestionTime,
        value: "2026-03-08T02:30".into(),
        quote: "March 8, 2026 at 2:30 AM".into(),
        mode: UpdateMode::Supply,
    });
    let mut continued = saved;
    continued
        .apply(&recover_time, 3, "Continue.", false)
        .unwrap();
    assert_eq!(
        continued.text(Field::QuestionTime),
        Some("2026-03-08T02:30")
    );
    validate_anchor_completion(&continued, &control(Intent::Clarify), "Continue.").unwrap();
    let mut bad_place = control(Intent::Clarify);
    bad_place.updates.push(Update {
        field: Field::ReaderPlace,
        value: "New York City, United States".into(),
        quote: "New York City, United States".into(),
        mode: UpdateMode::Supply,
    });
    assert!(continued.apply(&bad_place, 4, "Continue.", false).is_err());
    let mut choose_new_place = control(Intent::Clarify);
    choose_new_place.updates.push(Update {
        field: Field::ReaderPlace,
        value: "London".into(),
        quote: "London".into(),
        mode: UpdateMode::Supply,
    });
    continued
        .apply(&choose_new_place, 4, "I'm asking from London.", false)
        .unwrap();
    assert!(!continued.device_reader_place);
}

#[test]
fn anchor_obligation_later_corrections_and_native_place_binding_outvote_old_source() {
    let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
    let mut supplied = control(Intent::Clarify);
    supplied.updates = vec![
        Update {
            field: Field::ReaderPlace,
            value: "New York City, United States".into(),
            quote: "New York City, United States".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::QuestionTime,
            value: "2026-03-08T02:30".into(),
            quote: "March 8, 2026 at 2:30 AM".into(),
            mode: UpdateMode::Supply,
        },
    ];
    consultation
        .apply(&supplied, 1, OMITTED_GAP_ANCHOR, false)
        .unwrap();
    assert!(
        consultation.additional.is_empty(),
        "Supplied historical fields need no new inquiries"
    );
    let mut correction = control(Intent::Correct);
    correction.updates.push(Update {
        field: Field::QuestionTime,
        value: "2026-03-08T03:30".into(),
        quote: "3:30 AM".into(),
        mode: UpdateMode::Correct,
    });
    consultation
        .apply(&correction, 3, "I meant 3:30 AM for that chart.", false)
        .unwrap();
    let slot = consultation.facts.get_mut(&Field::ReaderPlace).unwrap();
    if let Slot::Resolved { observation } = slot {
        observation.evidence = Evidence::NativePlace {
            query: "New York City, United States".into(),
            candidate_id: "new-york-native".into(),
            context: "Native resolved reader coordinates".into(),
            original: Box::new(observation.evidence.clone()),
        };
    }
    validate_anchor_completion(&consultation, &control(Intent::Clarify), "Continue.").unwrap();
    consultation
        .apply(&control(Intent::Clarify), 4, "Continue.", false)
        .unwrap();
    assert_eq!(
        consultation.text(Field::QuestionTime),
        Some("2026-03-08T03:30")
    );
    let plan = consultation.plan(Some(&device_anchor_for_obligation()));
    assert!(!plan.needs.iter().any(|need| matches!(
        need.key,
        RequirementKey::Field(Field::ReaderPlace | Field::QuestionTime)
    )));
    let actual_time =
        horary_ai_core::chart_input::resolve_chart_time("2026-03-08T03:30", "America/New_York", "")
            .unwrap();
    assert_eq!(actual_time, 1772955000000.);
}

#[test]
fn anchor_obligation_authorized_current_time_correction_can_fill_an_omitted_original() {
    let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
    let mut original_place = control(Intent::Clarify);
    original_place.updates.push(Update {
        field: Field::ReaderPlace,
        value: "New York City, United States".into(),
        quote: "New York City, United States".into(),
        mode: UpdateMode::Supply,
    });
    consultation
        .apply(&original_place, 1, OMITTED_GAP_ANCHOR, false)
        .unwrap();
    assert_eq!(consultation.text(Field::QuestionTime), None);
    let words = "I meant March 8, 2026 at 3:30 AM for that chart.";
    let mut correction = control(Intent::Correct);
    correction.updates.push(Update {
        field: Field::QuestionTime,
        value: "2026-03-08T03:30".into(),
        quote: "March 8, 2026 at 3:30 AM".into(),
        mode: UpdateMode::Correct,
    });
    validate_anchor_completion(&consultation, &correction, words).unwrap();
    consultation.apply(&correction, 2, words, false).unwrap();
    assert_eq!(
        consultation.text(Field::QuestionTime),
        Some("2026-03-08T03:30")
    );
    assert!(
        matches!(&consultation.facts[&Field::QuestionTime],Slot::Resolved {observation}
        if matches!(&observation.evidence,Evidence::User {quote,..} if quote == "March 8, 2026 at 3:30 AM"))
    );
    validate_anchor_completion(&consultation, &control(Intent::Clarify), "Continue.").unwrap();
}

#[test]
fn anchor_obligation_current_answers_bind_only_the_respective_pending_anchor() {
    for key in [
        RequirementKey::Field(Field::QuestionTime),
        RequirementKey::ChartMoment,
    ] {
        let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
        consultation.facts.insert(
            Field::ReaderPlace,
            resolved("New York City, United States".into()),
        );
        consultation.requested = Some(key);
        let words = "That question became clear on March 8, 2026 at 3:30 AM.";
        let mut answer = control(Intent::Clarify);
        answer.updates.push(Update {
            field: Field::QuestionTime,
            value: "2026-03-08T03:30".into(),
            quote: "March 8, 2026 at 3:30 AM".into(),
            mode: UpdateMode::Supply,
        });
        validate_anchor_completion(&consultation, &answer, words).unwrap();
        consultation.apply(&answer, 2, words, false).unwrap();
        assert_eq!(
            consultation.text(Field::QuestionTime),
            Some("2026-03-08T03:30")
        );
        let mut wrong = historical_case(OMITTED_GAP_ANCHOR);
        wrong.facts.insert(
            Field::ReaderPlace,
            resolved("New York City, United States".into()),
        );
        wrong.requested = Some(RequirementKey::ChartPlace);
        assert!(validate_anchor_completion(&wrong, &answer, words).is_err());
    }
    for key in [
        RequirementKey::Field(Field::ReaderPlace),
        RequirementKey::ChartPlace,
    ] {
        let mut consultation = historical_case(OMITTED_GAP_ANCHOR);
        consultation
            .facts
            .insert(Field::QuestionTime, resolved("2026-03-08T03:30".into()));
        consultation.requested = Some(key);
        let mut answer = control(Intent::Clarify);
        answer.updates.push(Update {
            field: Field::ReaderPlace,
            value: "London".into(),
            quote: "London".into(),
            mode: UpdateMode::Supply,
        });
        validate_anchor_completion(&consultation, &answer, "I was in London for that question.")
            .unwrap();
        consultation
            .apply(&answer, 2, "I was in London for that question.", false)
            .unwrap();
        assert_eq!(consultation.text(Field::ReaderPlace), Some("London"));
    }
}

#[test]
fn anchor_obligation_temporal_tail_does_not_claim_a_supplied_reader_place() {
    for words in [
        "Will I marry within a year? Use my earlier consultation from yesterday.",
        "Will I marry within a year? Use my earlier consultation from yesterday at 3:30 PM.",
        "Will I marry within a year? Use my earlier consultation in November 2026.",
    ] {
        let mut consultation = historical_case(words);
        validate_anchor_completion(&consultation, &control(Intent::Clarify), words).unwrap();
        consultation
            .apply(&control(Intent::Clarify), 2, words, false)
            .unwrap();
        let plan = consultation.plan(Some(&device_anchor_for_obligation()));
        for field in [Field::ReaderPlace, Field::QuestionTime] {
            assert!(plan
                .needs
                .iter()
                .any(|need| need.key == RequirementKey::Field(field)));
        }
        assert!(ReadyReading::prepare(&consultation, device_anchor_for_obligation()).is_err());
    }
}
