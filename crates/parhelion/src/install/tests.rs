use super::*;
use crate::format::build_test_package_with_physical_payload;
use serde_json::json;
use tempfile::TempDir;

mod account_replacement;
mod native_lifecycle;
mod progress;
mod spill;
mod transaction_safety;

fn test_manifest_sunrise_json() -> serde_json::Value {
    json!({
        "badge_node_hashes": [
            "0x53554E42",
            "0x53554E54",
            "0x53554E48",
            "0x53554E57"
        ],
        "badge_name_hash": "0x858256D5",
        "badge_description_hash": "0x8F21C532",
        "badge_icon_tag": "0x81D40000",
        "watermark_layer_tag": "0x81D40001",
        "watermarked_icon_containers": []
    })
}

fn test_manifest_project_with_unlocks(rows: &[(u16, u8, u16)]) -> ManifestProject {
    let weapons = rows
        .iter()
        .enumerate()
        .map(|(index, &(definition_index, bank, slot))| {
            let seed = u32::try_from(index).unwrap() * 0x10;
            json!({
                "namespace": format!("parhelion.test-{index}"),
                "name": format!("Test {index}"),
                "item": {
                    "hash": format!("0x{:08X}", 0x7000_0001 + seed),
                    "index": 100 + index,
                    "definition_tag": format!("0x{:08X}", 0x8080_2000 + seed),
                    "string_tag": format!("0x{:08X}", 0x8080_2001 + seed)
                },
                "collectible": {
                    "hash": format!("0x{:08X}", 0x7000_0002 + seed),
                    "index": 200 + index
                },
                "unlock": {
                    "hash": format!("0x{:08X}", 0x7000_0003 + seed),
                    "definition_index": definition_index,
                    "bank": bank,
                    "slot": slot
                },
                "donor": {
                    "item_hash": format!("0x{:08X}", 0x7100_0001 + seed),
                    "definition_tag": format!("0x{:08X}", 0x8080_3000 + seed),
                    "string_tag": format!("0x{:08X}", 0x8080_3001 + seed)
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "weapons": weapons,
        "sunrise": test_manifest_sunrise_json()
    }))
    .unwrap()
}

#[test]
#[ignore = "requires PARHELION_TEST_STAGED_RUN, PARHELION_TEST_TARGET_PACKAGES, and PARHELION_TEST_BACKUP_ROOT"]
fn configured_staged_run_installs_through_the_transaction() {
    let staged_run_directory = std::env::var_os("PARHELION_TEST_STAGED_RUN")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_STAGED_RUN must point to a staged run");
    let target_packages_directory = std::env::var_os("PARHELION_TEST_TARGET_PACKAGES")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_TARGET_PACKAGES must point to Shadowkeep packages");
    let backup_root = std::env::var_os("PARHELION_TEST_BACKUP_ROOT")
        .map(PathBuf::from)
        .expect("PARHELION_TEST_BACKUP_ROOT must be configured");
    let manifest: ManifestDocument = sundial::package_authoring::read_json(
        File::open(staged_run_directory.join(MANIFEST_FILE_NAME))
            .expect("configured staged manifest should open"),
    )
    .expect("configured staged manifest should decode");
    let report = install_staged_packages(&InstallRequest::new(
        staged_run_directory,
        target_packages_directory.clone(),
        backup_root,
    ))
    .unwrap_or_else(|error| panic!("configured install should succeed: {error}"));

    assert_eq!(report.artifacts.len(), manifest.artifacts.len());
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(&target_packages_directory)
            .expect("installed authored package set should reopen");
    let globals =
        sundial::package_authoring::resolve_live_named_tag(&manager, "investment_globals", None)
            .expect("installed package set should expose one live investment-globals tag");
    assert!(
        !manager
            .read_tag(globals)
            .expect("installed investment globals should read")
            .is_empty()
    );
    for weapon in &manifest.project.weapons {
        for (description, tag) in [
            ("definition", weapon.item.definition_tag),
            ("strings", weapon.item.string_tag),
        ] {
            let tag = tiger_pkg::TagHash(tag.get());
            assert!(
                manager.get_entry(tag).is_some(),
                "installed {} {description} entry {tag} is missing",
                weapon.namespace
            );
            assert!(
                !manager
                    .read_tag(tag)
                    .unwrap_or_else(|error| panic!(
                        "installed {} {description} tag {tag} should read: {error}",
                        weapon.namespace
                    ))
                    .is_empty(),
                "installed {} {description} tag {tag} is empty",
                weapon.namespace
            );
        }
    }
    for icon in manifest
        .project
        .sunrise
        .watermarked_icon_containers
        .iter()
        .copied()
        .chain([manifest.project.sunrise.badge_icon_tag])
    {
        crate::watermark::validate_icon_definition_graph(&manager, tiger_pkg::TagHash(icon.get()))
            .unwrap_or_else(|error| {
                panic!(
                    "installed icon definition 0x{:08X} is invalid: {error}",
                    icon.get()
                )
            });
    }
    if let Some(result) = &report.profile_sync {
        let sync = result
            .as_ref()
            .expect("installed Collections unlocks should synchronize");
        println!(
            "PARHELION_PROFILE_SYNC={} total={} newly_set={}",
            sync.settings_path.display(),
            sync.total_unlocks,
            sync.newly_set_unlocks
        );
    }
    println!("PARHELION_BACKUP={}", report.backup_directory.display());
    for artifact in report.artifacts {
        println!(
            "PARHELION_INSTALLED={} {}",
            artifact.file_name, artifact.sha256
        );
    }
}

struct Fixture {
    _temporary: TempDir,
    staging: PathBuf,
    target: PathBuf,
    backups: PathBuf,
    staged_bytes: BTreeMap<String, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let staging = temporary.path().join("staging");
        let target = temporary.path().join("packages");
        let backups = temporary.path().join("backups");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(temporary.path().join("destiny2.exe"), b"test marker").unwrap();
        let runtime = temporary.path().join("bin").join("x64");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("oo2core_3_win64.dll"), b"test marker").unwrap();
        let staged_bytes = AUTHORED_PACKAGES
            .iter()
            .map(|profile| {
                let bytes = build_test_package_with_physical_payload(
                    profile.package_id,
                    profile.patch_id,
                    profile.file_name.as_bytes(),
                )
                .expect("staged test package should build");
                fs::write(staging.join(profile.file_name), &bytes).unwrap();
                (profile.file_name.to_owned(), bytes)
            })
            .collect();
        for profile in CANONICAL_PACKAGES {
            fs::write(
                target.join(profile.stock_file_name(profile.stock_patch_id)),
                package_bytes(
                    profile.package_id,
                    profile.stock_patch_id,
                    0xAABB_CCDD_EEFF_0011,
                    b"stock",
                ),
            )
            .unwrap();
        }
        let fixture = Self {
            _temporary: temporary,
            staging,
            target,
            backups,
            staged_bytes,
        };
        fixture.write_manifest(None);
        fixture
    }

    fn request(&self) -> InstallRequest {
        let mut request = InstallRequest::new(
            self.staging.clone(),
            self.target.clone(),
            self.backups.clone(),
        );
        request.game_running_check = game_stopped;
        request.runtime_feature_check = runtime_supported;
        // These byte-level transaction fixtures have no native item tables.
        // Native lifecycle and replacement tests exercise the production review.
        request.skip_replacement_review = true;
        request
    }

    fn sunrise_cache_path(&self) -> PathBuf {
        self._temporary
            .path()
            .join("bin")
            .join("x64")
            .join("Sunrise")
            .join("cache")
            .join("build_data.bin")
    }

    fn write_sunrise_cache(&self, bytes: &[u8]) -> PathBuf {
        let path = self.sunrise_cache_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn root_sunrise_cache_path(&self) -> PathBuf {
        self._temporary
            .path()
            .join("Sunrise")
            .join("cache")
            .join("build_data.bin")
    }

    fn write_root_sunrise_cache(&self, bytes: &[u8]) -> PathBuf {
        let path = self.root_sunrise_cache_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn write_package_header_cache(&self, identity: &str, bytes: &[u8]) -> PathBuf {
        let path = self._temporary.path().join(format!(
            "{PACKAGE_HEADER_CACHE_PREFIX}{identity}{PACKAGE_HEADER_CACHE_SUFFIX}"
        ));
        fs::write(&path, bytes).unwrap();
        path
    }

    fn artifact_json(&self) -> Vec<serde_json::Value> {
        CANONICAL_ARTIFACT_FILE_NAMES
            .iter()
            .map(|name| {
                let digest = digest_file(&self.staging.join(name)).unwrap();
                json!({
                    "file_name": name,
                    "byte_length": digest.byte_length,
                    "sha256": digest.sha256,
                })
            })
            .collect()
    }

    fn source_artifact_json(&self) -> Vec<serde_json::Value> {
        discover_target_source_artifact_names(&self.target)
            .unwrap()
            .into_iter()
            .map(|name| {
                let digest = digest_file(&self.target.join(&name)).unwrap();
                json!({
                    "file_name": name,
                    "byte_length": digest.byte_length,
                    "sha256": digest.sha256,
                })
            })
            .collect()
    }

    fn write_manifest(&self, artifacts: Option<Vec<serde_json::Value>>) {
        let manifest = self.manifest_json(artifacts);
        fs::write(
            self.staging.join(MANIFEST_FILE_NAME),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn manifest_json(&self, artifacts: Option<Vec<serde_json::Value>>) -> serde_json::Value {
        json!({
            "schema": MANIFEST_SCHEMA,
            "source_package_directory": self.target.display().to_string(),
            "source_artifacts": self.source_artifact_json(),
            "ignored_authored_files": [],
            "selection_fingerprint": recipe_selection_fingerprint(&[]).unwrap(),
            "selected_recipe_files": [],
            "project": {
                "weapons": [],
                "sunrise": test_manifest_sunrise_json()
            },
            "artifacts": artifacts.unwrap_or_else(|| self.artifact_json()),
        })
    }

    fn write_synthetic_transaction(
        &self,
        state: InstallTransactionState,
    ) -> (InstallTransactionRecord, BTreeMap<String, Vec<u8>>) {
        fs::create_dir_all(&self.backups).unwrap();
        let backup_directory = self.backups.join("synthetic-transaction");
        fs::create_dir(&backup_directory).unwrap();
        let mut original_bytes = BTreeMap::new();
        let artifacts = CANONICAL_ARTIFACT_FILE_NAMES
            .iter()
            .enumerate()
            .map(|(index, file_name)| {
                let profile = AUTHORED_PACKAGES
                    .iter()
                    .find(|profile| profile.file_name == *file_name)
                    .unwrap();
                let bytes = package_bytes(
                    profile.package_id,
                    profile.patch_id,
                    SUNDIAL_BUILD_SIGNATURE,
                    format!("original package {index}").as_bytes(),
                );
                let target_path = self.target.join(file_name);
                let backup_path = backup_directory.join(file_name);
                fs::write(&target_path, &bytes).unwrap();
                fs::write(&backup_path, &bytes).unwrap();
                original_bytes.insert((*file_name).to_owned(), bytes);
                let temporary_file_name = format!(".parhelion-install-synthetic-{index}.tmp");
                fs::write(
                    self.target.join(&temporary_file_name),
                    &self.staged_bytes[*file_name],
                )
                .unwrap();
                InstallTransactionArtifact {
                    remove_target: false,
                    file_name: (*file_name).to_owned(),
                    authored: TransactionDigest::from(
                        &digest_file(&self.staging.join(file_name)).unwrap(),
                    ),
                    original: Some(TransactionDigest::from(&digest_file(&backup_path).unwrap())),
                    backup_file_name: Some((*file_name).to_owned()),
                    temporary_file_name,
                }
            })
            .collect();
        let transaction = InstallTransactionRecord {
            account_cleanup: None,
            client_settings: None,
            schema: INSTALL_TRANSACTION_SCHEMA,
            state,
            target_packages_directory: fs::canonicalize(&self.target).unwrap(),
            backup_directory: fs::canonicalize(backup_directory).unwrap(),
            artifacts,
        };
        write_install_transaction(
            &self.target.join(INSTALL_TRANSACTION_FILE_NAME),
            &transaction,
        )
        .unwrap();
        (transaction, original_bytes)
    }

    fn recovery_request(&self) -> RecoveryRequest {
        RecoveryRequest {
            target_packages_directory: self.target.clone(),
            backup_root: self.backups.clone(),
            game_running_check: game_stopped,
        }
    }
}

fn game_stopped() -> Result<bool, String> {
    Ok(false)
}

fn installed_fixture() -> Fixture {
    let fixture = Fixture::new();
    for (name, bytes) in &fixture.staged_bytes {
        fs::write(fixture.target.join(name), bytes).unwrap();
    }
    fixture
}

#[test]
#[ignore = "read-only; requires PARHELION_UNINSTALL_REVIEW_PACKAGES with installed custom packages"]
fn native_uninstall_review_identifies_the_complete_installed_set_without_mutation() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_UNINSTALL_REVIEW_PACKAGES").unwrap());
    let first = preview_uninstall(&packages).unwrap();
    assert!(!first.artifacts().is_empty());
    assert_eq!(preview_uninstall(&packages).unwrap(), first);
    eprintln!(
        "Reviewed {} recognized custom packages without changing any files",
        first.artifacts().len()
    );
}

#[test]
fn uninstall_backs_up_complete_set_preserves_stock_accounts_and_invalidates_caches() {
    let fixture = installed_fixture();
    let cache = fixture.write_sunrise_cache(b"stale build cache");
    let header = fixture.write_package_header_cache("0000e2e9", b"stale header");
    let unrelated_cache =
        fixture.write_package_header_cache("uninstall-test", b"not a native cache name");
    let settings = fixture._temporary.path().join("settings.json");
    fs::write(&settings, b"keep saved account data").unwrap();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert_eq!(plan.artifacts().len(), AUTHORED_PACKAGES.len());
    let report = uninstall_custom_packages(&plan, &fixture.backups, game_stopped).unwrap();
    assert_eq!(report.removed_files.len(), AUTHORED_PACKAGES.len());
    assert!(!cache.exists());
    assert!(!header.exists());
    assert!(unrelated_cache.exists());
    assert_eq!(fs::read(&settings).unwrap(), b"keep saved account data");
    for (name, bytes) in &fixture.staged_bytes {
        assert!(!fixture.target.join(name).exists());
        assert_eq!(
            fs::read(report.backup_directory.join(name)).unwrap(),
            *bytes
        );
    }
    assert!(
        preview_uninstall(&fixture.target)
            .unwrap()
            .artifacts()
            .is_empty()
    );
    prune_package_backups(&fixture.backups, 1).unwrap();
    assert!(report.backup_directory.exists());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn uninstall_rejects_running_game_stale_reviews_unsigned_targets_and_nested_backups() {
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert!(uninstall_custom_packages(&plan, &fixture.backups, game_running).is_err());
    assert!(
        uninstall_custom_packages(&plan, &fixture.target.join("backups"), game_stopped).is_err()
    );
    let name = AUTHORED_PACKAGES[0].file_name;
    let mut changed = fixture.staged_bytes[name].clone();
    changed.push(123);
    fs::write(fixture.target.join(name), changed).unwrap();
    assert!(uninstall_custom_packages(&plan, &fixture.backups, game_stopped).is_err());
    for profile in AUTHORED_PACKAGES {
        assert!(fixture.target.join(profile.file_name).exists());
    }
    fs::write(fixture.target.join(name), b"not a recognized package").unwrap();
    assert!(preview_uninstall(&fixture.target).is_err());
}

#[test]
#[ignore = "copies real packages into a disposable directory; requires PARHELION_UNINSTALL_TEST_PACKAGES and PARHELION_UNINSTALL_TEST_ITEM_HASH"]
fn account_cleanup_and_package_removal_commit_or_rollback_together() {
    let source = PathBuf::from(std::env::var_os("PARHELION_UNINSTALL_TEST_PACKAGES").unwrap());
    let hash = u32::from_str_radix(
        &std::env::var("PARHELION_UNINSTALL_TEST_ITEM_HASH").unwrap(),
        16,
    )
    .unwrap();
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("packages");
    let backups = temporary.path().join("backups");
    fs::create_dir(&target).unwrap();
    #[cfg(windows)]
    {
        let runtime_directory = temporary.path().join("bin/x64");
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::copy(
            source.parent().unwrap().join("bin/x64/oo2core_3_win64.dll"),
            runtime_directory.join("oo2core_3_win64.dll"),
        )
        .unwrap();
    }
    // Copy, never link or mutate the live installation. Native ownership checks stay enabled.
    let names = discover_target_source_artifact_names(&source).unwrap();
    for name in &names {
        fs::copy(source.join(name), target.join(name)).unwrap();
    }
    let authored = preview_uninstall(&source).unwrap();
    assert!(!authored.artifacts().is_empty());
    let settings = temporary.path().join("settings.json");
    for version in [6, 8, 16] {
        for fail_after in [Some(1), None] {
            for artifact in authored.artifacts() {
                fs::copy(
                    source.join(&artifact.file_name),
                    target.join(&artifact.file_name),
                )
                .unwrap();
            }
            let original = serde_json::to_vec_pretty(&json!({
                "version": version, "unknown_setting": {"keep": true},
                "state": {"account": {"primary_soid": "0x0000000000000001"},
                    "characters": [{"soid": "0x0000000000000002", "class": 0,
                    "equipment": {}, "inventory": [{"instance_soid": "0x0000000000000101",
                        "definition_hash": hash, "level": 106, "quantity": 1, "plugs": null}]}]}
            }))
            .unwrap();
            fs::write(&settings, &original).unwrap();
            let plan = preview_uninstall_with_account_cleanup(&target).unwrap();
            let cleanup = plan
                .account_cleanup()
                .unwrap_or_else(|| panic!("{:?}", plan.account_cleanup_error()));
            assert_eq!(cleanup.removed_items.get(&hash), Some(&1));
            // A concurrent account edit must be rejected before any package removal.
            let external = [original.as_slice(), b" "].concat();
            fs::write(&settings, &external).unwrap();
            assert!(uninstall_custom_packages(&plan, &backups, game_stopped).is_err());
            assert_eq!(fs::read(&settings).unwrap(), external);
            assert_eq!(
                preview_uninstall(&target).unwrap(),
                plan.clone().without_account_cleanup()
            );
            fs::write(&settings, &original).unwrap();
            assert_account_uninstall_outcome(&plan, &backups, &settings, &original, fail_after);
            eprintln!(
                "v{version}: account/package {}",
                if fail_after.is_some() {
                    "rollback"
                } else {
                    "commit"
                }
            );
        }
    }
}

fn assert_account_uninstall_outcome(
    plan: &UninstallPlan,
    backups: &Path,
    settings: &Path,
    original: &[u8],
    fail_after: Option<usize>,
) {
    let result = uninstall::uninstall_inner(
        plan,
        backups,
        game_stopped,
        fail_after,
        DEFAULT_CACHE_INVALIDATION_OPS,
    );
    let backup = if fail_after.is_some() {
        let error = result.unwrap_err();
        assert!(error.to_string().contains("restored"), "{error}");
        assert_eq!(fs::read(settings).unwrap(), original);
        assert_eq!(
            preview_uninstall(plan.target()).unwrap(),
            plan.clone().without_account_cleanup()
        );
        error.backup_directory.unwrap()
    } else {
        let report = result.unwrap();
        assert_eq!(
            report.cleaned_account,
            Some(settings.canonicalize().unwrap())
        );
        assert_eq!(
            fs::read(settings).unwrap(),
            plan.account_cleanup().unwrap().cleaned_bytes
        );
        assert!(
            preview_uninstall(plan.target())
                .unwrap()
                .artifacts()
                .is_empty()
        );
        report.backup_directory
    };
    assert_eq!(
        fs::read(backup.join("account-settings.json")).unwrap(),
        original
    );
    assert!(!plan.target().join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn uninstall_failures_restore_every_removed_package() {
    for count in [1, AUTHORED_PACKAGES.len()] {
        let fixture = installed_fixture();
        let plan = preview_uninstall(&fixture.target).unwrap();
        let error = uninstall::uninstall_inner(
            &plan,
            &fixture.backups,
            game_stopped,
            Some(count),
            DEFAULT_CACHE_INVALIDATION_OPS,
        )
        .unwrap_err();
        assert!(error.to_string().contains("restored"), "{error}");
        assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
    }
}

#[test]
fn uninstall_cache_failure_restores_packages_and_keeps_a_recovery_backup() {
    let fixture = installed_fixture();
    fixture.write_sunrise_cache(b"cache");
    let plan = preview_uninstall(&fixture.target).unwrap();
    let ops = CacheInvalidationOps {
        rename: fail_cache_quarantine_rename,
        cleanup: cleanup_cache_quarantine,
    };
    let error =
        uninstall::uninstall_inner(&plan, &fixture.backups, game_stopped, None, ops).unwrap_err();
    assert!(error.backup_directory.unwrap().exists());
    assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
}

#[test]
fn interrupted_uninstall_is_recoverable_and_refuses_changed_survivors() {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    fn starts_during_rollback() -> Result<bool, String> {
        Ok(CALLS.fetch_add(1, Ordering::SeqCst) >= 2)
    }
    let fixture = installed_fixture();
    let plan = preview_uninstall(&fixture.target).unwrap();
    assert!(
        uninstall::uninstall_inner(
            &plan,
            &fixture.backups,
            starts_during_rollback,
            Some(1),
            DEFAULT_CACHE_INVALIDATION_OPS
        )
        .is_err()
    );
    assert!(fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    let survivor = AUTHORED_PACKAGES[0].file_name;
    fs::write(fixture.target.join(survivor), b"external change").unwrap();
    let request = RecoveryRequest {
        target_packages_directory: fixture.target.clone(),
        backup_root: fixture.backups.clone(),
        game_running_check: game_stopped,
    };
    assert!(recover_interrupted_install(&request).is_err());
    assert_eq!(
        fs::read(fixture.target.join(survivor)).unwrap(),
        b"external change"
    );
    fs::write(
        fixture.target.join(survivor),
        &fixture.staged_bytes[survivor],
    )
    .unwrap();
    recover_interrupted_install(&request).unwrap();
    assert_eq!(preview_uninstall(&fixture.target).unwrap(), plan);
}

#[test]
fn interrupted_uninstall_restores_account_and_packages_without_overwriting_newer_account_edits() {
    let fixture = installed_fixture();
    let (mut record, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in &mut record.artifacts {
        artifact.remove_target = true;
        let original = &fixture.staged_bytes[&artifact.file_name];
        fs::write(record.backup_directory.join(&artifact.file_name), original).unwrap();
        fs::write(fixture.target.join(&artifact.file_name), original).unwrap();
        artifact.original = Some(artifact.authored.clone());
    }
    let settings = fixture._temporary.path().join("settings.json");
    let backup = record.backup_directory.join("account-settings.json");
    fs::write(&backup, br#"{"version":8,"value":"original account"}"#).unwrap();
    fs::write(&settings, br#"{"version":8,"value":"cleaned account"}"#).unwrap();
    record.account_cleanup = Some(
        serde_json::from_value(json!({
            "relative_path": "settings.json",
            "original": TransactionDigest::from(&digest_file(&backup).unwrap()),
            "cleaned": TransactionDigest::from(&digest_file(&settings).unwrap())
        }))
        .unwrap(),
    );
    write_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME), &record)
        .unwrap();
    let removed = fixture.target.join(&record.artifacts[0].file_name);
    fs::remove_file(&removed).unwrap();
    fs::write(
        &settings,
        br#"{"version":8,"value":"external account edit"}"#,
    )
    .unwrap();
    assert!(recover_interrupted_install(&fixture.recovery_request()).is_err());
    assert!(
        !removed.exists(),
        "must reconcile the account before restoring packages"
    );
    assert_eq!(
        fs::read(&settings).unwrap(),
        br#"{"version":8,"value":"external account edit"}"#
    );
    fs::write(&settings, br#"{"version":8,"value":"cleaned account"}"#).unwrap();
    recover_interrupted_install(&fixture.recovery_request()).unwrap();
    assert_eq!(
        fs::read(&settings).unwrap(),
        br#"{"version":8,"value":"original account"}"#
    );
    for (name, bytes) in fixture.staged_bytes {
        assert_eq!(fs::read(fixture.target.join(name)).unwrap(), bytes);
    }
}

fn runtime_supported(_packages: &Path) -> Result<(), String> {
    Ok(())
}

fn game_running() -> Result<bool, String> {
    Ok(true)
}

static GAME_STARTS_AFTER_PREFLIGHT_CALLS: AtomicU64 = AtomicU64::new(0);
static GAME_STARTS_DURING_RECOVERY_CALLS: AtomicU64 = AtomicU64::new(0);

fn game_starts_after_preflight() -> Result<bool, String> {
    Ok(GAME_STARTS_AFTER_PREFLIGHT_CALLS.fetch_add(1, Ordering::SeqCst) > 0)
}

fn game_starts_during_recovery() -> Result<bool, String> {
    Ok(GAME_STARTS_DURING_RECOVERY_CALLS.fetch_add(1, Ordering::SeqCst) > 0)
}

fn game_check_failed() -> Result<bool, String> {
    Err("process scan unavailable".to_owned())
}

fn fail_cache_quarantine_rename(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "injected cache quarantine rename failure",
    ))
}

fn retain_cache_quarantine(
    quarantined_cache_path: &Path,
    _quarantine_directory: &Path,
) -> Option<PathBuf> {
    Some(quarantined_cache_path.to_path_buf())
}

fn package_bytes(package_id: u16, patch_id: u16, build_signature: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = PackageHeaderPrefix {
        version: SHADOWKEEP_HEADER_VERSION,
        package_id,
        build_signature,
        patch_id,
    }
    .encode()
    .to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn package_header_cache_names_are_narrowly_scoped() {
    assert!(is_package_header_cache_name("cache_phr_0000e2e9.dat"));
    assert!(is_package_header_cache_name("cache_phr_ABCD.dat"));
    assert!(!is_package_header_cache_name("cache_phr_.dat"));
    assert!(!is_package_header_cache_name("cache_phr_active.tmp"));
    assert!(!is_package_header_cache_name("other_0000e2e9.dat"));
}

#[test]
fn installs_exact_verified_set_and_backs_up_only_existing_targets() {
    let fixture = Fixture::new();
    let existing_names = CANONICAL_PACKAGES[..2]
        .iter()
        .map(|profile| profile.authored_file_name)
        .collect::<Vec<_>>();
    for profile in &CANONICAL_PACKAGES[..2] {
        fs::write(
            fixture.target.join(profile.authored_file_name),
            package_bytes(
                profile.package_id,
                profile.authored_patch_id,
                SUNDIAL_BUILD_SIGNATURE,
                format!("original-{}", profile.authored_file_name).as_bytes(),
            ),
        )
        .unwrap();
    }

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());
    assert!(!path_is_within(
        &report.backup_directory,
        &fs::canonicalize(&fixture.target).unwrap()
    ));
    for artifact in &report.artifacts {
        assert_eq!(
            fs::read(&artifact.target_path).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
        let replaced = existing_names.contains(&artifact.file_name.as_str());
        assert_eq!(artifact.replaced_existing_file, replaced);
        assert_eq!(artifact.backup_path.is_some(), replaced);
    }
    let backup_names = fs::read_dir(&report.backup_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        backup_names,
        existing_names
            .iter()
            .map(|name| (*name).to_owned())
            .chain(["install-complete.json".to_owned()])
            .collect()
    );
    for name in existing_names {
        assert!(
            fs::read(report.backup_directory.join(name))
                .unwrap()
                .ends_with(format!("original-{name}").as_bytes())
        );
    }
}

#[test]
fn invalidates_existing_sunrise_build_cache_after_verified_install_and_reports_backup() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale Sunrise build-data cache";
    let cache_path = fixture.write_sunrise_cache(cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report
        .invalidated_sunrise_cache
        .expect("existing cache should be invalidated");
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(invalidated.byte_length, cache_bytes.len() as u64);
    assert_eq!(invalidated.retained_quarantine_path, None);
    assert_eq!(fs::read(&invalidated.backup_path).unwrap(), cache_bytes);
    assert!(path_is_within(
        &invalidated.backup_path,
        &report.backup_directory
    ));
    assert_eq!(
        invalidated.backup_path,
        report
            .backup_directory
            .join(SUNRISE_CACHE_BACKUP_DIRECTORY)
            .join("build_data.bin")
    );
    assert_eq!(
        invalidated.sha256,
        digest_file(&invalidated.backup_path).unwrap().sha256
    );
    assert!(
        fs::read_dir(cache_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
}

#[test]
fn invalidates_client_package_header_caches_after_verified_install() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale package-header cache";
    let cache_path = fixture.write_package_header_cache("0000e2e9", cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    assert_eq!(report.invalidated_package_header_caches.len(), 1);
    let invalidated = &report.invalidated_package_header_caches[0];
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(fs::read(&invalidated.backup_path).unwrap(), cache_bytes);
    assert!(path_is_within(
        &invalidated.backup_path,
        &report.backup_directory
    ));
    assert_eq!(invalidated.retained_quarantine_path, None);
}

#[test]
fn invalidates_root_layout_sunrise_build_cache() {
    let fixture = Fixture::new();
    let cache_bytes = b"stale root-layout Sunrise cache";
    let cache_path = fixture.write_root_sunrise_cache(cache_bytes);
    let canonical_cache_path = fs::canonicalize(&cache_path).unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report
        .invalidated_sunrise_cache
        .expect("root-layout cache should be invalidated");
    assert_eq!(invalidated.cache_path, canonical_cache_path);
    assert_eq!(invalidated.retained_quarantine_path, None);
    assert_eq!(fs::read(invalidated.backup_path).unwrap(), cache_bytes);
}

#[test]
fn dual_sunrise_cache_layouts_are_rejected_before_mutation() {
    let fixture = Fixture::new();
    let root_cache = fixture.write_root_sunrise_cache(b"root cache");
    let bin_cache = fixture.write_sunrise_cache(b"bin cache");

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("Both supported"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(root_cache).unwrap(), b"root cache");
    assert_eq!(fs::read(bin_cache).unwrap(), b"bin cache");
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn missing_sunrise_build_cache_is_left_missing_and_reported_as_noop() {
    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert!(!cache_path.exists());
    assert_eq!(report.invalidated_sunrise_cache, None);
    assert!(
        !report
            .backup_directory
            .join(SUNRISE_CACHE_BACKUP_DIRECTORY)
            .exists()
    );
}

#[test]
fn non_regular_sunrise_cache_is_rejected_before_package_or_backup_mutation() {
    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(&cache_path).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
    assert!(cache_path.is_dir());
}

#[cfg(unix)]
#[test]
fn symlinked_sunrise_cache_is_rejected_before_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    let real_cache = fixture._temporary.path().join("real-build-data.bin");
    fs::write(&real_cache, b"cache").unwrap();
    symlink(&real_cache, &cache_path).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(real_cache).unwrap(), b"cache");
}

#[cfg(windows)]
#[test]
fn symlinked_sunrise_cache_is_rejected_before_mutation_when_supported() {
    use std::os::windows::fs::symlink_file;

    let fixture = Fixture::new();
    let cache_path = fixture.sunrise_cache_path();
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    let real_cache = fixture._temporary.path().join("real-build-data.bin");
    fs::write(&real_cache, b"cache").unwrap();
    if symlink_file(&real_cache, &cache_path).is_err() {
        return;
    }

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("must be a regular file"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(real_cache).unwrap(), b"cache");
}

#[cfg(unix)]
#[test]
fn sunrise_cache_ancestor_redirect_outside_game_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let external_sunrise = external.path().join("Sunrise");
    let external_cache = external_sunrise.join("cache").join("build_data.bin");
    fs::create_dir_all(external_cache.parent().unwrap()).unwrap();
    fs::write(&external_cache, b"external cache").unwrap();
    symlink(&external_sunrise, fixture._temporary.path().join("Sunrise")).unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("outside the validated game root"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(external_cache).unwrap(), b"external cache");
}

#[cfg(windows)]
#[test]
fn sunrise_cache_ancestor_redirect_outside_game_root_is_rejected_when_supported() {
    use std::os::windows::fs::symlink_dir;

    let fixture = Fixture::new();
    let external = tempfile::tempdir().unwrap();
    let external_sunrise = external.path().join("Sunrise");
    let external_cache = external_sunrise.join("cache").join("build_data.bin");
    fs::create_dir_all(external_cache.parent().unwrap()).unwrap();
    fs::write(&external_cache, b"external cache").unwrap();
    if symlink_dir(&external_sunrise, fixture._temporary.path().join("Sunrise")).is_err() {
        return;
    }

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("outside the validated game root"));
    assert!(!fixture.backups.exists());
    assert_eq!(fs::read(external_cache).unwrap(), b"external cache");
}

#[test]
fn hash_mismatch_is_rejected_before_any_target_or_backup_mutation() {
    let fixture = Fixture::new();
    let target_path = fixture.target.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    let original = package_bytes(
        CANONICAL_PACKAGE_IDS[0],
        AUTHORED_PATCH_ID,
        SUNDIAL_BUILD_SIGNATURE,
        b"original",
    );
    fs::write(&target_path, &original).unwrap();
    fs::write(
        fixture.staging.join(CANONICAL_ARTIFACT_FILE_NAMES[0]),
        b"tampered-after-manifest",
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("does not match its manifest"));
    assert_eq!(fs::read(target_path).unwrap(), original);
    assert!(!fixture.backups.exists());
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        name == ".parhelion-transaction.lock" || !name.starts_with(".parhelion-")
    }));
}

#[test]
fn forged_manifest_digest_cannot_hide_tampered_block_bytes() {
    let fixture = Fixture::new();
    let profile = AUTHORED_PACKAGES[0];
    let artifact_path = fixture.staging.join(profile.file_name);
    let mut bytes = fs::read(&artifact_path).unwrap();
    let payload = profile.file_name.as_bytes();
    let payload_offset = bytes
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("fixture payload should be stored in its physical block");
    bytes[payload_offset] ^= 1;
    fs::write(&artifact_path, bytes).unwrap();
    fixture.write_manifest(None);

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("failed structural validation"));
    assert!(error.message.contains("failed SHA-1"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn forged_manifest_digest_cannot_hide_a_truncated_package() {
    let fixture = Fixture::new();
    let profile = AUTHORED_PACKAGES[0];
    let artifact_path = fixture.staging.join(profile.file_name);
    let mut bytes = fs::read(&artifact_path).unwrap();
    bytes.pop();
    fs::write(&artifact_path, bytes).unwrap();
    fixture.write_manifest(None);

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("failed structural validation"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn changed_stock_input_is_rejected_before_any_target_or_backup_mutation() {
    let fixture = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let source_name = profile.stock_file_name(profile.stock_patch_id);
    fs::write(
        fixture.target.join(&source_name),
        package_bytes(
            profile.package_id,
            profile.stock_patch_id,
            0xAABB_CCDD_EEFF_0011,
            b"stock changed after staging",
        ),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("changed since this staged build"));
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn sparse_affected_stock_chains_are_fingerprinted_and_set_changes_are_rejected() {
    let fixture = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let sparse_name = profile.stock_file_name(1);
    fs::write(
        fixture.target.join(&sparse_name),
        package_bytes(
            profile.package_id,
            1,
            0xAABB_CCDD_EEFF_0011,
            b"sparse stock generation",
        ),
    )
    .unwrap();
    fixture.write_manifest(None);

    let report = install_staged_packages(&fixture.request()).unwrap();
    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());

    let changed_set = Fixture::new();
    fs::write(
        changed_set.target.join(&sparse_name),
        package_bytes(
            profile.package_id,
            1,
            0xAABB_CCDD_EEFF_0011,
            b"added after staging",
        ),
    )
    .unwrap();
    let error = install_staged_packages(&changed_set.request()).unwrap_err();
    assert!(error.message.contains("package-chain set changed"));
    assert!(!changed_set.backups.exists());
}

#[test]
fn target_temp_allocation_failure_cleans_every_earlier_prepared_file() {
    let fixture = Fixture::new();
    let validated = validate_request(&fixture.request()).unwrap();

    let error = prepare_temporary_files_inner(&validated, Some(2)).unwrap_err();

    assert!(error.contains("allocation failure"));
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".parhelion-install-")
    }));
}

#[test]
fn injected_mid_commit_failure_restores_old_files_and_removes_new_files() {
    let fixture = Fixture::new();
    let cache_path = fixture.write_sunrise_cache(b"cache must survive failed install");
    let first = CANONICAL_ARTIFACT_FILE_NAMES[0];
    let first_profile = AUTHORED_PACKAGES
        .iter()
        .find(|profile| profile.file_name == first)
        .unwrap();
    let first_original = package_bytes(
        first_profile.package_id,
        first_profile.patch_id,
        SUNDIAL_BUILD_SIGNATURE,
        b"first-original",
    );
    fs::write(fixture.target.join(first), &first_original).unwrap();

    let error =
        install_staged_packages_inner(&fixture.request(), Some(2), DEFAULT_CACHE_INVALIDATION_OPS)
            .unwrap_err();
    let rollback = error.rollback.unwrap();

    assert!(rollback.succeeded(), "{:?}", rollback.errors);
    assert_eq!(
        fs::read(fixture.target.join(first)).unwrap(),
        first_original
    );
    assert!(
        !fixture
            .target
            .join(CANONICAL_ARTIFACT_FILE_NAMES[1])
            .exists()
    );
    for name in &CANONICAL_ARTIFACT_FILE_NAMES[2..] {
        assert!(!fixture.target.join(name).exists());
    }
    assert_eq!(
        fs::read(cache_path).unwrap(),
        b"cache must survive failed install"
    );
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        !name.starts_with(".parhelion-install-") || name == INSTALL_TRANSACTION_FILE_NAME
    }));
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn cache_quarantine_rename_failure_rolls_packages_back_and_preserves_cache() {
    let fixture = Fixture::new();
    let cache_path = fixture.write_sunrise_cache(b"cache before failed quarantine");
    let cache_ops = CacheInvalidationOps {
        rename: fail_cache_quarantine_rename,
        cleanup: cleanup_cache_quarantine,
    };

    let error = install_staged_packages_inner(&fixture.request(), None, cache_ops).unwrap_err();

    assert!(error.message.contains("atomically quarantine"));
    assert!(error.rollback.as_ref().unwrap().succeeded());
    assert_eq!(
        fs::read(&cache_path).unwrap(),
        b"cache before failed quarantine"
    );
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
    assert!(
        fs::read_dir(cache_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
}

#[test]
fn successful_cache_rename_is_commit_boundary_and_retained_quarantine_is_reported() {
    let fixture = Fixture::new();
    let cache_bytes = b"cache retained in quarantine";
    let cache_path = fixture.write_sunrise_cache(cache_bytes);
    let cache_ops = CacheInvalidationOps {
        rename: rename_cache_into_quarantine,
        cleanup: retain_cache_quarantine,
    };

    let report = install_staged_packages_inner(&fixture.request(), None, cache_ops).unwrap();

    assert!(!cache_path.exists());
    let invalidated = report.invalidated_sunrise_cache.unwrap();
    let retained = invalidated
        .retained_quarantine_path
        .expect("injected cleanup should retain quarantine");
    assert_eq!(fs::read(&retained).unwrap(), cache_bytes);
    assert!(
        retained
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name
                .to_string_lossy()
                .starts_with(SUNRISE_CACHE_QUARANTINE_PREFIX))
    );
    assert_eq!(fs::read(invalidated.backup_path).unwrap(), cache_bytes);
    for artifact in report.artifacts {
        assert_eq!(
            fs::read(&artifact.target_path).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
    }
}

#[test]
fn manifest_rejects_duplicate_missing_extra_and_traversal_names() {
    for mutation in ["duplicate", "missing", "extra", "traversal"] {
        let fixture = Fixture::new();
        let mut artifacts = fixture.artifact_json();
        match mutation {
            "duplicate" => artifacts[1]["file_name"] = artifacts[0]["file_name"].clone(),
            "missing" => {
                artifacts.pop();
            }
            "extra" => artifacts.push(json!({
                "file_name": "extra.pkg",
                "byte_length": 1,
                "sha256": "0".repeat(64),
            })),
            "traversal" => {
                artifacts[0]["file_name"] = json!("../outside.pkg");
            }
            _ => unreachable!(),
        }
        fixture.write_manifest(Some(artifacts));

        let error = install_staged_packages(&fixture.request()).unwrap_err();

        assert!(
            error.message.contains("duplicate")
                || error.message.contains("exactly")
                || error.message.contains("Missing required authored packages")
                || error.message.contains("Unrecognized authored package")
                || error.message.contains("plain file name"),
            "unexpected error for {mutation}: {error}"
        );
        assert!(!fixture.backups.exists());
    }
}

#[test]
fn unmanifested_direct_package_is_rejected() {
    let fixture = Fixture::new();
    fs::write(fixture.staging.join("unexpected.pkg"), b"unexpected").unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error.message.contains("canonical package files")
            || error.message.contains("Unrecognized authored package")
            || error
                .message
                .contains("package files do not match the recipe-selected manifest set")
    );
    assert!(!fixture.backups.exists());
}

#[test]
fn target_inside_staging_and_backup_inside_target_are_rejected() {
    let fixture = Fixture::new();
    let nested_target = fixture.staging.join("nested-target");
    fs::create_dir(&nested_target).unwrap();
    let mut nested_target_request = InstallRequest::new(
        fixture.staging.clone(),
        nested_target,
        fixture.backups.clone(),
    );
    nested_target_request.game_running_check = game_stopped;
    nested_target_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&nested_target_request)
            .unwrap_err()
            .message
            .contains("nested")
    );

    let nested_staging = fixture.target.join("nested-staging");
    fs::create_dir(&nested_staging).unwrap();
    let mut nested_staging_request = InstallRequest::new(
        nested_staging,
        fixture.target.clone(),
        fixture.backups.clone(),
    );
    nested_staging_request.game_running_check = game_stopped;
    nested_staging_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&nested_staging_request)
            .unwrap_err()
            .message
            .contains("nested")
    );

    let backup_inside_target = fixture.target.join("backups");
    let mut invalid_backup_request = InstallRequest::new(
        fixture.staging.clone(),
        fixture.target.clone(),
        backup_inside_target,
    );
    invalid_backup_request.game_running_check = game_stopped;
    invalid_backup_request.runtime_feature_check = runtime_supported;
    assert!(
        install_staged_packages(&invalid_backup_request)
            .unwrap_err()
            .message
            .contains("outside")
    );
}

#[test]
fn game_running_checks_refuse_initial_and_precommit_races() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.game_running_check = game_running;

    assert!(
        install_staged_packages(&request)
            .unwrap_err()
            .message
            .contains("game is running")
    );

    GAME_STARTS_AFTER_PREFLIGHT_CALLS.store(0, Ordering::SeqCst);
    request.game_running_check = game_starts_after_preflight;
    let error = install_staged_packages(&request).unwrap_err();
    assert!(
        error
            .message
            .contains("started during installation preflight")
    );
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }

    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    assert!(fs::read_dir(&fixture.target).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".parhelion-install-")
    }));
    request.game_running_check = game_stopped;
    let report = install_staged_packages(&request).unwrap();
    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());

    let failed_check = Fixture::new();
    let mut request = failed_check.request();
    request.game_running_check = game_check_failed;
    let error = install_staged_packages(&request).unwrap_err();
    assert!(error.message.contains("Could not check"));
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!failed_check.target.join(name).exists());
    }
}

#[test]
fn strict_package_headers_and_stock_chain_are_checked_before_backup() {
    let wrong_staged = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let staged_path = wrong_staged.staging.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    fs::write(
        staged_path,
        package_bytes(
            profile.package_id,
            profile.authored_patch_id,
            0xDEAD_BEEF_DEAD_BEEF,
            b"foreign",
        ),
    )
    .unwrap();
    wrong_staged.write_manifest(None);
    assert!(
        install_staged_packages(&wrong_staged.request())
            .unwrap_err()
            .message
            .contains(&format!(
                "expected 38/{:04X}/patch {}",
                profile.package_id, profile.authored_patch_id
            ))
    );
    assert!(!wrong_staged.backups.exists());

    let missing_stock = Fixture::new();
    fs::remove_file(
        missing_stock
            .target
            .join(profile.stock_file_name(profile.stock_patch_id)),
    )
    .unwrap();
    assert!(
        install_staged_packages(&missing_stock.request())
            .unwrap_err()
            .message
            .contains("stock chain package")
    );
    assert!(!missing_stock.backups.exists());
}

#[test]
fn foreign_or_aliased_authored_targets_are_never_overwritten() {
    let foreign = Fixture::new();
    let profile = CANONICAL_PACKAGES[0];
    let canonical_target = foreign.target.join(CANONICAL_ARTIFACT_FILE_NAMES[0]);
    let foreign_bytes = package_bytes(
        profile.package_id,
        profile.authored_patch_id,
        0x1111_2222_3333_4444,
        b"foreign",
    );
    fs::write(&canonical_target, &foreign_bytes).unwrap();
    assert!(
        install_staged_packages(&foreign.request())
            .unwrap_err()
            .message
            .contains(&format!(
                "expected 38/{:04X}/patch {}",
                profile.package_id, profile.authored_patch_id
            ))
    );
    assert_eq!(fs::read(canonical_target).unwrap(), foreign_bytes);
    assert!(!foreign.backups.exists());

    let alias = Fixture::new();
    let alias_profile = CANONICAL_PACKAGES[1];
    let alias_path = alias.target.join(format!(
        "foreign_alias_{}.pkg",
        alias_profile.authored_patch_id
    ));
    fs::write(
        &alias_path,
        package_bytes(
            alias_profile.package_id,
            alias_profile.authored_patch_id,
            0x1111_2222_3333_4444,
            b"alias",
        ),
    )
    .unwrap();
    assert!(
        install_staged_packages(&alias.request())
            .unwrap_err()
            .message
            .contains("aliases")
    );
    assert!(alias_path.exists());
    assert!(!alias.backups.exists());

    let higher_patch = Fixture::new();
    let higher_profile = CANONICAL_PACKAGES
        .iter()
        .copied()
        .find(|profile| profile.package_id == 0x0709)
        .unwrap();
    let unsupported_patch_id = higher_profile.authored_patch_id + 1;
    let higher_path = higher_patch
        .target
        .join(higher_profile.stock_file_name(unsupported_patch_id));
    fs::write(
        &higher_path,
        package_bytes(
            higher_profile.package_id,
            unsupported_patch_id,
            0x1111_2222_3333_4444,
            b"higher",
        ),
    )
    .unwrap();
    assert!(
        install_staged_packages(&higher_patch.request())
            .unwrap_err()
            .message
            .contains(&format!("unsupported patch {unsupported_patch_id}"))
    );
    assert!(higher_path.exists());
    assert!(!higher_patch.backups.exists());
}

#[test]
fn rollback_refuses_to_overwrite_a_concurrently_changed_target() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.backups).unwrap();
    let file_name = CANONICAL_ARTIFACT_FILE_NAMES[0];
    let target_path = fixture.target.join(file_name);
    let backup_path = fixture.backups.join(file_name);
    let original_bytes = package_bytes(
        CANONICAL_PACKAGE_IDS[0],
        AUTHORED_PATCH_ID,
        SUNDIAL_BUILD_SIGNATURE,
        b"original",
    );
    fs::write(&backup_path, &original_bytes).unwrap();
    let original = OriginalArtifact {
        file_name: file_name.to_owned(),
        target_path: target_path.clone(),
        backup_path: Some(backup_path),
        digest: Some(digest_file(&fixture.backups.join(file_name)).unwrap()),
    };
    let staged_digest = digest_file(&fixture.staging.join(file_name)).unwrap();
    let prepared = PreparedArtifact {
        remove_target: false,
        manifest: ArtifactMetadata {
            file_name: file_name.to_owned(),
            byte_length: staged_digest.byte_length,
            sha256: staged_digest.sha256,
        },
        target_path: target_path.clone(),
        temporary_path: fixture.staging.join("unused.tmp"),
    };
    fs::write(&target_path, b"concurrent external change").unwrap();

    // Exercise the shared digest reconciliation with a single, already-prepared target.
    let transaction = InstallTransactionRecord {
        schema: INSTALL_TRANSACTION_SCHEMA,
        state: InstallTransactionState::Pending,
        target_packages_directory: fixture.target.clone(),
        backup_directory: fixture.backups.clone(),
        account_cleanup: None,
        client_settings: None,
        artifacts: vec![InstallTransactionArtifact {
            remove_target: false,
            file_name: original.file_name.clone(),
            authored: TransactionDigest::from(&prepared.manifest),
            original: original.digest.as_ref().map(TransactionDigest::from),
            backup_file_name: Some(original.file_name),
            temporary_file_name: ".parhelion-install-unused.tmp".to_owned(),
        }],
    };
    let rollback = rollback_transaction(&transaction, game_stopped);

    assert!(!rollback.succeeded());
    assert!(rollback.errors[0].contains("matches neither"));
    assert_eq!(
        fs::read(target_path).unwrap(),
        b"concurrent external change"
    );
}

#[test]
fn recovery_before_first_replace_marks_the_transaction_recovered() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    assert_eq!(
        outcome,
        RecoveryOutcome::Recovered {
            restored_files: Vec::new(),
            removed_new_files: Vec::new(),
        }
    );
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
        assert!(!fixture.target.join(&artifact.temporary_file_name).exists());
    }
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn recovery_mid_set_restores_every_authored_target_from_verified_backups() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in transaction.artifacts.iter().take(3) {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered { restored_files, .. } = outcome else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(restored_files.len(), 3);
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
    }
}

#[test]
fn recovery_removes_a_new_package_that_had_no_original() {
    let fixture = Fixture::new();
    let (mut transaction, _) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let artifact = transaction.artifacts.last_mut().unwrap();
    fs::remove_file(
        transaction
            .backup_directory
            .join(artifact.backup_file_name.as_ref().unwrap()),
    )
    .unwrap();
    artifact.original = None;
    artifact.backup_file_name = None;
    let new_file_name = artifact.file_name.clone();
    fs::write(
        fixture.target.join(&new_file_name),
        &fixture.staged_bytes[&new_file_name],
    )
    .unwrap();
    write_install_transaction(
        &fixture.target.join(INSTALL_TRANSACTION_FILE_NAME),
        &transaction,
    )
    .unwrap();

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered {
        removed_new_files, ..
    } = outcome
    else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(removed_new_files.len(), 1);
    assert!(!fixture.target.join(new_file_name).exists());
}

#[test]
fn recovery_after_all_replaces_but_before_commit_marker_restores_the_old_set() {
    let fixture = Fixture::new();
    let (transaction, originals) =
        fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    for artifact in &transaction.artifacts {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }

    let outcome = recover_interrupted_install(&fixture.recovery_request()).unwrap();

    let RecoveryOutcome::Recovered { restored_files, .. } = outcome else {
        panic!("pending transaction should be recovered");
    };
    assert_eq!(restored_files.len(), transaction.artifacts.len());
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            originals[&artifact.file_name]
        );
    }
}

#[test]
fn committed_transaction_is_never_rolled_back_and_only_its_temps_are_cleaned() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Committed);
    let unrelated = fixture.target.join(".parhelion-install-unrelated.tmp");
    fs::write(&unrelated, b"not owned by this transaction").unwrap();
    for artifact in &transaction.artifacts {
        fs::write(
            fixture.target.join(&artifact.file_name),
            &fixture.staged_bytes[&artifact.file_name],
        )
        .unwrap();
    }
    fs::remove_dir_all(&fixture.backups).unwrap();

    let mut recovery_request = fixture.recovery_request();
    recovery_request.game_running_check = game_running;
    let outcome = recover_interrupted_install(&recovery_request).unwrap();

    assert_eq!(outcome, RecoveryOutcome::CommittedTransaction);
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
        assert!(!fixture.target.join(&artifact.temporary_file_name).exists());
    }
    assert!(unrelated.exists());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn recovered_transaction_is_cleaned_without_querying_the_game() {
    let fixture = Fixture::new();
    fixture.write_synthetic_transaction(InstallTransactionState::Recovered);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_check_failed;

    let outcome = recover_interrupted_install(&request).unwrap();

    assert_eq!(
        outcome,
        RecoveryOutcome::Recovered {
            restored_files: Vec::new(),
            removed_new_files: Vec::new(),
        }
    );
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
}

#[test]
fn external_target_change_refuses_recovery_before_any_file_is_restored() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let authored = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&authored.file_name),
        &fixture.staged_bytes[&authored.file_name],
    )
    .unwrap();
    let externally_changed = &transaction.artifacts[1];
    fs::write(
        fixture.target.join(&externally_changed.file_name),
        b"external package replacement",
    )
    .unwrap();

    let error = recover_interrupted_install(&fixture.recovery_request()).unwrap_err();

    assert!(
        error
            .message
            .contains("neither the recorded original nor authored")
    );
    assert_eq!(
        fs::read(fixture.target.join(&authored.file_name)).unwrap(),
        fixture.staged_bytes[&authored.file_name]
    );
    assert_eq!(
        fs::read(fixture.target.join(&externally_changed.file_name)).unwrap(),
        b"external package replacement"
    );
    assert_eq!(
        read_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME))
            .unwrap()
            .state,
        InstallTransactionState::Pending
    );
}

#[test]
fn pending_recovery_is_blocked_while_the_game_is_running() {
    let fixture = Fixture::new();
    fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_running;

    let error = recover_interrupted_install(&request).unwrap_err();

    assert!(error.message.contains("game is running"));
}

#[test]
fn recovery_rechecks_the_game_after_reconciliation_before_restoring() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let authored = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&authored.file_name),
        &fixture.staged_bytes[&authored.file_name],
    )
    .unwrap();
    GAME_STARTS_DURING_RECOVERY_CALLS.store(0, Ordering::SeqCst);
    let mut request = fixture.recovery_request();
    request.game_running_check = game_starts_during_recovery;

    let error = recover_interrupted_install(&request).unwrap_err();

    assert!(error.message.contains("game is running"));
    assert_eq!(
        fs::read(fixture.target.join(&authored.file_name)).unwrap(),
        fixture.staged_bytes[&authored.file_name]
    );
    assert_eq!(
        read_install_transaction(&fixture.target.join(INSTALL_TRANSACTION_FILE_NAME))
            .unwrap()
            .state,
        InstallTransactionState::Pending
    );
}

#[test]
fn a_new_install_recovers_a_pending_transaction_before_preflight() {
    let fixture = Fixture::new();
    let (transaction, _) = fixture.write_synthetic_transaction(InstallTransactionState::Pending);
    let first = &transaction.artifacts[0];
    fs::write(
        fixture.target.join(&first.file_name),
        &fixture.staged_bytes[&first.file_name],
    )
    .unwrap();

    let report = install_staged_packages(&fixture.request()).unwrap();

    assert_eq!(report.artifacts.len(), CANONICAL_ARTIFACT_FILE_NAMES.len());
    assert!(!fixture.target.join(INSTALL_TRANSACTION_FILE_NAME).exists());
    for artifact in &transaction.artifacts {
        assert_eq!(
            fs::read(fixture.target.join(&artifact.file_name)).unwrap(),
            fixture.staged_bytes[&artifact.file_name]
        );
    }
}

#[test]
fn manifest_source_directory_must_exist_and_match_target() {
    let fixture = Fixture::new();
    let other = fixture._temporary.path().join("other-packages");
    fs::create_dir(&other).unwrap();
    let mut manifest = fixture.manifest_json(None);
    manifest["source_package_directory"] = json!(other.display().to_string());
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("not the selected target"));
    assert!(!fixture.backups.exists());
}

#[test]
fn non_current_manifest_schema_is_rejected_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["schema"] = json!(MANIFEST_SCHEMA + 1);
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error
            .message
            .contains("Unsupported Parhelion manifest schema")
    );
    assert!(!fixture.backups.exists());
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}

#[test]
fn manifest_unknown_fields_are_rejected_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["project"]["sunrise"]["unexpected"] = json!(true);
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(error.message.contains("unknown field"));
    assert!(!fixture.backups.exists());
}

#[test]
fn manifest_recipe_fingerprint_is_consumed_before_mutation() {
    let fixture = Fixture::new();
    let mut manifest = fixture.manifest_json(None);
    manifest["selection_fingerprint"] = json!("0".repeat(64));
    fs::write(
        fixture.staging.join(MANIFEST_FILE_NAME),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = install_staged_packages(&fixture.request()).unwrap_err();

    assert!(
        error
            .message
            .contains("does not match the manifest fingerprint")
    );
    assert!(!fixture.backups.exists());
}

#[test]
fn manifest_unlock_rows_must_have_unique_definitions_and_account_slots() {
    let duplicate_definition = test_manifest_project_with_unlocks(&[
        (21_613, ACCOUNT_UNLOCK_BANK, 11_923),
        (21_613, ACCOUNT_UNLOCK_BANK, 11_924),
    ]);
    assert!(
        validate_manifest_unlocks(&duplicate_definition)
            .unwrap_err()
            .message
            .contains("repeats authored unlock definition")
    );

    let duplicate_slot = test_manifest_project_with_unlocks(&[
        (21_613, ACCOUNT_UNLOCK_BANK, 11_923),
        (21_614, ACCOUNT_UNLOCK_BANK, 11_923),
    ]);
    assert!(
        validate_manifest_unlocks(&duplicate_slot)
            .unwrap_err()
            .message
            .contains("repeats authored unlock bank 1, slot 11923")
    );
}
