//! Ordered, immutable operation replay reconstructs GEPA's deterministic state.
//! A prepared or uncertain operation is never submitted again automatically.
use crate::{keep, load, read, verify, Plan, Result};
use fs2::FileExt;
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
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
    max_physical: u64,
    _lock: fs::File,
}
impl Journal {
    pub fn retain_ownership(&self) -> Result<fs::File> {
        self._lock.try_clone().map_err(|error| error.to_string())
    }
    pub fn open(root: &Path, plan: &Plan) -> Result<Self> {
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
            max_teacher: plan.max_teacher_calls,
            max_physical: plan.max_physical_generation_attempts,
            _lock: lock,
        })
    }
    pub fn begin(
        &mut self,
        kind: &str,
        request: &Value,
        teacher: u64,
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
            println!(
                "{}",
                json!({"event":"operation_reused","sequence":sequence,"kind":kind})
            );
            return Ok(Operation::Reused(load(&directory.join("response.json"))?));
        }
        let mut used_teacher = 0u64;
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
            }
        }
        if used_teacher.saturating_add(teacher) > self.max_teacher
            || used_physical.saturating_add(physical) > self.max_physical
        {
            return Err(format!("Hard paid-call reservation would exceed budget: teacher {used_teacher}+{teacher}/{}, student {used_physical}+{physical}/{}; no request submitted", self.max_teacher,self.max_physical));
        }
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        keep(&directory.join("request.json"), &identity)?;
        keep(
            &directory.join("reservation.json"),
            &json!({"teacher_calls":teacher,"physical_generation_attempts":physical,"reservation_before_submission":true}),
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
    fn interrupted_operations_cannot_be_replayed_as_paid_requests() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("operations")).unwrap();
        let lock = fs::File::create(root.path().join("run.lock")).unwrap();
        let mut journal = Journal {
            root: root.path().into(),
            next: 0,
            plan_sha256: digest("p"),
            max_teacher: 1,
            max_physical: 9,
            _lock: lock,
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
            max_physical: 9,
            _lock: fs::File::create(root.path().join("run.lock")).unwrap(),
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
