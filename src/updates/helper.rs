//! The helper runs from an owned copy of the old executable, outside the GUI.
use super::{
    files::{self, BACKUP, Plan, RUNNER},
    handoff,
    transaction::{self, Outcome},
};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Startup {
    directory: PathBuf,
    recovered: bool,
}

/// Handles updater-only arguments before normal startup and argument parsing.
pub(crate) fn startup() -> Result<Option<Startup>, String> {
    let args: Vec<_> = std::env::args_os().collect();
    let Some(mode) = args.get(1) else {
        return Ok(None);
    };
    if mode == handoff::APPLY {
        let result = args
            .get(2)
            .ok_or("Missing update workspace.".to_owned())
            .and_then(|path| run(Path::new(path)));
        if let Err(error) = &result {
            if let Some(path) = args.get(2) {
                let directory = Path::new(path);
                // Only report into a workspace whose plan and helper identity validate.
                if validate_helper(directory).is_ok() {
                    let _ = files::write_new(&directory.join("error.txt"), error.as_bytes());
                    if directory.join("proceed").is_file() {
                        rfd::MessageDialog::new()
                            .set_title("Sundial Update Failed")
                            .set_description(error)
                            .set_level(rfd::MessageLevel::Error)
                            .show();
                    }
                }
            }
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if mode != handoff::STARTED && mode != handoff::RECOVERED {
        return Ok(None);
    }
    let directory = PathBuf::from(args.get(2).ok_or("Missing update workspace.")?)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let plan = Plan::read(&directory)?;
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    if executable != plan.target {
        return Err("The restarted executable does not match the update plan.".into());
    }
    let recovered = mode == handoff::RECOVERED;
    files::verify(
        &executable,
        if recovered {
            &plan.old_digest
        } else {
            &plan.new_digest
        },
    )?;
    if !recovered && !super::release::versions_match(env!("CARGO_PKG_VERSION"), &plan.version) {
        return Err(
            "The downloaded executable does not match the selected release version.".into(),
        );
    }
    Ok(Some(Startup {
        directory,
        recovered,
    }))
}

impl Startup {
    pub(crate) fn window_created(self) -> Result<(), String> {
        if self.recovered {
            let reason =
                files::read_small(&self.directory.join("rolled-back.txt")).unwrap_or_else(|_| {
                    b"The update could not start. Recovery details could not be saved.".to_vec()
                });
            rfd::MessageDialog::new().set_title("Previous Version Restored")
                .set_description(format!("The update could not start, so Sundial restored the previous executable.\n\n{}",
                    String::from_utf8_lossy(&reason)))
                .set_level(rfd::MessageLevel::Warning).show();
        } else {
            files::write_new(
                &self.directory.join("started"),
                std::process::id().to_string().as_bytes(),
            )?;
        }
        Ok(())
    }
}

fn validate_helper(directory: &Path) -> Result<Plan, String> {
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let plan = Plan::read(&directory)?;
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    if executable != directory.join(RUNNER) {
        return Err("The update helper is not in its staging workspace.".into());
    }
    files::verify(&executable, &plan.old_digest)?;
    Ok(plan)
}

fn run(directory: &Path) -> Result<(), String> {
    let plan = validate_helper(directory)?;
    files::verify(&directory.join(files::PAYLOAD), &plan.new_digest)?;
    let lock = files::open_lock(&plan.target)?;
    files::write_new(&directory.join("ready"), b"1")?;
    let deadline = Instant::now() + Duration::from_secs(120);
    while fs2::FileExt::try_lock_exclusive(&lock).is_err() {
        if Instant::now() >= deadline {
            return Err("Sundial did not close in time. The update was not installed.".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !directory.join("proceed").is_file() {
        return Ok(());
    }
    let outcome = transaction::apply(directory, &plan, || launch_and_check(directory, &plan))?;
    finish(directory, &plan, outcome, || {
        let mut command = handoff::command(&plan.target);
        command.arg(handoff::RECOVERED).arg(directory);
        add_install(&mut command, &plan);
        command.spawn().map(|_| ()).map_err(|error| {
            format!(
                "The previous executable was restored but could not restart: {error}. Open {}.",
                plan.target.display()
            )
        })
    })
}

fn finish(
    directory: &Path,
    plan: &Plan,
    outcome: Outcome,
    restart: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    // Receipts are diagnostic. Once replacement or restoration succeeds, an
    // unwritable receipt must not report a false failure or prevent recovery.
    match outcome {
        Outcome::Installed => {
            let _ = files::write_new(&directory.join("complete"), plan.version.as_bytes());
            Ok(())
        }
        Outcome::Restored(error) => {
            let _ = files::write_new(&directory.join("rolled-back.txt"), error.as_bytes());
            restart()
        }
    }
}

fn add_install(command: &mut std::process::Command, plan: &Plan) {
    if let Some(install) = &plan.install {
        command.arg("--install").arg(install);
    }
}

fn launch_and_check(directory: &Path, plan: &Plan) -> Result<(), String> {
    let mut command = handoff::command(&plan.target);
    command.arg(handoff::STARTED).arg(directory);
    add_install(&mut command, plan);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start the updated executable: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if startup_ready(&mut child, directory)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill().map_err(|error| format!("The updated executable did not open its window and could not be stopped: {error}. Backup: {}", directory.join(BACKUP).display()))?;
            child.wait().map_err(|error| error.to_string())?;
            return Err("The updated executable did not open its window in time.".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn startup_ready(child: &mut std::process::Child, directory: &Path) -> Result<bool, String> {
    if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
        return Err(format!(
            "The updated executable exited during startup ({status})."
        ));
    }
    // A receipt from an already exited process is not a healthy restart.
    Ok(files::read_small(&directory.join("started"))
        .is_ok_and(|started| started == child.id().to_string().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn an_exited_process_is_not_accepted_even_with_a_matching_startup_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let mut child = handoff::command(&std::env::current_exe().unwrap())
            .arg("--list")
            .spawn()
            .unwrap();
        child.wait().unwrap();
        files::write_new(
            &directory.path().join("started"),
            child.id().to_string().as_bytes(),
        )
        .unwrap();
        assert!(startup_ready(&mut child, directory.path()).is_err());
    }

    #[test]
    fn receipt_failures_do_not_hide_success_or_prevent_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let plan = Plan {
            target: directory.path().join("sundial"),
            version: "v0.5".into(),
            old_digest: String::new(),
            new_digest: String::new(),
            install: None,
        };
        fs::create_dir(directory.path().join("complete")).unwrap();
        fs::create_dir(directory.path().join("rolled-back.txt")).unwrap();
        finish(directory.path(), &plan, Outcome::Installed, || {
            panic!("An installed update must not restart the previous version")
        })
        .unwrap();
        let mut restarted = false;
        finish(
            directory.path(),
            &plan,
            Outcome::Restored("start failed".into()),
            || {
                restarted = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(restarted);
        assert!(
            finish(
                directory.path(),
                &plan,
                Outcome::Restored("start failed".into()),
                || { Err("restart failed".into()) }
            )
            .is_err()
        );
    }

    #[test]
    fn helper_process() {
        let Some(directory) = std::env::var_os("SUNDIAL_UPDATE_TEST_WORKSPACE") else {
            return;
        };
        let should_fail = std::env::var_os("SUNDIAL_UPDATE_TEST_FAIL").is_some();
        let result = run(Path::new(&directory));
        assert_eq!(result.is_err(), should_fail, "{result:?}");
    }

    #[test]
    fn helper_waits_for_the_parent_lease_and_never_applies_canceled_or_changed_payloads() {
        for tamper in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let directory = tempfile::Builder::new()
                .prefix(files::PREFIX)
                .tempdir_in(root.path())
                .unwrap();
            let current = std::env::current_exe().unwrap();
            let target = root.path().join(if cfg!(windows) {
                "sundial.exe"
            } else {
                "sundial"
            });
            files::copy_new(&current, &target).unwrap();
            let target = target.canonicalize().unwrap();
            files::copy_new(&target, &directory.path().join(RUNNER)).unwrap();
            files::copy_new(&target, &directory.path().join(files::PAYLOAD)).unwrap();
            let original = files::digest(&target).unwrap();
            let plan = Plan {
                target: target.clone(),
                version: "v99.0".into(),
                old_digest: original.clone(),
                new_digest: original.clone(),
                install: None,
            };
            plan.write(directory.path()).unwrap();
            let lock = files::open_lock(&target).unwrap();
            fs2::FileExt::try_lock_exclusive(&lock).unwrap();
            let mut command = handoff::command(&directory.path().join(RUNNER));
            command
                .args([
                    "--exact",
                    "updates::helper::tests::helper_process",
                    "--nocapture",
                ])
                .env("SUNDIAL_UPDATE_TEST_WORKSPACE", directory.path());
            if tamper {
                command.env("SUNDIAL_UPDATE_TEST_FAIL", "1");
            }
            let mut child = command.spawn().unwrap();
            wait_for(&mut child, || directory.path().join("ready").is_file());
            assert!(!directory.path().join("replacement-started").exists());
            if tamper {
                fs::write(
                    directory.path().join(files::PAYLOAD),
                    b"changed after readiness",
                )
                .unwrap();
                files::write_new(&directory.path().join("proceed"), b"1").unwrap();
            }
            drop(lock);
            wait_for(&mut child, || false);
            assert!(child.wait().unwrap().success());
            assert_eq!(files::digest(&target).unwrap(), original);
            assert!(!directory.path().join(BACKUP).exists());
        }
    }

    fn wait_for(child: &mut std::process::Child, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if ready() || child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("updater helper did not complete its handshake");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
