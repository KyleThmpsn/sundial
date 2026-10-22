use super::*;

fn package(path: &Path, package_id: u16, patch: u16, signature: u64) {
    let bytes = PackageHeaderPrefix {
        version: SHADOWKEEP_HEADER_VERSION,
        package_id,
        build_signature: signature,
        patch_id: patch,
    }
    .encode();
    fs::write(path, bytes).expect("test package header should be written");
}

fn stock_source(root: &Path) -> PathBuf {
    let packages = root.join("packages");
    fs::create_dir(&packages).expect("package directory should be created");
    for profile in CANONICAL_PACKAGES {
        package(
            &packages.join(profile.stock_file_name(profile.stock_patch_id)),
            profile.package_id,
            profile.stock_patch_id,
            0xF6F1_7D76_7F79_5A39,
        );
    }
    packages
}

fn snapshot(package_directory: PathBuf, staging_root: PathBuf) -> BatchBuildSnapshot {
    snapshot_with_recipe(package_directory, staging_root, WeaponRecipe::every_end())
}

fn snapshot_with_recipe(
    package_directory: PathBuf,
    staging_root: PathBuf,
    recipe: WeaponRecipe,
) -> BatchBuildSnapshot {
    BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory,
        staging_root,
        ignore_installed_authored_overlays: true,
        recipes: vec![recipe],
        maps: Vec::new(),
    })
    .expect("single-recipe test snapshot should be valid")
}

fn configured_real_packages() -> Option<PathBuf> {
    std::env::var_os("SUNDIAL_STOCK_PACKAGES")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

#[test]
fn source_inspection_recognizes_a_complete_installed_authored_set() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    for profile in CANONICAL_PACKAGES {
        package(
            &packages.join(profile.authored_file_name),
            profile.package_id,
            profile.authored_patch_id,
            SUNDIAL_BUILD_SIGNATURE,
        );
    }
    let asset = authored_package(PARHELION_ASSET_PACKAGE_ID)
        .expect("the canonical asset package profile must exist");
    package(
        &packages.join(asset.file_name),
        asset.package_id,
        asset.patch_id,
        SUNDIAL_BUILD_SIGNATURE,
    );
    let request = snapshot(packages, directory.path().join("staging"));

    let report = inspect_snapshot(&request).expect("complete authored set should be ignored");

    let mut expected = CANONICAL_ARTIFACT_FILE_NAMES.map(str::to_owned);
    expected.sort();
    assert_eq!(report.ignored_authored_files, expected);
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and PARHELION_TEST_STAGED_RUN"]
fn configured_staged_run_reopens_with_complete_stock_shaped_icon_graphs() {
    use sundial::package_authoring::PackageManager;
    use tiger_pkg::{DestinyVersion, GameVersion, TagHash};

    let packages = configured_real_packages()
        .expect("SUNDIAL_STOCK_PACKAGES must point to Shadowkeep packages");
    let staged_run = std::env::var_os("PARHELION_TEST_STAGED_RUN")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_STAGED_RUN must point to a staged run");
    let manifest_text = fs::read_to_string(staged_run.join(MANIFEST_FILE_NAME))
        .expect("staged manifest should read");
    let document: crate::manifest::ManifestDocument =
        serde_json::from_str(&manifest_text).expect("staged manifest should decode");
    document
        .validate()
        .expect("staged manifest should validate");
    let artifacts = authored_packages_for_file_names(
        document
            .artifacts
            .iter()
            .map(|artifact| artifact.file_name.as_str()),
    )
    .expect("staged manifest should select a complete recognized package set");
    let ignored = CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let source = PackageSource::prepare(&packages, &ignored)
        .expect("temporary stock package view should be created");
    for artifact in artifacts {
        let name = artifact.file_name;
        let staged = staged_run.join(name);
        assert!(staged.is_file(), "{} is missing", staged.display());
        fs::hard_link(&staged, source.path().join(name))
            .expect("staged package should hard-link into the isolated package view");
    }

    let manager = PackageManager::new(
        source.path(),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("isolated authored package view should open");
    let sunrise = &document.project.sunrise;
    let mut definitions = sunrise
        .watermarked_icon_containers
        .iter()
        .map(|tag| TagHash(tag.get()))
        .collect::<BTreeSet<_>>();
    definitions.insert(TagHash(sunrise.badge_icon_tag.get()));
    assert!(
        definitions.len() >= 2,
        "manifest contains no authored icon graph"
    );

    for definition in definitions {
        crate::watermark::validate_icon_definition_graph(&manager, definition)
            .unwrap_or_else(|error| panic!("authored icon {definition} is invalid: {error}"));
    }
}

#[test]
fn source_inspection_ignores_a_recognized_partial_authored_set() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let profile = CANONICAL_PACKAGES[0];
    package(
        &packages.join(profile.authored_file_name),
        profile.package_id,
        profile.authored_patch_id,
        SUNDIAL_BUILD_SIGNATURE,
    );
    let request = snapshot(packages, directory.path().join("staging"));

    let report = inspect_snapshot(&request)
        .expect("a recognized partial prior generation should be ignored");

    assert_eq!(
        report.ignored_authored_files,
        vec![profile.authored_file_name.to_owned()]
    );
}

#[test]
fn source_inspection_ignores_owned_spill_but_rejects_foreign_occupancy() {
    let directory = tempfile::tempdir().unwrap();
    let packages = stock_source(directory.path());
    let spill = authored_package(PARHELION_ASSET_PACKAGE_ID + 1).unwrap();
    package(
        &packages.join(spill.file_name),
        spill.package_id,
        0,
        SUNDIAL_BUILD_SIGNATURE,
    );
    let request = snapshot(packages.clone(), directory.path().join("staging"));
    let report = inspect_snapshot(&request).unwrap();
    assert_eq!(
        report.ignored_authored_files,
        vec![spill.file_name.to_owned()]
    );
    package(
        &packages.join(spill.file_name),
        spill.package_id,
        0,
        0xAABB_CCDD_EEFF_0011,
    );
    assert!(inspect_snapshot(&request).is_err());
}

#[test]
fn preflight_rejects_direct_and_normalized_staging_paths_inside_packages() {
    let directory = tempfile::tempdir().unwrap();
    let packages = stock_source(directory.path());
    for staging in [
        packages.join("output"),
        packages
            .join("not-created")
            .join("..")
            .join("normalized-output"),
    ] {
        let request = snapshot(packages.clone(), staging);
        let error = preflight_snapshot(&request).unwrap_err();
        assert!(error.contains("outside the live packages directory"));
    }
}

#[cfg(windows)]
#[test]
fn preflight_rejects_a_case_aliased_windows_staging_path_inside_packages() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let case_alias = PathBuf::from(packages.to_string_lossy().to_uppercase()).join("output");
    let request = snapshot(packages, case_alias);

    let error =
        preflight_snapshot(&request).expect_err("case-aliased live package output must fail");

    assert!(error.contains("outside the live packages directory"));
}

#[test]
fn source_fingerprint_covers_sparse_stock_generations_and_detects_changes() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let sparse = packages.join(CANONICAL_PACKAGES[0].stock_file_name(1));
    package(
        &sparse,
        CANONICAL_PACKAGES[0].package_id,
        1,
        0xF6F1_7D76_7F79_5A39,
    );

    let before = source_artifact_reports(&packages).unwrap();
    assert_eq!(before.len(), CANONICAL_PACKAGE_IDS.len() + 1);
    assert!(
        before
            .iter()
            .any(|artifact| artifact.file_name.ends_with("_1.pkg"))
    );

    let mut changed = fs::read(&sparse).unwrap();
    changed.extend_from_slice(b"changed after compilation started");
    fs::write(&sparse, changed).unwrap();
    let after = source_artifact_reports(&packages).unwrap();
    assert_ne!(before, after);
    assert!(
        validate_source_artifacts_unchanged(&before, &after)
            .unwrap_err()
            .contains("changed while")
    );
}
