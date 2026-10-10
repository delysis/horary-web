//! Independently authored deal input cases. Gold stays on the evaluation side.
//!
//! Existing catalogue loaders execute the sibling JSON banks. These offline
//! checks seal their source meanings before any hosted optimization: action is
//! not goal, actor is not title owner, and a genuine missing fact differs from a
//! reviewed method boundary. Private validation is deliberately not included.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use crate::reading_contracts::{self, Facet, Field, Method, RequirementKey};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};

    const CASES: &str =
        include_str!("../test-fixtures/elicitation/deal-action-training-20261010.json");
    const RUBRICS: &str =
        include_str!("../test-fixtures/readings/deal-action-training-rubrics-20261010.json");
    const CASE_SHA256: &str = "e2eb558c27ae8aa8f67c4f406296d3f6b97808689e2812e9afd891be1c2e26b9";
    const RUBRIC_SHA256: &str = "4952065bb91ab40ed524355ac6e07a02e3af89da2c37da3555bf27a122ac42ae";

    fn cases() -> Vec<Value> {
        serde_json::from_str(CASES).expect("Author-created case array is JSON")
    }

    fn fact<'a>(expected: &'a Value, field: &str) -> Option<&'a str> {
        expected["facts"].as_array()?.iter().find_map(|fact| {
            (fact["field"].as_str() == Some(field))
                .then(|| fact["contains"].as_str())
                .flatten()
        })
    }

    fn need(expected: &Value, key: RequirementKey) -> bool {
        expected["needs"]
            .as_array()
            .expect("Every authored expectation has explicit needs")
            .iter()
            .any(|value| {
                serde_json::from_value::<RequirementKey>(value.clone())
                    .is_ok_and(|actual| actual == key)
            })
    }

    fn expected_states(case: &Value) -> Vec<&Value> {
        let mut states = vec![&case["expected"]];
        if !case["follow_up_expected"].is_null() {
            states.push(&case["follow_up_expected"]);
        }
        states
    }

    #[test]
    fn independently_authored_gold_is_sealed_and_balanced() {
        assert_eq!(
            format!("{:x}", Sha256::digest(CASES.as_bytes())),
            CASE_SHA256
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(RUBRICS.as_bytes())),
            RUBRIC_SHA256
        );
        let cases = cases();
        assert_eq!(cases.len(), 24);
        let mut ids = BTreeSet::new();
        let mut counts = BTreeMap::new();
        for case in &cases {
            let id = case["id"].as_str().expect("Case ID");
            assert!(id.starts_with("deal10-") && ids.insert(id));
            let method: Method = serde_json::from_value(case["method"].clone()).unwrap();
            assert!(matches!(
                method,
                Method::MovableDeal | Method::Property | Method::Rental | Method::BusinessProperty
            ));
            let mode = case["mode"].as_str().expect("Case mode");
            *counts.entry((method.name(), mode)).or_insert(0) += 1;
            assert!(!case["words"].as_str().unwrap().contains("deal10v-"));
            assert!(case["source_pages"].as_str().is_some_and(|s| !s.is_empty()));
        }
        for method in ["movable_deal", "property", "rental", "business_property"] {
            for mode in ["explicit", "implicit", "missing"] {
                assert_eq!(counts.get(&(method, mode)), Some(&2));
            }
        }
        // Private validation is never compiled into a runtime bank or writer.
        assert!(!CASES.contains("authored_reserved_validation"));
        assert!(!RUBRICS.contains("deal10v-"));
    }

    #[test]
    fn every_expected_action_is_native_canonical_and_actor_scoped() {
        assert_eq!(
            reading_contracts::allowed_values(Field::DealCapacity),
            &["buy", "sell", "rent"]
        );
        let mut profit_actions = BTreeSet::new();
        for case in cases() {
            for expected in expected_states(&case) {
                let facet: Facet = serde_json::from_value(expected["facet"].clone()).unwrap();
                for actual in expected["facts"].as_array().unwrap() {
                    let field: Field = serde_json::from_value(actual["field"].clone()).unwrap();
                    let value = actual["contains"].as_str().unwrap();
                    let allowed = reading_contracts::allowed_values(field);
                    if !allowed.is_empty() {
                        assert!(
                            allowed.contains(&value),
                            "{} asserts noncanonical {}={value}",
                            case["id"],
                            field.name()
                        );
                    }
                }
                if let Some(action) = fact(expected, "deal_capacity") {
                    assert!(["buy", "sell", "rent"].contains(&action));
                    if facet == Facet::Profit {
                        profit_actions.insert(action.to_owned());
                    }
                } else if case["method"] != "business_property" {
                    assert!(need(expected, RequirementKey::Field(Field::DealCapacity)));
                }
                if case["method"] == "business_property" {
                    assert!(
                        !need(expected, RequirementKey::Field(Field::DealCapacity)),
                        "The available work-property program does not depend on buy versus rent"
                    );
                }
                if fact(expected, "deal_actor").is_none() {
                    assert!(need(expected, RequirementKey::Field(Field::DealActor)));
                }
                if facet == Facet::Profit {
                    if let Some(beneficiary) = fact(expected, "deal_beneficiary") {
                        if beneficiary != "querent" {
                            let words = format!(
                                "{} {}",
                                case["words"].as_str().unwrap(),
                                case["follow_up"].as_str().unwrap_or_default()
                            );
                            assert!(words.contains(beneficiary));
                            assert!(expected["relationships"]
                                .get(beneficiary.to_lowercase())
                                .is_some());
                        }
                    } else {
                        assert!(need(
                            expected,
                            RequirementKey::Field(Field::DealBeneficiary)
                        ));
                    }
                } else {
                    assert!(fact(expected, "deal_beneficiary").is_none());
                    assert!(!need(
                        expected,
                        RequirementKey::Field(Field::DealBeneficiary)
                    ));
                }
                assert!(expected["owner"].as_str().is_some());
            }
        }
        assert_eq!(
            profit_actions.len(),
            3,
            "Profit is tested with all three actions"
        );
    }

    #[test]
    fn missing_facts_do_not_hide_capability_limits_or_title_guesses() {
        let cases = cases();
        for case in &cases {
            let expected = &case["expected"];
            let missing = case["mode"] == "missing";
            assert_eq!(!expected["needs"].as_array().unwrap().is_empty(), missing);
            if missing {
                assert!(case["follow_up"].as_str().is_some_and(|s| !s.is_empty()));
                let after = &case["follow_up_expected"];
                assert!(after["needs"].as_array().unwrap().is_empty());
                assert_eq!(
                    after["ready"].as_bool(),
                    Some(case["method"] != "business_property")
                );
            } else {
                assert!(case["follow_up"].is_null());
                assert_eq!(
                    expected["ready"].as_bool(),
                    Some(case["method"] != "business_property")
                );
            }
            if need(expected, RequirementKey::Owner) {
                assert!(!matches!(
                    expected["facet"].as_str(),
                    Some("event" | "timing")
                ));
                assert_eq!(expected["owner"], "");
            }
        }
        for id in [
            "deal10-movable-implicit-buy-quality",
            "deal10-rental-implicit-lease-home",
            "deal10-business-implicit-studio-lease",
            "deal10-business-implicit-shop-offer",
        ] {
            let case = cases.iter().find(|c| c["id"] == id).unwrap();
            assert_eq!(case["expected"]["owner"], "");
            assert_eq!(fact(&case["expected"], "deal_actor"), Some("querent"));
            assert!(!need(&case["expected"], RequirementKey::Owner));
        }
    }

    #[test]
    fn actual_contracting_side_survives_owner_and_intermediary_distinctions() {
        let cases = cases();
        let agent = cases
            .iter()
            .find(|c| c["id"] == "deal10-movable-implicit-sale-agent")
            .unwrap();
        assert_eq!(fact(&agent["expected"], "deal_actor"), Some("Felix"));
        assert_eq!(agent["expected"]["owner"], "Felix");
        assert!(agent["words"]
            .as_str()
            .unwrap()
            .contains("I am not the contracting seller"));
        for id in [
            "deal10-movable-explicit-buy-relative",
            "deal10-property-explicit-buy-profit",
            "deal10-rental-explicit-tenant",
            "deal10-business-explicit-rent-profit",
        ] {
            let case = cases.iter().find(|c| c["id"] == id).unwrap();
            let expected = &case["expected"];
            assert_eq!(fact(expected, "deal_actor"), Some("querent"));
            assert_ne!(expected["owner"], "querent");
            assert_eq!(fact(expected, "deal_party"), expected["owner"].as_str());
        }
    }

    #[test]
    fn venue_and_relative_date_are_separate_from_native_reader_anchor() {
        for case in cases() {
            for expected in expected_states(&case) {
                if fact(expected, "event_time") == Some("2026-10-09") {
                    assert!(fact(expected, "question_time").is_none());
                    assert!(expected["chart_local_time"].is_null());
                }
                if expected["ready"] == true {
                    assert_eq!(expected["chart_place_contains"], "Woodbridge");
                }
            }
        }
        let no_device = cases()
            .into_iter()
            .find(|c| c["id"] == "deal10-rental-missing-reader-place")
            .unwrap();
        assert_eq!(no_device["device_available"], false);
        assert!(need(&no_device["expected"], RequirementKey::ChartPlace));
        assert_eq!(
            fact(&no_device["expected"], "event_place"),
            Some("Cambridge")
        );
        assert!(fact(&no_device["expected"], "reader_place").is_none());
        assert_eq!(
            fact(&no_device["follow_up_expected"], "reader_place"),
            Some("Woodbridge")
        );
    }

    #[test]
    fn matching_source_rubrics_keep_input_and_interpretation_gates_separate() {
        let rows: Vec<crate::reading_eval::Rubric> =
            serde_json::from_str(RUBRICS).expect("Strict current native rubric type");
        assert_eq!(rows.len(), 24);
        let cases = cases();
        for row in rows {
            let case = cases.iter().find(|case| case["id"] == row.case_id).unwrap();
            assert_eq!(
                serde_json::to_value(row.declared_method).unwrap(),
                case["method"]
            );
            assert_eq!(row.mode, case["mode"].as_str().unwrap());
            let ids: BTreeSet<_> = row
                .decisive_tests
                .iter()
                .map(|test| test.id.as_str())
                .collect();
            for expected in [
                "classify.actual_matter",
                "extract.action_not_goal",
                "extract.actor_title_counterparty",
                "elicit.only_real_gap",
                "anchor.reader_not_venue",
                "handoff.source_complete",
                "read.question_answer",
            ] {
                assert!(ids.contains(expected), "{} lacks {expected}", row.case_id);
            }
            assert_eq!(
                row.support.contract_status == "expert_review",
                row.declared_method == Method::BusinessProperty
            );
            if row.declared_method == Method::BusinessProperty {
                assert!(ids.contains("read.business_review_boundary"));
            }
            let inquiry = row
                .decisive_tests
                .iter()
                .find(|test| test.id == "elicit.only_real_gap")
                .unwrap();
            if case["mode"] == "missing" {
                assert!(inquiry.required_evidence.contains("authored real gap"));
            } else {
                assert!(inquiry.required_evidence.contains("absence of a redundant"));
                assert!(inquiry
                    .required_evidence
                    .contains("no inquiry or supplying turn"));
            }
            if row.declared_method == Method::MovableDeal && row.facet == Facet::Profit {
                assert!(ids.contains("read.incoming_money_not_goods"));
                assert!(row
                    .required_roles
                    .iter()
                    .any(|role| role.role == "Incoming money from buyers/customers"));
                assert!(row.source.printed_pages.contains("156, 158"));
            }
            if row.facet == Facet::Profit {
                assert!(ids.contains("extract.beneficiary_not_actor_or_title"));
                assert!(row
                    .context_requirements
                    .iter()
                    .any(|requirement| requirement.field == "deal_beneficiary"));
                assert!(row
                    .required_roles
                    .iter()
                    .any(|role| role.role == "Actual profit beneficiary"));
            }
            assert!(row.forbidden_inferences.iter().any(|s| s.contains("title")));
            assert!(row
                .support
                .qualification
                .contains("independent source-correct interpretation"));
        }
    }
}
