//! Opt-in process cancellation workflow with a repeatable receipt and no game packages.
use anyhow::{Context, Result, ensure};
use parhelion_import::cancellation;
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
#[ignore = "Helper process launched only by cancellation_stops_active_tools_and_releases_the_worker"]
fn cancellation_child() {
    let Some(ready) = env::var_os("PARHELION_CANCEL_CHILD_READY") else {
        return;
    };
    fs::write(ready, b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "Requires a fresh PARHELION_CANCEL_OUTPUT artifact directory"]
fn cancellation_stops_active_tools_and_releases_the_worker() -> Result<()> {
    let output = PathBuf::from(
        env::var_os("PARHELION_CANCEL_OUTPUT").context("Set PARHELION_CANCEL_OUTPUT")?,
    );
    ensure!(
        !output.exists(),
        "Use a fresh cancellation artifact directory"
    );
    fs::create_dir_all(&output)?;
    let ready = output.join("child-ready");
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    let mut command = Command::new(env::current_exe()?);
    command
        .args(["--exact", "cancellation_child", "--ignored", "--nocapture"])
        .env("PARHELION_CANCEL_CHILD_READY", &ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let trigger = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = ready.exists();
        let clicked = Instant::now();
        signal.store(true, Ordering::Relaxed);
        (ready, clicked)
    });
    let mut pid = 0;
    let result = cancellation::run(Arc::clone(&cancel), || {
        let child = command.spawn()?;
        pid = child.id();
        cancellation::wait_with_output(child)
    });
    let (ready, clicked) = trigger.join().expect("Cancellation trigger");
    let stopped_ms = clicked.elapsed().as_millis();
    ensure!(ready, "Child process failed to start");
    ensure!(
        result
            .as_ref()
            .err()
            .is_some_and(cancellation::is_cancelled),
        "Active process did not report cancellation"
    );
    ensure!(
        stopped_ms < 2_000,
        "Cancellation waited for the tool to finish"
    );
    let denied = cancellation::run(cancel, || {
        fs::write(output.join("must-not-publish"), b"unexpected")?;
        Ok(())
    });
    ensure!(
        denied
            .as_ref()
            .err()
            .is_some_and(cancellation::is_cancelled),
        "Already cancelled work started"
    );
    ensure!(
        !output.join("must-not-publish").exists(),
        "Cancelled output was published"
    );
    cancellation::check()?;
    cancellation::run(Arc::new(AtomicBool::new(false)), || {
        fs::write(output.join("next-job.txt"), b"completed")?;
        Ok(())
    })?;
    fs::write(
        output.join("verified-cancellation.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "child_pid":pid,"cancel_to_return_ms":stopped_ms,"active_tool_cancelled":true,
            "cancelled_output_absent":true,"next_job_completed":true
        }))?,
    )?;
    Ok(())
}
