use super::{
    files::{self, PAYLOAD, Plan},
    release::{Asset, Release},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
use tempfile::TempDir;

#[derive(Default)]
pub(super) struct Progress {
    pub received: AtomicU64,
    pub cancel: AtomicBool,
}

pub(super) struct Prepared {
    pub directory: TempDir,
    pub plan: Plan,
    pub lock: File,
}

pub(super) fn prepare(release: &Release, progress: &Progress) -> Result<Prepared, String> {
    let asset = release.asset.as_ref().map_err(Clone::clone)?;
    let target = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    if cfg!(windows)
        && fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .permissions()
            .readonly()
    {
        return Err(
            "The Sundial executable is read-only. Make it writable before updating.".into(),
        );
    }
    let lock = files::open_lock(&target)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| "Another Sundial update is already in progress.".to_owned())?;
    let directory = tempfile::Builder::new()
        .prefix(files::PREFIX)
        .tempdir_in(
            target
                .parent()
                .ok_or("The executable has no parent folder.")?,
        )
        .map_err(|error| format!("Cannot stage the update beside Sundial: {error}"))?;
    let old_digest = files::digest(&target)?;
    let payload = directory.path().join(PAYLOAD);
    let archive = directory.path().join("download");
    download(asset, &archive, progress)?;
    super::archive::extract(&archive, &payload, asset, progress)?;
    files::verify_executable(&payload)?;
    let new_digest = files::digest(&payload)?;
    fs::remove_file(&archive).map_err(|error| error.to_string())?;
    fs::set_permissions(
        &payload,
        fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .permissions(),
    )
    .map_err(|error| error.to_string())?;
    files::verify(&target, &old_digest)?;
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("Download canceled.".into());
    }
    Ok(Prepared {
        directory,
        lock,
        plan: Plan {
            target,
            version: release.version.clone(),
            old_digest,
            new_digest,
            install: None,
        },
    })
}

fn download(asset: &Asset, destination: &PathBuf, progress: &Progress) -> Result<(), String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(&asset.url)
        .header(
            "User-Agent",
            &format!("Sundial/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|error| format!("Could not download the update: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    transfer(response.body_mut().as_reader(), &mut file, asset, progress)?;
    file.sync_all().map_err(|error| error.to_string())
}

fn transfer(
    mut reader: impl Read,
    mut writer: impl Write,
    asset: &Asset,
    progress: &Progress,
) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut received = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("Download canceled.".into());
        }
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("Download interrupted: {error}"))?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > asset.size {
            return Err("The update exceeded its published size.".into());
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        hash.update(&buffer[..count]);
        progress.received.store(received, Ordering::Relaxed);
    }
    if received != asset.size || format!("{:x}", hash.finalize()) != asset.digest {
        return Err("The update failed size or checksum verification. The installed executable was not changed.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_verified_downloads_are_accepted() {
        let expected = b"a verified release";
        let asset = Asset {
            url: String::new(),
            size: expected.len() as u64,
            kind: super::super::release::ArchiveKind::Zip,
            member: String::new(),
            digest: format!("{:x}", Sha256::digest(expected)),
        };
        for (bytes, cancel, valid) in [
            (expected.as_slice(), false, true),
            (&expected[..5], false, false),
            (b"wrong release data".as_slice(), false, false),
            (b"oversized untrusted release".as_slice(), false, false),
            (expected.as_slice(), true, false),
        ] {
            let progress = Progress::default();
            progress.cancel.store(cancel, Ordering::Relaxed);
            let mut output = Vec::new();
            assert_eq!(
                transfer(bytes, &mut output, &asset, &progress).is_ok(),
                valid
            );
            if valid {
                assert_eq!(output, expected);
            }
            assert!(output.len() <= expected.len());
        }
    }
}
