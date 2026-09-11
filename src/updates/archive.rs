//! Read only the expected executable. Archive paths never become output paths.
use super::{
    download::Progress,
    release::{ArchiveKind, Asset, MAX_DOWNLOAD_BYTES},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::Ordering,
};

pub(super) fn extract(
    source: &Path,
    destination: &Path,
    asset: &Asset,
    progress: &Progress,
) -> Result<(), String> {
    let source = File::open(source).map_err(|error| error.to_string())?;
    match asset.kind {
        ArchiveKind::Zip => extract_zip(source, destination, &asset.member, progress),
        ArchiveKind::TarGz => extract_tar(source, destination, &asset.member, progress),
    }
}

fn extract_zip(
    source: File,
    destination: &Path,
    member: &str,
    progress: &Progress,
) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(source).map_err(|error| error.to_string())?;
    if archive.len() > 1024 {
        return Err("The update archive has too many entries.".into());
    }
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        // Archive names select a member, never a filesystem destination.
        if entry.name().replace('\\', "/") != member {
            continue;
        }
        if found || entry.is_dir() || entry.is_symlink() || entry.encrypted() {
            return Err("The update archive does not contain a unique regular executable.".into());
        }
        let size = entry.size();
        copy_executable(&mut entry, destination, size, progress)?;
        found = true;
    }
    if !found {
        return Err(format!("The update archive is missing {member}."));
    }
    Ok(())
}

fn extract_tar(
    source: File,
    destination: &Path,
    member: &str,
    progress: &Progress,
) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(source);
    let reader = BoundedReader {
        inner: decoder,
        remaining: MAX_DOWNLOAD_BYTES * 2,
        progress,
    };
    let mut archive = tar::Archive::new(reader);
    let mut found = false;
    for (index, entry) in archive
        .entries()
        .map_err(|error| error.to_string())?
        .enumerate()
    {
        if index >= 1024 {
            return Err("The update archive has too many entries.".into());
        }
        let mut entry = entry.map_err(|error| error.to_string())?;
        if entry.path_bytes().as_ref() != member.as_bytes() {
            continue;
        }
        if found || !entry.header().entry_type().is_file() {
            return Err("The update archive does not contain a unique regular executable.".into());
        }
        let size = entry.size();
        copy_executable(&mut entry, destination, size, progress)?;
        found = true;
    }
    if !found {
        return Err(format!("The update archive is missing {member}."));
    }
    Ok(())
}

fn copy_executable(
    reader: impl Read,
    destination: &Path,
    size: u64,
    progress: &Progress,
) -> Result<(), String> {
    if size == 0 || size > MAX_DOWNLOAD_BYTES {
        return Err("The archived executable has an unsupported size.".into());
    }
    let mut reader = BoundedReader {
        inner: reader,
        remaining: size + 1,
        progress,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let copied = io::copy(&mut reader, &mut file).map_err(|error| error.to_string())?;
    if copied != size {
        return Err("The archived executable is truncated or oversized.".into());
    }
    file.flush()
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

struct BoundedReader<'a, R> {
    inner: R,
    remaining: u64,
    progress: &'a Progress,
}
impl<R: Read> Read for BoundedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.progress.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("Download canceled."));
        }
        if self.remaining == 0 {
            return Err(io::Error::other(
                "The archive exceeds the extraction limit.",
            ));
        }
        let limit = buffer
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buffer[..limit])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn both_archives_extract_only_the_named_payload_and_reject_missing_or_linked_payloads() {
        let directory = tempfile::tempdir().unwrap();
        for kind in [ArchiveKind::Zip, ArchiveKind::TarGz] {
            let archive_path = directory.path().join("archive");
            let member = "Sundial-v1.0-test/sundial";
            let file = File::create(&archive_path).unwrap();
            match kind {
                ArchiveKind::Zip => {
                    let mut zip = zip::ZipWriter::new(file);
                    let options = zip::write::SimpleFileOptions::default();
                    zip.start_file("../outside", options).unwrap();
                    zip.write_all(b"ignored").unwrap();
                    zip.start_file(member, options).unwrap();
                    zip.write_all(b"executable").unwrap();
                    zip.finish().unwrap();
                }
                ArchiveKind::TarGz => {
                    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
                        file,
                        flate2::Compression::default(),
                    ));
                    let mut header = tar::Header::new_gnu();
                    header.set_size(10);
                    header.set_mode(0o755);
                    header.set_cksum();
                    tar.append_data(&mut header, member, b"executable".as_slice())
                        .unwrap();
                    tar.into_inner().unwrap().finish().unwrap();
                }
            }
            let output = directory.path().join("payload");
            let mut asset = Asset {
                url: String::new(),
                size: 0,
                digest: String::new(),
                kind,
                member: member.into(),
            };
            extract(&archive_path, &output, &asset, &Progress::default()).unwrap();
            assert_eq!(fs::read(&output).unwrap(), b"executable");
            fs::remove_file(&output).unwrap();
            asset.member = "missing".into();
            assert!(extract(&archive_path, &output, &asset, &Progress::default()).is_err());
            assert!(!output.exists());
        }
        let archive_path = directory.path().join("linked.tar.gz");
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            File::create(&archive_path).unwrap(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        tar.append_link(&mut header, "sundial", "../elsewhere")
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let asset = Asset {
            url: String::new(),
            size: 0,
            digest: String::new(),
            kind: ArchiveKind::TarGz,
            member: "sundial".into(),
        };
        assert!(
            extract(
                &archive_path,
                &directory.path().join("payload"),
                &asset,
                &Progress::default()
            )
            .is_err()
        );
    }
}
