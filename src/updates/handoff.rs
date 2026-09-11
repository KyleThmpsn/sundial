use super::{
    download::Prepared,
    files::{self, RUNNER},
};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) const APPLY: &str = "--sundial-apply-update";
pub(super) const STARTED: &str = "--sundial-update-started";
pub(super) const RECOVERED: &str = "--sundial-update-recovered";

pub(super) struct Handoff {
    pub directory: PathBuf,
    lock: Option<File>,
    child: Child,
}

impl Handoff {
    pub fn commit(&mut self) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err(
                "The update helper stopped before restart. Sundial has not been replaced.".into(),
            );
        }
        files::write_new(&self.directory.join("proceed"), b"1")?;
        // The helper must wait for process exit, not just the GUI object's drop.
        // The OS closes this handle when the confirmed restart exits Sundial.
        if let Some(lock) = self.lock.take() {
            std::mem::forget(lock);
        }
        Ok(())
    }
}

impl Drop for Handoff {
    fn drop(&mut self) {
        if self.lock.is_some() {
            // Abort before releasing the lease, including a failed intent-marker write.
            // This child belongs exclusively to this handoff and cannot replace files yet.
            let _ = self.child.kill();
            if self.child.wait().is_ok() {
                let _ = std::fs::remove_dir_all(&self.directory);
            }
        }
    }
}

pub(super) fn begin(mut prepared: Prepared, install: PathBuf) -> Result<Handoff, String> {
    prepared.plan.install = Some(install);
    files::verify(&prepared.plan.target, &prepared.plan.old_digest)?;
    files::verify(
        &prepared.directory.path().join(files::PAYLOAD),
        &prepared.plan.new_digest,
    )?;
    let runner = prepared.directory.path().join(RUNNER);
    files::copy_new(&prepared.plan.target, &runner)?;
    files::verify(&runner, &prepared.plan.old_digest)?;
    prepared.plan.write(prepared.directory.path())?;
    let mut command = command(&runner);
    let mut child = command
        .arg(APPLY)
        .arg(prepared.directory.path())
        .spawn()
        .map_err(|error| format!("Could not start the update helper: {error}"))?;
    if let Err(error) = wait_ready(&mut child, prepared.directory.path()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(Handoff {
        directory: prepared.directory.keep(),
        lock: Some(prepared.lock),
        child,
    })
}

fn wait_ready(child: &mut Child, directory: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if directory.join("ready").is_file() {
            return Ok(());
        }
        if directory.join("error.txt").is_file() {
            return Err(
                String::from_utf8_lossy(&files::read_small(&directory.join("error.txt"))?)
                    .into_owned(),
            );
        }
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err(
                "The update helper stopped before it was ready. Sundial has not been replaced."
                    .into(),
            );
        }
        if Instant::now() >= deadline {
            return Err(
                "The update helper did not become ready. Sundial has not been replaced.".into(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}
