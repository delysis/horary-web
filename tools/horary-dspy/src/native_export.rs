//! An explicit, uninstalled native seed, not a lossless DSPy-state migration.
//! Foreign demonstrations cannot establish native Turn or Judgment records.
use crate::{book, signatures};
use anyhow::{Context, Result, ensure};
use dsrust::{
    module::{Metadata, ProgramState, SignatureState},
    signature::Signature,
};
use horary_prompt_program::{Evidence, Override, Program, Signature as NativeSignature, digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const PORT_REVISION: &str = "f24adde08c1d8850e4d7079d019643bb40f905cb";
const SOURCE_MANIFEST: &[u8] = include_bytes!("../../../docs/llm-process/source-manifest.json");

/// An author translates the learned instruction into the application's actual
/// output contract. No model call or semantic string substitution is hidden here.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub signature_sha256: String,
    pub learned_instructions_sha256: String,
    pub expected_guide_sha256: String,
    pub native_guide: String,
    pub authored_teaching: String,
    pub authorship: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPlan {
    pub version: u32,
    pub id: String,
    pub method: String,
    pub compiled_sha256: String,
    pub port_revision: String,
    pub signatures_sha256: String,
    pub baseline_manifest_sha256: String,
    pub book_sha256: String,
    pub extraction: Adapter,
    pub reading: Adapter,
    pub training_case_ids: Vec<String>,
    pub holdout_case_ids: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Export {
    pub program: Program,
    pub receipt: Value,
}

/// Source requirements for an explicit authored export plan. These identify the
/// compiled tool's contract; they do not claim that the source won an experiment.
pub fn requirements(method: &str) -> Result<Value> {
    substantive(method)?;
    Ok(json!({
        "port_revision":PORT_REVISION,
        "signatures_sha256":digest(include_bytes!("signatures.rs")),
        "baseline_manifest_sha256":digest(SOURCE_MANIFEST),
        "book_sha256":book::OCR_SHA256,
        "extraction_signature_sha256":signature_digest(&signatures::extraction(method)?)?,
        "reading_signature_sha256":signature_digest(&signatures::reading(method)?)?
    }))
}

fn substantive(method: &str) -> Result<()> {
    ensure!(
        signatures::METHODS.contains(&method) && !matches!(method, "wish" | "unclassified"),
        "Select a substantive reading family, not a routing sentinel"
    );
    Ok(())
}

pub(crate) fn signature_digest(signature: &Signature) -> Result<String> {
    // Saved fields alone omit their names, directions and types. Bind those
    // to the declaration as well as its baseline instruction and ordered fields.
    Ok(digest(serde_json::to_vec(&json!({
        "signature":SignatureState::of(signature),
        "inputs":signature.inputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>(),
        "outputs":signature.outputs.iter().map(|f|json!({"name":f.name,"type":f.annotation()})).collect::<Vec<_>>()
    }))?))
}

pub(crate) fn exact_keys(value: &Value, keys: &[&str], label: &str) -> Result<()> {
    let actual = value
        .as_object()
        .with_context(|| format!("{label} must be an object"))?;
    let expected = keys.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        actual.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected,
        "{label} has missing or unexpected fields"
    );
    Ok(())
}

/// Validate before the port's permissive loader can zip fields or skip names.
/// All foreign demos are withheld; their hashes remain reviewable in the receipt.
fn compiled_state(bytes: &[u8], method: &str) -> Result<ProgramState> {
    let value: Value = serde_json::from_slice(bytes).context("Invalid saved DSPy JSON")?;
    let extraction = format!("extraction.{method}");
    let reading = format!("reading.{method}");
    exact_keys(&value, &["metadata", &extraction, &reading], "Saved family")?;
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
    for name in [&extraction, &reading] {
        exact_keys(
            &value[name],
            &["traces", "train", "demos", "signature", "lm"],
            name,
        )?;
        exact_keys(
            &value[name]["signature"],
            &["instructions", "fields"],
            "Saved signature",
        )?;
        let fields = value[name]["signature"]["fields"]
            .as_array()
            .context("Saved fields must be an array")?;
        for field in fields {
            exact_keys(field, &["prefix", "description"], "Saved field")?;
        }
    }
    let state: ProgramState = serde_json::from_value(value)?;
    ensure!(
        state.metadata == Metadata::default(),
        "DSPy dependency metadata differs from the pinned port"
    );
    for (name, declaration) in [
        (&extraction, signatures::extraction(method)?),
        (&reading, signatures::reading(method)?),
    ] {
        let saved = state
            .get(name)
            .context("Missing required family predictor")?;
        ensure!(
            saved.traces.is_empty() && saved.train.is_empty() && saved.lm.is_none(),
            "Saved predictor includes runtime, training or model state"
        );
        ensure!(
            saved.signature.fields == SignatureState::of(&declaration).fields,
            "Saved predictor {name} field count, order, description or prefix differs"
        );
        ensure!(
            !saved.signature.instructions.trim().is_empty(),
            "Saved predictor {name} has no instruction"
        );
        let permitted = declaration
            .inputs
            .iter()
            .map(|f| f.name.as_str())
            .chain(declaration.outputs.iter().map(|f| f.name.as_str()))
            .collect::<BTreeSet<_>>();
        for demo in &saved.demos {
            ensure!(
                demo.keys().all(|k| permitted.contains(k.as_str()))
                    && demo.values().all(Value::is_string),
                "Saved demo includes unknown, private or differently typed fields"
            );
            ensure!(
                declaration
                    .outputs
                    .iter()
                    .all(|f| demo.contains_key(&f.name)),
                "Saved demo omits a required output"
            );
        }
    }
    Ok(state)
}

/// Export only the two exact predictors of a classified Journey, preserving the
/// native baseline. A version-2 standalone reading Module has a branch selector
/// and is not this artifact; the strict key check rejects silent projection.
/// Export only two exact selectors, preserving the entire native baseline and
/// its primary quotations. This never installs a candidate or certifies scores.
pub fn export(compiled: &[u8], baseline_manifest: &[u8], plan: &ExportPlan) -> Result<Export> {
    substantive(&plan.method)?;
    ensure!(plan.version == 1, "Unsupported native export plan version");
    let required = requirements(&plan.method)?;
    ensure!(
        plan.compiled_sha256 == digest(compiled),
        "Compiled DSPy source hash differs"
    );
    ensure!(
        plan.port_revision == PORT_REVISION
            && plan.signatures_sha256 == required["signatures_sha256"],
        "Pinned port or signature source differs"
    );
    ensure!(
        plan.book_sha256 == book::OCR_SHA256,
        "Primary book identity differs"
    );
    ensure!(
        plan.baseline_manifest_sha256 == digest(baseline_manifest)
            && plan.baseline_manifest_sha256 == required["baseline_manifest_sha256"],
        "Native baseline source manifest drifted from this tool"
    );
    let state = compiled_state(compiled, &plan.method)?;
    let mut overrides = Vec::new();
    let mut transfers = Vec::new();
    for (function, stage, phase, adapter, declaration) in [
        (
            "extraction",
            "intake",
            Some("complete_selected_program"),
            &plan.extraction,
            signatures::extraction(&plan.method)?,
        ),
        (
            "reading",
            "judgment",
            None,
            &plan.reading,
            signatures::reading(&plan.method)?,
        ),
    ] {
        let predictor = format!("{function}.{}", plan.method);
        let saved = state
            .get(&predictor)
            .context("Missing required predictor")?;
        ensure!(
            adapter.signature_sha256 == signature_digest(&declaration)?,
            "{function} declaration identity differs"
        );
        ensure!(
            adapter.learned_instructions_sha256 == digest(&saved.signature.instructions),
            "{function} authored adapter is bound to different learned instructions"
        );
        ensure!(
            adapter.expected_guide_sha256 == digest(&adapter.native_guide),
            "{function} native guide differs from its captured hash"
        );
        ensure!(
            !adapter.authorship.trim().is_empty() && adapter.authorship.len() <= 1024,
            "{function} adapter needs bounded explicit authorship"
        );
        ensure!(
            adapter.authored_teaching.trim().len() >= 80
                && adapter.authored_teaching.len() <= 16_000,
            "{function} needs explicit bounded native-contract teaching"
        );
        let replacement_text = format!(
            "{}\n\n<authored_native_teaching>\n{}\n</authored_native_teaching>\n",
            adapter.native_guide, adapter.authored_teaching
        );
        overrides.push(Override {
            stage: stage.into(),
            recognition_phase: phase.map(str::to_owned),
            method: Some(plan.method.clone()),
            expected_guide_sha256: adapter.expected_guide_sha256.clone(),
            replacement_text: replacement_text.clone(),
            edits: vec![],
        });
        transfers.push(json!({
            "predictor":predictor,"signature_sha256":adapter.signature_sha256,
            "learned_instructions_sha256":adapter.learned_instructions_sha256,
            "learned_instructions":saved.signature.instructions,
            "authored_teaching_sha256":digest(&adapter.authored_teaching),"authored_teaching":adapter.authored_teaching,
            "authorship":adapter.authorship,"baseline_guide_sha256":adapter.expected_guide_sha256,
            "native_seed_guide_sha256":digest(&replacement_text),
            "withheld_demonstrations":saved.demos.iter().enumerate().map(|(index,demo)|Ok(json!({
                "index":index,"sha256":digest(serde_json::to_vec(demo)?),
                "reason":if function=="extraction" {"Foreign flat facts and inference prose cannot establish native source-authorized Turn updates."} else {"Foreign prose and eN citations cannot establish native Judgment verdict, checks, source rules or prerequisite bindings."}
            }))).collect::<Result<Vec<_>>>()?
        }));
    }
    let program = Program {
        version:1, id:plan.id.clone(), baseline_manifest_sha256:plan.baseline_manifest_sha256.clone(), overrides,
        rationale:"New, explicitly authored native-format seed derived from source-pinned DSPy instructions. Foreign demonstrations are withheld. Requires fresh native journey and independent source/answer gates before installation; DSPy scores do not qualify this translation.".into(),
        evidence:plan.evidence.clone(), training_case_ids:plan.training_case_ids.clone(), holdout_case_ids:plan.holdout_case_ids.clone(),
    };
    program.validate().map_err(anyhow::Error::msg)?;
    for (adapter, stage, phase) in [
        (
            &plan.extraction,
            "intake",
            Some("complete_selected_program"),
        ),
        (&plan.reading, "judgment", None),
    ] {
        program
            .apply(
                NativeSignature {
                    stage,
                    recognition_phase: phase,
                    method: Some(&plan.method),
                },
                &adapter.native_guide,
            )
            .map_err(anyhow::Error::msg)?;
    }
    Ok(Export {
        receipt: json!({
            "kind":"dspy-explicit-native-seed","version":1,"method":plan.method,"compiled_sha256":plan.compiled_sha256,
        "export_plan_canonical_sha256":digest(serde_json::to_vec(plan)?),"native_program_canonical_sha256":digest(serde_json::to_vec(&program)?),
            "port_revision":PORT_REVISION,"signatures_sha256":plan.signatures_sha256,
            "baseline_manifest_sha256":plan.baseline_manifest_sha256,"book_sha256":plan.book_sha256,
            "transfers":transfers,"demonstrations_imported":0,"provider_calls":0,
            "unchanged_compiled_winner":false,"application_installed":false,"source_correct_reading_qualified":false,
            "limits":"This authored translation is a new candidate. Source hashes and native selector mechanics establish provenance, not semantic correctness. All original compiled instructions and demonstrations remain in the external hash-bound source artifact."
        }),
        program,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsrust::module::PredictorState;
    use std::collections::BTreeMap;

    fn fixture() -> (Vec<u8>, ExportPlan) {
        let method = "movable_deal";
        let extraction = signatures::extraction(method).unwrap();
        let reading = signatures::reading(method).unwrap();
        let mut state = ProgramState::new(BTreeMap::from([
            (
                format!("extraction.{method}"),
                PredictorState::of(&extraction, &[]),
            ),
            (
                format!("reading.{method}"),
                PredictorState::of(&reading, &[]),
            ),
        ]));
        for p in state.predictors.values_mut() {
            p.signature
                .instructions
                .push_str(" Synthetic learned teaching; mechanics only.");
        }
        let adapter = |function: &str, declaration: &Signature| {
            let guide = format!(
                "Synthetic native {function} guide keeps its actual typed worksheet and source authority.\n<book_extracts>\nSynthetic protected quotation.\n</book_extracts>\n"
            );
            Adapter {signature_sha256:signature_digest(declaration).unwrap(), learned_instructions_sha256:digest(&state.get(&format!("{function}.{method}")).unwrap().signature.instructions),expected_guide_sha256:digest(&guide),native_guide:guide,
                authored_teaching:"Synthetic explicit native-contract adapter: preserve the actual Turn or Judgment schema and all native source authority and guards. This is an offline mechanics test, not a source reading or model-quality result.".into(),authorship:"Offline synthetic test author".into()}
        };
        let plan = ExportPlan {
            version: 1,
            id: "native-synthetic-seed".into(),
            method: method.into(),
            compiled_sha256: String::new(),
            port_revision: PORT_REVISION.into(),
            signatures_sha256: digest(include_bytes!("signatures.rs")),
            baseline_manifest_sha256: digest(SOURCE_MANIFEST),
            book_sha256: book::OCR_SHA256.into(),
            extraction: adapter("extraction", &extraction),
            reading: adapter("reading", &reading),
            training_case_ids: vec!["synthetic-training".into()],
            holdout_case_ids: vec!["synthetic-heldout".into()],
            evidence: vec![Evidence {
                case_id: "synthetic-training".into(),
                file: "source-only-test.json".into(),
                json_pointer: "/finding".into(),
                sha256: digest("Synthetic training receipt"),
            }],
        };
        let bytes = serde_json::to_vec(&state).unwrap();
        let mut plan = plan;
        plan.compiled_sha256 = digest(&bytes);
        (bytes, plan)
    }

    #[test]
    fn exact_scopes_keep_classifier_updates_other_families_and_repair_authority() {
        let (compiled, plan) = fixture();
        let result = export(&compiled, SOURCE_MANIFEST, &plan).unwrap();
        let p = &result.program;
        for (stage, phase, guide) in [
            (
                "intake",
                Some("complete_selected_program"),
                &plan.extraction.native_guide,
            ),
            ("judgment", None, &plan.reading.native_guide),
        ] {
            let (changed, applied) = p
                .apply(
                    NativeSignature {
                        stage,
                        recognition_phase: phase,
                        method: Some(&plan.method),
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
            assert!(
                p.apply(
                    NativeSignature {
                        stage,
                        recognition_phase: phase,
                        method: Some("money")
                    },
                    guide
                )
                .unwrap()
                .is_none()
            );
        }
        for phase in ["classify_question", "update_selected_program"] {
            assert!(
                p.apply(
                    NativeSignature {
                        stage: "intake",
                        recognition_phase: Some(phase),
                        method: Some(&plan.method)
                    },
                    &plan.extraction.native_guide
                )
                .unwrap()
                .is_none()
            );
        }
        let input = json!({"recognition_phase":"complete_selected_program","consultation":{"frame":{"state":"resolved","observation":{"value":{"method":"movable_deal"}}}}});
        let repair = json!({"original_input":input,"previous_worksheet":{"method":"money"},"native_validation_error":"Synthetic rejection"});
        assert_eq!(
            p.apply(
                horary_prompt_program::signature("intake", &repair),
                &plan.extraction.native_guide
            )
            .unwrap(),
            p.apply(
                horary_prompt_program::signature("intake", &repair["original_input"]),
                &plan.extraction.native_guide
            )
            .unwrap()
        );
        assert_eq!(result.receipt["application_installed"], false);
        assert_eq!(result.receipt["source_correct_reading_qualified"], false);
    }

    fn changed_state(mutator: impl FnOnce(&mut Value)) -> Result<Export> {
        let (bytes, mut plan) = fixture();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        mutator(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        plan.compiled_sha256 = digest(&bytes);
        export(&bytes, SOURCE_MANIFEST, &plan)
    }

    #[test]
    fn refuses_missing_extra_wrong_order_and_saved_private_runtime_state() {
        for mutate in [
            |v: &mut Value| {
                v.as_object_mut().unwrap().remove("reading.movable_deal");
            },
            |v: &mut Value| {
                v["classifier"] = v["extraction.movable_deal"].clone();
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            },
            |v: &mut Value| {
                v["extraction.movable_deal"]["signature"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            },
            |v: &mut Value| {
                v["reading.movable_deal"]["lm"] = json!({"api_key":"SYNTHETIC"});
            },
            |v: &mut Value| {
                v["reading.movable_deal"]["private_score"] = json!(1);
            },
            |v: &mut Value| {
                v["metadata"]["dependency_versions"]["dspy"] = json!("different");
            },
        ] {
            assert!(changed_state(mutate).is_err());
        }
    }

    #[test]
    fn standalone_two_predictor_reader_is_not_silently_exported_as_a_journey() {
        let (bytes, mut plan) = fixture();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["reading_branch.movable_deal"] = serde_json::to_value(
            dsrust::module::PredictorState::of(&crate::reading::selector_signature(), &[]),
        )
        .unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        plan.compiled_sha256 = digest(&bytes);
        let error = export(&bytes, SOURCE_MANIFEST, &plan)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("Saved family has missing or unexpected fields"));
    }

    #[test]
    fn refuses_source_instruction_signature_book_and_guide_drift() {
        let (compiled, plan) = fixture();
        for mutate in [
            |p: &mut ExportPlan| p.compiled_sha256 = digest("different compiled"),
            |p: &mut ExportPlan| p.baseline_manifest_sha256 = digest("different manifest"),
            |p: &mut ExportPlan| p.book_sha256 = digest("different book"),
            |p: &mut ExportPlan| p.signatures_sha256 = digest("different signatures"),
            |p: &mut ExportPlan| p.extraction.signature_sha256 = digest("different declaration"),
            |p: &mut ExportPlan| {
                p.reading.learned_instructions_sha256 = digest("different learned instruction")
            },
            |p: &mut ExportPlan| p.extraction.native_guide.push_str("drift"),
            |p: &mut ExportPlan| p.reading.authored_teaching.clear(),
        ] {
            let mut changed = plan.clone();
            mutate(&mut changed);
            assert!(export(&compiled, SOURCE_MANIFEST, &changed).is_err());
        }
        assert!(export(&compiled, b"changed current manifest", &plan).is_err());
        let exported = export(&compiled, SOURCE_MANIFEST, &plan).unwrap();
        assert!(
            exported
                .program
                .apply(
                    NativeSignature {
                        stage: "judgment",
                        recognition_phase: None,
                        method: Some("movable_deal")
                    },
                    "Changed actual guide"
                )
                .is_err()
        );
    }

    #[test]
    fn foreign_demos_are_hashed_and_withheld_without_minting_native_records() {
        let (compiled, mut plan) = fixture();
        let mut value: Value = serde_json::from_slice(&compiled).unwrap();
        let demo = json!({"question":"Synthetic fixture only.","status":"answer","answer":"Foreign prose.","analysis":"A claim about e1.","evidence":"e1","unresolved":""});
        value["reading.movable_deal"]["demos"] = json!([demo]);
        let bytes = serde_json::to_vec(&value).unwrap();
        plan.compiled_sha256 = digest(&bytes);
        let exported = export(&bytes, SOURCE_MANIFEST, &plan).unwrap();
        let withheld = &exported.receipt["transfers"][1]["withheld_demonstrations"];
        assert_eq!(withheld.as_array().unwrap().len(), 1);
        assert_eq!(
            withheld[0]["sha256"],
            digest(serde_json::to_vec(&demo).unwrap())
        );
        assert_eq!(exported.receipt["demonstrations_imported"], 0);
        assert!(
            !serde_json::to_string(&exported.program)
                .unwrap()
                .contains("Foreign prose")
        );
        value["reading.movable_deal"]["demos"][0]["__review_source"] =
            json!("Private synthetic review");
        let bytes = serde_json::to_vec(&value).unwrap();
        plan.compiled_sha256 = digest(&bytes);
        assert!(export(&bytes, SOURCE_MANIFEST, &plan).is_err());
    }

    #[test]
    fn protected_source_injection_and_reserved_evidence_are_refused() {
        let (compiled, mut plan) = fixture();
        plan.reading
            .authored_teaching
            .push_str("<book_extracts>New quotation.</book_extracts>");
        assert!(export(&compiled, SOURCE_MANIFEST, &plan).is_err());
        let (_, mut plan) = fixture();
        plan.evidence[0].case_id = "synthetic-heldout".into();
        assert!(export(&compiled, SOURCE_MANIFEST, &plan).is_err());
        let (_, mut plan) = fixture();
        plan.method = "wish".into();
        assert!(export(&compiled, SOURCE_MANIFEST, &plan).is_err());
    }
}
