//! Native release acceptance on copied package inputs, never the live installation.
use super::*;

struct NativeFixture {
    _temporary: TempDir,
    request: InstallRequest,
    manifest: ManifestDocument,
    settings: PathBuf,
    account_before: Vec<u8>,
}

impl NativeFixture {
    fn copy_from_environment() -> Self {
        let source =
            PathBuf::from(std::env::var_os("PARHELION_LIFECYCLE_SOURCE_PACKAGES").unwrap());
        let stage = PathBuf::from(std::env::var_os("PARHELION_TEST_STAGED_RUN").unwrap());
        let mut manifest: ManifestDocument =
            serde_json::from_slice(&fs::read(stage.join(MANIFEST_FILE_NAME)).unwrap()).unwrap();
        manifest.validate().unwrap();
        let temporary = TempDir::new().unwrap();
        let target = temporary.path().join("packages");
        fs::create_dir(&target).unwrap();
        let install = source.parent().unwrap();
        let runtime = temporary.path().join("bin/x64");
        fs::create_dir_all(&runtime).unwrap();
        fs::copy(
            install.join("destiny2.exe"),
            temporary.path().join("destiny2.exe"),
        )
        .unwrap();
        fs::copy(
            install.join("bin/x64/oo2core_3_win64.dll"),
            runtime.join("oo2core_3_win64.dll"),
        )
        .unwrap();
        for artifact in &manifest.source_artifacts {
            fs::copy(
                source.join(&artifact.file_name),
                target.join(&artifact.file_name),
            )
            .unwrap();
        }
        // This is a pre-unlocked test account. The lifecycle must not edit it.
        let mut slots = manifest
            .project
            .weapons
            .iter()
            .map(|weapon| {
                assert_eq!(weapon.unlock.bank, 1);
                weapon.unlock.slot
            })
            .collect::<Vec<_>>();
        slots.sort_unstable();
        slots.dedup();
        let account_before = serde_json::to_vec(&json!({
            "version": 16, "release_test_sentinel": {"keep": true},
            "state": {"unlocks": {"account_flag_runs": slots.into_iter().map(|slot| [slot, 1]).collect::<Vec<_>>()}}
        })).unwrap();
        let settings = temporary.path().join("settings.json");
        fs::write(&settings, &account_before).unwrap();
        let staged_copy = temporary.path().join("staged");
        copy_stage_for_target(&stage, &staged_copy, &target, &mut manifest);
        let mut request =
            InstallRequest::new(staged_copy, target, temporary.path().join("backups"));
        // Only the disposable copy uses synthetic process/runtime availability. Native
        // ownership, file digests, transaction, backup and recovery checks remain enabled.
        request.game_running_check = game_stopped;
        request.runtime_feature_check = |_| Ok(());
        Self {
            _temporary: temporary,
            request,
            manifest,
            settings,
            account_before,
        }
    }

    fn assert_stock_and_account_unchanged(&self) {
        for artifact in &self.manifest.source_artifacts {
            let digest = digest_file(
                &self
                    .request
                    .target_packages_directory
                    .join(&artifact.file_name),
            )
            .unwrap();
            assert_eq!(
                digest.byte_length, artifact.byte_length,
                "{}",
                artifact.file_name
            );
            assert_eq!(digest.sha256, artifact.sha256, "{}", artifact.file_name);
        }
        assert_eq!(fs::read(&self.settings).unwrap(), self.account_before);
    }

    fn assert_authored_set(&self) {
        let plan = preview_uninstall(&self.request.target_packages_directory).unwrap();
        let mut actual = plan.artifacts().to_vec();
        let mut expected = self.manifest.artifacts.clone();
        actual.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        expected.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        assert_eq!(actual, expected);
    }
}

// Rebind only the fixture manifest to byte-identical copied inputs. Production
// source-directory validation remains enabled, and the original stage is untouched.
fn copy_stage_for_target(
    source: &Path,
    staged: &Path,
    target: &Path,
    manifest: &mut ManifestDocument,
) {
    verify_source_artifacts(target, &manifest.source_artifacts).unwrap();
    fs::create_dir(staged).unwrap();
    for name in manifest
        .artifacts
        .iter()
        .map(|artifact| &artifact.file_name)
        .chain(manifest.selected_recipe_files.iter())
    {
        let destination = staged.join(name);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source.join(name), destination).unwrap();
    }
    manifest.source_package_directory = fs::canonicalize(target)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fs::write(
        staged.join(MANIFEST_FILE_NAME),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "copies native files into a disposable directory; requires PARHELION_LIFECYCLE_SOURCE_PACKAGES and PARHELION_TEST_STAGED_RUN"]
fn staged_native_packages_install_repeat_recover_and_uninstall_without_changing_stock() {
    let fixture = NativeFixture::copy_from_environment();
    let failed =
        install_staged_packages_inner(&fixture.request, Some(1), DEFAULT_CACHE_INVALIDATION_OPS)
            .unwrap_err();
    assert!(
        failed
            .rollback
            .as_ref()
            .is_some_and(|rollback| rollback.succeeded()),
        "{failed}"
    );
    assert!(
        preview_uninstall(&fixture.request.target_packages_directory)
            .unwrap()
            .artifacts()
            .is_empty()
    );
    fixture.assert_stock_and_account_unchanged();

    let first = install_staged_packages(&fixture.request).unwrap();
    assert!(
        first
            .artifacts
            .iter()
            .all(|artifact| !artifact.replaced_existing_file)
    );
    assert_eq!(
        first
            .profile_sync
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .newly_set_unlocks,
        0
    );
    fixture.assert_authored_set();
    let repeated = install_staged_packages(&fixture.request).unwrap();
    assert!(
        repeated
            .artifacts
            .iter()
            .all(|artifact| artifact.replaced_existing_file && artifact.backup_path.is_some())
    );
    fixture.assert_authored_set();

    verify_interrupted_uninstall_recovery(&fixture);
    let plan = preview_uninstall(&fixture.request.target_packages_directory).unwrap();
    let report =
        uninstall_custom_packages(&plan, &fixture.request.backup_root, game_stopped).unwrap();
    assert_eq!(report.removed_files.len(), fixture.manifest.artifacts.len());
    assert!(
        preview_uninstall(&fixture.request.target_packages_directory)
            .unwrap()
            .artifacts()
            .is_empty()
    );
    for artifact in &fixture.manifest.artifacts {
        let backup = digest_file(&report.backup_directory.join(&artifact.file_name)).unwrap();
        assert_eq!(backup.sha256, artifact.sha256);
    }
    fixture.assert_stock_and_account_unchanged();
    println!(
        "Native lifecycle passed: {} authored packages; {} stock files unchanged; account unchanged",
        fixture.manifest.artifacts.len(),
        fixture.manifest.source_artifacts.len()
    );
}

fn verify_interrupted_uninstall_recovery(fixture: &NativeFixture) {
    static CHECKS: AtomicU64 = AtomicU64::new(0);
    fn starts_during_rollback() -> Result<bool, String> {
        Ok(CHECKS.fetch_add(1, Ordering::SeqCst) >= 2)
    }
    let plan = preview_uninstall(&fixture.request.target_packages_directory).unwrap();
    assert!(
        uninstall::uninstall_inner(
            &plan,
            &fixture.request.backup_root,
            starts_during_rollback,
            Some(1),
            DEFAULT_CACHE_INVALIDATION_OPS
        )
        .is_err()
    );
    assert!(
        fixture
            .request
            .target_packages_directory
            .join(INSTALL_TRANSACTION_FILE_NAME)
            .is_file()
    );
    let outcome = recover_interrupted_install(&RecoveryRequest {
        target_packages_directory: fixture.request.target_packages_directory.clone(),
        backup_root: fixture.request.backup_root.clone(),
        game_running_check: game_stopped,
    })
    .unwrap();
    assert!(matches!(outcome, RecoveryOutcome::Recovered { .. }));
    assert_eq!(
        preview_uninstall(&fixture.request.target_packages_directory).unwrap(),
        plan
    );
}
