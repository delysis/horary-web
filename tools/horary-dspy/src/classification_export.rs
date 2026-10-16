//! Source-qualified, authored classification teaching; never a state migration.
//! Producer provenance and the target native contract are separate identities.
use crate::{
    book,
    native_export::{Adapter, Export, PORT_REVISION, exact_keys, signature_digest},
    signatures,
};
use anyhow::{Context, Result, ensure};
use dsrust::module::{Metadata, ProgramState, SignatureState};
use horary_prompt_program::{Evidence, Override, Program, Signature as NativeSignature, digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SOURCE_MANIFEST: &[u8] = include_bytes!("../../../docs/llm-process/source-manifest.json");
const NATIVE_CONTRACT: &[u8] = include_bytes!("../../../src-tauri/src/reading_contracts.rs");

/// The explicit null is required. A family, omitted field or wildcard phase is
/// not an authored classification target and must not silently be substituted.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub stage: String,
    pub recognition_phase: String,
    pub method: (),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub id: String,
    pub compiled_sha256: String,
    pub producer_source_receipt_sha256: String,
    pub port_revision: String,
    pub signatures_sha256: String,
    pub book_sha256: String,
    pub baseline_manifest_sha256: String,
    pub native_contract_sha256: String,
    pub native_target: Target,
    pub classification: Adapter,
    pub training_case_ids: Vec<String>,
    pub holdout_case_ids: Vec<String>,
    pub evidence: Vec<Evidence>,
}

/// This is the actual version-1 component-source.json emitted by the pinned
/// producer. Other teaching implementations remain provenance, not a demand
/// that an authored native translation use the same optimizer executable.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Producer {
    version: u32,
    method: String,
    function: String,
    predictor_name: String,
    port_revision: String,
    signatures_sha256: String,
    declaration_sha256: String,
    catalogue_sha256: String,
    functions_sha256: String,
    optimizer_source_sha256: String,
    book_sha256: String,
    teaching_sources_sha256: BTreeMap<String, String>,
    compiled_sha256: String,
    dataset_sha256: String,
}

fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn requirements() -> Result<Value> {
    Ok(json!({
        "plan_version":1,"predictor":"classification","producer_scope":{"version":1,"method":"all","function":"classification"},
        "native_target":{"stage":"intake","recognition_phase":"classify_question","method":null},
        "port_revision":PORT_REVISION,"signatures_sha256":digest(include_bytes!("signatures.rs")),
        "classification_signature_sha256":signature_digest(&signatures::auxiliary("classification")?)?,
        "book_sha256":book::OCR_SHA256,"baseline_manifest_sha256":digest(SOURCE_MANIFEST),
        "native_contract_sha256":digest(NATIVE_CONTRACT),
        "authored_adapter_required":true,"demos_importable":false,
        "limits":"Producer source receipt and target native guide are separately bound. This is an uninstalled new teaching seed, not semantic qualification or admission into a different DSPy queue."
    }))
}

fn producer(bytes: &[u8], compiled: &[u8], plan: &Plan) -> Result<Producer> {
    ensure!(
        digest(bytes) == plan.producer_source_receipt_sha256,
        "Producer source receipt hash differs"
    );
    let producer: Producer =
        serde_json::from_slice(bytes).context("Invalid producer source receipt")?;
    ensure!(
        producer.version == 1
            && producer.method == "all"
            && producer.function == "classification"
            && producer.predictor_name == "classification",
        "Producer is not the exact mixed-family classification component"
    );
    ensure!(
        producer.port_revision == PORT_REVISION
            && producer.signatures_sha256 == digest(include_bytes!("signatures.rs"))
            && producer.declaration_sha256
                == signature_digest(&signatures::auxiliary("classification")?)?,
        "Producer classification declaration or pinned port differs"
    );
    ensure!(
        producer.compiled_sha256 == digest(compiled) && producer.book_sha256 == book::OCR_SHA256,
        "Producer compiled bytes or primary book differs"
    );
    ensure!(
        producer.catalogue_sha256 == digest(include_bytes!("catalogue.rs"))
            && producer.functions_sha256 == digest(include_bytes!("functions.rs")),
        "Producer classification catalogue or function contract differs"
    );
    for (name, bytes) in [
        (
            "coverage.json",
            include_bytes!("../coverage.json").as_slice(),
        ),
        (
            "reading-catalogue.json",
            include_bytes!("../../../docs/llm-process/reading-catalogue.json").as_slice(),
        ),
    ] {
        ensure!(
            producer.teaching_sources_sha256.get(name) == Some(&digest(bytes)),
            "Producer taxonomy source {name} differs"
        );
    }
    ensure!(
        sha(&producer.optimizer_source_sha256)
            && sha(&producer.dataset_sha256)
            && !producer.teaching_sources_sha256.is_empty()
            && producer
                .teaching_sources_sha256
                .values()
                .all(|value| sha(value)),
        "Producer has invalid source or dataset identities"
    );
    Ok(producer)
}

fn compiled_state(bytes: &[u8]) -> Result<ProgramState> {
    let value: Value =
        serde_json::from_slice(bytes).context("Invalid saved classification JSON")?;
    exact_keys(
        &value,
        &["metadata", "classification"],
        "Saved classification",
    )?;
    exact_keys(
        &value["metadata"],
        &["dependency_versions"],
        "DSPy metadata",
    )?;
    exact_keys(
        &value["metadata"]["dependency_versions"],
        &["dspy"],
        "DSPy versions",
    )?;
    let saved = &value["classification"];
    exact_keys(
        saved,
        &["traces", "train", "demos", "signature", "lm"],
        "Classification predictor",
    )?;
    exact_keys(
        &saved["signature"],
        &["instructions", "fields"],
        "Classification signature",
    )?;
    for field in saved["signature"]["fields"]
        .as_array()
        .context("No ordered classification fields")?
    {
        exact_keys(field, &["prefix", "description"], "Classification field")?;
    }
    let state: ProgramState = serde_json::from_value(value)?;
    let saved = state
        .get("classification")
        .context("No classification predictor")?;
    let declaration = signatures::auxiliary("classification")?;
    ensure!(
        state.metadata == Metadata::default(),
        "Pinned DSPy metadata differs"
    );
    ensure!(
        saved.traces.is_empty() && saved.train.is_empty() && saved.lm.is_none(),
        "Classifier includes private runtime, training or model state"
    );
    ensure!(
        saved.signature.fields == SignatureState::of(&declaration).fields
            && !saved.signature.instructions.trim().is_empty(),
        "Classification field contract or learned instructions differ"
    );
    let permitted = declaration
        .inputs
        .iter()
        .map(|field| field.name.as_str())
        .chain(declaration.outputs.iter().map(|field| field.name.as_str()))
        .collect::<BTreeSet<_>>();
    for demo in &saved.demos {
        ensure!(
            demo.keys().all(|name| permitted.contains(name.as_str()))
                && demo.values().all(Value::is_string)
                && declaration
                    .outputs
                    .iter()
                    .all(|field| demo.contains_key(&field.name)),
            "Classification demo has private, unknown, mistyped or missing output fields"
        );
    }
    Ok(state)
}

/// Preserve the target guide verbatim and append explicitly authored teaching.
/// Foreign classification labels never mint native Turn/frame/person records.
pub fn export(
    compiled: &[u8],
    producer_source: &[u8],
    baseline_manifest: &[u8],
    plan: &Plan,
) -> Result<Export> {
    ensure!(
        plan.version == 1,
        "Unsupported classification export plan version"
    );
    ensure!(
        plan.native_target.stage == "intake"
            && plan.native_target.recognition_phase == "classify_question",
        "Classification export requires intake/classify_question/method:null"
    );
    ensure!(
        plan.compiled_sha256 == digest(compiled),
        "Compiled classification hash differs"
    );
    ensure!(
        plan.port_revision == PORT_REVISION
            && plan.signatures_sha256 == digest(include_bytes!("signatures.rs"))
            && plan.book_sha256 == book::OCR_SHA256,
        "Classification source, pinned port or primary book differs"
    );
    ensure!(
        plan.baseline_manifest_sha256 == digest(baseline_manifest)
            && plan.baseline_manifest_sha256 == digest(SOURCE_MANIFEST)
            && plan.native_contract_sha256 == digest(NATIVE_CONTRACT),
        "Target native baseline or classification contract drifted"
    );
    let producer = producer(producer_source, compiled, plan)?;
    let state = compiled_state(compiled)?;
    let saved = state.get("classification").context("No saved classifier")?;
    let adapter = &plan.classification;
    ensure!(
        adapter.signature_sha256 == signature_digest(&signatures::auxiliary("classification")?)?
            && adapter.learned_instructions_sha256 == digest(&saved.signature.instructions),
        "Authored adapter declaration or learned instruction identity differs"
    );
    ensure!(
        adapter.expected_guide_sha256 == digest(&adapter.native_guide),
        "Target classifier guide differs from its captured hash"
    );
    ensure!(
        !adapter.authorship.trim().is_empty()
            && adapter.authorship.len() <= 1024
            && adapter.authored_teaching.trim().len() >= 80
            && adapter.authored_teaching.len() <= 16_000,
        "Classification needs bounded explicit authored native-contract teaching"
    );
    let replacement = format!(
        "{}\n\n<authored_native_teaching>\n{}\n</authored_native_teaching>\n",
        adapter.native_guide, adapter.authored_teaching
    );
    let program = Program {
        version:1,id:plan.id.clone(),baseline_manifest_sha256:plan.baseline_manifest_sha256.clone(),
        overrides:vec![Override {stage:"intake".into(),recognition_phase:Some("classify_question".into()),method:None,
            expected_guide_sha256:adapter.expected_guide_sha256.clone(),replacement_text:replacement.clone(),edits:vec![]}],
        rationale:"New explicitly authored native classification teaching derived from separately pinned DSPy producer instructions. No foreign fields or demonstrations are converted into native records. Requires fresh native first-message and independent source/answer qualification before installation.".into(),
        evidence:plan.evidence.clone(),training_case_ids:plan.training_case_ids.clone(),holdout_case_ids:plan.holdout_case_ids.clone(),
    };
    program.validate().map_err(anyhow::Error::msg)?;
    program
        .apply(
            NativeSignature {
                stage: "intake",
                recognition_phase: Some("classify_question"),
                method: None,
            },
            &adapter.native_guide,
        )
        .map_err(anyhow::Error::msg)?
        .context("Native classification override did not apply")?;
    Ok(Export {
        receipt: json!({
            "kind":"dspy-explicit-native-classification-seed","version":1,"predictor":"classification",
            "compiled_sha256":plan.compiled_sha256,"producer_source_receipt_sha256":plan.producer_source_receipt_sha256,
            "producer_source":producer,"target_baseline_manifest_sha256":plan.baseline_manifest_sha256,"target_native_contract_sha256":plan.native_contract_sha256,
            "exporter_source_sha256":digest(include_bytes!("classification_export.rs")),
            "export_plan_canonical_sha256":digest(serde_json::to_vec(plan)?),"native_program_canonical_sha256":digest(serde_json::to_vec(&program)?),
            "native_target":plan.native_target,"signature_sha256":adapter.signature_sha256,
            "learned_instructions_sha256":adapter.learned_instructions_sha256,"learned_instructions":saved.signature.instructions,
            "authored_teaching_sha256":digest(&adapter.authored_teaching),"authorship":adapter.authorship,
            "baseline_guide_sha256":adapter.expected_guide_sha256,"native_seed_guide_sha256":digest(&replacement),
            "withheld_demonstrations":saved.demos.iter().enumerate().map(|(index,demo)|Ok(json!({"index":index,"sha256":digest(serde_json::to_vec(demo)?),"reason":"Foreign method/facet/branch/principal/clarification outputs do not establish native Turn, frame, source spans or person records."}))).collect::<Result<Vec<_>>>()?,
            "demonstrations_imported":0,"provider_calls":0,"new_dspy_queue_admission":false,
            "unchanged_compiled_winner":false,"application_installed":false,"classification_semantics_qualified":false,"source_correct_reading_qualified":false,
            "limits":"Producer provenance and target native adaptation are distinct. No source hashes, optimizer score or authored field mapping establishes native semantic correctness. Original compiled state and producer receipt stay external and hash-bound."
        }),
        program,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsrust::module::PredictorState;

    fn fixture() -> (Vec<u8>, Vec<u8>, Plan) {
        let signature = signatures::auxiliary("classification").unwrap();
        let mut state = ProgramState::new(BTreeMap::from([(
            "classification".into(),
            PredictorState::of(&signature, &[]),
        )]));
        state
            .predictors
            .get_mut("classification")
            .unwrap()
            .signature
            .instructions
            .push_str(" Synthetic learned instruction, not a qualified result.");
        let bytes = serde_json::to_vec(&state).unwrap();
        let guide = "Synthetic native classifier guide. Keep the supplied Turn schema and source authority unchanged.\n<book_extracts>\nSynthetic protected quotation.\n</book_extracts>\n";
        let producer = Producer {
            version: 1,
            method: "all".into(),
            function: "classification".into(),
            predictor_name: "classification".into(),
            port_revision: PORT_REVISION.into(),
            signatures_sha256: digest(include_bytes!("signatures.rs")),
            declaration_sha256: signature_digest(&signature).unwrap(),
            catalogue_sha256: digest(include_bytes!("catalogue.rs")),
            functions_sha256: digest(include_bytes!("functions.rs")),
            optimizer_source_sha256: digest("A distinct immutable producer implementation"),
            book_sha256: book::OCR_SHA256.into(),
            compiled_sha256: digest(&bytes),
            dataset_sha256: digest("Synthetic producer bank identity, not inspected"),
            teaching_sources_sha256: BTreeMap::from([
                (
                    "coverage.json".into(),
                    digest(include_bytes!("../coverage.json")),
                ),
                (
                    "reading-catalogue.json".into(),
                    digest(include_bytes!(
                        "../../../docs/llm-process/reading-catalogue.json"
                    )),
                ),
                (
                    "source-manifest.json".into(),
                    digest("Distinct historical producer manifest"),
                ),
            ]),
        };
        let source = serde_json::to_vec(&producer).unwrap();
        let plan = Plan {
            version:1,id:"synthetic-native-classification-seed".into(),compiled_sha256:digest(&bytes),
            producer_source_receipt_sha256:digest(&source),port_revision:PORT_REVISION.into(),
            signatures_sha256:digest(include_bytes!("signatures.rs")),book_sha256:book::OCR_SHA256.into(),
            baseline_manifest_sha256:digest(SOURCE_MANIFEST),native_contract_sha256:digest(NATIVE_CONTRACT),
            native_target:Target {stage:"intake".into(),recognition_phase:"classify_question".into(),method:()},
            classification:Adapter {
                signature_sha256:signature_digest(&signature).unwrap(),
                learned_instructions_sha256:digest(&state.get("classification").unwrap().signature.instructions),
                expected_guide_sha256:digest(guide),native_guide:guide.into(),
                authored_teaching:"Synthetic authored native adaptation: retain the supplied Turn schema and literal question; leave native facts, people, branch authority and follow-up extraction to their actual application stages. No foreign output records are imported.".into(),
                authorship:"Synthetic offline source author".into(),
            },
            training_case_ids:vec!["synthetic-training-id".into()],holdout_case_ids:vec!["synthetic-validation-id".into()],
            evidence:vec![Evidence {case_id:"synthetic-training-id".into(),file:"synthetic-source-review.json".into(),json_pointer:"/finding".into(),sha256:digest("Synthetic source review")}],
        };
        (bytes, source, plan)
    }

    fn changed_compiled(mutator: impl FnOnce(&mut Value)) -> (Vec<u8>, Vec<u8>, Plan) {
        let (compiled, source, mut plan) = fixture();
        let mut state: Value = serde_json::from_slice(&compiled).unwrap();
        mutator(&mut state);
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut producer: Producer = serde_json::from_slice(&source).unwrap();
        producer.compiled_sha256 = digest(&bytes);
        let source = serde_json::to_vec(&producer).unwrap();
        plan.compiled_sha256 = digest(&bytes);
        plan.producer_source_receipt_sha256 = digest(&source);
        (bytes, source, plan)
    }

    #[test]
    fn historical_producer_and_current_target_remain_separate_and_unqualified() {
        let (compiled, source, plan) = fixture();
        let result = export(&compiled, &source, SOURCE_MANIFEST, &plan).unwrap();
        assert_ne!(
            result.receipt["producer_source"]["optimizer_source_sha256"],
            digest(include_bytes!("optimize.rs"))
        );
        assert_ne!(
            result.receipt["producer_source"]["teaching_sources_sha256"]["source-manifest.json"],
            result.receipt["target_baseline_manifest_sha256"]
        );
        assert_eq!(
            result.receipt["producer_source_receipt_sha256"],
            digest(&source)
        );
        assert_eq!(result.receipt["native_target"]["method"], Value::Null);
        assert_eq!(result.receipt["application_installed"], false);
        assert_eq!(result.receipt["classification_semantics_qualified"], false);
        assert_eq!(result.receipt["new_dspy_queue_admission"], false);
        assert_eq!(result.program.overrides.len(), 1);
        assert!(result.program.overrides[0].method.is_none());
    }

    #[test]
    fn exact_classification_scope_preserves_quotes_baseline_and_other_stages() {
        let (compiled, source, plan) = fixture();
        let result = export(&compiled, &source, SOURCE_MANIFEST, &plan).unwrap();
        let guide = &plan.classification.native_guide;
        let (changed, applied) = result
            .program
            .apply(
                NativeSignature {
                    stage: "intake",
                    recognition_phase: Some("classify_question"),
                    method: None,
                },
                guide,
            )
            .unwrap()
            .unwrap();
        assert!(changed.starts_with(guide));
        assert_eq!(
            horary_prompt_program::guide_parts(guide)
                .unwrap()
                .quoted_source,
            horary_prompt_program::guide_parts(&changed)
                .unwrap()
                .quoted_source
        );
        assert_eq!(applied.original_guide_sha256, digest(guide));
        for (stage, phase, method) in [
            ("intake", Some("complete_selected_program"), Some("money")),
            (
                "intake",
                Some("update_selected_program"),
                Some("relationship"),
            ),
            ("intake", None, None),
            ("judgment", None, Some("money")),
            ("conversation", None, None),
            ("place", None, None),
            ("moment", None, None),
        ] {
            assert!(
                result
                    .program
                    .apply(
                        NativeSignature {
                            stage,
                            recognition_phase: phase,
                            method
                        },
                        guide
                    )
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            result
                .program
                .apply(
                    NativeSignature {
                        stage: "intake",
                        recognition_phase: Some("classify_question"),
                        method: None
                    },
                    "A drifted guide"
                )
                .is_err()
        );
    }

    #[test]
    fn applying_a_seed_changes_only_actual_system_teaching_not_user_schema_or_repair() {
        let (compiled, source, plan) = fixture();
        let result = export(&compiled, &source, SOURCE_MANIFEST, &plan).unwrap();
        let mut messages = json!([
            {"role":"system","content":plan.classification.native_guide},
            {"role":"user","content":{"input":{"latest_words":"Synthetic actual words"},"worksheet_contract":{"type":"object","required":["frame"]}}},
            {"role":"assistant","content":"Rejected synthetic proposal"},
            {"role":"user","content":{"original_input":{"latest_words":"Synthetic actual words"},"native_validation_error":"Original synthetic repair"}}
        ]);
        let before = messages.clone();
        horary_prompt_program::apply_messages(
            &result.program,
            NativeSignature {
                stage: "intake",
                recognition_phase: Some("classify_question"),
                method: None,
            },
            &mut messages,
        )
        .unwrap()
        .unwrap();
        assert_ne!(messages[0], before[0]);
        assert_eq!(
            messages.as_array().unwrap()[1..],
            before.as_array().unwrap()[1..]
        );
    }

    #[test]
    fn all_foreign_demos_are_hashed_and_withheld_never_native_records() {
        let (compiled, source, plan) = changed_compiled(|value| {
            value["classification"]["demos"] = json!([{"question":"FOREIGN SYNTHETIC DEMO","method":"money","facet":"event","branch":"synthetic","principal":"foreign principal","clarification":""}]);
        });
        let result = export(&compiled, &source, SOURCE_MANIFEST, &plan).unwrap();
        assert_eq!(
            result.receipt["withheld_demonstrations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(sha(result.receipt["withheld_demonstrations"][0]["sha256"]
            .as_str()
            .unwrap()));
        assert_eq!(result.receipt["demonstrations_imported"], 0);
        let program = serde_json::to_string(&result.program).unwrap();
        assert!(!program.contains("FOREIGN SYNTHETIC DEMO"));
        assert!(!program.contains("foreign principal"));
    }

    #[test]
    fn wrong_predictor_metadata_fields_runtime_and_private_demos_are_refused() {
        for change in 0..8 {
            let (compiled, source, plan) = changed_compiled(|value| match change {
                0 => {
                    value["reading.money"] = value["classification"].clone();
                }
                1 => {
                    let old = value
                        .as_object_mut()
                        .unwrap()
                        .remove("classification")
                        .unwrap();
                    value["classify_question"] = old;
                }
                2 => value["classification"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1),
                3 => value["classification"]["lm"] = json!({"runtime":"PRIVATE"}),
                4 => value["classification"]["train"] = json!([{"private":"PRIVATE"}]),
                5 => value["classification"]["demos"] = json!([{"expected":"PRIVATE GOLD"}]),
                6 => value["metadata"]["dependency_versions"]["dspy"] = json!("foreign-port"),
                _ => {
                    value["classification"]["signature"]["unknown_field"] =
                        json!("Silent substitution")
                }
            });
            assert!(
                export(&compiled, &source, SOURCE_MANIFEST, &plan).is_err(),
                "change {change}"
            );
        }
    }

    #[test]
    fn producer_declaration_book_taxonomy_scope_and_receipt_identity_are_refused_on_drift() {
        for change in 0..9 {
            let (compiled, source, mut plan) = fixture();
            let mut producer: Value = serde_json::from_slice(&source).unwrap();
            match change {
                0 => producer["method"] = json!("money"),
                1 => producer["function"] = json!("reading"),
                2 => producer["declaration_sha256"] = json!(digest("different declaration")),
                3 => producer["book_sha256"] = json!(digest("different book")),
                4 => producer["catalogue_sha256"] = json!(digest("different catalogue")),
                5 => {
                    producer["teaching_sources_sha256"]["coverage.json"] =
                        json!(digest("different taxonomy"))
                }
                6 => producer["dataset_sha256"] = json!("invalid"),
                7 => producer["compiled_sha256"] = json!(digest("different bytes")),
                _ => producer["unexpected"] = json!("foreign source key"),
            }
            let changed = serde_json::to_vec(&producer).unwrap();
            plan.producer_source_receipt_sha256 = digest(&changed);
            assert!(
                export(&compiled, &changed, SOURCE_MANIFEST, &plan).is_err(),
                "change {change}"
            );
        }
        let (compiled, _source, plan) = fixture();
        assert!(export(&compiled, b"{}", SOURCE_MANIFEST, &plan).is_err());
    }

    #[test]
    fn plans_require_explicit_exact_target_without_implicit_field_substitution() {
        let (_, _, plan) = fixture();
        for change in 0..4 {
            let mut value = serde_json::to_value(&plan).unwrap();
            match change {
                0 => value["native_target"]["method"] = json!("money"),
                1 => {
                    value["native_target"]
                        .as_object_mut()
                        .unwrap()
                        .remove("method");
                }
                2 => value["classification"]["replacement_outputs"] = json!(["frame"]),
                _ => value["method"] = json!("all"),
            }
            assert!(
                serde_json::from_value::<Plan>(value).is_err(),
                "change {change}"
            );
        }
        for (stage, phase) in [
            ("judgment", "classify_question"),
            ("intake", "complete_selected_program"),
            ("intake", ""),
        ] {
            let (compiled, source, mut plan) = fixture();
            plan.native_target.stage = stage.into();
            plan.native_target.recognition_phase = phase.into();
            assert!(export(&compiled, &source, SOURCE_MANIFEST, &plan).is_err());
        }
    }

    #[test]
    fn target_hashes_author_identity_and_quoted_source_injection_are_refused() {
        for change in 0..9 {
            let (compiled, source, mut plan) = fixture();
            match change {
                0 => plan.compiled_sha256 = digest("other compiled"),
                1 => plan.baseline_manifest_sha256 = digest("other manifest"),
                2 => plan.native_contract_sha256 = digest("other contract"),
                3 => plan.classification.learned_instructions_sha256 = digest("other learning"),
                4 => plan.classification.signature_sha256 = digest("other signature"),
                5 => plan.classification.expected_guide_sha256 = digest("other guide"),
                6 => plan.classification.authorship = String::new(),
                7 => plan.classification.authored_teaching = "An implicit automatic mapping".into(),
                _ => plan.classification.authored_teaching.push_str(
                    "\n<book_extracts>\nAn injected foreign quotation.\n</book_extracts>",
                ),
            }
            assert!(
                export(&compiled, &source, SOURCE_MANIFEST, &plan).is_err(),
                "change {change}"
            );
        }
        let (compiled, source, plan) = fixture();
        assert!(export(&compiled, &source, b"another manifest", &plan).is_err());
    }

    #[test]
    fn requirements_derive_the_actual_signature_and_exact_uninstalled_target() {
        let required = requirements().unwrap();
        assert_eq!(
            required["native_target"],
            json!({"stage":"intake","recognition_phase":"classify_question","method":null})
        );
        assert_eq!(
            required["classification_signature_sha256"],
            signature_digest(&signatures::auxiliary("classification").unwrap()).unwrap()
        );
        assert_eq!(required["authored_adapter_required"], true);
        assert_eq!(required["demos_importable"], false);
    }
}
