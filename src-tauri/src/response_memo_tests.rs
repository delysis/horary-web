//! Offline exact-response tests use invented receipts, never a provider.
use super::*;
use horary_prompt_program::response_memo::{Index, Kind, Memo, Pin, Source, VERSION};

fn model() -> Value {
    json!({"provider":"google_gemini_api","id":"synthetic-model","endpoint":"https://example.invalid",
        "decoding":"hosted unconstrained text","thinking_level":"minimal","temperature":0,"seed":0,"local_inference":false})
}
fn gen(content: String) -> NativeGenerationResult {
    serde_json::from_value(json!({"content":content,"promptTokens":0,"generatedTokens":0,"elapsedMs":0,
        "tokensPerSecond":0.,"promptCacheHit":false,"cachedPromptTokens":0,"prefilledPromptTokens":0})).unwrap()
}
struct Offline {
    root: tempfile::TempDir,
    dir: PathBuf,
    memo: Option<Arc<Memo>>,
    fresh: AtomicU64,
    calls: Mutex<Vec<Value>>,
}
impl Offline {
    fn new(memo: Option<Arc<Memo>>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("native/cases/synthetic");
        fs::create_dir_all(dir.join("calls")).unwrap();
        let fixture = json!({"id":"synthetic","words":"A source-only synthetic concern"});
        let initial = json!({"session":Session::default(),"device":null,"clock":42});
        write_new(&dir.join("fixture.json"), &fixture).unwrap();
        write_new(&dir.join("initial.json"), &initial).unwrap();
        Self {
            root,
            dir,
            memo,
            fresh: AtomicU64::new(0),
            calls: Mutex::new(vec![]),
        }
    }
    fn request(&self, stage: Stage, matter: Matter, input: &Value, schema: &Value) -> Value {
        let (prompt, _) = trial_prompt(stage, matter, input, schema, None).unwrap();
        json!({"stage":stage,"matter":matter,"input":input,"schema":schema,
            "prompt":serde_json::from_str::<Value>(&prompt).unwrap(),"provider":model(),
            "memo_wire_request":crate::hosted_gemma_eval::wire_request(&prompt,schema,1000).unwrap(),
            "memo_context":{"case_id":"synthetic",
                "fixture_sha256":horary_prompt_program::digest(fs::read(self.dir.join("fixture.json")).unwrap()),
                "initial_sha256":horary_prompt_program::digest(fs::read(self.dir.join("initial.json")).unwrap())}})
    }
    fn sealed(&self) -> Index {
        let root = self.root.path().canonicalize().unwrap();
        write_new(&root.join("request.json"),&json!({"kind":"reading_journey",
            "request":{"case":{"id":"synthetic"},"native_executable_sha256":horary_prompt_program::digest("synthetic-native")}})).unwrap();
        write_new(
            &root.join("response.json"),
            &json!({"outcome":{"id":"synthetic","scope":"reading_journey_function_only",
                "full_reading":true,"execution_status":"completed","first_turn_execution_completed":true,
                "follow_up_execution_completed":null,"execution_error":null,
                "physical_generation_attempts_complete":true,"fresh_initial_sha256_verified":true,
                "captured_checkpoint_reused":false,"target_function_invoked":true,"target_stage_authentic_inputs":true,
                "deadline_cancelled":false,"group_cancelled":false,"infrastructure_error":null,"provider_stop":null}}),
        )
        .unwrap();
        write_new(
            &root.join("native/manifest.json"),
            &json!({"model":model(),"sources":{"synthetic.rs":"invented"}}),
        )
        .unwrap();
        let calls = self.calls.lock().unwrap();
        for (i, call) in calls.iter().enumerate() {
            write_new(
                &self.dir.join(format!("calls/{:04}-request.json", i + 1)),
                &call["request"],
            )
            .unwrap();
            write_new(
                &self.dir.join(format!("calls/{:04}-result.json", i + 1)),
                call,
            )
            .unwrap();
        }
        let mut files = vec![];
        fn tree(root: &Path, dir: &Path, files: &mut Vec<Value>) {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    tree(root, &path, files);
                } else {
                    files.push(json!({"file":path.strip_prefix(root).unwrap(),"sha256":horary_prompt_program::digest(fs::read(path).unwrap())}));
                }
            }
        }
        tree(&root, &root, &mut files);
        let completion = json!({"response_sha256":horary_prompt_program::digest(fs::read(root.join("response.json")).unwrap()),"artifacts":files});
        write_new(&root.join("completed.json"), &completion).unwrap();
        Index {
            version: VERSION.into(),
            case_id: "synthetic".into(),
            fixture_sha256: horary_prompt_program::digest(
                fs::read(self.dir.join("fixture.json")).unwrap(),
            ),
            initial_sha256: horary_prompt_program::digest(
                fs::read(self.dir.join("initial.json")).unwrap(),
            ),
            excluded_stage: "condition".into(),
            sources: vec![Source {
                root: root.clone(),
                kind: Kind::NativeOperation,
                seal: Pin {
                    file: root.join("completed.json"),
                    sha256: horary_prompt_program::digest(
                        fs::read(root.join("completed.json")).unwrap(),
                    ),
                },
            }],
        }
    }
}
impl Runtime for Offline {
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        _audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String> {
        let request = self.request(stage, matter, input, schema);
        let signature = memo_signature(&request, None)?;
        let cached = self
            .memo
            .as_ref()
            .map(|m| m.lookup(&signature))
            .transpose()?
            .flatten();
        let (generation, receipt, reuse) = if let Some((raw, witness)) = cached {
            (
                serde_json::from_value(raw).map_err(|e| e.to_string())?,
                Value::Null,
                Some(witness),
            )
        } else {
            let n = self.fresh.fetch_add(1, Ordering::AcqRel);
            if n >= 2 {
                return Err("Offline test refuses unscripted generation".into());
            }
            let generation=gen(json!({"mode":"select","place_id":if n==0{"not-a-candidate"}else{"synthetic-place"},
                "query":"","clarification":"","basis":"Only the actual supplied candidate may be used"}).to_string());
            let body = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":generation.content}]}}]});
            let receipt = json!({"provider":"google_gemini_api","submitted":true,"request":signature.wire_request,
                "response":{"http_status":200,"body":body},"native_result":{"Ok":generation},
                "generation_attempts":[{"submitted":true,"http_status":200,"body":body}]});
            (generation, receipt, None)
        };
        self.calls
            .lock()
            .unwrap()
            .push(json!({"request":request,"result":{"Ok":generation},
            "provider_receipt":receipt,"response_reuse":reuse}));
        Ok(generation)
    }
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String> {
        tasks
            .iter()
            .map(|(stage, matter, input, schema)| {
                self.generate(*stage, *matter, input, schema, None)
            })
            .collect()
    }
    fn publish(&self, _session: &mut Session) -> Result<(), String> {
        Ok(())
    }
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
    fn directory(&self) -> &Path {
        &self.dir
    }
}

#[test]
fn exact_response_reuse_reconstructs_native_work_and_rejects_the_original_bad_answer_again() {
    let original = Offline::new(None);
    let input = json!({"candidates":[{"id":"synthetic-place"}]});
    let mut source = Session::default();
    crate::horary_executor::execute(
        &mut source,
        &original,
        Stage::Place,
        input.clone(),
        &[],
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(source.method.records.len(), 2);
    assert!(source.method.records[0].validation_error.is_some());
    assert!(source.method.records[1].validation_error.is_none());
    let index = original.sealed();
    let current = Offline::new(Some(Arc::new(Memo::load(index.clone()).unwrap())));
    let mut session = Session::default();
    assert!(session.method.records.is_empty());
    crate::horary_executor::execute(
        &mut session,
        &current,
        Stage::Place,
        input.clone(),
        &[],
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(current.fresh.load(Ordering::Acquire), 0);
    assert_eq!(session.method.records.len(), 2);
    assert_eq!(session.method.records[0].raw, source.method.records[0].raw);
    assert!(session.method.records[0].validation_error.is_some());
    assert_eq!(
        session.method.records[1].worksheet,
        source.method.records[1].worksheet
    );
    assert!(session.chart.is_none()); // No chart/record/permit was injected.
    let calls = current.calls.lock().unwrap();
    assert_eq!(input_journey_generation_attempts(&calls), 0);
    assert!(reading::provider_responses_settled(&calls));
    assert!(calls
        .iter()
        .all(|c| c["provider_receipt"].is_null() && !c["response_reuse"].is_null()));
    let mut fabricated = calls.clone();
    fabricated[0]["result"]["Ok"]["content"] = json!("Substituted output");
    assert!(!reading::provider_responses_settled(&fabricated));
    drop(calls);
    let mut target_index = index;
    target_index.excluded_stage = "place".into();
    let target = Offline::new(Some(Arc::new(Memo::load(target_index).unwrap())));
    crate::horary_executor::execute(
        &mut Session::default(),
        &target,
        Stage::Place,
        input,
        &[],
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(target.fresh.load(Ordering::Acquire), 2); // Includes its actual repair.
}

#[test]
fn exact_response_key_keeps_full_repair_and_current_schema_and_branch_order() {
    let runtime = Offline::new(None);
    let base = json!({"candidates":[{"id":"synthetic-place"}]});
    let schema = crate::horary_step::response_schema_for(Stage::Place, Matter::Other, &base, &[]);
    let request = runtime.request(Stage::Place, Matter::Other, &base, &schema);
    let signature = memo_signature(&request, None).unwrap();
    let mut changed = request.clone();
    changed["input"] =
        json!({"original_input":base,"native_validation_error":"Changed current guard"});
    assert_ne!(
        memo_signature(&changed, None).unwrap().key().unwrap(),
        signature.key().unwrap()
    );
    changed = request.clone();
    changed["schema"]["required"] = json!(["new_current_contract_field"]);
    assert_ne!(
        memo_signature(&changed, None).unwrap().key().unwrap(),
        signature.key().unwrap()
    );
    let batch = json!({"provider":model(),"memo_context":request["memo_context"],
        "tasks":[["place","other",request["input"],schema],["moment","other",{"selected_timezone":"UTC"},{}]],
        "prompts":[request["prompt"],[{"role":"system","content":"Other actual guide"},{"role":"user","content":"Other actual input"}]]});
    assert_eq!(
        memo_signature(&batch, Some(0)).unwrap().key().unwrap(),
        signature.key().unwrap()
    );
    assert_ne!(
        memo_signature(&batch, Some(1)).unwrap().key().unwrap(),
        signature.key().unwrap()
    );
}
