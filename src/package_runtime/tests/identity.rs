use super::*;
use crate::package_authoring::validate_package_authoring_runtime as validate_for_parhelion;

#[test]
fn runtime_version_uses_the_declared_runtime_name() {
    let sunrise = fixture::module("Sunrise", false, None);
    assert_eq!(runtime_version(&sunrise), Some(("Sunrise", "0.3.2".into())));
    let dawn = fixture::module("Dawn", true, None);
    assert_eq!(runtime_version(&dawn), Some(("Dawn", "0.1".into())));
    for brand in ["Dawn", " dawn ", "DAWN"] {
        let named = fixture::module(brand, false, None);
        assert_eq!(runtime_version(&named), Some(("Dawn", "0.1".into())));
    }
    let sunrise_with_dawn_markers = fixture::module("Sunrise", true, None);
    assert_eq!(
        runtime_version(&sunrise_with_dawn_markers),
        Some(("Sunrise", "0.3.2".into()))
    );
    assert!(runtime_version(&fixture::module("Steam Client API", true, None)).is_none());
    assert!(runtime_version(&fixture::module("Steam Client API", false, None)).is_none());
    assert!(runtime_version(b"Dawn coo_executor").is_none());
}

#[test]
fn runtime_inspection_and_parhelion_agree_at_both_dll_locations_and_require_capabilities() {
    for location in installation::RuntimeLocation::ALL {
        let directory = fixture::install();
        let module_path = location.directory(directory.path()).join("steam_api64.dll");
        fs::create_dir_all(module_path.parent().unwrap()).unwrap();
        for (brand, markers) in [
            ("Sunrise", false),
            ("Sunrise", true),
            ("Dawn", false),
            ("Dawn", true),
        ] {
            fs::write(&module_path, fixture::module(brand, markers, None)).unwrap();
            let inspection = installation::RuntimeInspection::inspect(directory.path());
            let copy = inspection.launch_copy().unwrap();
            let dawn = brand == "Dawn";
            assert_eq!(copy.dawn, dawn);
            assert_eq!(copy.name(), if dawn { "Dawn" } else { "Sunrise" });
            assert_eq!(
                copy.version.as_deref(),
                Some(if brand == "Dawn" { "0.1" } else { "0.3.2" })
            );
            assert_eq!(copy.bundled_schema, Some(6));
            validate_for_parhelion(&directory.path().join("packages")).unwrap();
        }
        // The manifest-cache magic is the third capability and belongs to the runtime, so it is
        // checked here alongside the two both runtimes share.
        for (missing, label) in [
            "generated package-header trust",
            "generated manifest routing",
            "generated content manifest",
        ]
        .into_iter()
        .enumerate()
        {
            fs::write(&module_path, fixture::module("Dawn", false, Some(missing))).unwrap();
            let error = validate_for_parhelion(&directory.path().join("packages")).unwrap_err();
            assert!(error.starts_with("Dawn 0.1"), "{error}");
            assert!(error.contains(label), "{error}");
        }
        fs::write(
            &module_path,
            fixture::module("Steam Client API", false, None),
        )
        .unwrap();
        assert!(validate_for_parhelion(&directory.path().join("packages")).is_err());
    }
}

#[test]
fn dawn_in_bin_does_not_hide_an_unsupported_root_dll() {
    let directory = fixture::install();
    let root = directory.path().join("steam_api64.dll");
    fs::write(&root, fixture::module("Steam Client API", false, None)).unwrap();
    fs::write(
        directory.path().join("bin/x64/steam_api64.dll"),
        fixture::module("Dawn", true, None),
    )
    .unwrap();
    let error = validate_for_parhelion(&directory.path().join("packages")).unwrap_err();
    assert!(error.contains("no recognized Sunrise or Dawn"), "{error}");
    // The reported path is resolved, so compare against the canonical form: a temporary
    // directory can sit under a short 8.3 ancestor that canonicalization expands.
    let reported = fs::canonicalize(&root).unwrap();
    assert!(error.contains(&reported.display().to_string()), "{error}");
}

/// Each runtime stamps its generated manifest cache with its own eight ASCII bytes: Sunrise writes
/// SUNCMANF, Dawn writes DAWNMANF. Looking for Sunrise's magic in a Dawn DLL reported Dawn as
/// missing content-manifest support and refused every Parhelion install on it.
#[test]
fn each_runtime_advertises_its_own_manifest_cache_magic() {
    let holds = |bytes: &[u8], marker: &[u8]| bytes.windows(marker.len()).any(|w| w == marker);
    for (brand, own, foreign) in [
        ("Dawn", b"DAWNMANF".as_slice(), b"SUNCMANF".as_slice()),
        ("Sunrise", b"SUNCMANF".as_slice(), b"DAWNMANF".as_slice()),
    ] {
        let module = fixture::module(brand, false, None);
        assert!(holds(&module, own), "{brand} must advertise its own magic");
        assert!(
            !holds(&module, foreign),
            "{brand} must not advertise the other runtime's magic"
        );
    }
}

#[test]
fn runtime_snapshot_fails_closed_and_detects_identity_changes() {
    let directory = fixture::install();
    let bin = directory.path().join("bin/x64/steam_api64.dll");
    assert!(installed_runtime(directory.path()).is_err());
    fs::write(&bin, b"unrecognized runtime").unwrap();
    assert!(installed_runtime(directory.path()).is_err());

    fs::write(&bin, fixture::module("Dawn", true, None)).unwrap();
    let snapshot = installed_runtime(directory.path()).unwrap();
    assert_eq!(snapshot.brand(), RuntimeBrand::Dawn);
    assert_eq!(snapshot.module_path(), fs::canonicalize(&bin).unwrap());
    verify_installed_runtime(directory.path(), &snapshot).unwrap();

    let root = directory.path().join("steam_api64.dll");
    fs::write(&root, fixture::module("Sunrise", true, None)).unwrap();
    let error = verify_installed_runtime(directory.path(), &snapshot).unwrap_err();
    assert!(error.contains("active runtime DLL changed"), "{error}");

    fs::remove_file(root).unwrap();
    fs::write(&bin, fixture::module("Sunrise", true, None)).unwrap();
    let error = verify_installed_runtime(directory.path(), &snapshot).unwrap_err();
    assert!(error.contains("runtime DLL changed"), "{error}");
}
