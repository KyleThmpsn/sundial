use super::*;
use crate::format::build_test_package_with_physical_payload;
use serde_json::json;
use tempfile::TempDir;

mod account_replacement;
mod caches;
mod commit;
mod native_lifecycle;
mod progress;
mod recovery;
mod runtime;
mod spill;
mod transaction_safety;
mod uninstall;
mod validation;

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

fn assert_account_uninstall_outcome(
    plan: &UninstallPlan,
    backups: &Path,
    settings: &Path,
    original: &[u8],
    fail_after: Option<usize>,
) {
    let result = crate::install::uninstall::uninstall_inner(
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
