//! Short references economize model tokens; the host restores exact citations.
#![forbid(unsafe_code)]
use crate::{packet::Packet, store::Result};
use horary_prompt_program::Evidence;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub citations: BTreeMap<String, Evidence>,
}
impl Index {
    pub fn verify_prompt(&self, prompt: &str) -> Result<()> {
        let submitted = prompt
            .split_once("COMPACT PACKET:\n")
            .or_else(|| prompt.split_once("COMPACT TRAINING PACKET:\n"));
        let Some((_, source)) = submitted else {
            return if self.citations.is_empty() {
                Ok(())
            } else {
                Err("Evidence map has no original submitted receipt table".into())
            };
        };
        let context: Value = serde_json::from_str(source)
            .map_err(|e| format!("Read original submitted receipt table: {e}"))?;
        if context["receipt_table"] != self.table() {
            return Err("Evidence map differs from the original submitted receipt table".into());
        }
        Ok(())
    }
    fn add(&mut self, evidence: Evidence) -> String {
        if let Some((id, _)) = self.citations.iter().find(|(_, old)| *old == &evidence) {
            return id.clone();
        }
        let id = format!("r{}", self.citations.len() + 1);
        self.citations.insert(id.clone(), evidence);
        id
    }
    fn citation(
        &mut self,
        packet: &Packet,
        case_id: &str,
        file: &str,
        pointer: &str,
    ) -> Result<String> {
        let case = packet
            .cases
            .iter()
            .find(|c| c.id == case_id)
            .ok_or("Unknown citation case")?;
        let reference = case
            .files
            .iter()
            .find(|f| f.file == file)
            .ok_or("Citation file absent")?;
        Ok(self.add(Evidence {
            case_id: case_id.into(),
            file: file.into(),
            json_pointer: pointer.into(),
            sha256: reference.sha256.clone(),
        }))
    }
    pub fn expand(&self, value: &mut Value) -> Result<()> {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.expand(item)?;
                }
            }
            Value::Object(fields) => {
                if let Some(refs) = fields.remove("evidence_refs") {
                    if fields.contains_key("evidence") {
                        return Err("Both short and full evidence provided".into());
                    }
                    let refs = refs.as_array().ok_or("Evidence refs must be an array")?;
                    if refs.len() > 2 {
                        return Err("At most two evidence references per claim".into());
                    }
                    let mut expanded = Vec::new();
                    for reference in refs {
                        let key = reference
                            .as_str()
                            .ok_or("Evidence reference must be a string")?;
                        expanded.push(
                            serde_json::to_value(
                                self.citations
                                    .get(key)
                                    .ok_or_else(|| format!("Unknown evidence reference {key}"))?,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                    fields.insert("evidence".into(), Value::Array(expanded));
                }
                for field in fields.values_mut() {
                    self.expand(field)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn shorten(&mut self, value: &mut Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.shorten(item);
                }
            }
            Value::Object(fields) => {
                if let Some(evidence) = fields.remove("evidence") {
                    let refs = evidence
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| serde_json::from_value::<Evidence>(v.clone()).ok())
                        .take(2)
                        .map(|e| Value::String(self.add(e)))
                        .collect();
                    fields.insert("evidence_refs".into(), Value::Array(refs));
                }
                for field in fields.values_mut() {
                    self.shorten(field);
                }
            }
            _ => {}
        }
    }
    fn table(&self) -> Value {
        let mut files: BTreeMap<(String, String, String), String> = BTreeMap::new();
        let mut file_table = BTreeMap::new();
        let mut receipts = BTreeMap::new();
        for (id, evidence) in &self.citations {
            let key = (
                evidence.case_id.clone(),
                evidence.file.clone(),
                evidence.sha256.clone(),
            );
            let next = format!("f{}", files.len() + 1);
            let file_id = files.entry(key).or_insert(next).clone();
            file_table.entry(file_id.clone()).or_insert_with(||json!({"case_id":evidence.case_id,"file":evidence.file,"sha256":evidence.sha256}));
            receipts.insert(
                id.clone(),
                json!({"file":file_id,"pointer":evidence.json_pointer}),
            );
        }
        json!({"files":file_table,"receipts":receipts})
    }
}

fn summarize_contract(schema: &Value) -> Value {
    match schema {
        Value::Object(fields) => {
            let mut kept = serde_json::Map::new();
            for (key, value) in fields {
                if key == "enum" && value.as_array().is_some_and(|v| v.len() > 20) {
                    kept.insert("enum_count".into(), json!(value.as_array().map(Vec::len)));
                } else {
                    kept.insert(key.clone(), summarize_contract(value));
                }
            }
            Value::Object(kept)
        }
        Value::Array(items) => Value::Array(items.iter().map(summarize_contract).collect()),
        _ => schema.clone(),
    }
}
// Reviewers may group a failure across adjacent stages ("condition/reception").
// Preserve that finding, but select each actual executor stage independently.
fn finding_stages(finding: &Value) -> impl Iterator<Item = &str> {
    finding["stage"]
        .as_str()
        .into_iter()
        .flat_map(|stage| stage.split('/').map(str::trim))
        .filter(|stage| {
            matches!(
                *stage,
                "intake"
                    | "place"
                    | "moment"
                    | "significators"
                    | "condition"
                    | "reception"
                    | "contacts"
                    | "location"
                    | "judgment"
                    | "explanation"
                    | "conversation"
            )
        })
}

fn prompt_scopes(reviews: &[Value]) -> BTreeSet<(String, Option<String>, Option<String>)> {
    reviews
        .iter()
        .flat_map(|review| review["findings"].as_array().into_iter().flatten())
        .filter(|finding| finding["repair_owner"] == "prompt")
        .flat_map(|finding| {
            finding_stages(finding).map(|stage| {
                (
                    stage.into(),
                    finding["recognition_phase"].as_str().map(str::to_owned),
                    finding["method"].as_str().map(str::to_owned),
                )
            })
        })
        .collect()
}

/// Keep mutable regions separate: joining across a withheld quotation would
/// give the writer an old_text that never existed in the actual system guide.
fn guide_document(
    text: &str,
    chunks: &mut Vec<String>,
    chunk_ids: &mut BTreeMap<String, usize>,
) -> Result<Value> {
    let parts = horary_prompt_program::guide_parts(text)?;
    let teaching_regions = parts
        .teaching
        .iter()
        .map(|region| {
            let ids = region
                .split_inclusive("\n\n")
                .map(|part| {
                    if let Some(id) = chunk_ids.get(part) {
                        *id
                    } else {
                        let id = chunks.len();
                        chunks.push(part.to_owned());
                        chunk_ids.insert(part.to_owned(), id);
                        id
                    }
                })
                .collect::<Vec<_>>();
            json!({"sha256":horary_prompt_program::digest(region),"bytes":region.len(),"chunks":ids})
        })
        .collect::<Vec<_>>();
    let quoted_source = parts
        .quoted_source
        .iter()
        .map(|block| json!({"sha256":horary_prompt_program::digest(block),"bytes":block.len()}))
        .collect::<Vec<_>>();
    Ok(json!({"teaching_regions":teaching_regions,"quoted_source":quoted_source}))
}

/// Share exact repeated values, never approximate evidence. The marker cannot
/// collide with an original object key, and IDs use exact serialized equality
/// rather than a truncated digest. Case identities and citation tables stay in
/// place so the host can inspect them without resolving the dictionary.
fn intern_reading_view(view: &mut Value) -> Result<()> {
    const MIN_BYTES: usize = 16;
    fn has_key(value: &Value, key: &str) -> bool {
        match value {
            Value::Object(fields) => {
                fields.contains_key(key) || fields.values().any(|v| has_key(v, key))
            }
            Value::Array(items) => items.iter().any(|v| has_key(v, key)),
            _ => false,
        }
    }
    fn eligible(path: &[String]) -> bool {
        if path.len() < 2 || path[0] == "receipt_table" {
            return false;
        }
        if path[0] == "cases" {
            // Keep summary/state containers and the observed result tag direct.
            if path.len() <= 3
                || (path.len() == 4
                    && [
                        "first_state",
                        "final_state",
                        "reading_rubric",
                        "evidence_refs",
                    ]
                    .contains(&path[3].as_str()))
                || (path.len() == 5
                    && ["first_state", "final_state"].contains(&path[3].as_str())
                    && path[4] == "methodResult")
            {
                return false;
            }
        }
        true
    }
    fn strings(value: &Value, candidates: &mut BTreeSet<String>) {
        match value {
            Value::String(text) if text.len() >= 64 => {
                candidates.insert(text.clone());
            }
            Value::Object(fields) => {
                for value in fields.values() {
                    strings(value, candidates);
                }
            }
            Value::Array(items) => {
                for value in items {
                    strings(value, candidates);
                }
            }
            _ => {}
        }
    }
    fn text_parts(text: &str, candidates: &[String], marker: &str) -> Value {
        let mut offset = 0;
        let mut parts = Vec::new();
        while offset < text.len() {
            let next = candidates
                .iter()
                .filter(|candidate| candidate.len() < text.len())
                .filter_map(|candidate| text[offset..].find(candidate).map(|at| (at, candidate)))
                .min_by(|(at, a), (other_at, b)| {
                    at.cmp(other_at)
                        .then_with(|| b.len().cmp(&a.len()))
                        .then_with(|| a.cmp(b))
                });
            let Some((at, candidate)) = next else {
                break;
            };
            if at > 0 {
                parts.push(json!(&text[offset..offset + at]));
            }
            parts.push(text_parts(candidate, candidates, marker));
            offset += at + candidate.len();
        }
        if parts.is_empty() {
            return json!(text);
        }
        if offset < text.len() {
            parts.push(json!(&text[offset..]));
        }
        json!({marker:parts})
    }
    fn segment(value: &mut Value, path: &mut Vec<String>, candidates: &[String], marker: &str) {
        if path.first().is_some_and(|p| p == "receipt_table") {
            return;
        }
        match value {
            Value::String(text) if eligible(path) => *value = text_parts(text, candidates, marker),
            Value::Object(fields) => {
                for (key, value) in fields {
                    path.push(key.clone());
                    segment(value, path, candidates, marker);
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    path.push(index.to_string());
                    segment(value, path, candidates, marker);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    fn count(
        value: &Value,
        path: &mut Vec<String>,
        counts: &mut BTreeMap<String, usize>,
    ) -> Result<()> {
        if path.first().is_some_and(|p| p == "receipt_table") {
            return Ok(());
        }
        if eligible(path) {
            let key = serde_json::to_string(value).map_err(|e| e.to_string())?;
            if key.len() >= MIN_BYTES {
                *counts.entry(key).or_default() += 1;
            }
        }
        match value {
            Value::Object(fields) => {
                for (name, value) in fields {
                    path.push(name.clone());
                    count(value, path, counts)?;
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    path.push(index.to_string());
                    count(value, path, counts)?;
                    path.pop();
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn replace(
        value: &mut Value,
        path: &mut Vec<String>,
        counts: &BTreeMap<String, usize>,
        marker: &str,
        ids: &mut BTreeMap<String, String>,
        values: &mut BTreeMap<String, Value>,
    ) -> Result<()> {
        if path.first().is_some_and(|p| p == "receipt_table") {
            return Ok(());
        }
        let key = if eligible(path) {
            let key = serde_json::to_string(value).map_err(|e| e.to_string())?;
            (counts.get(&key).copied().unwrap_or_default() > 1).then_some(key)
        } else {
            None
        };
        if let Some(id) = key.as_ref().and_then(|key| ids.get(key)) {
            *value = json!({marker:id});
            return Ok(());
        }
        match value {
            Value::Object(fields) => {
                for (name, value) in fields {
                    path.push(name.clone());
                    replace(value, path, counts, marker, ids, values)?;
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    path.push(index.to_string());
                    replace(value, path, counts, marker, ids, values)?;
                    path.pop();
                }
            }
            _ => {}
        }
        if let Some(key) = key {
            let id = format!("{:x}", ids.len() + 1);
            ids.insert(key, id.clone());
            values.insert(id.clone(), std::mem::replace(value, json!({marker:id})));
        }
        Ok(())
    }
    fn ref_counts(value: &Value, marker: &str, counts: &mut BTreeMap<String, usize>) {
        match value {
            Value::Object(fields) => {
                if fields.len() == 1 {
                    if let Some(id) = fields.get(marker).and_then(Value::as_str) {
                        *counts.entry(id.into()).or_default() += 1;
                        return;
                    }
                }
                for value in fields.values() {
                    ref_counts(value, marker, counts);
                }
            }
            Value::Array(items) => {
                for value in items {
                    ref_counts(value, marker, counts);
                }
            }
            _ => {}
        }
    }
    fn inline(value: &mut Value, marker: &str, id: &str, replacement: &Value) {
        match value {
            Value::Object(fields) => {
                if fields.len() == 1 && fields.get(marker).and_then(Value::as_str) == Some(id) {
                    *value = replacement.clone();
                    return;
                }
                for value in fields.values_mut() {
                    inline(value, marker, id, replacement);
                }
            }
            Value::Array(items) => {
                for value in items {
                    inline(value, marker, id, replacement);
                }
            }
            _ => {}
        }
    }
    fn object_shapes(
        value: &Value,
        path: &mut Vec<String>,
        counts: &mut BTreeMap<Vec<String>, usize>,
    ) {
        if path.first().is_some_and(|p| p == "receipt_table") {
            return;
        }
        match value {
            Value::Object(fields) => {
                if eligible(path) && fields.len() > 1 {
                    *counts.entry(fields.keys().cloned().collect()).or_default() += 1;
                }
                for (key, value) in fields {
                    path.push(key.clone());
                    object_shapes(value, path, counts);
                    path.pop();
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    path.push(index.to_string());
                    object_shapes(value, path, counts);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    fn share_object_keys(
        value: &mut Value,
        path: &mut Vec<String>,
        shapes: &BTreeMap<Vec<String>, String>,
        marker: &str,
        object_marker: &str,
    ) {
        if path.first().is_some_and(|p| p == "receipt_table") {
            return;
        }
        match value {
            Value::Object(fields) => {
                let keys: Vec<_> = fields.keys().cloned().collect();
                for (key, value) in fields.iter_mut() {
                    path.push(key.clone());
                    share_object_keys(value, path, shapes, marker, object_marker);
                    path.pop();
                }
                if eligible(path) {
                    if let Some(id) = shapes.get(&keys) {
                        let ordered: Vec<_> = std::mem::take(fields).into_values().collect();
                        *value = json!({object_marker:[{marker:id},ordered]});
                    }
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    path.push(index.to_string());
                    share_object_keys(value, path, shapes, marker, object_marker);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let original = view.clone();
    let original_bytes = serde_json::to_vec(&original).map_err(|e| e.to_string())?;
    let mut marker = "@".to_owned();
    while has_key(view, &marker) {
        marker.push('_');
    }
    let mut text_marker = "~".to_owned();
    while has_key(view, &text_marker) {
        text_marker.push('_');
    }
    let mut object_marker = "#".to_owned();
    while has_key(view, &object_marker) {
        object_marker.push('_');
    }
    let mut candidates = BTreeSet::new();
    strings(view, &mut candidates);
    segment(
        view,
        &mut Vec::new(),
        &candidates.into_iter().collect::<Vec<_>>(),
        &text_marker,
    );
    let mut counts = BTreeMap::new();
    count(view, &mut Vec::new(), &mut counts)?;
    let mut values = BTreeMap::new();
    replace(
        view,
        &mut Vec::new(),
        &counts,
        &marker,
        &mut BTreeMap::new(),
        &mut values,
    )?;
    // Sharing a parent can leave a child referenced once. Inline those children
    // so the dictionary contains only values that still occur more than once.
    loop {
        let mut uses = BTreeMap::new();
        ref_counts(view, &marker, &mut uses);
        for value in values.values() {
            ref_counts(value, &marker, &mut uses);
        }
        let single: Vec<_> = values
            .iter()
            .filter(|(id, value)| {
                let occurrences = uses.get(*id).copied().unwrap_or_default();
                let size = value.to_string().len();
                let reference_size = json!({&marker:id}).to_string().len();
                occurrences <= 1
                    || size.saturating_mul(occurrences - 1)
                        <= reference_size * occurrences + id.len() + 4
            })
            .map(|(id, _)| id.clone())
            .collect();
        if single.is_empty() {
            break;
        }
        for id in single {
            let replacement = values
                .remove(&id)
                .expect("collected existing dictionary ID");
            inline(view, &marker, &id, &replacement);
            for value in values.values_mut() {
                inline(value, &marker, &id, &replacement);
            }
        }
    }
    // Distinct worksheets/contracts often have the same long member names.
    // Share their ordered keys as well; every original field/value is retained.
    let mut shape_counts = BTreeMap::new();
    object_shapes(view, &mut Vec::new(), &mut shape_counts);
    for value in values.values() {
        object_shapes(
            value,
            &mut vec!["dictionary".into(), "value".into()],
            &mut shape_counts,
        );
    }
    let mut shapes = BTreeMap::new();
    let mut next = values
        .keys()
        .filter_map(|id| usize::from_str_radix(id, 16).ok())
        .max()
        .unwrap_or_default()
        + 1;
    for (keys, occurrences) in shape_counts {
        let id = format!("{next:x}");
        let original: serde_json::Map<String, Value> =
            keys.iter().map(|key| (key.clone(), Value::Null)).collect();
        let encoded = json!({&object_marker:[{&marker:&id},vec![Value::Null;keys.len()]]});
        let saving = Value::Object(original)
            .to_string()
            .len()
            .saturating_sub(encoded.to_string().len());
        if saving * occurrences > json!(keys).to_string().len() + id.len() + 4 {
            values.insert(id.clone(), json!(keys));
            shapes.insert(keys, id);
            next += 1;
        }
    }
    share_object_keys(view, &mut Vec::new(), &shapes, &marker, &object_marker);
    for value in values.values_mut() {
        share_object_keys(
            value,
            &mut vec!["dictionary".into(), "value".into()],
            &shapes,
            &marker,
            &object_marker,
        );
    }
    if !values.is_empty() || *view != original {
        view["context_interning"] = json!({"format":"exact-json-values-v1","ref_key":marker,
            "text_key":text_marker,"object_key":object_marker,
            "expanded_sha256":horary_prompt_program::digest(&original_bytes),
            "instructions":"An object containing only ref_key resolves to the named value in values. An object containing only text_key is an ordered array of exact text parts: resolve each part and concatenate without separators. An object containing only object_key is [ordered_keys,ordered_values]: resolve both arrays, then pair each key with its value to reconstruct the object. Resolve recursively before interpreting a field. All facts, roles, requests, worksheets, source quotations and RAW strings retain their exact values and bytes. Value IDs are not evidence_refs: cite only receipt_table IDs.",
            "values":values});
    }
    if serde_json::to_vec(view).map_err(|e| e.to_string())?.len() >= original_bytes.len() {
        *view = original;
    }
    Ok(())
}

/// All full primary files are preserved in Packet; this is the bounded model view.
pub fn context(packet: &Packet, reviews: Option<&[Value]>) -> Result<(Value, Index)> {
    context_view(packet, reviews, true)
}

fn context_view(packet: &Packet, reviews: Option<&[Value]>, share: bool) -> Result<(Value, Index)> {
    let mut index = Index::default();
    let mut cases = Vec::new();
    let mut calls = Vec::new();
    let mut outputs = BTreeMap::new();
    let mut output_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    let mut input_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut consultations = BTreeMap::new();
    let scopes = reviews.map(prompt_scopes);
    let edit_stage = reviews.and_then(|reviews| {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for finding in reviews
            .iter()
            .flat_map(|review| review["findings"].as_array().into_iter().flatten())
        {
            if finding["repair_owner"] == "prompt" {
                for stage in finding_stages(finding) {
                    *counts.entry(stage.into()).or_default() += 1;
                }
            }
        }
        counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(stage, _)| stage)
    });
    let mut selected_guides = Vec::new();
    let mut source_documents = BTreeMap::new();
    let full_reading = packet
        .cases
        .iter()
        .any(|case| case.summary["full_reading"] == true);
    let full_reading_judge = reviews.is_none() && full_reading;
    let mut guide_documents = BTreeMap::new();
    let mut chunks = Vec::new();
    let mut chunk_ids = BTreeMap::new();
    let mut schema_hashes = BTreeSet::new();
    for guide in &packet.guides {
        if scopes.as_ref().is_some_and(|scopes| {
            !scopes.iter().any(|(stage, phase, method)| {
                stage == &guide.stage
                    && phase
                        .as_ref()
                        .is_none_or(|p| guide.recognition_phase.as_ref() == Some(p))
                    && method
                        .as_ref()
                        .is_none_or(|m| guide.method.as_ref() == Some(m))
            })
        }) {
            continue;
        }
        let full_text = packet.guide_text(guide)?;
        let regions = horary_prompt_program::guide_parts(&full_text)?;
        if full_reading_judge {
            for block in &regions.quoted_source {
                source_documents
                    .entry(horary_prompt_program::digest(block))
                    .or_insert_with(|| block.to_string());
            }
        }
        if edit_stage.as_deref() == Some(guide.stage.as_str())
            && !guide_documents.contains_key(&guide.sha256)
        {
            guide_documents.insert(
                guide.sha256.clone(),
                guide_document(&full_text, &mut chunks, &mut chunk_ids)?,
            );
        }
        selected_guides.push(json!({"stage":guide.stage,"recognition_phase":guide.recognition_phase,"method":guide.method,"sha256":guide.sha256,
            "teaching_regions":regions.teaching.iter().map(|region|json!({"sha256":horary_prompt_program::digest(region),"bytes":region.len()})).collect::<Vec<_>>(),
            "quoted_source":regions.quoted_source.iter().map(|block|json!({"sha256":horary_prompt_program::digest(block),"bytes":block.len()})).collect::<Vec<_>>() }));
    }
    for case in &packet.cases {
        let root = format!("cases/{}", case.id);
        let outcome_file = format!("{root}/outcome.json");
        let fixture_file = format!("{root}/fixture.json");
        let first_file = format!("{root}/first-turn.json");
        let final_file = format!("{root}/final.json");
        let grade_ref = index.citation(packet, &case.id, &outcome_file, "/grade")?;
        let reply_ref = index.citation(packet, &case.id, &outcome_file, "/grade/actual/reply")?;
        let expected_ref = index.citation(packet, &case.id, &fixture_file, "/expected")?;
        let words_ref = index.citation(packet, &case.id, &fixture_file, "/words")?;
        let first_ref = index.citation(packet, &case.id, &first_file, "/session/messages")?;
        let final_ref = index.citation(packet, &case.id, &final_file, "/session/messages")?;
        let mut summary = case.summary.clone();
        // Exact first/final wrappers are projected in the compact current format.
        for state in ["first_state", "final_state"] {
            let consultation = &summary[state]["consultation"]["consultation_ref"];
            if let Some(sha) = consultation.as_str() {
                if let Some(value) = packet.consultations.get(sha) {
                    consultations.insert(sha.to_owned(), value.clone());
                }
            }
        }
        let after = &summary["follow_up"];
        let submitted = after
            .pointer("/execution_provenance/user_turn_submitted")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                matches!(
                    after["status"].as_str(),
                    Some(
                        "executed"
                            | "executed after matching elicitation"
                            | "executed after one eligible authored proposal"
                    )
                ) && after["words"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())
                    && after["result"].is_object()
            });
        let after_ref = if submitted && !after["after_state_grade"].is_null() {
            Some(index.citation(packet, &case.id, &outcome_file, "/follow_up/grade")?)
        } else {
            None
        };
        summary["evidence_refs"] = json!({"native_grade":grade_ref,"reply":reply_ref,"expected":expected_ref,
            "words":words_ref,"first_dialogue":first_ref,"final_dialogue":final_ref,"after_grade":after_ref});
        if summary["full_reading"] == true {
            let rubric_file = format!("{root}/reading-rubric.json");
            summary["evidence_refs"]["reading_rubric"] =
                json!(index.citation(packet, &case.id, &rubric_file, "")?);
            for (name, file, pointer) in [
                ("first_hurdles", &first_file, "/hurdles"),
                ("final_hurdles", &final_file, "/hurdles"),
                ("final_method", &final_file, "/session/method"),
            ] {
                summary["evidence_refs"][name] =
                    json!(index.citation(packet, &case.id, file, pointer)?);
            }
            if summary["first_state"]["methodRecords"] == summary["final_state"]["methodRecords"] {
                summary["first_state"]["methodRecords"] =
                    json!({"same_as":"final_state.methodRecords"});
            }
        }
        // ReadyReading is revalidated by the native grader, not by repeating bindings in the model prompt.
        for state in ["first_state", "final_state"] {
            if let Some(object) = summary[state].as_object_mut() {
                if case.summary["full_reading"] == true {
                    // Keep actual answer, verdict, checked worksheet and source
                    // IDs. Binding bytes remain in the immutable final method.
                    if let Some(result) = object
                        .get_mut("methodResult")
                        .and_then(Value::as_object_mut)
                    {
                        result.remove("binding");
                    }
                    continue;
                }
                let kind = object
                    .get("methodResult")
                    .map(|r| r["result"].clone())
                    .unwrap_or(Value::Null);
                object.insert("methodResult".into(), json!({"result":kind}));
            }
        }
        cases.push(json!({"id":case.id,"method":case.method,"mode":case.mode,"summary":summary}));
        for call in &case.calls {
            let input_sha = call["input_sha256"].as_str().ok_or("Call input missing")?;
            let input = packet
                .inputs
                .get(input_sha)
                .ok_or("Referenced input absent")?;
            let input_id = if let Some(id) = input_ids.get(input_sha) {
                id.clone()
            } else {
                let id = format!("i{}", input_ids.len() + 1);
                input_ids.insert(input_sha.to_owned(), id.clone());
                let mut view = input.clone();
                if let Some(object) = view.as_object_mut() {
                    // The quoted guide already teaches these repeated invariant paragraphs.
                    for field in [
                        "instruction",
                        "context_authority",
                        "legacy_user_fact_sources",
                        "available_revisions",
                        "authority",
                        "anchor_policy",
                        "state_reminder",
                    ] {
                        object.remove(field);
                    }
                    if let Some(consultation) = object.get("consultation") {
                        if let Some(sha) = consultation["consultation_ref"].as_str() {
                            // First/final facts are canonical in the view; intermediate states remain exact in source receipts.
                            if !consultations.contains_key(sha) {
                                object.remove("consultation");
                            }
                        }
                    }
                }
                inputs.insert(id.clone(), view);
                id
            };
            let output_id = call["output_sha256"].as_str().map(|sha| {
                if let Some(id) = output_ids.get(sha) {
                    return id.clone();
                }
                let id = format!("o{}", output_ids.len() + 1);
                output_ids.insert(sha.to_owned(), id.clone());
                outputs.insert(
                    id.clone(),
                    packet.outputs.get(sha).cloned().unwrap_or(Value::Null),
                );
                id
            });
            let request = call["request_file"]
                .as_str()
                .ok_or("Request source missing")?;
            let result = call["result_file"]
                .as_str()
                .ok_or("Result source missing")?;
            let input_ref = index.citation(
                packet,
                &case.id,
                request,
                call["input_pointer"].as_str().unwrap_or("/input"),
            )?;
            let guide_ref = index.citation(
                packet,
                &case.id,
                request,
                call["guide_pointer"]
                    .as_str()
                    .unwrap_or("/prompt/0/content"),
            )?;
            let result_ref = if output_id.is_some() {
                index.citation(
                    packet,
                    &case.id,
                    result,
                    call["result_pointer"]
                        .as_str()
                        .unwrap_or("/result/Ok/content"),
                )?
            } else {
                index.citation(packet, &case.id, result, "/result")?
            };
            let provider_ref = call["provider_pointer"]
                .as_str()
                .map(|pointer| index.citation(packet, &case.id, result, pointer))
                .transpose()?;
            schema_hashes.insert(
                call["schema_sha256"]
                    .as_str()
                    .ok_or("Missing schema digest")?
                    .to_owned(),
            );
            calls.push(json!({"case_id":case.id,"sequence":call["sequence"],"stage":call["stage"],
                "batch_branch":call["batch_branch"],"batch_size":call["batch_size"],"decoder":call["decoder"],
                "provider":call["provider"],"provider_wire":call["provider_wire"],"provider_ref":provider_ref,
                "phase":call["recognition_phase"],"method":call["method"],"guide_sha":call["guide_sha256"],
                "schema_sha":call["schema_sha256"],"input":input_id,"output":output_id,
                "native_error":call["native_validation_error"],"backend_error":call["result_error"],
                "input_ref":input_ref,"guide_ref":guide_ref,"result_ref":result_ref}));
        }
    }
    let schemas: BTreeMap<_, _> = schema_hashes
        .into_iter()
        .filter_map(|sha| {
            packet
                .schemas
                .get(&sha)
                .map(|v| (sha, summarize_contract(v)))
        })
        .collect();
    let mut reviews = reviews.map(|r| r.to_vec()).unwrap_or_default();
    for review in &mut reviews {
        index.shorten(review);
        if let Some(findings) = review["findings"].as_array_mut() {
            for finding in findings {
                if let Some(summary) = finding["summary"].as_str() {
                    finding["summary"] = json!(summary.chars().take(240).collect::<String>());
                }
            }
        }
    }
    let mut view = json!({"format":"compact-ref-v1","phase":packet.phase,"manifest_sha256":packet.manifest_sha256,
        "candidate_sha256":packet.candidate_sha256,"qualification":packet.qualification,
        "omissions":["Full originals are retained by SHA; this bounded view omits duplicate consultation changes/histories, chart geometry, repeated ReadyReading bindings and invariant input instructions.",
            "Inputs are original accepted task views plus native repair errors by sequence; intermediate accepted state is omitted unless final/first. Large schema enums show enum_count; exact schema stays immutable.",
            if full_reading_judge {"Guides have exact SHA and typed scope. source_documents contains exact deduplicated book_extracts blocks for independent reading review; full guides, provider wire requests/responses and every branch remain hashed originals. Hosted evidence does not qualify the on-device model."} else {"guides are typed selectors with full guide SHA. Each document contains ordered teaching_regions; concatenate teaching_chunks within one region only. Immutable book_extracts blocks separate regions, are omitted and retained by hash/length in full originals. This projection cannot certify quoted source interpretation. Writer receives only scopes implicated by training findings."},
            "Output evidence_refs resolve through receipt_table. The host expands them and verifies every file hash and JSON pointer. Never invent a reference."],
        "editable_stage":edit_stage,"cases":cases,"calls":calls,"outputs":outputs,"inputs":inputs,"accepted_consultations":consultations,
        "guide_scopes":selected_guides,"guide_documents":guide_documents,"teaching_chunks":chunks,
        "source_documents":source_documents,
        "schema_summaries":schemas,"training_reviews":reviews,"receipt_table":index.table()});
    if full_reading && share {
        intern_reading_view(&mut view)?;
    }
    Ok((view, index))
}

pub fn schema(mut schema: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if let Some(properties) =
                    fields.get_mut("properties").and_then(Value::as_object_mut)
                {
                    if properties.remove("evidence").is_some() {
                        properties.insert("evidence_refs".into(),json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":2}));
                    }
                    for name in ["reason", "summary"] {
                        if let Some(property) = properties.get_mut(name) {
                            property["maxLength"] = json!(if name == "reason" { 160 } else { 240 });
                        }
                    }
                }
                if let Some(required) = fields.get_mut("required").and_then(Value::as_array_mut) {
                    for name in required {
                        if *name == "evidence" {
                            *name = json!("evidence_refs");
                        }
                    }
                }
                for child in fields.values_mut() {
                    visit(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    visit(child);
                }
            }
            _ => {}
        }
    }
    visit(&mut schema);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded_view(mut view: Value) -> Value {
        fn expand(
            value: &mut Value,
            marker: &str,
            text_marker: &str,
            object_marker: &str,
            values: &Value,
            stack: &mut BTreeSet<String>,
        ) {
            match value {
                Value::Object(fields) => {
                    if fields.len() == 1 {
                        if let Some(id) = fields.get(marker).and_then(Value::as_str) {
                            let id = id.to_owned();
                            assert!(stack.insert(id.clone()), "cyclic dictionary value");
                            *value = values.get(&id).expect("known dictionary ID").clone();
                            expand(value, marker, text_marker, object_marker, values, stack);
                            stack.remove(&id);
                            return;
                        }
                        if let Some(parts) =
                            fields.get_mut(text_marker).and_then(Value::as_array_mut)
                        {
                            let mut text = String::new();
                            for part in parts {
                                expand(part, marker, text_marker, object_marker, values, stack);
                                text.push_str(part.as_str().expect("exact text part"));
                            }
                            *value = json!(text);
                            return;
                        }
                        if let Some(mut pair) = fields.remove(object_marker) {
                            expand(&mut pair, marker, text_marker, object_marker, values, stack);
                            let keys = pair[0].as_array().expect("ordered object keys");
                            let ordered = pair[1].as_array().expect("ordered object values");
                            assert_eq!(keys.len(), ordered.len());
                            let object: serde_json::Map<String, Value> = keys
                                .iter()
                                .zip(ordered)
                                .map(|(key, value)| {
                                    (key.as_str().unwrap().to_owned(), value.clone())
                                })
                                .collect();
                            assert_eq!(object.len(), keys.len());
                            *value = Value::Object(object);
                            return;
                        }
                    }
                    for value in fields.values_mut() {
                        expand(value, marker, text_marker, object_marker, values, stack);
                    }
                }
                Value::Array(items) => {
                    for value in items {
                        expand(value, marker, text_marker, object_marker, values, stack);
                    }
                }
                _ => {}
            }
        }
        if let Some(dictionary) = view.as_object_mut().unwrap().remove("context_interning") {
            expand(
                &mut view,
                dictionary["ref_key"].as_str().unwrap(),
                dictionary["text_key"].as_str().unwrap(),
                dictionary["object_key"].as_str().unwrap(),
                &dictionary["values"],
                &mut BTreeSet::new(),
            );
            assert_eq!(
                horary_prompt_program::digest(serde_json::to_vec(&view).unwrap()),
                dictionary["expanded_sha256"]
            );
        }
        view
    }

    #[test]
    fn short_repeated_role_labels_and_raw_prose_share_without_losing_meaning() {
        let label = "Mercury (Lord 7)";
        let statement = "Mercury is peregrine; no essential dignity is established for this role.";
        let original = json!({"inputs":{
            "i1":{"roles":vec![json!({"label":label,"finding":statement});24]},
            "i2":{"roles":vec![json!({"label":label,"finding":statement});24]}},
            "outputs":{"o1":format!("RAW answer: {statement} Do not infer mutual affection."),
                "o2":format!("Different RAW answer: {statement} Keep the direction of reception.")},
            "receipt_table":{"r1":{"file":"cases/a/final.json","pointer":"/session/method"}}});
        let mut compact = original.clone();
        intern_reading_view(&mut compact).unwrap();
        assert_eq!(expanded_view(compact.clone()), original);
        assert_eq!(compact["receipt_table"], original["receipt_table"]);
        let dictionary = compact["context_interning"]["values"].as_object().unwrap();
        assert!(dictionary.values().any(|value| value == statement));
        assert!(compact.to_string().len() < original.to_string().len());
    }

    #[test]
    fn exact_interning_preserves_nested_values_raw_outputs_and_colliding_marker_keys() {
        let fact = json!({"id":"jupiter-position","longitude":121.25,"house":1,
            "testimony":"Exact location testimony with degree, direction, source and role preserved. ".repeat(4)});
        let worksheet = json!({"facts":[fact.clone(),fact.clone()],"source":"Printed p. 147: \"A numbered source quotation.\""});
        let raw = format!(
            "{{\"value\":1,\"value\":2,\"because\":\"{}\"}}",
            "Raw duplicate keys. ".repeat(12)
        );
        let original = json!({"format":"compact-ref-v1","manifest_sha256":"a".repeat(64),
            "qualification":format!("Scope: {};",fact["testimony"].as_str().unwrap()),
            "cases":[{"id":"lost_object-implicit","method":"lost_object","summary":{
                "full_reading":true,"first_state":{"methodResult":{"result":"judgment","worksheet":worksheet}},
                "final_state":{"methodResult":{"result":"judgment","worksheet":worksheet}}}}],
            "inputs":{"i1":{"reading_request":{"roles":[worksheet.clone(),worksheet.clone()]},"facts":[fact.clone(),fact.clone()]}},
            "outputs":{"raw":raw,"malformed":"{\"unclosed\": RAW","collision":{"@":"v1","~":[],"#":"v1"}},
            "receipt_table":{"files":{"f1":{"sha256":"b".repeat(64),"file":"cases/a/final.json"}},"receipts":{"r1":{"file":"f1","pointer":"/session/method"}}}});
        let mut compact = original.clone();
        intern_reading_view(&mut compact).unwrap();
        assert!(compact.to_string().len() < original.to_string().len());
        assert_eq!(compact["context_interning"]["ref_key"], "@_");
        assert_eq!(compact["context_interning"]["text_key"], "~_");
        assert_eq!(compact["context_interning"]["object_key"], "#_");
        assert_eq!(compact["receipt_table"], original["receipt_table"]);
        assert_eq!(compact["qualification"], original["qualification"]);
        assert_eq!(compact["cases"][0]["id"], "lost_object-implicit");
        assert_eq!(
            compact["cases"][0]["summary"]["final_state"]["methodResult"]["result"],
            "judgment"
        );
        let mut again = original.clone();
        intern_reading_view(&mut again).unwrap();
        assert_eq!(compact, again);
        assert_eq!(expanded_view(compact), original);
    }

    #[test]
    #[ignore = "read-only saved campaign size check; requires HORARY_PACKET_SMOKE_CAMPAIGN and HORARY_PACKET_SMOKE_SPLIT"]
    fn saved_reading_context_fits_without_changing_evidence() {
        use crate::{packet, store, types::Split};
        use std::path::PathBuf;
        let campaign = PathBuf::from(std::env::var("HORARY_PACKET_SMOKE_CAMPAIGN").unwrap());
        let split_file = PathBuf::from(std::env::var("HORARY_PACKET_SMOKE_SPLIT").unwrap());
        let split: Split = serde_json::from_value(store::json(&split_file).unwrap()).unwrap();
        let case_id = std::env::var("HORARY_PACKET_SMOKE_CASE")
            .unwrap_or_else(|_| "lost_object-implicit".into());
        let case = split.cases.iter().find(|case| case.id == case_id).unwrap();
        let state = tempfile::tempdir().unwrap();
        let packet = packet::build(
            &campaign,
            state.path(),
            &split,
            std::slice::from_ref(&case.id),
            &horary_prompt_program::digest(store::read(&campaign.join("manifest.json")).unwrap()),
            case.partition == crate::types::Partition::ReservedValidation,
            None,
        )
        .unwrap();
        let reviews = std::env::var("HORARY_PACKET_SMOKE_REVIEW")
            .ok()
            .map(|path| {
                store::json(&PathBuf::from(path)).unwrap()["reviews"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|review| review["case_id"] == case_id)
                    .cloned()
                    .collect::<Vec<_>>()
            });
        assert!(reviews.as_ref().is_none_or(|reviews| !reviews.is_empty()));
        let (original, _) = context_view(&packet, reviews.as_deref(), false).unwrap();
        let (compact, index) = context(&packet, reviews.as_deref()).unwrap();
        assert_eq!(expanded_view(compact.clone()), original);
        let before = serde_json::to_vec(&original).unwrap();
        let after = serde_json::to_vec(&compact).unwrap();
        let (prompt, output_schema) = if reviews.is_some() {
            (
                crate::propose_prompt(&compact, &split).unwrap(),
                schema(crate::types::propose_schema()),
            )
        } else {
            (
                crate::judge_prompt(&compact).unwrap(),
                schema(crate::types::judge_schema()),
            )
        };
        let submitted_bytes = prompt.len() + output_schema.to_string().len();
        if let Ok(output) = std::env::var("HORARY_PACKET_SMOKE_OUTPUT") {
            let output = PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            store::atomic(&output.join("original-context.json"), &before, true).unwrap();
            store::atomic(&output.join("interned-context.json"), &after, true).unwrap();
        }
        assert!(
            submitted_bytes <= 120_000,
            "{submitted_bytes} byte review submission exceeds its budget"
        );
        assert_eq!(compact["receipt_table"], original["receipt_table"]);
        index.verify_prompt(&prompt).unwrap();
        for evidence in index.citations.values() {
            packet::validate_evidence(&packet, state.path(), evidence).unwrap();
        }
        assert!(original["outputs"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_string));
        for (sha, block) in original["source_documents"].as_object().unwrap() {
            assert_eq!(*sha, horary_prompt_program::digest(block.as_str().unwrap()));
        }
        if reviews.is_some() {
            assert!(original["source_documents"].as_object().unwrap().is_empty());
            assert!(!original["guide_documents"].as_object().unwrap().is_empty());
            assert!(!original["teaching_chunks"]
                .to_string()
                .contains("<book_extracts>"));
        }
        eprintln!("{} exact context: {} -> {} bytes; total_submission_bytes={}; before_sha256={}; after_sha256={}; receipts={}; sources={}",
            case_id, before.len(), after.len(), submitted_bytes, horary_prompt_program::digest(&before), horary_prompt_program::digest(&after),
            index.citations.len(), original["source_documents"].as_object().unwrap().len());
    }

    #[test]
    fn grouped_findings_select_real_stages_without_broadening_method_or_phase() {
        let reviews = [json!({"findings":[
            {"repair_owner":"prompt","stage":"condition / reception","recognition_phase":null,"method":"relationship"},
            {"repair_owner":"prompt","stage":"intake","recognition_phase":"classify_question","method":null},
            {"repair_owner":"native_code","stage":"significators","recognition_phase":null,"method":null},
            {"repair_owner":"prompt","stage":"unknown_stage","recognition_phase":null,"method":null}
        ]})];
        assert_eq!(
            prompt_scopes(&reviews),
            BTreeSet::from([
                ("condition".into(), None, Some("relationship".into())),
                ("reception".into(), None, Some("relationship".into())),
                ("intake".into(), Some("classify_question".into()), None)
            ])
        );
    }
    #[test]
    fn writer_can_see_method_appendix_without_reconstructing_quoted_source() {
        let text = "General teaching.\n<book_extracts>Source one</book_extracts>\nMethod teaching.\n<book_extracts>Source two</book_extracts>\nFinal teaching.";
        let mut chunks = Vec::new();
        let document = guide_document(text, &mut chunks, &mut BTreeMap::new()).unwrap();
        let regions = document["teaching_regions"].as_array().unwrap();
        assert_eq!(regions.len(), 3);
        let reconstructed = regions
            .iter()
            .map(|region| {
                region["chunks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| chunks[id.as_u64().unwrap() as usize].as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reconstructed,
            vec![
                "General teaching.\n",
                "\nMethod teaching.\n",
                "\nFinal teaching."
            ]
        );
        for (region, actual) in regions.iter().zip(&reconstructed) {
            assert_eq!(region["sha256"], horary_prompt_program::digest(actual));
            assert_eq!(region["bytes"], actual.len());
        }
        assert_eq!(document["quoted_source"].as_array().unwrap().len(), 2);
        assert!(!document.to_string().contains("Source one"));
        assert!(chunks.iter().all(|chunk| !chunk.contains("book_extracts")));
        assert!(
            guide_document("<book_extracts>Unclosed", &mut chunks, &mut BTreeMap::new()).is_err()
        );
    }
    #[test]
    fn short_claims_expand_without_changing_hashes_or_pointers() {
        let mut index = Index::default();
        let evidence = Evidence {
            case_id: "a".into(),
            file: "cases/a/outcome.json".into(),
            json_pointer: "/grade/semantic_pass".into(),
            sha256: "a".repeat(64),
        };
        let id = index.add(evidence.clone());
        assert_eq!(index.add(evidence.clone()), id);
        let prompt = format!(
            "Self-contained.\nCOMPACT PACKET:\n{}",
            json!({"receipt_table":index.table()})
        );
        index.verify_prompt(&prompt).unwrap();
        let mut changed = index.clone();
        changed.citations.get_mut(&id).unwrap().json_pointer = "/unrelated".into();
        assert!(changed.verify_prompt(&prompt).is_err());
        assert!(index.verify_prompt("No submitted receipt table").is_err());
        let mut result = json!({"dimension":{"evidence_refs":[id],"reason":"native result"}});
        index.expand(&mut result).unwrap();
        assert_eq!(
            result["dimension"]["evidence"][0],
            serde_json::to_value(evidence).unwrap()
        );
        assert!(index
            .expand(&mut json!({"evidence_refs":["unknown"]}))
            .is_err());
    }
    #[test]
    fn compact_output_contract_does_not_repeat_long_citations() {
        let compact = schema(crate::types::judge_schema());
        let text = compact.to_string();
        assert!(text.contains("evidence_refs"));
        assert!(!text.contains("json_pointer"));
        assert!(!text.contains("sha256"));
    }
}
