use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    mem::size_of,
    path::{Path, PathBuf},
};

use tiger_pkg::manager::PackagePath;

use crate::{
    AuthoringError, AuthoringResult,
    appended_tags::MAX_PACKAGE_ENTRY_COUNT,
    format::{ENTRY_COUNT_OFFSET, W64_PLATFORM},
    package_profile::{PACKAGE_HEADER_PREFIX_SIZE, PackageHeaderPrefix, SHADOWKEEP_HEADER_VERSION},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageIdentity {
    pub platform: String,
    pub name: String,
    pub language: Option<String>,
    pub package_id: u16,
    /// Filename without the patch suffix or `.pkg` extension.
    pub stem: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchFile {
    pub patch: u8,
    pub path: PathBuf,
    /// Entry count declared by this generation, retained so retired datum indices are not reused.
    pub entry_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchChain {
    pub directory: PathBuf,
    pub identity: PackageIdentity,
    pub files: Vec<PatchFile>,
}

impl PatchChain {
    pub fn latest(&self) -> &PatchFile {
        self.files
            .last()
            .expect("a discovered patch chain always contains a file")
    }

    pub fn next_patch(&self) -> AuthoringResult<u8> {
        self.latest().patch.checked_add(1).ok_or_else(|| {
            AuthoringError::InvalidInput(format!(
                "Package {:04x} already uses the maximum patch index 255",
                self.identity.package_id
            ))
        })
    }

    pub fn output_file_name(&self) -> AuthoringResult<String> {
        Ok(format!("{}_{}.pkg", self.identity.stem, self.next_patch()?))
    }

    /// Highest entry-table extent declared by any generation in this patch chain.
    ///
    /// A later patch may shrink its table, but native registration can retain the identity of
    /// retired datum slots. New authored tags therefore begin after this high-water mark rather
    /// than after only the latest generation's entry count.
    pub fn historical_entry_count_high_water(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.entry_count)
            .max()
            .expect("a discovered patch chain always contains a file")
    }
}

pub fn discover_patch_chain(directory: &Path, package_id: u16) -> AuthoringResult<PatchChain> {
    let entries = fs::read_dir(directory)
        .map_err(|error| AuthoringError::io("list package directory", directory, error))?;
    let mut files = BTreeMap::new();
    let mut identity = None;

    for entry in entries {
        let entry = entry.map_err(|error| {
            AuthoringError::io("read package directory entry", directory, error)
        })?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| AuthoringError::io("inspect package directory entry", &path, error))?
            .is_file()
        {
            continue;
        }
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
        {
            continue;
        }
        let Some(candidate) = parse_candidate(&path, package_id)? else {
            continue;
        };
        if identity.as_ref().is_some_and(|known| known != &candidate.0) {
            return Err(AuthoringError::InvalidPackage(format!(
                "Package id {package_id:04x} is used by more than one filename identity"
            )));
        }
        identity = Some(candidate.0);
        if files.insert(candidate.1.patch, candidate.1).is_some() {
            return Err(AuthoringError::InvalidPackage(format!(
                "Package id {package_id:04x} has a duplicate patch index"
            )));
        }
    }

    let identity = identity.ok_or_else(|| {
        AuthoringError::InvalidInput(format!(
            "No patchable package with id {package_id:04x} exists in {}",
            directory.display()
        ))
    })?;
    Ok(PatchChain {
        directory: directory.to_path_buf(),
        identity,
        files: files.into_values().collect(),
    })
}

fn parse_candidate(
    path: &Path,
    requested_package_id: u16,
) -> AuthoringResult<Option<(PackageIdentity, PatchFile)>> {
    let path_text = path.to_str().ok_or_else(|| {
        AuthoringError::InvalidPackage(format!(
            "Package filename is not valid Unicode: {}",
            path.display()
        ))
    })?;
    let parsed = PackagePath::parse(path_text);
    let filename_package_id = parsed
        .as_ref()
        .and_then(|hint| u16::from_str_radix(&hint.id, 16).ok());
    let (header, platform, entry_count) = read_header(path)?;

    if let Some(filename_package_id) = filename_package_id
        && filename_package_id != header.package_id
    {
        return Err(AuthoringError::InvalidPackage(format!(
            "Package filename {} claims id {filename_package_id:04x}, but its header declares {:04x}",
            path.display(),
            header.package_id
        )));
    }
    if header.package_id != requested_package_id {
        return Ok(None);
    }
    if header.version != SHADOWKEEP_HEADER_VERSION || platform != W64_PLATFORM {
        return Err(AuthoringError::InvalidPackage(format!(
            "Package {} has header version/platform {}/{platform}; expected {SHADOWKEEP_HEADER_VERSION}/{W64_PLATFORM}",
            path.display(),
            header.version
        )));
    }
    let parsed = parsed.ok_or_else(|| {
        AuthoringError::InvalidPackage(format!(
            "Package {} has no supported filename identity for output naming",
            path.display()
        ))
    })?;
    let patch = u8::try_from(header.patch_id).map_err(|_| {
        AuthoringError::InvalidPackage(format!(
            "Package {} declares patch {}, above the supported maximum 255",
            path.display(),
            header.patch_id
        ))
    })?;
    if parsed.patch != patch {
        return Err(AuthoringError::InvalidPackage(format!(
            "Package filename {} claims patch {}, but its header declares {patch}",
            path.display(),
            parsed.patch
        )));
    }
    if parsed.platform != "w64" {
        return Err(AuthoringError::InvalidPackage(format!(
            "Package filename {} is not a supported w64 output name",
            path.display()
        )));
    }
    let suffix = format!("_{}.pkg", parsed.patch);
    let stem = parsed.filename.strip_suffix(&suffix).ok_or_else(|| {
        AuthoringError::InvalidPackage(format!(
            "Package filename {} has a noncanonical patch suffix",
            parsed.filename
        ))
    })?;
    Ok(Some((
        PackageIdentity {
            platform: parsed.platform,
            name: parsed.name,
            language: parsed.language,
            package_id: header.package_id,
            stem: stem.to_owned(),
        },
        PatchFile {
            patch,
            path: path.to_path_buf(),
            entry_count,
        },
    )))
}

fn read_header(path: &Path) -> AuthoringResult<(PackageHeaderPrefix, u16, usize)> {
    const ENTRY_COUNT_FIELD_END: usize = ENTRY_COUNT_OFFSET + size_of::<u32>();
    let mut bytes = [0u8; ENTRY_COUNT_FIELD_END];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| AuthoringError::io("read package header", path, error))?;
    let prefix: &[u8; PACKAGE_HEADER_PREFIX_SIZE] = bytes[..PACKAGE_HEADER_PREFIX_SIZE]
        .try_into()
        .expect("the package prefix is inside the extended header read");
    let platform = u16::from_le_bytes(
        bytes[2..4]
            .try_into()
            .expect("the fixed package header platform is in bounds"),
    );
    let entry_count = u32::from_le_bytes(
        bytes[ENTRY_COUNT_OFFSET..ENTRY_COUNT_FIELD_END]
            .try_into()
            .expect("the fixed package entry-count field is in bounds"),
    ) as usize;
    if !(1..=MAX_PACKAGE_ENTRY_COUNT).contains(&entry_count) {
        return Err(AuthoringError::InvalidPackage(format!(
            "Package {} declares entry count {entry_count}, outside 1..={MAX_PACKAGE_ENTRY_COUNT}",
            path.display()
        )));
    }
    Ok((PackageHeaderPrefix::parse(prefix), platform, entry_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(path: &Path, package_id: u16, patch_id: u16, entry_count: u32) {
        const ENTRY_COUNT_FIELD_END: usize = ENTRY_COUNT_OFFSET + size_of::<u32>();
        let prefix = PackageHeaderPrefix {
            version: SHADOWKEEP_HEADER_VERSION,
            package_id,
            build_signature: 0x1234_5678_9ABC_DEF0,
            patch_id,
        }
        .encode();
        let mut bytes = [0u8; ENTRY_COUNT_FIELD_END];
        bytes[..prefix.len()].copy_from_slice(&prefix);
        bytes[2..4].copy_from_slice(&W64_PLATFORM.to_le_bytes());
        bytes[ENTRY_COUNT_OFFSET..ENTRY_COUNT_FIELD_END]
            .copy_from_slice(&entry_count.to_le_bytes());
        fs::write(path, bytes).expect("test package header should be created");
    }

    #[test]
    fn discovers_the_highest_patch_and_preserves_identity() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_globals_058f_0.pkg"),
            0x058f,
            0,
            100,
        );
        package(
            &directory.path().join("w64_globals_058f_2.pkg"),
            0x058f,
            2,
            120,
        );
        package(
            &directory.path().join("w64_other_0600_9.pkg"),
            0x0600,
            9,
            50,
        );

        let chain = discover_patch_chain(directory.path(), 0x058f)
            .expect("matching patch chain should be found");

        assert_eq!(chain.identity.stem, "w64_globals_058f");
        assert_eq!(
            chain
                .files
                .iter()
                .map(|file| file.patch)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(chain.next_patch().expect("next patch should fit"), 3);
        assert_eq!(
            chain.output_file_name().expect("output name should build"),
            "w64_globals_058f_3.pkg"
        );
        assert_eq!(chain.historical_entry_count_high_water(), 120);
    }

    #[test]
    fn preserves_the_entry_count_high_water_after_a_later_generation_shrinks() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_sandbox_01bb_1.pkg"),
            0x01BB,
            1,
            6_469,
        );
        package(
            &directory.path().join("w64_sandbox_01bb_6.pkg"),
            0x01BB,
            6,
            6_468,
        );

        let chain = discover_patch_chain(directory.path(), 0x01BB)
            .expect("the shrinking chain should be discoverable");

        assert_eq!(chain.latest().entry_count, 6_468);
        assert_eq!(chain.historical_entry_count_high_water(), 6_469);
    }

    #[test]
    fn rejects_two_identities_for_one_package_id() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_globals_058f_0.pkg"),
            0x058f,
            0,
            100,
        );
        package(
            &directory.path().join("w64_other_058f_1.pkg"),
            0x058f,
            1,
            100,
        );

        let error = discover_patch_chain(directory.path(), 0x058f)
            .expect_err("conflicting identities must fail");

        assert!(
            error
                .to_string()
                .contains("more than one filename identity")
        );
    }

    #[test]
    fn refuses_to_advance_past_patch_255() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_globals_058f_255.pkg"),
            0x058f,
            255,
            100,
        );
        let chain = discover_patch_chain(directory.path(), 0x058f)
            .expect("matching patch chain should be found");

        assert!(chain.next_patch().is_err());
    }

    #[test]
    fn rejects_filename_and_header_package_id_mismatches() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_globals_058f_0.pkg"),
            0x0600,
            0,
            100,
        );

        let error = discover_patch_chain(directory.path(), 0x058f)
            .expect_err("filename/header package id mismatch must fail");

        assert!(error.to_string().contains("claims id 058f"));
        assert!(error.to_string().contains("declares 0600"));
    }

    #[test]
    fn rejects_filename_and_header_patch_mismatches() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_globals_058f_1.pkg"),
            0x058f,
            2,
            100,
        );

        let error = discover_patch_chain(directory.path(), 0x058f)
            .expect_err("filename/header patch mismatch must fail");

        assert!(error.to_string().contains("claims patch 1"));
        assert!(error.to_string().contains("declares 2"));
    }

    #[test]
    fn supports_non_hex_filename_ids_using_the_header_identity() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        package(
            &directory.path().join("w64_client_bootstrap_unp1_0.pkg"),
            0x0109,
            0,
            100,
        );

        let chain = discover_patch_chain(directory.path(), 0x0109)
            .expect("header identity should discover a non-hex filename id");

        assert_eq!(chain.identity.package_id, 0x0109);
        assert_eq!(chain.identity.stem, "w64_client_bootstrap_unp1");
        assert_eq!(
            chain.output_file_name().expect("output name should build"),
            "w64_client_bootstrap_unp1_1.pkg"
        );
    }
}
