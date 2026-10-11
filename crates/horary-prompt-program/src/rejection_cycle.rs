//! Proof of repeated native rejection, never a completion permit or score.
#![forbid(unsafe_code)]
use crate::{digest, original_input};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STOP: &str =
    "Native repair made no progress; the same proposal and rejection repeated. This task remains unfinished.";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Witness {
    pub event: String,
    pub version: u8,
    pub key: String,
    pub stage: String,
    pub revision: u64,
    pub first_record_index: usize,
    pub record_index: usize,
    pub input_sha256: String,
    pub guide_sha256: String,
    pub schema_sha256: String,
    pub first_raw_sha256: String,
    pub raw_sha256: String,
    pub validation_error_sha256: String,
}

fn hash(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Parsed proposals compare as JSON values; malformed output retains exact
/// raw identity instead of collapsing every parse failure into null.
pub fn same_rejection(first: &Value, repeated: &Value) -> bool {
    if first["stage"].as_str().is_none()
        || first["revision"].as_u64().is_none()
        || first["stage"] != repeated["stage"]
        || first["revision"] != repeated["revision"]
        || first.get("input").is_none_or(|v| !v.is_object())
        || repeated.get("input").is_none_or(|v| !v.is_object())
        || original_input(&first["input"]) != original_input(&repeated["input"])
        || first["validationError"].as_str().is_none_or(str::is_empty)
        || first["validationError"] != repeated["validationError"]
        || ["guideSha256", "schemaSha256"]
            .iter()
            .any(|field| hash(&first[*field]).is_none() || first[*field] != repeated[*field])
    {
        return false;
    }
    let (Some(first_raw), Some(raw)) = (first["raw"].as_str(), repeated["raw"].as_str()) else {
        return false;
    };
    match (first.get("worksheet"), repeated.get("worksheet")) {
        (Some(Value::Null), Some(Value::Null)) => first_raw == raw,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Authenticate the audit against both retained rejections, their actual
/// transitions, an original-input record and the one paused native job.
/// Numeric input hashes remain native seals; do not rehash them with another
/// serde map/float representation in the optimizer.
pub fn verify(session: &Value, value: &Value) -> Result<Witness, String> {
    let w: Witness = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let records = session["method"]["records"]
        .as_array()
        .ok_or("No native rejection records")?;
    let first = records
        .get(w.first_record_index)
        .ok_or("Missing first rejection")?;
    let repeated = records
        .get(w.record_index)
        .ok_or("Missing repeated rejection")?;
    if w.event != "stage_no_progress"
        || w.version != 1
        || w.first_record_index >= w.record_index
        || !same_rejection(first, repeated)
        || repeated["stage"] != w.stage
        || repeated["revision"] != w.revision
        || repeated["guideSha256"] != w.guide_sha256
        || repeated["schemaSha256"] != w.schema_sha256
        || hash(&Value::String(w.input_sha256.clone())).is_none()
        || digest(first["raw"].as_str().ok_or("No first raw output")?) != w.first_raw_sha256
        || digest(repeated["raw"].as_str().ok_or("No repeated raw output")?) != w.raw_sha256
        || digest(
            repeated["validationError"]
                .as_str()
                .ok_or("No native error")?,
        ) != w.validation_error_sha256
    {
        return Err("No-progress witness differs from its rejected work".into());
    }
    let audit = session["audit"]
        .as_array()
        .ok_or("No native transition journal")?;
    let transition = |index: usize, to: &str| {
        audit.iter().any(|event| {
            event["event"] == "stage_transition"
                && event["key"] == w.key
                && event["stage"] == w.stage
                && event["recordIndex"].as_u64() == Some(index as u64)
                && event["to"] == to
        })
    };
    if audit.iter().filter(|event| *event == value).count() != 1
        || !transition(w.first_record_index, "repairing")
        || !transition(w.record_index, "paused")
    {
        return Err("No-progress records do not belong to this native job".into());
    }
    let jobs = session["method"]["flow"]["jobs"]
        .as_array()
        .ok_or("No native jobs")?;
    let matching: Vec<_> = jobs.iter().filter(|job| job["key"] == w.key).collect();
    if matching.len() != 1
        || matching[0]["stage"] != w.stage
        || matching[0]["revision"] != w.revision
        || matching[0]["inputSha256"] != w.input_sha256
        || matching[0]["phase"]["state"] != "paused"
        || matching[0]["phase"]["error"] != STOP
        || !records.iter().any(|record| {
            record["stage"] == w.stage
                && record["revision"] == w.revision
                && record["input"].is_object()
                && record["input"].get("original_input").is_none()
                && record["inputSha256"] == w.input_sha256
                && record["input"] == *original_input(&repeated["input"])
        })
    {
        return Err("No-progress pause is not bound to original native work".into());
    }
    Ok(w)
}

/// Bind an observer's stop envelope to the actual snapshot and the native
/// proof. Provider settlement is checked separately by each trace consumer.
pub fn verify_stop(snapshot: &Value, witness: &Value) -> Result<Witness, String> {
    let proof = verify(&snapshot["session"], &witness["no_progress"])?;
    let record = &snapshot["session"]["method"]["records"][proof.record_index];
    let error = match witness["snapshot"].as_str() {
        Some("first-turn.json") => &snapshot["result"]["Err"],
        Some("final.json") => &snapshot["follow_up"]["result"]["Err"],
        _ => return Err("No-progress stop lacks an exact native snapshot".into()),
    };
    if witness["stop"] != STOP
        || witness["stop_kind"] != "no_progress"
        || *error != STOP
        || witness["record_index"].as_u64() != Some(proof.record_index as u64)
        || witness["stage"] != proof.stage
        || witness["revision"] != proof.revision
        || witness["input_sha256"] != proof.input_sha256
        || witness["original_input"] != *original_input(&record["input"])
        || witness["validation_error"] != record["validationError"]
        || witness["paused_job"]["key"] != proof.key
        || !snapshot["session"]["method"]["flow"]["jobs"]
            .as_array()
            .is_some_and(|jobs| jobs.iter().any(|job| job == &witness["paused_job"]))
    {
        return Err("No-progress stop changed its actual native witness".into());
    }
    Ok(proof)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    fn record(proposal: Value, error: &str, repair: bool) -> Value {
        let original = json!({"question":"Synthetic matter","facts":[1,2]});
        json!({"stage":"condition","revision":7,"input":if repair {json!({"original_input":original})} else {original.clone()},
            "inputSha256":digest(if repair {"repair".into()} else {original.to_string()}),
            "guideSha256":digest("guide"),"schemaSha256":digest("schema"),
            "worksheet":proposal,"raw":proposal.to_string(),"validationError":error})
    }
    pub(crate) fn session() -> (Value, Value) {
        let first = record(json!({"state":"bad"}), "Native error A", false);
        let other = record(json!({"state":"other"}), "Native error B", true);
        let repeated = record(json!({"state":"bad"}), "Native error A", true);
        let w = json!({"event":"stage_no_progress","version":1,"key":"work","stage":"condition","revision":7,
            "firstRecordIndex":0,"recordIndex":2,"inputSha256":first["inputSha256"],
            "guideSha256":first["guideSha256"],"schemaSha256":first["schemaSha256"],
            "firstRawSha256":digest(first["raw"].as_str().unwrap()),"rawSha256":digest(repeated["raw"].as_str().unwrap()),
            "validationErrorSha256":digest("Native error A")});
        let s = json!({"method":{"records":[first,other,repeated],"flow":{"jobs":[{
            "key":"work","stage":"condition","revision":7,"inputSha256":w["inputSha256"],
            "phase":{"state":"paused","error":STOP}}]}},"audit":[
            {"event":"stage_transition","key":"work","stage":"condition","recordIndex":0,"to":"repairing"},
            {"event":"stage_transition","key":"work","stage":"condition","recordIndex":1,"to":"repairing"},
            w,{"event":"stage_transition","key":"work","stage":"condition","recordIndex":2,"to":"paused"}]});
        (s, w)
    }
    #[test]
    fn cycle_proof_binds_both_native_rejections_and_original_paused_job() {
        let (session, witness) = session();
        let proof = verify(&session, &witness).unwrap();
        assert_eq!(proof.first_record_index, 0);
        assert_eq!(proof.record_index, 2);
        for (field, value) in [
            ("raw", json!("modified output")),
            ("worksheet", json!({"state":"good"})),
            ("validationError", Value::Null),
            ("guideSha256", json!(digest("new guide"))),
            ("schemaSha256", json!(digest("new schema"))),
            ("revision", json!(8)),
            ("input", json!({"original_input":{"question":"different"}})),
        ] {
            let mut changed = session.clone();
            changed["method"]["records"][2][field] = value;
            assert!(verify(&changed, &witness).is_err(), "{field}");
        }
        let mut changed = session.clone();
        changed["method"]["flow"]["jobs"][0]["key"] = json!("another-work");
        assert!(verify(&changed, &witness).is_err());
        let mut changed = session.clone();
        changed["audit"].as_array_mut().unwrap().remove(0);
        assert!(verify(&changed, &witness).is_err());
        let mut changed = session.clone();
        changed["method"]["flow"]["jobs"][0]["phase"]["state"] = json!("complete");
        assert!(verify(&changed, &witness).is_err());
    }
    #[test]
    fn parsed_cosmetic_changes_match_but_distinct_parse_failures_do_not() {
        let first = record(json!({"a":1,"b":2}), "Native error", false);
        let mut repeated = first.clone();
        repeated["raw"] = json!("{\"b\":2, \"a\":1}");
        assert!(same_rejection(&first, &repeated));
        let mut first = first;
        first["worksheet"] = Value::Null;
        first["raw"] = json!("malformed A");
        repeated["worksheet"] = Value::Null;
        repeated["raw"] = json!("malformed B");
        assert!(!same_rejection(&first, &repeated));
        repeated["raw"] = first["raw"].clone();
        assert!(same_rejection(&first, &repeated));
    }
    #[test]
    fn observer_stop_cannot_swap_job_input_or_hide_later_paid_sibling() {
        let (session, event) = session();
        let snapshot = json!({"result":{"Err":STOP},"session":session});
        let record = &snapshot["session"]["method"]["records"][2];
        let witness = json!({"snapshot":"first-turn.json","record_index":2,"stage":"condition","revision":7,
            "input_sha256":event["inputSha256"],"original_input":original_input(&record["input"]),
            "validation_error":record["validationError"],"stop":STOP,"stop_kind":"no_progress",
            "paused_job":snapshot["session"]["method"]["flow"]["jobs"][0],"no_progress":event});
        verify_stop(&snapshot, &witness).unwrap();
        let mut later = snapshot.clone();
        later["session"]["method"]["records"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"stage":"reception","validationError":null}));
        assert_eq!(verify_stop(&later, &witness).unwrap().record_index, 2);
        for (field, value) in [
            ("input_sha256", json!(digest("foreign"))),
            ("record_index", json!(3)),
            ("original_input", json!({"question":"another"})),
            ("paused_job", json!({"key":"foreign"})),
        ] {
            let mut changed = witness.clone();
            changed[field] = value;
            assert!(verify_stop(&snapshot, &changed).is_err(), "{field}");
        }
        let mut interrupted = snapshot;
        interrupted["result"]["Err"] = json!("Cancelled");
        assert!(verify_stop(&interrupted, &witness).is_err());
    }
}
