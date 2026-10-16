//! Synthetic fresh application journeys; no imported accepted state or model.
use super::*;
use crate::horary_pipeline::Runtime;

struct Journey {
    directory: tempfile::TempDir,
    proposal: Turn,
    calls: std::sync::Mutex<Vec<(Stage, Value)>>,
}

impl Runtime for Journey {
    fn device_location(&self) -> Result<Option<crate::geocode::LocationCandidate>, String> {
        Ok(Some(crate::geocode::LocationCandidate {
            id: "device".into(),
            label: "Synthetic device place".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        }))
    }
    fn generate(
        &self,
        stage: Stage,
        _matter: Matter,
        input: &Value,
        _schema: &Value,
        _audio: Option<&[u8]>,
    ) -> Result<crate::native_llama_worker::NativeGenerationResult, String> {
        let input = crate::horary_step::original_input(input);
        self.calls.lock().unwrap().push((stage, input.clone()));
        let value = match stage {
            Stage::Intake => {
                let mut proposal = self.proposal.clone();
                if input["recognition_phase"] == "classify_question" {
                    proposal.subject = None;
                    proposal.people.clear();
                    proposal.updates.clear();
                } else {
                    proposal.intent = Intent::Clarify;
                    proposal.question = None;
                    proposal.frame = None;
                }
                json!(proposal)
            }
            Stage::Place => {
                json!({"mode":"select","place_id":"device","query":"","clarification":"","basis":"Actual synthetic device candidate"})
            }
            Stage::Significators => {
                let options: Options = serde_json::from_value(input["native_role_options"].clone())
                    .map_err(|e| e.to_string())?;
                let facts: Vec<Fact> =
                    serde_json::from_value(input["house_rulers_and_positions"].clone())
                        .map_err(|e| e.to_string())?;
                let moon_claimed = options.choices.iter().filter_map(|c| c.house).any(|house| {
                    facts.iter().any(|f| {
                        f.kind == "house"
                            && f.label == format!("House {house}")
                            && f.planets.iter().any(|p| p == "Moon")
                    })
                });
                let selections: Vec<_> = options.choices.iter().filter(|choice| choice.id != "moon.contextual" || !moon_claimed).map(|choice| json!({"id":choice.id,"reason":"Synthetic selection from actual native options"})).collect();
                json!({"selections":selections,"summary":"Synthetic accepted role handoff; no model-quality claim","unknowns":[]})
            }
            Stage::Conversation => {
                return Err("Unexpected conversational detour in a complete synthetic input".into())
            }
            _ => {
                let mut value = stage_worksheet(stage);
                if stage == Stage::Condition {
                    for (facet, key) in [
                        ("essential", "own_dignity"),
                        ("house_capacity", "ability_to_act"),
                    ] {
                        value["checks"][key]["evidence"] = json!(input["facts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|f| f["condition_facet"] == facet)
                            .map(|f| f["id"].clone())
                            .collect::<Vec<_>>());
                    }
                    value["checks"]["context_exceptions"]["evidence"] = json!(input["facts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|f| matches!(
                            f["condition_facet"].as_str(),
                            Some("retrograde" | "solar_opposition")
                        ))
                        .map(|f| f["id"].clone())
                        .collect::<Vec<_>>());
                }
                if stage == Stage::Contacts {
                    value["basis"] = json!("complex_unverified");
                }
                if stage == Stage::Reception {
                    value["edge_interpretations"] =
                        crate::horary_pipeline::reception_receipt_fixture(input);
                }
                if stage == Stage::Judgment {
                    value["verdict"] = json!("unresolved");
                    let boundary = input["facts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|f| f["kind"] == "boundary")
                        .ok_or("Actual native boundary missing")?;
                    value["checks"]["scope_of_answer"]["evidence"] = json!([boundary["id"]]);
                }
                value
            }
        };
        Ok(crate::native_llama_worker::NativeGenerationResult {
            content: value.to_string(),
            prompt_tokens: 0,
            generated_tokens: 0,
            elapsed_ms: 0,
            total_wall_ms: None,
            batch_size: None,
            lesson_bank_hit: None,
            lesson_prepare_ms: None,
            tokens_per_second: 0.,
            prompt_cache_hit: false,
            cached_prompt_tokens: 0,
            prefilled_prompt_tokens: 0,
            first_token_ms: None,
            cold_cache_bytes: None,
        })
    }
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<crate::native_llama_worker::NativeGenerationResult>, String> {
        tasks
            .iter()
            .map(|(stage, matter, input, schema)| {
                self.generate(*stage, *matter, input, schema, None)
            })
            .collect()
    }
    fn publish(&self, _session: &mut crate::conversation::Session) -> Result<(), String> {
        Ok(())
    }
    fn check(&self) -> Result<(), String> {
        if self.calls.lock().unwrap().len() > 20 {
            Err("Synthetic journey exceeded native repair limit".into())
        } else {
            Ok(())
        }
    }
    fn directory(&self) -> &std::path::Path {
        self.directory.path()
    }
}

#[test]
fn fresh_native_message_journeys_reach_actual_bound_judgment_without_state_import() {
    for authored in [
        parcel(),
        outgoing(),
        visit("friend", false),
        visit("unknown", true),
    ] {
        // Only synthetic original words and mock proposals enter the fresh
        // session. No accepted state, expected roles or chart is imported.
        let words = authored.question.resolved().unwrap().clone();
        let runtime = Journey {
            directory: tempfile::tempdir().unwrap(),
            proposal: authored.changes[0].proposal.clone(),
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let mut session = crate::conversation::Session::default();
        session.messages.push(crate::conversation::Message {
            role: "user".into(),
            text: words.clone(),
        });
        let previous = session.clone();
        crate::horary_pipeline::run(
            &mut session,
            &runtime,
            &crate::geocode::GeocodeState::default(),
            anchor().timestamp_ms,
            None,
            &previous,
        )
        .unwrap();
        let Some(crate::reading_contracts::ReadingResult::Judgment {
            binding, verdict, ..
        }) = &session.method.result
        else {
            panic!("Actual native judgment handoff missing")
        };
        assert_eq!(binding.question, words);
        assert_eq!(binding.frame.method, authored.method().unwrap());
        assert_eq!(verdict, "unresolved");
        assert!(session.method.flow.pending.is_empty());
        let calls = runtime.calls.lock().unwrap();
        for stage in [Stage::Contacts, Stage::Judgment] {
            let (_, input) = calls.iter().find(|(actual, _)| *actual == stage).unwrap();
            assert_eq!(
                input["native_arrival"]["reading_input_sha256"],
                binding.input_sha256
            );
            assert_eq!(
                input["native_arrival"]["method"],
                json!(binding.frame.method)
            );
        }
        assert!(!calls.iter().any(|(stage, _)| *stage == Stage::Conversation));
        assert!(session
            .method
            .flow
            .jobs
            .iter()
            .any(|job| job.stage == Stage::Judgment
                && matches!(job.phase(), crate::horary_step::Phase::Complete { .. })));
    }
}
