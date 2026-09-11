//! Replacement and rollback never overwrite an executable changed by another writer.
use super::files::{self, BACKUP, PAYLOAD, Plan};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) enum Outcome {
    Installed,
    Restored(String),
}

pub(super) fn apply(
    directory: &Path,
    plan: &Plan,
    launch: impl FnOnce() -> Result<(), String>,
) -> Result<Outcome, String> {
    let payload = directory.join(PAYLOAD);
    files::verify(&plan.target, &plan.old_digest)?;
    files::verify(&payload, &plan.new_digest)?;
    let backup = directory.join(BACKUP);
    files::copy_new(&plan.target, &backup)?;
    files::verify(&backup, &plan.old_digest)?;
    files::write_new(&directory.join("replacement-started"), b"1")?;
    let replaced = replace_checked(&payload, &plan.target, &plan.old_digest);
    let result = match replaced {
        Ok(()) => launch(),
        Err(error) if files::digest(&plan.target).ok().as_deref() == Some(&plan.new_digest) => {
            Err(error)
        }
        Err(error) => return Err(error),
    };
    match result {
        Ok(()) => Ok(Outcome::Installed),
        Err(error) => {
            restore(directory, plan).map_err(|restore_error| format!(
                "{error}\nRecovery could not replace the executable: {restore_error}\nThe previous executable is at {}.", backup.display()))?;
            Ok(Outcome::Restored(error))
        }
    }
}

fn restore(directory: &Path, plan: &Plan) -> Result<(), String> {
    let backup = directory.join(BACKUP);
    files::verify(&backup, &plan.old_digest)?;
    files::verify(&plan.target, &plan.new_digest)?;
    let rollback = directory.join("rollback");
    files::copy_new(&backup, &rollback)?;
    replace_checked(&rollback, &plan.target, &plan.new_digest)
}

fn replace_checked(source: &Path, target: &Path, expected: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        files::verify(target, expected)?;
        match crate::storage::replace_path(source, target) {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!("Could not replace {}: {error}", target.display()));
            }
            Err(error)
                if !matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
                ) && !matches!(error.raw_os_error(), Some(26 | 32 | 33)) =>
            {
                return Err(error.to_string());
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn replacement_retains_backup_and_failed_launch_rolls_back_without_losing_external_edits() {
        for scenario in [
            "success",
            "launch failure",
            "external edit",
            "bad payload",
            "changed target",
        ] {
            let root = tempfile::tempdir().unwrap();
            let directory = tempfile::Builder::new()
                .prefix(files::PREFIX)
                .tempdir_in(root.path())
                .unwrap();
            let target = root.path().join("sundial");
            let payload = directory.path().join(PAYLOAD);
            fs::write(&target, b"old executable").unwrap();
            fs::write(&payload, b"new executable").unwrap();
            let plan = Plan {
                target: target.clone(),
                version: "v99.0".into(),
                old_digest: files::digest(&target).unwrap(),
                new_digest: files::digest(&payload).unwrap(),
                install: None,
            };
            if scenario == "bad payload" {
                fs::write(&payload, b"corrupted").unwrap();
            }
            if scenario == "changed target" {
                fs::write(&target, b"outside change").unwrap();
            }
            let result = apply(directory.path(), &plan, || {
                assert_eq!(fs::read(&target).unwrap(), b"new executable");
                if scenario == "external edit" {
                    fs::write(&target, b"outside change").unwrap();
                }
                if scenario == "success" {
                    Ok(())
                } else {
                    Err("start failed".into())
                }
            });
            assert_outcome(scenario, result, &target);
            if !matches!(scenario, "bad payload" | "changed target") {
                assert_eq!(
                    fs::read(directory.path().join(BACKUP)).unwrap(),
                    b"old executable"
                );
            }
        }
    }
    fn assert_outcome(scenario: &str, result: Result<Outcome, String>, target: &Path) {
        match scenario {
            "success" => {
                assert!(matches!(result, Ok(Outcome::Installed)));
                assert_eq!(fs::read(target).unwrap(), b"new executable");
            }
            "launch failure" => {
                assert!(matches!(result, Ok(Outcome::Restored(_))));
                assert_eq!(fs::read(target).unwrap(), b"old executable");
            }
            "external edit" | "changed target" => {
                assert!(result.is_err());
                assert_eq!(fs::read(target).unwrap(), b"outside change");
            }
            _ => {
                assert!(result.is_err());
                assert_eq!(fs::read(target).unwrap(), b"old executable");
            }
        }
    }
}
