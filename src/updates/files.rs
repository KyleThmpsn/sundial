//! Checked updater files stay next to the executable on the same filesystem.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) const PREFIX: &str = ".sundial-update-";
pub(super) const PLAN: &str = "plan.json";
pub(super) const PAYLOAD: &str = "payload";
pub(super) const RUNNER: &str = if cfg!(windows) {
    "runner.exe"
} else {
    "runner"
};
pub(super) const BACKUP: &str = if cfg!(windows) {
    "previous.exe"
} else {
    "previous-sundial"
};

#[derive(Debug, Serialize, Deserialize)]
// Shared by the old helper and the new application. Preserve this wire contract
// when changing startup or adding fields in later releases.
pub(super) struct Plan {
    pub target: PathBuf,
    pub version: String,
    pub old_digest: String,
    pub new_digest: String,
    pub install: Option<PathBuf>,
}

impl Plan {
    pub fn read(directory: &Path) -> Result<Self, String> {
        let directory = directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(PREFIX))
        {
            return Err("Invalid update workspace.".into());
        }
        let bytes = read_small(&directory.join(PLAN))?;
        let plan: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if !plan.target.is_absolute()
            || plan.target.parent() != directory.parent()
            || plan.target.file_name().is_none()
            || !super::release::valid_digest(&plan.old_digest)
            || !super::release::valid_digest(&plan.new_digest)
            || super::release::version_components(&plan.version).is_none()
        {
            return Err("The update plan does not match its executable folder.".into());
        }
        // Target existence and type are checked immediately before reading or
        // replacing it. A missing target must not hide an otherwise valid error report.
        Ok(plan)
    }

    pub fn write(&self, directory: &Path) -> Result<(), String> {
        write_new(
            &directory.join(PLAN),
            &serde_json::to_vec(self).map_err(|error| error.to_string())?,
        )
    }
}

pub(super) fn plain_file(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file.", path.display()));
    }
    Ok(())
}

pub(super) fn digest(path: &Path) -> Result<String, String> {
    plain_file(path)?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn verify(path: &Path, expected: &str) -> Result<(), String> {
    if digest(path)? != expected {
        return Err(format!(
            "{} changed or failed checksum verification. Replacement was stopped.",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    plain_file(path)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("Update metadata is too large.".into());
    }
    Ok(bytes)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

pub(super) fn copy_new(source: &Path, destination: &Path) -> Result<(), String> {
    plain_file(source)?;
    let mut source_file = File::open(source).map_err(|error| error.to_string())?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    std::io::copy(&mut source_file, &mut destination_file).map_err(|error| error.to_string())?;
    destination_file
        .set_permissions(
            source_file
                .metadata()
                .map_err(|error| error.to_string())?
                .permissions(),
        )
        .and_then(|()| destination_file.sync_all())
        .map_err(|error| error.to_string())
}

pub(super) fn open_lock(target: &Path) -> Result<File, String> {
    let mut name = target
        .file_name()
        .ok_or("The executable has no filename.")?
        .to_os_string();
    name.push(".update.lock");
    let path = target.with_file_name(name);
    if path.try_exists().map_err(|error| error.to_string())? {
        plain_file(&path)?;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
        .map_err(|error| format!("Cannot write to the Sundial folder: {error}. Move Sundial to a folder you can write to, then try again."))
}

pub(super) fn verify_executable(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut header = [0; 64];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    if cfg!(windows) {
        use std::io::{Seek, SeekFrom};
        if &header[..2] != b"MZ" {
            return Err("The download is not a Windows executable.".into());
        }
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
        file.seek(SeekFrom::Start(offset.into()))
            .map_err(|error| error.to_string())?;
        let mut pe = [0; 6];
        file.read_exact(&mut pe)
            .map_err(|error| error.to_string())?;
        if &pe[..4] != b"PE\0\0" || pe[4..] != [0x64, 0x86] {
            return Err("The update is not a 64-bit Windows executable.".into());
        }
    } else if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[18..20] != [62, 0]
    {
        return Err("The update is not a 64-bit Linux executable.".into());
    }
    Ok(())
}
