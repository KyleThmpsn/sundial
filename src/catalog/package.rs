use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

use sha2::{Digest, Sha256};

const PACKAGE_HEADER_SIZE: usize = 0x170;
const VERSION_OFFSET: usize = 0;
const PLATFORM_OFFSET: usize = 2;
const VARIANT_OFFSET: usize = 0x1A;
const FILE_SIZE_OFFSET: usize = 0x164;
const FINGERPRINT_DOMAIN: &[u8] = b"SUNDIAL_PACKAGE_HEADERS_V3\0";

pub(crate) fn validate_install(install: &Path) -> Result<(), String> {
    let oodle_relative = Path::new("bin").join("x64").join("oo2core_3_win64.dll");
    if install.join("destiny2.exe").is_file()
        && install.join("packages").is_dir()
        && install.join(&oodle_relative).is_file()
    {
        Ok(())
    } else {
        Err(format!(
            "Not a Shadowkeep install: expected destiny2.exe, packages, and {}",
            oodle_relative.display()
        ))
    }
}

pub(super) fn install_fingerprint(install: &Path) -> Result<String, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(install.join("packages"))
        .map_err(|e| format!("Could not read packages: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Could not inspect packages: {e}"))?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pkg"))
        {
            paths.push(path);
        }
    }
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut digest = Sha256::new();
    digest.update(FINGERPRINT_DOMAIN);
    for path in paths {
        hash_package_header(&mut digest, &path)?;
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn hash_package_header(digest: &mut Sha256, path: &Path) -> Result<(), String> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Package filename is not valid Unicode: {}", path.display()))?;
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let file_size = file
        .metadata()
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
        .len();
    let mut header = [0u8; PACKAGE_HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(|error| format!("Could not read package header {}: {error}", path.display()))?;

    digest.update((filename.len() as u64).to_le_bytes());
    digest.update(filename.as_bytes());
    digest.update(file_size.to_le_bytes());
    digest.update((header.len() as u64).to_le_bytes());
    // Valid package-table rewrites update the hashed-region metadata in
    // this header, so hashing the header detects authored catalog changes
    // without streaming every payload block during startup.
    digest.update(header);

    if header_u16(&header, VERSION_OFFSET) != 38 || header_u16(&header, PLATFORM_OFFSET) != 2 {
        return Err(format!(
            "Package {} is not a version-38 w64 package",
            path.display()
        ));
    }

    if !matches!(header[VARIANT_OFFSET], 0 | 1) {
        return Err(format!(
            "Package {} has unknown header variant {}",
            path.display(),
            header[VARIANT_OFFSET]
        ));
    }

    let declared_file_size = u64::from(header_u32(&header, FILE_SIZE_OFFSET));
    if declared_file_size != file_size {
        return Err(format!(
            "Package {} declares size {declared_file_size} but is {file_size} bytes",
            path.display()
        ));
    }
    Ok(())
}

fn header_u16(header: &[u8; PACKAGE_HEADER_SIZE], offset: usize) -> u16 {
    u16::from_le_bytes(
        header[offset..offset + 2]
            .try_into()
            .expect("fixed package header field is in bounds"),
    )
}

fn header_u32(header: &[u8; PACKAGE_HEADER_SIZE], offset: usize) -> u32 {
    u32::from_le_bytes(
        header[offset..offset + 4]
            .try_into()
            .expect("fixed package header field is in bounds"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDirectory;
    use std::path::PathBuf;

    fn package_fixture(metadata_hash_byte: u8, payload_byte: u8) -> Vec<u8> {
        const FILE_SIZE: usize = 0x500;
        let mut bytes = vec![0u8; FILE_SIZE];
        bytes[VERSION_OFFSET..VERSION_OFFSET + 2].copy_from_slice(&38u16.to_le_bytes());
        bytes[PLATFORM_OFFSET..PLATFORM_OFFSET + 2].copy_from_slice(&2u16.to_le_bytes());
        bytes[VARIANT_OFFSET] = 1;
        bytes[0x118] = metadata_hash_byte;
        bytes[FILE_SIZE_OFFSET..FILE_SIZE_OFFSET + 4]
            .copy_from_slice(&(FILE_SIZE as u32).to_le_bytes());
        bytes[0x480] = payload_byte;
        bytes
    }

    fn write_fixture(root: &TestDirectory, bytes: &[u8]) -> PathBuf {
        let packages = root.0.join("packages");
        fs::create_dir_all(&packages).expect("test package directory should be created");
        let path = packages.join("w64_test_058f_0.pkg");
        fs::write(&path, bytes).expect("test package should be written");
        path
    }

    #[test]
    fn fingerprint_detects_same_size_valid_header_rewrites_without_timestamp_help() {
        let root = TestDirectory::new("package-fingerprint-header");
        let path = write_fixture(&root, &package_fixture(0x31, 0x51));
        let before = install_fingerprint(&root.0).expect("first fingerprint should succeed");
        fs::write(&path, package_fixture(0x32, 0x51))
            .expect("same-size valid-header rewrite should succeed");
        let after = install_fingerprint(&root.0).expect("second fingerprint should succeed");

        assert_ne!(before, after);
    }

    #[test]
    fn fingerprint_does_not_hash_package_payload_bodies() {
        let root = TestDirectory::new("package-fingerprint-payload");
        let path = write_fixture(&root, &package_fixture(0x31, 0x51));
        let before = install_fingerprint(&root.0).expect("first fingerprint should succeed");
        fs::write(&path, package_fixture(0x31, 0x52))
            .expect("same-size payload rewrite should succeed");
        let after = install_fingerprint(&root.0).expect("second fingerprint should succeed");

        assert_eq!(before, after);
    }

    #[test]
    fn fingerprint_rejects_a_declared_file_size_mismatch() {
        let root = TestDirectory::new("package-fingerprint-file-size");
        let mut bytes = package_fixture(0x31, 0x51);
        bytes[FILE_SIZE_OFFSET..FILE_SIZE_OFFSET + 4].copy_from_slice(&0x1000u32.to_le_bytes());
        write_fixture(&root, &bytes);

        assert!(
            install_fingerprint(&root.0)
                .expect_err("a false file size must fail")
                .contains("declares size")
        );
    }
}
