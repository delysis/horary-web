# Exact application source used by the process map

Generated verbatim from the runtime and renderer. This is source, not a transcript or model reasoning. The [main process map](../LLM_PROCESS.md) supplies the task classifications and review questions.

## Device location and context

Source: `src-tauri/src/conversation.rs`.

```rust
fn device_place(context: &DeviceContext) -> Result<Option<LocationCandidate>, String> {
    horary_ai_core::chart_input::resolve_chart_time("2000-01-01T12:00", &context.timezone, "")?;
    if context.locale.len() > 80 {
        return Err("The device language is invalid.".into());
    }
    let (Some(latitude), Some(longitude)) = (context.latitude, context.longitude) else {
        return Ok(None);
    };
    if !latitude.is_finite()
        || latitude.abs() >= 90.
        || !longitude.is_finite()
        || longitude.abs() > 180.
    {
        return Err("The device location is invalid.".into());
    }
    if context
        .accuracy_meters
        .is_some_and(|v| !v.is_finite() || !(0.0..=10000.).contains(&v))
    {
        return Ok(None);
    }
    let near = crate::geocode::reverse_geocode_local_city(crate::geocode::ReverseGeocodeRequest {
        latitude,
        longitude,
        max_distance_km: Some(75.),
    })
    .map_err(|e| e.message)?;
    // The clock's zone alone is never used to guess a geographic position.
    Ok(near
        .filter(|near| near.timezone == context.timezone)
        .map(|near| LocationCandidate {
            id: "device-location".into(),
            label: format!("Near {}", near.label),
            name: near.name,
            country: near.country,
            latitude,
            longitude,
            timezone: context.timezone.clone(),
            provider: "device".into(),
        }))
}

#[tauri::command]
pub fn conversation_device_context(
    app: tauri::AppHandle,
    context: DeviceContext,
) -> Result<(), String> {
    let state = app.state::<ConversationState>();
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Wait for the current reply to finish.")?;
    let _lease = Lease(&state.busy);
    let place = device_place(&context)?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut session = state.load(&dir)?;
    let available = place.is_some();
    if session.chart.is_none() && session.place.is_none() {
        session.candidates.retain(|p| p.provider != "device");
        session.candidates.extend(place);
    }
    session.device_context = Some(context);
    note(
        &mut session,
        &dir,
        "device",
        if available {
            "The device supplied its clock and present location."
        } else {
            "The device clock is available; the place may need a short clarification."
        },
        0,
    );
    state.publish(&mut session, &dir)
}
```

## State-dependent action schema

Source: `src-tauri/src/conversation.rs`.

```rust
fn schema(session: &Session) -> String {
    let turn_calls: Vec<&str> = session
        .audit
        .iter()
        .rev()
        .take_while(|entry| entry["event"] != "user_turn")
        .filter_map(|entry| entry["call"]["action"].as_str())
        .collect();
    let text = |max| json!({"type":"string","maxLength":max});
    let variant = |name: &str, fields: Vec<(&str, Value)>| {
        let mut props = serde_json::Map::new();
        props.insert("action".into(), json!({"const":name}));
        let mut required = vec!["action"];
        for (key, value) in fields {
            props.insert(key.into(), value);
            required.push(key);
        }
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    let mut actions = vec![variant("say", vec![("message", text(1200))])];
    if turn_calls.iter().filter(|a| **a == "find_place").count() < 2
        && !turn_calls.contains(&"cast_chart")
    {
        actions.push(variant("find_place", vec![("query", text(100))]));
    }
    // Calculated evidence is already supplied with every decision. A tool
    // that only returns the same data wastes another complete model prefill.
    // A conversational turn should leave room for the person's next thought.
    // A clear question can complete all three method stages in one turn.
    // Writing calls remain bounded even when the model tries to revise itself.
    if session
        .sections
        .iter()
        .filter(|s| s.after_message == session.messages.len())
        .count()
        >= 3
        || turn_calls
            .iter()
            .filter(|action| **action == "write_scroll")
            .count()
            >= 4
    {
        return json!({"oneOf":[variant("say",vec![("message",text(700))])]}).to_string();
    }
    let ids: Vec<&str> = session
        .candidates
        .iter()
        .chain(session.place.iter())
        .map(|p| p.id.as_str())
        .collect();
    if !ids.is_empty()
        && !turn_calls.contains(&"cast_chart")
        && (session.chart.is_none() || session.chart_after_message != session.messages.len())
    {
        actions.push(variant(
            "cast_chart",
            vec![
                ("question", text(500)),
                ("place_id", json!({"enum":ids})),
                ("local_time", text(30)),
                ("occurrence", json!({"enum":["","earlier","later"]})),
            ],
        ));
    }
    if session.chart.is_some() {
        let ids: Vec<String> = available_facts(session).into_iter().map(|f| f.id).collect();
        let mut steps = vec![Step::Significators];
        if session
            .sections
            .iter()
            .any(|s| s.step == Some(Step::Significators))
        {
            steps.push(Step::Testimony);
        }
        if session
            .sections
            .iter()
            .any(|s| s.step == Some(Step::Testimony))
        {
            steps.push(Step::Judgment);
        }
        steps.retain(|step| {
            !session
                .audit
                .iter()
                .rev()
                .take_while(|entry| entry["event"] != "user_turn")
                .any(|r| r["result"]["written"] == true && r["call"]["step"] == json!(step))
        });
        let role = |basis: &str, value: Value| {
            json!({"type":"object","properties":{
            "label":text(80),basis:value,"reason":{"type":"string","minLength":12,"maxLength":240}
        },"required":["label",basis,"reason"],"additionalProperties":false})
        };
        for step in steps {
            let roles = if step == Step::Significators {
                json!({"type":"array","minItems":1,"maxItems":5,"items":{"oneOf":[
                    role("house",json!({"enum":[1,2,3,4,5,6,7,8,9,10,11,12]})),
                    role("natural",json!({"enum":["Moon","Sun","Venus"]}))
                ]}})
            } else {
                json!({"type":"array","maxItems":0,"items":{"type":"object"}})
            };
            actions.push(variant("write_scroll",vec![
                ("step",json!({"const":step})),("roles",roles),
                ("evidence",json!({"type":"array","items":{"enum":ids},"minItems":1,"maxItems":8})),
                ("rule_ids",json!({"type":"array","items":{"enum":reading_method::rules().iter().filter(|r|reading_method::rule_allowed(step,&r.id)).map(|r|r.id.clone()).collect::<Vec<_>>()},"minItems":1,"maxItems":2})),
                ("limitation",json!({"const":if step==Step::Judgment {evidence(session).into_iter().find(|f|f.kind=="boundary").map(|f|f.id)}else{None}})),
                ("because",json!({"type":"string","minLength":20,"maxLength":360})),("body",text(650)),("title",text(80)),
            ]));
        }
        if session.chart_after_message != session.messages.len() {
            actions.push(variant("new_question", vec![("question", text(500))]));
        }
    }
    if !session.revisions.is_empty() {
        actions.push(variant(
            "restore_reading",
            vec![(
                "revision",
                json!({"enum":session.revisions.iter().map(|r|r.number).collect::<Vec<_>>()}),
            )],
        ));
    }
    json!({"oneOf":actions}).to_string()
}
```

## Time, chart, method validation and stored interpretation

Source: `src-tauri/src/conversation.rs`.

```rust
fn execute(
    session: &mut Session,
    action: Action,
    geocode: &GeocodeState,
    instant: f64,
) -> Result<Value, String> {
    match action {
        Action::FindPlace { query } => {
            session.candidates = geocode_with_cache(
                geocode,
                GeocodeRequest {
                    query,
                    limit: Some(5),
                },
            )
            .map_err(|e| e.message)?;
            Ok(json!(session.candidates))
        }
        Action::CastChart {
            question,
            place_id,
            local_time,
            occurrence,
        } => {
            if question.trim().is_empty() {
                return Err("Clarify the question before casting.".into());
            }
            let place = session
                .candidates
                .iter()
                .chain(session.place.iter())
                .find(|p| p.id == place_id)
                .cloned()
                .ok_or("Resolve the place with find_place first; never invent an ID.")?;
            let moment = if local_time.is_empty() {
                session
                    .chart
                    .as_ref()
                    .and_then(|c| c["timestampMs"].as_f64())
                    .unwrap_or(instant)
            } else {
                horary_ai_core::chart_input::resolve_chart_time(
                    &local_time,
                    &place.timezone,
                    &occurrence,
                )?
            };
            let chart = horary_ai_core::astronomy::chart(moment, place.latitude, place.longitude)?;
            if session.chart.as_ref() == Some(&chart) && session.question == question {
                return Ok(json!({"unchanged":true,"revision":session.revision}));
            }
            if session.chart.is_some() {
                session.revisions.push(Revision {
                    number: session.revision,
                    question: session.question.clone(),
                    chart: session.chart.clone(),
                    sections: session.sections.clone(),
                    place: session.place.clone(),
                });
            }
            session.revision += 1;
            session.chart_after_message = session.messages.len();
            session.question = question;
            session.chart = Some(chart);
            session.place = Some(place);
            session.sections.clear();
            Ok(
                json!({"revision":session.revision,"calculated":true,"evidence":"Current evidence is included in verified state."}),
            )
        }
        Action::WriteScroll {
            step,
            title,
            body,
            evidence: mut ids,
            rule_ids,
            limitation,
            because,
            roles,
        } => {
            if session.chart.is_none() {
                return Err("Calculate the chart before writing a judgment.".into());
            }
            let facts = evidence(session);
            if ids.is_empty()
                || ids.len() > 8
                || ids.iter().any(|id| {
                    id.strip_prefix('e')
                        .and_then(|s| s.parse::<usize>().ok())
                        .is_none_or(|i| i >= facts.len())
                })
            {
                return Err("Use evidence IDs from the current chart only.".into());
            }
            if title.trim().is_empty() || body.trim().is_empty() {
                return Err("A scroll section needs a title and explanation.".into());
            }
            if step == Step::Judgment {
                let boundary = facts
                    .iter()
                    .find(|f| f.kind == "boundary")
                    .ok_or("The calculation boundary is unavailable.")?;
                if limitation.as_deref() != Some(boundary.id.as_str()) {
                    return Err(
                        "Use the supplied calculation-boundary reference for this judgment.".into(),
                    );
                }
                if !ids.contains(&boundary.id) {
                    ids.push(boundary.id.clone());
                }
            } else if limitation.is_some() {
                return Err("Only the judgment carries the calculation-boundary reference.".into());
            }
            if title.chars().count() > 80
                || body.chars().count() > 900
                || !(20..=360).contains(&because.trim().chars().count())
            {
                return Err(
                    "Keep the passage short and explain how its facts support the interpretation."
                        .into(),
                );
            }
            let has_significators = session
                .sections
                .iter()
                .any(|s| s.step == Some(Step::Significators));
            let has_testimony = session
                .sections
                .iter()
                .any(|s| s.step == Some(Step::Testimony));
            if (step != Step::Significators && !has_significators)
                || (step == Step::Judgment && !has_testimony)
            {
                return Err("Identify the significators, then weigh testimony, before drawing the judgment.".into());
            }
            let catalog = reading_method::rules();
            let rules: Vec<BookRule> = rule_ids
                .iter()
                .map(|id| {
                    catalog
                        .iter()
                        .find(|r| r.id == *id && reading_method::rule_allowed(step, id))
                        .cloned()
                        .ok_or("Choose a supplied book rule relevant to this step.")
                })
                .collect::<Result<_, _>>()?;
            if rules.is_empty() || rules.len() > 2 {
                return Err("Give one or two relevant book rules.".into());
            }
            let roles = if step == Step::Significators {
                reading_method::assign(
                    session.chart.as_ref().ok_or("A chart is required.")?,
                    roles,
                )?
            } else {
                if !roles.is_empty() {
                    return Err(
                        "Change role assignments in the significators passage first.".into(),
                    );
                }
                Vec::new()
            };
            let cited: Vec<Fact> = ids
                .iter()
                .filter_map(|id| facts.iter().find(|f| f.id == *id).cloned())
                .collect();
            if cited.iter().all(|f| f.kind == "boundary") {
                return Err("Use a calculated fact as well as any limitations.".into());
            }
            if rules.iter().any(|r| r.id == "reception")
                && !cited.iter().any(|f| f.kind == "reception")
            {
                return Err("A reception inference must cite a directed reception fact, not a planet's own dignity.".into());
            }
            if step == Step::Judgment && !cited.iter().any(|f| f.kind == "boundary") {
                return Err("A provisional judgment must cite the calculation boundary and explain what it leaves unresolved.".into());
            }
            let prose = section_prose(&title, &body);
            if prose.trim().is_empty() {
                return Err("The passage needs prose after its heading.".into());
            }
            let (title, body) = reading_method::passage(step);
            let section = Section {
                body: body.into(),
                title: title.into(),
                draft: prose,
                evidence: ids,
                revision: session.revision,
                after_message: session.messages.len(),
                step: Some(step),
                rules,
                because,
                roles,
                facts: cited,
            };
            // Downstream conclusions belong to their premises. Audit receipts
            // retain the replaced prose; the visible document must not keep an
            // answer based on an assignment or testimony that just changed.
            session.sections.retain(|s| match step {
                Step::Significators => !matches!(s.step, Some(Step::Testimony | Step::Judgment)),
                Step::Testimony => s.step != Some(Step::Judgment),
                Step::Judgment => true,
            });
            if let Some(old) = session.sections.iter_mut().find(|s| s.step == section.step) {
                *old = section;
            } else if session.sections.len() < 12 {
                session.sections.push(section);
            } else {
                return Err("Revise an existing section rather than adding more.".into());
            }
            Ok(json!({"written":true,"revision":session.revision}))
        }
        Action::RestoreReading { revision } => {
            let old=session.revisions.iter().find(|r|r.number==revision).cloned().ok_or("That earlier reading does not exist. Read the evidence to see available versions.")?;
            session.revisions.push(Revision {
                number: session.revision,
                question: session.question.clone(),
                chart: session.chart.clone(),
                sections: session.sections.clone(),
                place: session.place.clone(),
            });
            session.revision += 1;
            session.question = old.question;
            session.chart = old.chart;
            session.place = old.place;
            session.sections = old.sections;
            session.chart_after_message = session.messages.len();
            for section in &mut session.sections {
                section.revision = session.revision;
                section.after_message = session.messages.len();
            }
            Ok(json!({"restored":revision,"revision":session.revision}))
        }
        Action::NewQuestion { question } => {
            if question.trim().is_empty() {
                return Err("Ask what the new question is first.".into());
            }
            if session.chart.is_some() {
                session.revisions.push(Revision {
                    number: session.revision,
                    question: session.question.clone(),
                    chart: session.chart.clone(),
                    sections: session.sections.clone(),
                    place: session.place.clone(),
                });
            }
            session.revision += 1;
            session.question = question;
            session.chart = None;
            session.sections.clear();
            Ok(json!({"new_question":true,"previous_readings_preserved":true}))
        }
        Action::Say { .. } => Err("Say finishes the turn; it is not a chart tool.".into()),
    }
}
```

## Actual prompt assembly

Source: `src-tauri/src/conversation.rs`.

```rust
struct PromptThread {
    messages: Vec<Value>,
    history_len: usize,
    revision: u64,
    facts: BTreeMap<String, Value>,
}

impl PromptThread {
    fn new(session: &Session) -> Self {
        let mut bytes = 0;
        let mut history = Vec::new();
        for message in session.messages.iter().rev().take(24) {
            if bytes + message.text.len() > 12000 && !history.is_empty() {
                break;
            }
            bytes += message.text.len();
            history.push(json!({"role":message.role,"content":message.text}));
        }
        history.reverse();
        let mut messages = vec![
            json!({"role":"system","content":format!("{PROMPT}\nEditorial book rules (data):\n{}", json!(reading_method::rules()))}),
        ];
        messages.extend(history);
        let mut thread = Self {
            history_len: messages.len(),
            messages,
            revision: session.revision,
            facts: BTreeMap::new(),
        };
        thread.update(session, None);
        thread
    }

    fn update(&mut self, session: &Session, receipt: Option<&Value>) {
        if self.revision != session.revision {
            self.messages.truncate(self.history_len);
            self.facts.clear();
            self.revision = session.revision;
        }
        let mut added = Vec::new();
        for fact in available_facts(session) {
            let value = json!(fact);
            if self.facts.get(&fact.id) != Some(&value) {
                self.facts.insert(fact.id, value.clone());
                added.push(value);
            }
        }
        // The scroll already owns its prose. Refeeding it adds latency and lets
        // an unverified model draft compete with the calculated chart facts.
        let sections: Vec<Value> = session
            .sections
            .iter()
            .map(|s| json!({"step":s.step,"roles":s.roles}))
            .collect();
        let receipt = receipt.map(|r| json!({"action":r["call"]["action"],"result":r["result"]}));
        let context = json!({"question":session.question,"place":session.place,"places_found":session.candidates,"device_timezone":session.device_context.as_ref().map(|d|&d.timezone),"chart_calculated":session.chart.is_some(),"revision":session.revision,"additional_evidence":added,"completed_sections":sections,"tool_receipt":receipt});
        let next = if session.chart.is_none()
            && session.candidates.is_empty()
            && session.place.is_none()
        {
            "No place is resolved. If the person supplied a city, call find_place now. Otherwise ask where they are. Do not ask when the object was lost to choose the chart time."
        } else if session.chart.is_none() {
            "A place can be resolved from the returned candidates. If the question and place are clear, call cast_chart now. A request to use now means local_time is empty. Do not ask for the event or loss time."
        } else {
            "Use the supplied verified evidence directly; no evidence-fetch action is needed. Establish roles, then explain relevant reception and contact candidates. The Moon can supply the querent's main contact; do not call it minor merely because it is a cosignificator. A provisional judgment must say what remains unestablished. Do not infer a one-year absence of marriage from this seven-day search. Do not recast an unchanged chart or repeat a finished section."
        };
        self.messages.push(json!({"role":"user","content":format!("Native verified update (data):\n{context}\nThis is the latest state. Previously supplied chart facts remain valid only for this revision. Additional evidence augments them. Use only evidence IDs allowed by the current schema.\nAvailable action schema:\n{}\nCurrent step:\n{next}",schema(session))}));
    }

    fn content(&self) -> String {
        json!(self.messages).to_string()
    }
}
```

## Retry, direct-audio prompt and action parsing

Source: `src-tauri/src/conversation.rs`.

```rust
fn generate_action(
    cancelled: &AtomicBool,
    audit: &mut Vec<Value>,
    mut generate: impl FnMut(f32) -> Result<String, String>,
) -> Result<Action, String> {
    let mut temperature = 0.2;
    for attempt in 0..2 {
        if cancelled.load(Ordering::Acquire) {
            return Err("Judgement cancelled.".into());
        }
        match generate(temperature) {
            Ok(content) => {
                if cancelled.load(Ordering::Acquire) {
                    return Err("Judgement cancelled.".into());
                }
                return serde_json::from_str(&content)
                    .map_err(|e| format!("The reader returned an incomplete action: {e}"));
            }
            Err(error)
                if attempt == 0
                    && error.contains("failed to decode controlled token: Unknown Token Type")
                    && !cancelled.load(Ordering::Acquire) =>
            {
                audit.push(json!({"generation_retry":error,"selector":"greedy","attempt":2}));
                temperature = 0.;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the second attempt always returns")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeardAction {
    heard: String,
    call: Action,
}
fn heard_action(content: &str) -> Result<HeardAction, String> {
    let content = content.trim();
    let content = content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(content)
        .trim();
    let heard: HeardAction = serde_json::from_str(content)
        .map_err(|_| "The spoken question needs another hearing.".to_owned())?;
    if heard.heard.trim().is_empty() || heard.heard.len() > 8000 {
        return Err("The spoken question needs another hearing.".into());
    }
    Ok(heard)
}
fn direct_audio_prompt(prompt: &str, action_schema: &str) -> Result<String, String> {
    let mut messages: Vec<Value> = serde_json::from_str(prompt).map_err(|e| e.to_string())?;
    messages.push(json!({"role":"user","content":format!("Hear the attached speech as the person's next turn. Understand and respond to it directly. Return one JSON object with two fields: heard (a short faithful summary of what they meant, never claimed as verbatim) and call (one action using this schema: {action_schema}). Ask a brief clarification if their words are unclear. No commentary outside the JSON object.")}));
    Ok(json!(messages).to_string())
}
```

## Complete orchestration loop

Source: `src-tauri/src/conversation.rs`.

```rust
fn run(app: &tauri::AppHandle, input: TurnInput) -> Result<Session, String> {
    let started = std::time::Instant::now();
    let state = app.state::<ConversationState>();
    if state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("A reply is already in progress.".into());
    }
    let _lease = Lease(&state.busy);
    // Acquire the conversation before consuming the one-use voice receipt.
    // A competing turn must leave the pending words intact.
    let (text, voice) = match input {
        TurnInput::Text(text) => (text, None),
        TurnInput::Voice(id) => {
            let voice = app.state::<crate::voice::VoiceState>().take(id)?;
            let text = match &voice.payload {
                crate::voice::VoicePayload::Text(text) => text.clone(),
                crate::voice::VoicePayload::Audio(_) => "Your spoken question…".into(),
            };
            (text, Some(voice))
        }
    };
    // Preparation and generation cannot move the question's submitted moment.
    let instant = voice.as_ref().map_or_else(now_ms, |v| v.received_at_ms);
    state.cancelled.store(false, Ordering::Release);
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut session = state.load(&dir)?;
    if text.trim().is_empty() || text.len() > 8000 {
        return Err("Please send a message of 1–8000 bytes.".into());
    }
    session.messages.push(Message {
        role: "user".into(),
        text,
    });
    let spoken_message = session.messages.len() - 1;
    let mut audio = None;
    if let Some(voice) = voice {
        session.audit.push(json!({"event":"voice_input","route":voice.mode,"preparationMs":voice.preparation_ms,"audioPersisted":false}));
        note(
            &mut session,
            &dir,
            "voice",
            match voice.mode {
                crate::voice::VoiceMode::Native => "Your spoken words became writing here.",
                crate::voice::VoiceMode::GemmaTranscription => {
                    "Your spoken words were transcribed for comparison."
                }
                _ => "The reader is hearing your question directly.",
            },
            voice.preparation_ms,
        );
        if let crate::voice::VoicePayload::Audio(bytes) = voice.payload {
            audio = Some(bytes);
        }
    }
    note(
        &mut session,
        &dir,
        "received",
        "Your words were kept. The question's moment was noted.",
        0,
    );
    {
        use sha2::{Digest, Sha256};
        session.audit.push(json!({"event":"user_turn","build":env!("HORARY_BUILD_GIT_SHA"),"model":"gemma-4-12b-qat","policySha256":format!("{:x}",Sha256::digest(format!("{PROMPT}\n{BOOK}"))),"modelManifest":crate::model_manifest::bundled_model_manifest().map_err(|e|e.message)?}));
    }
    session.status = "Preparing the local reader…".into();
    state.publish(&mut session, &dir)?;
    let result: Result<(), String> = (|| {
        let acquisition = app.state::<AcquisitionState>();
        if !acquisition.0.status(&dir).map_err(|e| e.message)?.ready {
            note(
                &mut session,
                &dir,
                "preparing",
                "The reader is quietly preparing.",
                started.elapsed().as_millis() as u64,
            );
            acquisition.0.begin().map_err(|e| e.message)?;
            acquisition.0.run(&dir).map_err(|e| e.message)?;
        }
        state.check()?;
        let native = app.state::<NativeLlamaState>();
        start_native_llama_in_dir(
            &dir,
            &native,
            serde_json::from_value(
                json!({"modelId":"gemma-4-12b-qat","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.message)?;
        note(
            &mut session,
            &dir,
            "ready",
            "The reader is here, with the book's method at hand.",
            started.elapsed().as_millis() as u64,
        );
        let mut thread = PromptThread::new(&session);
        for _ in 0..8 {
            state.check()?;
            session.status = "Considering your question…".into();
            state.publish(&mut session, &dir)?;
            let action_schema = schema(&session);
            let first_audio = audio.take();
            let prompt = thread.content();
            let prompt = if first_audio.is_some() {
                direct_audio_prompt(&prompt, &action_schema)?
            } else {
                prompt
            };
            let prompt_hash = {
                use sha2::{Digest, Sha256};
                format!("{:x}", Sha256::digest(&prompt))
            };
            let inference_start = std::time::Instant::now();
            let mut measurements = Vec::new();
            let generated = if let Some(bytes) = first_audio {
                generate_native(
                    &native,
                    prompt.clone(),
                    NativeGenerateOptions {
                        audio: Some(bytes),
                        max_tokens: 400,
                        temperature: 0.,
                        cancel: Some(state.cancelled.clone()),
                        ..Default::default()
                    },
                )
                .map_err(|e| e.message)
                .and_then(|answer| {
                    measurements.push(inference_measurement(&answer));
                    let heard = heard_action(&answer.content)?;
                    state.check()?;
                    session.messages[spoken_message].text =
                        format!("From your spoken words: {}", heard.heard.trim());
                    session
                        .audit
                        .push(json!({"voiceUnderstanding":heard.heard,"verbatim":false}));
                    thread = PromptThread::new(&session);
                    Ok(heard.call)
                })
            } else {
                generate_action(&state.cancelled, &mut session.audit, |temperature| {
                    generate_native(
                    &native,
                    prompt.clone(),
                    NativeGenerateOptions {
                        max_tokens: 800,
                        temperature,
                        response_schema: Some(action_schema.clone()),
                        cancel: Some(state.cancelled.clone()),
                        ..Default::default()
                    },
                )
                .map(|answer| {
                    log::info!("reader inference: prompt_tokens={} output_tokens={} duration_ms={} tokens_per_second={:.2}",answer.prompt_tokens,answer.generated_tokens,answer.elapsed_ms,answer.tokens_per_second);
                    measurements.push(inference_measurement(&answer));
                    answer.content
                })
                .map_err(|e| e.message)
                })
            };
            for measurement in measurements {
                session
                    .audit
                    .push(json!({"generation":measurement,"promptSha256":prompt_hash}));
                if let Ok(item) = review_progress::record_with_inference(
                    &dir,
                    "inference",
                    "The reader considered this step.",
                    measurement.elapsed_ms,
                    Some(measurement),
                ) {
                    session.progress.push(item);
                }
            }
            let action = generated?;
            note(
                &mut session,
                &dir,
                "considered",
                "A piece of the question was considered.",
                inference_start.elapsed().as_millis() as u64,
            );
            state.check()?;
            if let Action::Say { message } = action {
                if message.trim().is_empty() {
                    return Err("The reader returned an empty reply. Please try again.".into());
                }
                session.messages.push(Message {
                    role: "assistant".into(),
                    text: message,
                });
                note(
                    &mut session,
                    &dir,
                    "reply",
                    "The conversation returns to you.",
                    started.elapsed().as_millis() as u64,
                );
                return Ok(());
            }
            let call = serde_json::to_value(&action).map_err(|e| e.to_string())?;
            session.status = match action {
                Action::FindPlace { .. } => "Finding the place…",
                Action::CastChart { .. } => "Calculating the chart…",
                Action::WriteScroll { .. } => "The reading is taking shape…",
                _ => "Consulting the method…",
            }
            .into();
            state.publish(&mut session, &dir)?;
            let result = execute(&mut session, action, &app.state::<GeocodeState>(), instant);
            let detail = match call["action"].as_str() {
                Some("find_place") => "A place was sought.",
                Some("cast_chart") => "The chart was calculated for the question's moment.",
                Some("write_scroll") => "A passage was connected to chart facts and a book rule.",
                Some("restore_reading") => "An earlier reading was brought back.",
                Some("new_question") => "A new question opened a new leaf.",
                _ => "The evidence was consulted.",
            };
            note(
                &mut session,
                &dir,
                if result.is_ok() {
                    "completed"
                } else {
                    "needs_attention"
                },
                if result.is_ok() {
                    detail
                } else {
                    "This step needs a correction before it can continue."
                },
                started.elapsed().as_millis() as u64,
            );
            let receipt = json!({"call":call,"result":match result {Ok(value)=>value,Err(error)=>json!({"error":error})},"revision":session.revision});
            let judgment_written =
                receipt["result"]["written"] == true && receipt["call"]["step"] == "judgment";
            session.audit.push(receipt.clone());
            thread.update(&session, Some(&receipt));
            state.publish(&mut session, &dir)?;
            if judgment_written && finish_working_reading(&mut session) {
                note(
                    &mut session,
                    &dir,
                    "reply",
                    "The chart and book method are ready to examine; the answer remains open.",
                    started.elapsed().as_millis() as u64,
                );
                return Ok(());
            }
        }
        Err("I’ve paused here to keep the reading focused. Tell me what you’d like to explore next.".into())
    })();
    session.status.clear();
    session.busy = false;
    if let Err(error) = result {
        note(
            &mut session,
            &dir,
            "paused",
            "The reading paused; your words and completed passages were kept.",
            started.elapsed().as_millis() as u64,
        );
        session
            .audit
            .push(json!({"interruption":error,"revision":session.revision}));
        session.messages.push(Message {
            role: "assistant".into(),
            text: if state.cancelled.load(Ordering::Acquire) { "We can pause here. Tell me what you’d like to change." } else { "I lost my place for a moment. Your words are still here; tell me where you’d like to continue." }.into(),
        });
    }
    state.publish(&mut session, &dir)?;
    Ok(session)
}
```

## Fixed native closing

Source: `src-tauri/src/conversation.rs`.

```rust
fn finish_working_reading(session: &mut Session) -> bool {
    if ![Step::Significators, Step::Testimony, Step::Judgment]
        .into_iter()
        .all(|step| session.sections.iter().any(|s| s.step == Some(step)))
    {
        return false;
    }
    session.messages.push(Message {role:"assistant".into(),text:"Your chart and its testimony are here. The answer remains open while we check how the pieces fit. You can explore the margins, or tell me anything that needs correcting.".into()});
    session.audit.push(json!({"event":"working_reading_ready","narration":"native","reason":"calculation_limits","revision":session.revision}));
    true
}
```

## Visible passage versus proposed interpretation

Source: `src/ReadingDocument.tsx`.

```tsx
export function ReadingPassage({ section }: { section: Section }) {
  return <section className="document-section" data-step={section.step}>
    <h2>{section.title}</h2>
    {!!section.roles?.length && <div className="roles-thread" aria-label="Who the chart represents">{section.roles.map((role, i) => <p key={i}><span>{role.label}</span><span className="thread-line" /><a href="#reading-chart" title={role.reason}>{role.planet}</a><small>{role.house ? `house ${role.house}` : 'natural role'}</small></p>)}</div>}
    {section.step && section.step !== 'significators' && !!section.facts?.length && <div className="calculated-testimony" aria-label="Calculated testimony">{section.facts.filter(f => f.kind !== 'boundary').slice(0, 2).map(f => <p key={f.id}>{factText(f.detail)}</p>)}</div>}
    {section.body.split('\n\n').map((p, n) => <p key={n}>{p}</p>)}
    <details className="margin-note"><summary>How this follows</summary>
      {section.facts?.length ? <><h3>In the chart</h3>{section.facts.map(f => <p key={f.id}>{factText(f.detail)}</p>)}</> : <p>The evidence behind this earlier passage was not recorded in this form.</p>}
      {!!section.roles?.length && <><h3>Why these roles</h3>{section.roles.map((r, i) => <p key={i}><em>{r.label}:</em> {r.reason}</p>)}</>}
      {!!section.rules?.length && <><h3>From the book</h3>{section.rules.map(rule => <div key={rule.id}><p><em>{rule.title}.</em> {rule.explanation}</p><p className="source-note">The Horary Textbook · printed pp. {rule.pages} · editorial paraphrase</p></div>)}</>}
      {section.because && <><h3>A proposed interpretation</h3>{section.draft?.split('\n\n').map((p,i)=><p key={i}>{p}</p>)}<p>{section.because}</p><p className="source-note">A working proposal to assess against the facts above. The cited facts and rule let you assess it; they do not prove it correct.</p></>}
    </details>
  </section>
}

```

## Actual speech route selection

Source: `src-tauri/src/voice.rs`.

```rust
pub async fn voice_finish(app: tauri::AppHandle) -> Result<VoiceReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<VoiceState>();
        state
            .finishing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "The last words are still being heard.")?;
        struct FinishLease<'a>(&'a AtomicBool);
        impl Drop for FinishLease<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _lease = FinishLease(&state.finishing);
        let bytes = state
            .capture
            .stop("horary-voice".into())
            .map_err(|e| e.to_string())?;
        if !has_sound(&bytes) {
            return Err("No audible words were recorded.".into());
        }
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "The question's moment could not be noted.")?
            .as_secs_f64()
            * 1000.;
        let started = std::time::Instant::now();
        let mode = VoiceMode::configured()?;
        let (mode, payload) = match mode {
            VoiceMode::Direct => (mode, VoicePayload::Audio(bytes)),
            VoiceMode::GemmaTranscription => (
                mode,
                VoicePayload::Text(crate::conversation::transcribe(&app, bytes)?),
            ),
            VoiceMode::Native | VoiceMode::Auto => {
                match crate::local_dictation::transcribe(&bytes, &state.cancelled) {
                    Ok(text) => (VoiceMode::Native, VoicePayload::Text(text)),
                    Err(error) if mode == VoiceMode::Native => return Err(error),
                    Err(_) => {
                        log::info!("voice: local dictation unavailable; using direct local audio");
                        (VoiceMode::Direct, VoicePayload::Audio(bytes))
                    }
                }
            }
        };
        let preparation_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let text = match &payload {
            VoicePayload::Text(text) => Some(text.clone()),
            VoicePayload::Audio(_) => None,
        };
        let id = state.next_id.fetch_add(1, Ordering::Relaxed);
        let mut pending = state
            .pending
            .lock()
            .map_err(|_| "Listening is unavailable.")?;
        if state.cancelled.load(Ordering::Acquire) {
            return Err("Listening cancelled.".into());
        }
        *pending = Some((
            id,
            PendingVoice {
                payload,
                received_at_ms,
                mode,
                preparation_ms,
            },
        ));
        log::info!(
            "voice: route={} preparation_ms={preparation_ms}",
            serde_json::to_string(&mode).unwrap_or_default()
        );
        Ok(VoiceReceipt { id, text })
    })
    .await
    .map_err(|e| e.to_string())?
}
```

## On-device transcription and its waits

Source: `src-tauri/src/local_dictation.rs`.

```rust
fn claim_authorization_request(requested: &AtomicBool) -> Result<(), String> {
    requested
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| "Local dictation permission is still waiting.".into())
}

pub fn transcribe(wav: &[u8], cancelled: &AtomicBool) -> Result<String, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Listening cancelled.".into());
    }
    #[cfg(target_os = "macos")]
    {
        apple(wav, cancelled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wav;
        Err("Local dictation is not available here.".into())
    }
}

#[cfg(target_os = "macos")]
fn apple(wav: &[u8], cancelled: &AtomicBool) -> Result<String, String> {
    use speech::{
        recognizer::SpeechRecognizer,
        request::{
            AudioBufferRecognitionRequest, CallbackQueue, RecognitionRequestOptions, TaskHint,
        },
        task::RecognitionTaskEvent,
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let recognizer = SpeechRecognizer::new().with_callback_queue(CallbackQueue::background());
    if !recognizer.is_available()
        || !recognizer
            .supports_on_device_recognition()
            .map_err(|e| e.to_string())?
    {
        return Err("Local dictation is not available for this language.".into());
    }
    let mut authorization = SpeechRecognizer::authorization_status();
    if authorization == speech::error::AuthorizationStatus::NotDetermined {
        // Read authorization on every turn, but ask only once per process. A
        // pending dialog must not add another six-second wait to each reply.
        claim_authorization_request(&AUTHORIZATION_REQUESTED)?;
        // A permission dialog must not trap the owned voice worker for the
        // framework's synchronous 30-second wait. Dropping this future is safe.
        authorization = tauri::async_runtime::block_on(async {
            let authorization = speech::async_api::AsyncSpeechRecognizer::request_authorization();
            tokio::pin!(authorization);
            let deadline = Instant::now() + Duration::from_secs(6);
            loop {
                tokio::select! {
                    result = &mut authorization => return result.map_err(|e| e.to_string()),
                    _ = tokio::time::sleep(Duration::from_millis(30)) => {
                        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                            return Err("Local dictation permission is not ready.".into());
                        }
                    }
                }
            }
        })?;
    }
    if !authorization.is_authorized() {
        return Err("Local dictation was not permitted.".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return Err("Listening cancelled.".into());
    }
    let samples = pcm_samples(wav)?;
    let request = AudioBufferRecognitionRequest::new().with_options(
        RecognitionRequestOptions::new()
            .with_requires_on_device_recognition(true)
            .with_should_report_partial_results(false)
            .with_task_hint(TaskHint::Dictation),
    );
    let (tx, rx) = mpsc::channel();
    let task = recognizer
        .start_audio_buffer_task(&request, move |event| match event {
            RecognitionTaskEvent::DidFinishRecognition(result) => {
                let _ = tx.send(Ok(result.transcript().to_owned()));
            }
            RecognitionTaskEvent::DidFinishSuccessfully(false) => {
                let _ = tx.send(Err("Local dictation could not finish.".to_owned()));
            }
            _ => {}
        })
        .map_err(|e| e.to_string())?;
    // The task owns its native copy. No audio URL or temporary recording exists.
    let result = (|| {
        for chunk in samples.chunks(16000) {
            if cancelled.load(Ordering::Acquire) {
                return Err("Listening cancelled.".into());
            }
            task.append_interleaved_i16(16000., 1, chunk)
                .map_err(|e| e.to_string())?;
        }
        task.end_audio();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("Listening cancelled.".into());
            }
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(result) => return result.and_then(checked_text),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Local dictation stopped.".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if task.error().is_some() {
                return Err("Local dictation could not finish.".into());
            }
            if Instant::now() >= deadline {
                return Err("Local dictation took too long.".into());
            }
        }
    })();
    task.cancel();
    result
}
```
