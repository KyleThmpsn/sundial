//! Transactional installation of a verified Parhelion staging run.
//!
//! Package paths are caller supplied. An injectable checker (defaulting to
//! Sundial's shared platform facade) checks the game before validation and again
//! immediately before the first package replacement.

#[cfg(test)]
mod tests;

mod recovery;
pub use recovery::recover_interrupted_install;
use recovery::*;
mod locking;
use locking::*;

mod caches;
use caches::*;

mod backups;
pub use backups::prune_package_backups;
use backups::*;

mod validation;
use validation::*;
mod account;
mod identities;
mod replacement;
pub use replacement::{ReplacementReview, preview_replacement};
#[cfg(test)]
pub(crate) use replacement::{test_review, test_review_with_sockets};
mod uninstall;
pub use uninstall::{
    UninstallPlan, UninstallReport, preview_uninstall, preview_uninstall_with_account_cleanup,
    uninstall_custom_packages,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sundial::investment::{
    AuthoredCollectionUnlock, AuthoredProfileSyncReport, synchronize_authored_collection_unlocks,
};
use sundial::package_authoring::{path_is_within, paths_equal, resolve_path_for_comparison};
use tiger_pkg::{Package, PackageD2PreBL};

use crate::SUNDIAL_BUILD_SIGNATURE;
use crate::artifact::{ArtifactMetadata, FileDigest, digest_file, has_pkg_extension};
use crate::format::PackageLayout;
use crate::manifest::{
    MANIFEST_FILE_NAME, MANIFEST_SCHEMA, ManifestDocument, ManifestProject,
    recipe_selection_fingerprint,
};
#[cfg(test)]
pub(crate) use crate::package_profile::AUTHORED_PATCH_ID;
#[cfg(test)]
pub(crate) use crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES;
use crate::package_profile::{
    ACCOUNT_UNLOCK_BANK, AUTHORED_PACKAGES, AuthoredPackage, CANONICAL_PACKAGES,
    PACKAGE_HEADER_PREFIX_SIZE, PackageHeaderPrefix, authored_package,
    authored_package_for_file_name, authored_packages_for_file_names, canonical_package,
};
pub(crate) use crate::package_profile::{CANONICAL_PACKAGE_IDS, SHADOWKEEP_HEADER_VERSION};
use crate::recipe::WeaponRecipe;

const SUNRISE_BUILD_CACHE_LAYOUTS: [&[&str]; 2] = [
    &["Sunrise", "cache", "build_data.bin"],
    &["bin", "x64", "Sunrise", "cache", "build_data.bin"],
];
const SUNRISE_CACHE_BACKUP_DIRECTORY: &str = "sunrise-cache";
const PACKAGE_HEADER_CACHE_PREFIX: &str = "cache_phr_";
const PACKAGE_HEADER_CACHE_SUFFIX: &str = ".dat";
const PACKAGE_HEADER_CACHE_BACKUP_DIRECTORY: &str = "package-header-caches";
const SUNRISE_CACHE_QUARANTINE_PREFIX: &str = ".parhelion-cache-quarantine-";
const INSTALL_TRANSACTION_FILE_NAME: &str = ".parhelion-install-transaction.json";
const INSTALL_TRANSACTION_SCHEMA: u32 = 1;
const AUTOMATIC_BACKUP_PREFIX: &str = "parhelion-backup-";
const RECIPE_BACKUP_DIRECTORY: &str = "recipes";
pub const DEFAULT_PACKAGE_BACKUP_RETENTION: usize = 3;
pub const MAX_PACKAGE_BACKUP_RETENTION: usize = 100;

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub type GameRunningCheck = fn() -> Result<bool, String>;
pub type RuntimeFeatureCheck = fn(&Path) -> Result<(), String>;

#[derive(Clone, Debug)]
pub struct RecoveryRequest {
    pub target_packages_directory: PathBuf,
    pub backup_root: PathBuf,
    pub game_running_check: GameRunningCheck,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    NoTransaction,
    CommittedTransaction,
    Recovered {
        restored_files: Vec<PathBuf>,
        removed_new_files: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug)]
pub struct InstallRequest {
    pub staged_run_directory: PathBuf,
    pub target_packages_directory: PathBuf,
    pub backup_root: PathBuf,
    /// Evaluated before validation and again immediately before the first package replacement.
    pub game_running_check: GameRunningCheck,
    /// Confirms that the selected runtime advertises the loader hooks required by authoring.
    /// Package rows, headers and cache state are validated independently by the installer.
    pub runtime_feature_check: RuntimeFeatureCheck,
    /// Number of automatic package-backup generations retained after a successful install.
    pub package_backup_retention: usize,
    /// Whether successful installs prune older automatic package-backup generations.
    pub limit_package_backups: bool,
    /// Whether the normalized recipes recorded by the staged manifest are copied into its backup.
    pub backup_recipe_snapshots: bool,
    /// Exact read-only proposal accepted by the user; stale proposals are rejected.
    pub confirmed_replacement: Option<ReplacementReview>,
    /// Synthetic package fixtures do not contain native ownership tables.
    #[cfg(test)]
    skip_replacement_review: bool,
}

impl InstallRequest {
    pub fn new(
        staged_run_directory: PathBuf,
        target_packages_directory: PathBuf,
        backup_root: PathBuf,
    ) -> Self {
        Self {
            staged_run_directory,
            target_packages_directory,
            backup_root,
            game_running_check: sundial::package_authoring::destiny_is_running,
            runtime_feature_check: sundial::package_authoring::validate_package_authoring_runtime,
            package_backup_retention: DEFAULT_PACKAGE_BACKUP_RETENTION,
            limit_package_backups: true,
            backup_recipe_snapshots: true,
            confirmed_replacement: None,
            #[cfg(test)]
            skip_replacement_review: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledArtifact {
    pub file_name: String,
    pub target_path: PathBuf,
    pub byte_length: u64,
    pub sha256: String,
    pub replaced_existing_file: bool,
    pub backup_path: Option<PathBuf>,
}

/// A stale Sunrise build-data cache removed after the package transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidatedSunriseCache {
    pub cache_path: PathBuf,
    pub backup_path: PathBuf,
    pub byte_length: u64,
    pub sha256: String,
    /// A best-effort cleanup failure left this adjacent quarantine path behind.
    pub retained_quarantine_path: Option<PathBuf>,
}

/// A stale client package-header cache removed after the package transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidatedPackageHeaderCache {
    pub cache_path: PathBuf,
    pub backup_path: PathBuf,
    pub byte_length: u64,
    pub sha256: String,
    /// A best-effort cleanup failure left this adjacent quarantine path behind.
    pub retained_quarantine_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallReport {
    pub manifest_schema: u32,
    pub staged_run_directory: PathBuf,
    pub target_packages_directory: PathBuf,
    pub backup_directory: PathBuf,
    pub artifacts: Vec<InstalledArtifact>,
    /// Obsolete optional authored overlays removed as part of this transaction.
    pub removed_obsolete_packages: Vec<PathBuf>,
    /// Normalized recipe snapshots copied beside the package backups for this generation.
    pub recipe_backup_directory: Option<PathBuf>,
    /// Older automatic backup generations removed after the transaction committed.
    pub pruned_backup_directories: Vec<PathBuf>,
    /// A non-fatal retention error. The package transaction still committed successfully.
    pub backup_prune_warning: Option<String>,
    /// Present when an existing Sunrise cache was backed up and invalidated.
    pub invalidated_sunrise_cache: Option<InvalidatedSunriseCache>,
    /// Client package-header caches backed up and invalidated to prevent stale manifest replay.
    pub invalidated_package_header_caches: Vec<InvalidatedPackageHeaderCache>,
    /// Account-state synchronization attempted after the package transaction committed.
    pub profile_sync: Option<Result<AuthoredProfileSyncReport, String>>,
    /// Account updated as part of this backed-up replacement transaction.
    pub cleaned_account: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackupPruneReport {
    pub retained_directories: Vec<PathBuf>,
    pub removed_directories: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RollbackReport {
    pub restored_files: Vec<PathBuf>,
    pub removed_new_files: Vec<PathBuf>,
    pub errors: Vec<String>,
}

impl RollbackReport {
    pub fn succeeded(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallError {
    pub message: String,
    pub backup_directory: Option<PathBuf>,
    pub rollback: Option<Box<RollbackReport>>,
}

impl InstallError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backup_directory: None,
            rollback: None,
        }
    }

    fn after_backup(message: impl Into<String>, backup_directory: &Path) -> Self {
        Self {
            message: message.into(),
            backup_directory: Some(backup_directory.to_path_buf()),
            rollback: None,
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(rollback) = &self.rollback {
            if rollback.succeeded() {
                formatter.write_str(" (the previous package set was restored)")?;
            } else {
                write!(
                    formatter,
                    " (rollback encountered {} error(s))",
                    rollback.errors.len()
                )?;
                write!(formatter, ": {}", rollback.errors.join("; "))?;
            }
        }
        if let Some(backup_directory) = &self.backup_directory {
            write!(formatter, " [backup: {}]", backup_directory.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for InstallError {}

#[derive(Debug)]
struct ValidatedRun {
    replacement_guard: Option<ReplacementReview>,
    staged_run_directory: PathBuf,
    target_packages_directory: PathBuf,
    backup_root: PathBuf,
    source_artifacts: Vec<ArtifactMetadata>,
    artifacts: Vec<ArtifactMetadata>,
    obsolete_artifacts: Vec<ArtifactMetadata>,
    selected_recipe_files: Vec<String>,
    authored_unlocks: Vec<AuthoredCollectionUnlock>,
    package_backup_retention: usize,
    limit_package_backups: bool,
    backup_recipe_snapshots: bool,
    sunrise_build_cache: ValidatedSunriseCache,
    package_header_caches: ValidatedPackageHeaderCaches,
}

#[derive(Clone, Debug)]
struct SunriseCacheCandidates {
    game_root: PathBuf,
    paths: [PathBuf; 2],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SunriseCacheFile {
    path: PathBuf,
    parent: PathBuf,
    digest: FileDigest,
}

#[derive(Debug)]
struct ValidatedSunriseCache {
    candidates: SunriseCacheCandidates,
    file: Option<SunriseCacheFile>,
}

#[derive(Debug)]
struct OriginalSunriseCache {
    candidates: SunriseCacheCandidates,
    file: Option<SunriseCacheFile>,
    backup_path: Option<PathBuf>,
}

#[derive(Debug)]
struct ValidatedPackageHeaderCaches {
    game_root: PathBuf,
    files: Vec<SunriseCacheFile>,
}

#[derive(Debug)]
struct OriginalPackageHeaderCaches {
    game_root: PathBuf,
    files: Vec<(SunriseCacheFile, PathBuf)>,
}

#[derive(Debug)]
struct OriginalArtifact {
    file_name: String,
    target_path: PathBuf,
    backup_path: Option<PathBuf>,
    digest: Option<FileDigest>,
}

#[derive(Debug)]
struct PreparedArtifact {
    remove_target: bool,
    manifest: ArtifactMetadata,
    target_path: PathBuf,
    temporary_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstallTransactionState {
    Pending,
    Committed,
    Recovered,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TransactionDigest {
    byte_length: u64,
    sha256: String,
}

impl From<&FileDigest> for TransactionDigest {
    fn from(value: &FileDigest) -> Self {
        Self {
            byte_length: value.byte_length,
            sha256: value.sha256.clone(),
        }
    }
}

impl From<&ArtifactMetadata> for TransactionDigest {
    fn from(value: &ArtifactMetadata) -> Self {
        Self {
            byte_length: value.byte_length,
            sha256: value.sha256.clone(),
        }
    }
}

impl PartialEq<FileDigest> for TransactionDigest {
    fn eq(&self, other: &FileDigest) -> bool {
        self.byte_length == other.byte_length && self.sha256 == other.sha256
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallTransactionArtifact {
    #[serde(default, skip_serializing_if = "is_false")]
    remove_target: bool,
    file_name: String,
    authored: TransactionDigest,
    original: Option<TransactionDigest>,
    backup_file_name: Option<String>,
    temporary_file_name: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallTransactionRecord {
    schema: u32,
    state: InstallTransactionState,
    target_packages_directory: PathBuf,
    backup_directory: PathBuf,
    artifacts: Vec<InstallTransactionArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_cleanup: Option<account::AccountCleanupRecord>,
}

type CacheQuarantineRename = fn(&Path, &Path) -> io::Result<()>;
type CacheQuarantineCleanup = fn(&Path, &Path) -> Option<PathBuf>;

#[derive(Clone, Copy)]
struct CacheInvalidationOps {
    rename: CacheQuarantineRename,
    cleanup: CacheQuarantineCleanup,
}

const DEFAULT_CACHE_INVALIDATION_OPS: CacheInvalidationOps = CacheInvalidationOps {
    rename: rename_cache_into_quarantine,
    cleanup: cleanup_cache_quarantine,
};

/// Verify and transactionally install one staged Parhelion package set.
pub fn install_staged_packages(request: &InstallRequest) -> Result<InstallReport, InstallError> {
    let _lock = lock_installation(&request.target_packages_directory)?;
    recover_interrupted_install_locked(&RecoveryRequest {
        target_packages_directory: request.target_packages_directory.clone(),
        backup_root: request.backup_root.clone(),
        game_running_check: request.game_running_check,
    })?;
    install_staged_packages_inner(request, None, DEFAULT_CACHE_INVALIDATION_OPS)
}

fn install_staged_packages_inner(
    request: &InstallRequest,
    fail_after_commits: Option<usize>,
    cache_ops: CacheInvalidationOps,
) -> Result<InstallReport, InstallError> {
    install_with_replacer(
        request,
        fail_after_commits,
        cache_ops,
        sundial::package_authoring::replace_file_from_path_atomically,
    )
}

type PackageReplace = fn(&Path, &Path) -> Result<(), String>;

fn install_with_replacer(
    request: &InstallRequest,
    fail_after_commits: Option<usize>,
    cache_ops: CacheInvalidationOps,
    replace: PackageReplace,
) -> Result<InstallReport, InstallError> {
    let _staged_run_lease =
        crate::workflow::staging_retention::lease_for_read(&request.staged_run_directory)
            .map_err(InstallError::validation)?;
    let validated = validate_request(request)?;
    let backup_directory = create_backup_directory(&validated.backup_root).map_err(|error| {
        InstallError::validation(format!("Could not create a package backup: {error}"))
    })?;
    if path_is_within(&backup_directory, &validated.target_packages_directory) {
        let _ = fs::remove_dir(&backup_directory);
        return Err(InstallError::validation(
            "The resolved backup directory is inside the target packages directory",
        ));
    }

    let originals = match backup_originals(&validated, &backup_directory) {
        Ok(originals) => originals,
        Err(message) => return Err(InstallError::after_backup(message, &backup_directory)),
    };
    let original_sunrise_cache =
        match backup_sunrise_cache(&validated.sunrise_build_cache, &backup_directory) {
            Ok(original) => original,
            Err(message) => {
                return Err(InstallError::after_backup(message, &backup_directory));
            }
        };
    let original_package_header_caches =
        match backup_package_header_caches(&validated.package_header_caches, &backup_directory) {
            Ok(original) => original,
            Err(message) => {
                return Err(InstallError::after_backup(message, &backup_directory));
            }
        };
    let recipe_backup_directory = match backup_recipe_snapshots(&validated, &backup_directory) {
        Ok(directory) => directory,
        Err(message) => {
            return Err(InstallError::after_backup(message, &backup_directory));
        }
    };
    let prepared = match prepare_temporary_files(&validated) {
        Ok(prepared) => prepared,
        Err(message) => return Err(InstallError::after_backup(message, &backup_directory)),
    };
    if let Err(message) = verify_targets_unchanged(&originals) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(message, &backup_directory));
    }
    if let Err(message) = verify_sunrise_cache_unchanged(&original_sunrise_cache) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(message, &backup_directory));
    }
    if let Err(message) = verify_package_header_caches_unchanged(&original_package_header_caches) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(message, &backup_directory));
    }
    if let Err(error) = validate_target_package_chain(&validated.target_packages_directory) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(
            format!("Target package headers changed after preflight: {error}"),
            &backup_directory,
        ));
    }
    if let Err(error) = verify_source_artifacts(
        &validated.target_packages_directory,
        &validated.source_artifacts,
    ) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(
            format!("Stock source packages changed after preflight: {error}"),
            &backup_directory,
        ));
    }
    let mut transaction =
        match build_install_transaction(&validated, &backup_directory, &originals, &prepared) {
            Ok(transaction) => transaction,
            Err(message) => {
                cleanup_prepared_files(&prepared);
                return Err(InstallError::after_backup(message, &backup_directory));
            }
        };
    let journal_path = validated
        .target_packages_directory
        .join(INSTALL_TRANSACTION_FILE_NAME);
    if let Some(cleanup) = validated
        .replacement_guard
        .as_ref()
        .and_then(ReplacementReview::account_cleanup)
    {
        match account::prepare(
            cleanup,
            &validated.target_packages_directory,
            &backup_directory,
        ) {
            Ok(record) => transaction.account_cleanup = Some(record),
            Err(error) => {
                cleanup_prepared_files(&prepared);
                return Err(InstallError::after_backup(
                    error.to_string(),
                    &backup_directory,
                ));
            }
        }
    }
    if let Err(error) = write_install_transaction(&journal_path, &transaction) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(error.message, &backup_directory));
    }
    if let Err(message) = check_game_immediately_before_commit(request).and_then(|()| {
        replacement::verify_account(
            &validated.target_packages_directory,
            validated.replacement_guard.as_ref(),
        )
    }) {
        cleanup_prepared_files(&prepared);
        transaction.state = InstallTransactionState::Recovered;
        if write_install_transaction(&journal_path, &transaction).is_ok() {
            remove_terminal_install_transaction(&journal_path);
        }
        return Err(InstallError::after_backup(message, &backup_directory));
    }

    commit_and_verify(
        CommitContext {
            validated: &validated,
            backup_directory: &backup_directory,
            originals: &originals,
            original_sunrise_cache: &original_sunrise_cache,
            original_package_header_caches: &original_package_header_caches,
            recipe_backup_directory,
            journal_path: &journal_path,
            fail_after_commits,
            cache_ops,
            game_running_check: request.game_running_check,
            replace,
        },
        prepared,
        transaction,
    )
}

fn check_game_before_validation(request: &InstallRequest) -> Result<(), InstallError> {
    let game_is_running = (request.game_running_check)().map_err(|error| {
        InstallError::validation(format!(
            "Could not check whether the game is running: {error}"
        ))
    })?;
    if game_is_running {
        return Err(InstallError::validation(
            "The game is running; close it before installing authored packages",
        ));
    }
    Ok(())
}

fn check_game_immediately_before_commit(request: &InstallRequest) -> Result<(), String> {
    let game_is_running = (request.game_running_check)()
        .map_err(|error| format!("Could not recheck whether the game is running: {error}"))?;
    if game_is_running {
        return Err(
            "The game started during installation preflight; no package files were changed"
                .to_owned(),
        );
    }
    Ok(())
}

struct ValidatedManifest {
    source_artifacts: Vec<ArtifactMetadata>,
    artifacts: Vec<ArtifactMetadata>,
    selected_recipe_files: Vec<String>,
    authored_unlocks: Vec<AuthoredCollectionUnlock>,
}

fn prepare_temporary_files(validated: &ValidatedRun) -> Result<Vec<PreparedArtifact>, String> {
    prepare_temporary_files_inner(validated, None)
}

fn prepare_temporary_files_inner(
    validated: &ValidatedRun,
    fail_before_allocation: Option<usize>,
) -> Result<Vec<PreparedArtifact>, String> {
    let mut prepared = Vec::with_capacity(validated.artifacts.len());
    for (index, artifact) in validated.artifacts.iter().enumerate() {
        let profile = authored_package_for_file_name(&artifact.file_name).ok_or_else(|| {
            format!(
                "Internal error: no authored package profile for {}",
                artifact.file_name
            )
        })?;
        let allocated = if fail_before_allocation == Some(index) {
            Err(format!(
                "Injected target temporary-file allocation failure at artifact {index}"
            ))
        } else {
            create_target_temporary_file(&validated.target_packages_directory, "install", index)
        };
        let (temporary_path, mut temporary_file) = match allocated {
            Ok(allocated) => allocated,
            Err(error) => {
                cleanup_prepared_files(&prepared);
                return Err(error);
            }
        };
        let staged_path = validated.staged_run_directory.join(&artifact.file_name);
        let copied = match copy_into_open_file(&staged_path, &mut temporary_file) {
            Ok(copied) => copied,
            Err(error) => {
                drop(temporary_file);
                remove_file_if_present(&temporary_path);
                cleanup_prepared_files(&prepared);
                return Err(format!(
                    "Could not prepare {} for installation: {error}",
                    artifact.file_name
                ));
            }
        };
        drop(temporary_file);
        if copied.byte_length != artifact.byte_length || copied.sha256 != artifact.sha256 {
            remove_file_if_present(&temporary_path);
            cleanup_prepared_files(&prepared);
            return Err(format!(
                "Staged artifact {} changed after manifest verification",
                artifact.file_name
            ));
        }
        if let Err(error) = validate_authored_package_file(&temporary_path, profile) {
            remove_file_if_present(&temporary_path);
            cleanup_prepared_files(&prepared);
            return Err(format!(
                "Prepared artifact {} failed package validation before commit: {}",
                artifact.file_name, error.message
            ));
        }
        prepared.push(PreparedArtifact {
            remove_target: false,
            manifest: artifact.clone(),
            target_path: validated
                .target_packages_directory
                .join(&artifact.file_name),
            temporary_path,
        });
    }
    for artifact in &validated.obsolete_artifacts {
        prepared.push(PreparedArtifact {
            remove_target: true,
            manifest: artifact.clone(),
            target_path: validated
                .target_packages_directory
                .join(&artifact.file_name),
            // Removal needs no payload, but its unique name keeps journal cleanup uniform.
            temporary_path: validated
                .target_packages_directory
                .join(format!(".parhelion-install-retire-{}.tmp", unique_token())),
        });
    }
    Ok(prepared)
}

fn verify_targets_unchanged(originals: &[OriginalArtifact]) -> Result<(), String> {
    for original in originals {
        match (
            &original.digest,
            fs::symlink_metadata(&original.target_path),
        ) {
            (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => {}
            (None, Err(error)) => {
                return Err(format!(
                    "Could not recheck target package {}: {error}",
                    original.target_path.display()
                ));
            }
            (None, Ok(_)) => {
                return Err(format!(
                    "Target package {} appeared after preflight",
                    original.target_path.display()
                ));
            }
            (Some(_), Err(error)) => {
                return Err(format!(
                    "Target package {} disappeared after backup: {error}",
                    original.target_path.display()
                ));
            }
            (Some(expected), Ok(metadata)) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(format!(
                        "Target package {} changed type after backup",
                        original.target_path.display()
                    ));
                }
                let current = digest_file(&original.target_path).map_err(|error| {
                    format!(
                        "Could not recheck target package {}: {error}",
                        original.target_path.display()
                    )
                })?;
                if &current != expected {
                    return Err(format!(
                        "Target package {} changed after backup",
                        original.target_path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReconciledTarget {
    Original,
    Authored,
}

fn verify_recovered_target(
    target_packages_directory: &Path,
    artifact: &InstallTransactionArtifact,
) -> Result<(), InstallError> {
    let target_path = target_packages_directory.join(&artifact.file_name);
    match &artifact.original {
        Some(original) => {
            let digest = digest_file(&target_path).map_err(|error| {
                InstallError::validation(format!(
                    "Could not verify recovered package {}: {error}",
                    target_path.display()
                ))
            })?;
            if original != &digest {
                return Err(InstallError::validation(format!(
                    "Recovered package does not match its original digest: {}",
                    target_path.display()
                )));
            }
        }
        None if target_path.exists() => {
            return Err(InstallError::validation(format!(
                "New package still exists after interrupted-install recovery: {}",
                target_path.display()
            )));
        }
        None => {}
    }
    Ok(())
}

struct CommitContext<'a> {
    validated: &'a ValidatedRun,
    backup_directory: &'a Path,
    originals: &'a [OriginalArtifact],
    original_sunrise_cache: &'a OriginalSunriseCache,
    original_package_header_caches: &'a OriginalPackageHeaderCaches,
    recipe_backup_directory: Option<PathBuf>,
    journal_path: &'a Path,
    fail_after_commits: Option<usize>,
    cache_ops: CacheInvalidationOps,
    game_running_check: GameRunningCheck,
    replace: PackageReplace,
}

fn commit_and_verify(
    context: CommitContext<'_>,
    prepared: Vec<PreparedArtifact>,
    mut transaction: InstallTransactionRecord,
) -> Result<InstallReport, InstallError> {
    let commit_result = (|| {
        if let (Some(cleanup), Some(record)) = (
            context
                .validated
                .replacement_guard
                .as_ref()
                .and_then(ReplacementReview::account_cleanup),
            &transaction.account_cleanup,
        ) {
            account::commit(
                cleanup,
                record,
                &context.validated.target_packages_directory,
                context.backup_directory,
            )
            .map_err(|error| error.to_string())?;
        }
        commit_prepared_files(&prepared, context.fail_after_commits, context.replace)?;
        verify_installed_files(&prepared)?;
        let sunrise_cache =
            invalidate_sunrise_cache(context.original_sunrise_cache, context.cache_ops)?;
        let package_header_caches = invalidate_package_header_caches(
            context.original_package_header_caches,
            context.cache_ops,
        )?;
        account::verify_committed(&transaction).map_err(|error| error.to_string())?;
        Ok((sunrise_cache, package_header_caches))
    })();
    let (invalidated_sunrise_cache, invalidated_package_header_caches) = match commit_result {
        Ok(caches) => caches,
        Err(message) => {
            cleanup_prepared_files(&prepared);
            // A replacement may return an error after rename (for example directory
            // fsync on Linux). Reconcile every target, not just successful calls.
            let rollback = rollback_transaction(&transaction, context.game_running_check);
            if rollback.succeeded() {
                transaction.state = InstallTransactionState::Recovered;
                if write_install_transaction(context.journal_path, &transaction).is_ok() {
                    remove_terminal_install_transaction(context.journal_path);
                }
            }
            return Err(InstallError {
                message,
                backup_directory: Some(context.backup_directory.to_path_buf()),
                rollback: Some(Box::new(rollback)),
            });
        }
    };

    // Once finalization starts, a journal write error may mean the terminal
    // record was renamed but not synced. Do not then roll packages back beneath
    // a possibly committed journal. Keep the backup and let recovery reconcile it.
    transaction.state = InstallTransactionState::Committed;
    if let Err(error) = write_install_transaction(context.journal_path, &transaction) {
        cleanup_prepared_files(&prepared);
        return Err(InstallError::after_backup(
            error.message,
            context.backup_directory,
        ));
    }

    let game_root = context.validated.target_packages_directory.parent();
    let profile_sync = if context.validated.authored_unlocks.is_empty() {
        None
    } else {
        Some(match game_root {
            Some(game_root) => synchronize_authored_collection_unlocks(
                game_root,
                &context.validated.authored_unlocks,
            ),
            None => Err("Installed package directory has no game root".to_owned()),
        })
    };
    let mut report = build_install_report(
        &context,
        &prepared,
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        profile_sync,
    );
    cleanup_prepared_files(&prepared);
    remove_terminal_install_transaction(context.journal_path);
    if let Err(error) = mark_backup_complete(&transaction) {
        report.backup_prune_warning = Some(error);
    }
    if context.validated.limit_package_backups {
        match prune_package_backups(
            &context.validated.backup_root,
            context.validated.package_backup_retention,
        ) {
            Ok(pruned) => report.pruned_backup_directories = pruned.removed_directories,
            Err(error) => report.backup_prune_warning = Some(error),
        }
    }
    Ok(report)
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}

fn commit_prepared_files(
    prepared: &[PreparedArtifact],
    fail_after_commits: Option<usize>,
    replace: PackageReplace,
) -> Result<(), String> {
    for (index, artifact) in prepared.iter().enumerate() {
        if artifact.remove_target {
            verify_target_matches_installed(artifact)?;
            fs::remove_file(&artifact.target_path).map_err(|error| {
                format!(
                    "Could not remove obsolete package {}: {error}",
                    artifact.manifest.file_name
                )
            })?;
        } else {
            replace(&artifact.temporary_path, &artifact.target_path).map_err(|error| {
                format!(
                    "Could not atomically install {}: {error}",
                    artifact.manifest.file_name
                )
            })?;
        }
        let committed = index + 1;
        if fail_after_commits == Some(committed) {
            return Err(format!(
                "Injected installation failure after {committed} committed artifact(s)"
            ));
        }
    }
    Ok(())
}

fn verify_installed_files(prepared: &[PreparedArtifact]) -> Result<(), String> {
    for artifact in prepared {
        if artifact.remove_target {
            match fs::symlink_metadata(&artifact.target_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                _ => {
                    return Err(format!(
                        "Obsolete package {} was not removed",
                        artifact.manifest.file_name
                    ));
                }
            }
        }
        let installed = digest_file(&artifact.target_path).map_err(|error| {
            format!(
                "Could not verify installed package {}: {error}",
                artifact.target_path.display()
            )
        })?;
        if installed.byte_length != artifact.manifest.byte_length
            || installed.sha256 != artifact.manifest.sha256
        {
            return Err(format!(
                "Installed package {} failed length/SHA-256 verification",
                artifact.manifest.file_name
            ));
        }
    }
    Ok(())
}

fn build_install_report(
    context: &CommitContext<'_>,
    prepared: &[PreparedArtifact],
    invalidated_sunrise_cache: Option<InvalidatedSunriseCache>,
    invalidated_package_header_caches: Vec<InvalidatedPackageHeaderCache>,
    profile_sync: Option<Result<AuthoredProfileSyncReport, String>>,
) -> InstallReport {
    let validated = context.validated;
    let artifacts = prepared
        .iter()
        .zip(context.originals)
        .filter(|(prepared, _)| !prepared.remove_target)
        .map(|(prepared, original)| InstalledArtifact {
            file_name: prepared.manifest.file_name.clone(),
            target_path: prepared.target_path.clone(),
            byte_length: prepared.manifest.byte_length,
            sha256: prepared.manifest.sha256.clone(),
            replaced_existing_file: original.backup_path.is_some(),
            backup_path: original.backup_path.clone(),
        })
        .collect();
    InstallReport {
        cleaned_account: validated
            .replacement_guard
            .as_ref()
            .filter(|review| review.changes_account())
            .and_then(ReplacementReview::account_cleanup)
            .map(|cleanup| cleanup.settings_path.clone()),
        manifest_schema: MANIFEST_SCHEMA,
        staged_run_directory: validated.staged_run_directory.clone(),
        target_packages_directory: validated.target_packages_directory.clone(),
        backup_directory: context.backup_directory.to_path_buf(),
        artifacts,
        removed_obsolete_packages: prepared
            .iter()
            .filter(|artifact| artifact.remove_target)
            .map(|artifact| artifact.target_path.clone())
            .collect(),
        recipe_backup_directory: context.recipe_backup_directory.clone(),
        pruned_backup_directories: Vec::new(),
        backup_prune_warning: None,
        invalidated_sunrise_cache,
        invalidated_package_header_caches,
        profile_sync,
    }
}

fn verify_target_matches_installed(prepared: &PreparedArtifact) -> Result<(), String> {
    let current = digest_file(&prepared.target_path).map_err(|error| {
        format!(
            "Could not verify {} before rollback: {error}",
            prepared.target_path.display()
        )
    })?;
    if current.byte_length != prepared.manifest.byte_length
        || current.sha256 != prepared.manifest.sha256
    {
        return Err(format!(
            "Refusing to overwrite {} during rollback because it changed after installation",
            prepared.target_path.display()
        ));
    }
    Ok(())
}

fn cleanup_prepared_files(prepared: &[PreparedArtifact]) {
    for artifact in prepared {
        remove_file_if_present(&artifact.temporary_path);
    }
}

fn create_target_temporary_file(
    target_directory: &Path,
    operation: &str,
    ordinal: usize,
) -> Result<(PathBuf, File), String> {
    for attempt in 0..128u64 {
        let path = target_directory.join(format!(
            ".parhelion-{operation}-{}-{ordinal}-{attempt}.tmp",
            unique_token()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create same-volume temporary file {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Err("Could not allocate a unique same-volume temporary file".to_owned())
}

fn copy_file_create_new(source: &Path, destination: &Path) -> io::Result<FileDigest> {
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    copy_into_open_file(source, &mut destination_file)
}

fn copy_into_open_file(source: &Path, destination: &mut File) -> io::Result<FileDigest> {
    let mut source_file = File::open(source)?;
    let mut digest = Sha256::new();
    let mut byte_length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source_file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
        byte_length += read as u64;
    }
    destination.sync_all()?;
    Ok(FileDigest {
        byte_length,
        sha256: format!("{:X}", digest.finalize()),
    })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unique_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032X}-{:08X}-{counter:016X}", process::id())
}

fn remove_file_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}
