//! Ordered, immutable operation replay reconstructs GEPA's deterministic state.
//! A prepared or uncertain operation is never submitted again automatically.
use crate::{keep, load, read, verify, Plan, Result};
use fs2::FileExt;
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub enum Operation {
    Reused(Value),
    Fresh(PathBuf),
}
pub struct Journal {
    pub root: PathBuf,
    next: usize,
    plan_sha256: String,
    max_teacher: u64,
    max_review: u64,
    max_physical: u64,
    _lock: fs::File,
    ownership_retained: AtomicBool,
}
impl Drop for Journal {
    fn drop(&mut self) {
        // A pre-exec child can briefly inherit this open-file description.
        // Only an explicit lease may extend ownership beyond this journal.
        if !self.ownership_retained.load(Ordering::Relaxed) {
            let _ = FileExt::unlock(&self._lock);
        }
    }
}
impl Journal {
    pub fn retain_ownership(&self) -> Result<fs::File> {
        let lease = self._lock.try_clone().map_err(|error| error.to_string())?;
        self.ownership_retained.store(true, Ordering::Relaxed);
        Ok(lease)
    }
    pub fn open(root: &Path, plan: &Plan) -> Result<Self> {
        Self::open_bounded(
            root,
            plan,
            plan.max_teacher_calls,
            plan.max_review_calls,
            plan.max_physical_generation_attempts,
        )
    }
    pub(crate) fn open_verification(
        root: &Path,
        plan: &impl serde::Serialize,
        max_review: u64,
    ) -> Result<Self> {
        Self::open_bounded(root, plan, 0, max_review, 0)
    }
    fn open_bounded(
        root: &Path,
        plan: &impl serde::Serialize,
        max_teacher: u64,
        max_review: u64,
        max_physical: u64,
    ) -> Result<Self> {
        fs::create_dir_all(root.join("operations")).map_err(|error| error.to_string())?;
        keep(&root.join("plan.json"), plan)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("run.lock"))
            .map_err(|error| error.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|error| format!("Another process owns this optimizer: {error}"))?;
        Ok(Self {
            root: root.into(),
            next: 0,
            plan_sha256: digest(read(&root.join("plan.json"))?),
            max_teacher,
            max_review,
            max_physical,
            _lock: lock,
            ownership_retained: AtomicBool::new(false),
        })
    }
    pub fn begin(
        &mut self,
        kind: &str,
        request: &Value,
        teacher: u64,
        physical: u64,
    ) -> Result<Operation> {
        self.begin_cost(kind, request, teacher, 0, physical)
    }
    pub fn begin_review(&mut self, request: &Value, new_submission: bool) -> Result<Operation> {
        self.begin_cost(
            "codex_input_review",
            request,
            0,
            u64::from(new_submission),
            0,
        )
    }
    pub fn begin_reading_review(
        &mut self,
        request: &Value,
        new_submission: bool,
    ) -> Result<Operation> {
        self.begin_cost(
            "codex_reading_review",
            request,
            0,
            u64::from(new_submission),
            0,
        )
    }
    pub(crate) fn begin_complete_journey_review(&mut self, request: &Value) -> Result<Operation> {
        self.begin_cost("codex_complete_journey_review", request, 0, 1, 0)
    }
    fn begin_cost(
        &mut self,
        kind: &str,
        request: &Value,
        teacher: u64,
        review: u64,
        physical: u64,
    ) -> Result<Operation> {
        let sequence = self.next;
        self.next += 1;
        let identity = json!({"sequence":sequence,"plan_sha256":self.plan_sha256,"kind":kind,"request":request});
        let directory = self.root.join("operations").join(format!("{sequence:05}"));
        if directory.exists() {
            keep(&directory.join("request.json"), &identity)?;
            let completed = directory.join("completed.json");
            if !completed.exists() {
                return Err(format!(
                    "Unsettled operation {} is preserved; reconcile it, do not resubmit",
                    directory.display()
                ));
            }
            let receipt = load(&completed)?;
            verify(
                &directory.join("response.json"),
                receipt["response_sha256"]
                    .as_str()
                    .ok_or("No response digest")?,
            )?;
            for artifact in receipt["artifacts"]
                .as_array()
                .ok_or("Missing operation artifacts")?
            {
                let relative = artifact["file"].as_str().ok_or("Invalid artifact path")?;
                let path = Path::new(relative);
                if path.is_absolute()
                    || path
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    return Err("Operation artifact escapes journal".into());
                }
                verify(
                    &directory.join(path),
                    artifact["sha256"].as_str().ok_or("No artifact hash")?,
                )?;
            }
            let cached = directory.join("cache-source.json");
            if cached.exists() {
                let source = load(&cached)?;
                if source["type"] == "sealed_baseline_control_import" {
                    crate::controls::verify_import(
                        &source,
                        &identity,
                        &load(&directory.join("response.json"))?,
                    )?;
                } else if source["type"] == crate::controls::COMPONENT_IMPORT {
                    crate::controls::verify_component_import(
                        &source,
                        &identity,
                        &load(&directory.join("response.json"))?,
                    )?;
                } else if source["type"] == crate::controls::COMPONENT_REVIEW_IMPORT {
                    crate::controls::verify_component_review_import(
                        &source,
                        &identity,
                        &load(&directory.join("response.json"))?,
                    )?;
                } else if source["type"] == crate::curriculum::IMPORT {
                    crate::curriculum::verify_import(
                        &source,
                        &identity,
                        &load(&directory.join("response.json"))?,
                    )?;
                }
            }
            println!(
                "{}",
                json!({"event":"operation_reused","sequence":sequence,"kind":kind})
            );
            return Ok(Operation::Reused(load(&directory.join("response.json"))?));
        }
        let mut used_teacher = 0u64;
        let mut used_review = 0u64;
        let mut used_physical = 0u64;
        for entry in
            fs::read_dir(self.root.join("operations")).map_err(|error| error.to_string())?
        {
            let directory = entry.map_err(|error| error.to_string())?.path();
            let reservation = directory.join("reservation.json");
            if reservation.exists() {
                let value = load(&reservation)?;
                used_teacher = used_teacher
                    .checked_add(
                        value["teacher_calls"]
                            .as_u64()
                            .ok_or("Bad teacher reservation")?,
                    )
                    .ok_or("Teacher budget overflow")?;
                used_physical = used_physical
                    .checked_add(
                        value["physical_generation_attempts"]
                            .as_u64()
                            .ok_or("Bad student reservation")?,
                    )
                    .ok_or("Student budget overflow")?;
                used_review = used_review
                    .checked_add(match value.get("review_calls") {
                        None => 0,
                        Some(value) => value.as_u64().ok_or("Bad reviewer reservation")?,
                    })
                    .ok_or("Reviewer budget overflow")?;
            }
        }
        if used_teacher.saturating_add(teacher) > self.max_teacher
            || used_review.saturating_add(review) > self.max_review
            || used_physical.saturating_add(physical) > self.max_physical
        {
            return Err(format!("Hard paid-call reservation would exceed budget: teacher {used_teacher}+{teacher}/{}, reviewer {used_review}+{review}/{}, student {used_physical}+{physical}/{}; no request submitted", self.max_teacher,self.max_review,self.max_physical));
        }
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        keep(&directory.join("request.json"), &identity)?;
        keep(
            &directory.join("reservation.json"),
            &json!({"teacher_calls":teacher,"review_calls":review,"physical_generation_attempts":physical,"reservation_before_submission":true}),
        )?;
        println!(
            "{}",
            json!({"event":"operation_prepared","sequence":sequence,"kind":kind})
        );
        Ok(Operation::Fresh(directory))
    }
    pub fn finish(&self, directory: &Path, response: &Value) -> Result<()> {
        keep(&directory.join("response.json"), response)?;
        fn files(base: &Path, directory: &Path, result: &mut Vec<Value>) -> Result<()> {
            for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|error| error.to_string())?;
                if kind.is_symlink() {
                    return Err("Operation artifacts must be ordinary files".into());
                }
                if kind.is_dir() {
                    files(base, &path, result)?;
                } else if path.file_name().is_none_or(|name| name != "completed.json") {
                    result.push(json!({"file":path.strip_prefix(base).map_err(|error|error.to_string())?,"sha256":digest(read(&path)?)}));
                }
            }
            Ok(())
        }
        let mut artifacts = Vec::new();
        files(directory, directory, &mut artifacts)?;
        keep(
            &directory.join("completed.json"),
            &json!({"response_sha256":digest(read(&directory.join("response.json"))?),"artifacts":artifacts}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_drop_releases_an_accidental_duplicate_descriptor() {
        let root = tempfile::tempdir().unwrap();
        let plan = json!({"synthetic":"journal lifetime"});
        let journal = Journal::open_verification(root.path(), &plan, 1).unwrap();
        // Deterministic equivalent of an inherited pre-exec descriptor; no fork.
        let inherited = journal._lock.try_clone().unwrap();
        assert!(Journal::open_verification(root.path(), &plan, 1).is_err());
        drop(journal);
        let reopened = Journal::open_verification(root.path(), &plan, 1).unwrap();
        assert!(inherited.metadata().is_ok());
        drop(reopened);
        drop(inherited);
    }

    #[test]
    fn explicit_retained_ownership_survives_journal_drop() {
        let root = tempfile::tempdir().unwrap();
        let plan = json!({"synthetic":"deliberate owner lease"});
        let journal = Journal::open_verification(root.path(), &plan, 1).unwrap();
        let lease = journal.retain_ownership().unwrap();
        assert!(Journal::open_verification(root.path(), &plan, 1).is_err());
        drop(journal);
        assert!(Journal::open_verification(root.path(), &plan, 1).is_err());
        FileExt::unlock(&lease).unwrap();
        let reopened = Journal::open_verification(root.path(), &plan, 1).unwrap();
        drop(reopened);
        drop(lease);
    }

    #[test]
    fn live_journal_blocks_a_second_owner_until_it_ends() {
        let root = tempfile::tempdir().unwrap();
        let plan = json!({"synthetic":"live owner exclusion"});
        let journal = Journal::open_verification(root.path(), &plan, 1).unwrap();
        assert!(Journal::open_verification(root.path(), &plan, 1)
            .err()
            .unwrap()
            .contains("Another process owns"));
        drop(journal);
        let reopened = Journal::open_verification(root.path(), &plan, 1).unwrap();
        drop(reopened);
    }

    #[test]
    fn independent_reviewer_reservations_are_hard_and_reuse_costs_nothing() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("operations")).unwrap();
        let mut j = Journal {
            root: root.path().into(),
            next: 0,
            plan_sha256: digest("plan"),
            max_teacher: 2,
            max_review: 1,
            max_physical: 9,
            _lock: fs::File::create(root.path().join("run.lock")).unwrap(),
            ownership_retained: AtomicBool::new(false),
        };
        let request = json!({"native_trace_sha":"case-a"});
        let Operation::Fresh(d) = j.begin_review(&request, true).unwrap() else {
            panic!()
        };
        assert!(j
            .begin_review(&json!({"native_trace_sha":"case-b"}), true)
            .err()
            .unwrap()
            .contains("budget"));
        j.next = 0;
        assert!(j
            .begin_review(&request, true)
            .err()
            .unwrap()
            .contains("Unsettled"));
        j.finish(&d, &json!({"native_trace_sha":"case-a","score":1}))
            .unwrap();
        j.next = 0;
        assert!(matches!(
            j.begin_review(&request, false).unwrap(),
            Operation::Reused(_)
        ));
        let Operation::Fresh(cache) = j.begin_review(&request, false).unwrap() else {
            panic!()
        };
        j.finish(&cache, &json!({"score":1})).unwrap();
        assert_eq!(
            load(&cache.join("reservation.json")).unwrap()["review_calls"],
            0
        );
        assert!(j
            .begin_review(&json!({"native_trace_sha":"case-c"}), true)
            .err()
            .unwrap()
            .contains("budget"));
    }
    #[test]
    fn interrupted_operations_cannot_be_replayed_as_paid_requests() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("operations")).unwrap();
        let lock = fs::File::create(root.path().join("run.lock")).unwrap();
        let mut journal = Journal {
            root: root.path().into(),
            next: 0,
            plan_sha256: digest("p"),
            max_teacher: 1,
            max_review: 1,
            max_physical: 9,
            _lock: lock,
            ownership_retained: AtomicBool::new(false),
        };
        let request = json!({"candidate":"c"});
        let Operation::Fresh(directory) = journal.begin("student", &request, 0, 9).unwrap() else {
            panic!()
        };
        journal.next = 0;
        assert!(journal
            .begin("student", &request, 0, 9)
            .err()
            .unwrap()
            .contains("Unsettled"));
        journal.finish(&directory, &json!({"score":1})).unwrap();
        journal.next = 0;
        assert!(matches!(
            journal.begin("student", &request, 0, 9).unwrap(),
            Operation::Reused(_)
        ));
        assert!(journal
            .begin("student", &request, 0, 9)
            .err()
            .unwrap()
            .contains("budget"));
        journal.next = 0;
        assert!(journal
            .begin("student", &json!({"candidate":"other"}), 0, 9)
            .err()
            .unwrap()
            .contains("Immutable"));
        fs::write(directory.join("response.json"), b"{}").unwrap();
        journal.next = 0;
        assert!(journal
            .begin("student", &request, 0, 9)
            .err()
            .unwrap()
            .contains("Source changed"));
    }

    #[test]
    fn completed_fresh_evaluation_replays_when_its_cache_now_exists() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("operations")).unwrap();
        let mut journal = Journal {
            root: root.path().into(),
            next: 0,
            plan_sha256: digest("p"),
            max_teacher: 1,
            max_review: 1,
            max_physical: 9,
            _lock: fs::File::create(root.path().join("run.lock")).unwrap(),
            ownership_retained: AtomicBool::new(false),
        };
        let request = json!({"case":"case-a","candidate":"fixed"});
        let Operation::Fresh(directory) = journal.begin("classification", &request, 0, 9).unwrap()
        else {
            panic!()
        };
        journal.finish(&directory, &json!({"score":1})).unwrap();
        journal.next = 0;
        // A cache is now available. This changes the physical cost, never identity.
        assert!(matches!(
            journal.begin("classification", &request, 0, 0).unwrap(),
            Operation::Reused(_)
        ));
        let Operation::Fresh(cache_hit) = journal.begin("classification", &request, 0, 0).unwrap()
        else {
            panic!()
        };
        journal.finish(&cache_hit, &json!({"score":1})).unwrap();
        journal.next = 1;
        assert!(matches!(
            journal.begin("classification", &request, 0, 0).unwrap(),
            Operation::Reused(_)
        ));
        assert_eq!(
            load(&directory.join("reservation.json")).unwrap()["physical_generation_attempts"],
            9
        );
        assert_eq!(
            load(&cache_hit.join("reservation.json")).unwrap()["physical_generation_attempts"],
            0
        );
    }
}
