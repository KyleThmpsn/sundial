//! A non-executable PE fixture with version/default resources and explicit capability strings.
use super::*;

pub(crate) fn install() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("bin/x64")).unwrap();
    fs::create_dir(directory.path().join("packages")).unwrap();
    fs::write(directory.path().join("destiny2.exe"), []).unwrap();
    fs::write(directory.path().join("bin/x64/oo2core_3_win64.dll"), []).unwrap();
    directory
}

pub(crate) fn module(brand: &str, dawn: bool, missing: Option<usize>) -> Vec<u8> {
    let defaults = if dawn {
        br#"{"version":6,"experiments":{"omega":{"coo_executor":false}}}"#.as_slice()
    } else {
        br#"{"version":6}"#.as_slice()
    };
    build(brand, dawn, missing, defaults)
}

pub(crate) fn module_with_schema(brand: &str, schema: u64) -> Vec<u8> {
    build(
        brand,
        false,
        None,
        format!("{{\"version\":{schema}}}").as_bytes(),
    )
}

/// `brand`'s image with Sundial's required capability strings, leaving out the one at `missing`.
/// A Dawn module also advertises the composition executor. Dawn identifies as 0.1.0.0 and
/// Sunrise as 0.3.2.0.
fn build(brand: &str, dawn: bool, missing: Option<usize>, defaults: &[u8]) -> Vec<u8> {
    let version = if brand.trim().eq_ignore_ascii_case("Dawn") {
        [0, 1, 0, 0]
    } else {
        [0, 3, 2, 0]
    };
    let mut strings: Vec<&[u8]> = Vec::new();
    if dawn {
        strings.extend([
            b"ev=coo_script mission=omega result=loaded format=lua".as_slice(),
            b"ev=coo_executor mission=omega mode=composition",
            b"Sunrise/scripts/omega.lua",
            b"coo_executor",
        ]);
    }
    // The manifest-cache magic belongs to whichever runtime this fixture stands for, so a Dawn
    // module advertises DAWNMANF and a Sunrise one SUNCMANF.
    let markers = PACKAGE_AUTHORING_RUNTIME_MARKERS
        .iter()
        .map(|(marker, _)| *marker)
        .chain([super::super::manifest_cache_marker(brand)]);
    strings.extend(
        markers
            .enumerate()
            .filter(|(index, _)| Some(*index) != missing)
            .map(|(_, marker)| marker),
    );
    crate::test_support::runtime::image(brand, version, defaults, &strings)
}
