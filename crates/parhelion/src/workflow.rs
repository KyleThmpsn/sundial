use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sundial::package_authoring::{path_is_within, resolve_path_for_comparison};

use crate::artifact::{ArtifactMetadata, digest_file, has_pkg_extension};
use crate::manifest::{
    MANIFEST_FILE_NAME, MANIFEST_SCHEMA, ManifestDocument, ManifestProject,
    recipe_selection_fingerprint,
};
#[cfg(test)]
use crate::package_profile::{CANONICAL_ARTIFACT_FILE_NAMES, CANONICAL_PACKAGE_IDS};
use crate::package_profile::{
    CANONICAL_PACKAGES, PACKAGE_HEADER_PREFIX_SIZE, PARHELION_ASSET_PACKAGE_ID,
    PackageHeaderPrefix, SHADOWKEEP_HEADER_VERSION, authored_package,
    authored_packages_for_file_names, canonical_package,
};
use crate::recipe::WeaponRecipe;
use crate::weapon::{
    build_weapon_project_after_catalog_validation, validate_weapon_clone_specs_against_catalog,
};
use crate::{NewWeaponProjectBundle, SUNDIAL_BUILD_SIGNATURE, WeaponProjectSpec};

mod package_views;
pub(crate) mod staging_retention;

/// A batch selected from Parhelion's recipe library.
///
/// The complete, explicitly enabled recipe set compiled into one package generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchBuildRequest {
    pub package_directory: PathBuf,
    pub staging_root: PathBuf,
    pub ignore_installed_authored_overlays: bool,
    pub recipes: Vec<WeaponRecipe>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchBuildSnapshot {
    pub request: BatchBuildRequest,
    pub fingerprint: String,
}

impl BatchBuildSnapshot {
    pub fn new(mut request: BatchBuildRequest) -> Result<Self, String> {
        if request.recipes.is_empty() {
            return Err("No recipes are included in the package set".to_owned());
        }
        request.recipes.sort_by_cached_key(|recipe| {
            (
                recipe.namespace.clone(),
                recipe.identity.item_hash.as_str().to_owned(),
            )
        });
        let fingerprint = recipe_selection_fingerprint(&request.recipes)?;
        Ok(Self {
            request,
            fingerprint,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BuildReport {
    pub weapons: Vec<WeaponBuildReport>,
    pub run_directory: PathBuf,
    pub manifest_path: PathBuf,
    pub artifacts: Vec<ArtifactMetadata>,
    pub selection_fingerprint: String,
    pub staged_recipe_paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildPhase {
    InspectingSource,
    CompilingProject,
    WritingPackages,
    ValidatingPackages,
    StagingRecipes,
    WritingManifest,
    Complete,
}

impl BuildPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InspectingSource => "Inspecting source packages",
            Self::CompilingProject => "Compiling weapon project",
            Self::WritingPackages => "Writing package set",
            Self::ValidatingPackages => "Validating package artifacts",
            Self::StagingRecipes => "Staging recipe snapshots",
            Self::WritingManifest => "Writing build manifest",
            Self::Complete => "Build validated",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildProgress {
    pub phase: BuildPhase,
    pub current_artifact: Option<String>,
    pub completed: usize,
    pub total: usize,
}

impl BuildProgress {
    fn phase(phase: BuildPhase, completed: usize, total: usize) -> Self {
        Self {
            phase,
            current_artifact: None,
            completed,
            total,
        }
    }

    fn artifact(
        phase: BuildPhase,
        current_artifact: impl Into<String>,
        completed: usize,
        total: usize,
    ) -> Self {
        Self {
            phase,
            current_artifact: Some(current_artifact.into()),
            completed,
            total,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponBuildReport {
    pub name: String,
    pub namespace: String,
    pub item_hash: u32,
    pub icon_definition_hash: u32,
    pub item_index: u16,
    pub collectible_hash: u32,
    pub collectible_index: u16,
    pub unlock_hash: u32,
    pub unlock_definition_index: u16,
    pub unlock_bank: u8,
    pub unlock_slot: u16,
}

fn default_data_root() -> PathBuf {
    sundial::package_authoring::parhelion_data_directory()
        .unwrap_or_else(|| env::current_dir().unwrap_or_default().join("parhelion"))
}

pub(crate) fn default_staging_root() -> PathBuf {
    default_data_root().join("staging")
}

pub(crate) fn default_backup_root() -> PathBuf {
    default_data_root().join("backups").join("packages")
}

#[cfg(test)]
fn preflight_snapshot(snapshot: &BatchBuildSnapshot) -> Result<(), String> {
    plan_snapshot(snapshot, &mut |_| {}).map(drop)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceInspection {
    ignored_authored_files: Vec<String>,
}

struct PlannedSnapshot {
    source_inspection: SourceInspection,
    source_artifacts: Vec<ArtifactMetadata>,
    bundle: NewWeaponProjectBundle,
}

fn plan_snapshot(
    snapshot: &BatchBuildSnapshot,
    progress: &mut impl FnMut(BuildProgress),
) -> Result<PlannedSnapshot, String> {
    progress(BuildProgress::phase(BuildPhase::InspectingSource, 0, 3));
    let project = project_spec(snapshot)?;
    progress(BuildProgress::phase(BuildPhase::InspectingSource, 1, 3));
    let source_inspection = inspect_snapshot(snapshot)?;
    progress(BuildProgress::phase(BuildPhase::InspectingSource, 2, 3));
    let install_directory = snapshot
        .request
        .package_directory
        .parent()
        .ok_or_else(|| "The package directory has no Shadowkeep install root".to_owned())?;
    validate_weapon_clone_specs_against_catalog(install_directory, project.weapons.iter())
        .map_err(|error| format!("Weapon project compatibility validation failed: {error}"))?;
    let source = PackageSource::prepare(
        &snapshot.request.package_directory,
        &source_inspection.ignored_authored_files,
    )?;
    let result = (|| {
        let source_artifacts = source_artifact_reports(source.path())?;
        progress(BuildProgress::phase(BuildPhase::InspectingSource, 3, 3));
        progress(BuildProgress::phase(BuildPhase::CompilingProject, 0, 1));
        let compilation = build_weapon_project_after_catalog_validation(source.path(), &project)
            .map_err(|error| format!("Weapon project compilation failed: {error}"));
        let source_artifacts_after = source_artifact_reports(source.path())?;
        validate_source_artifacts_unchanged(&source_artifacts, &source_artifacts_after)?;
        let bundle = compilation?;
        progress(BuildProgress::phase(BuildPhase::CompilingProject, 1, 1));
        Ok(PlannedSnapshot {
            source_inspection,
            source_artifacts,
            bundle,
        })
    })();
    // Compilation has released its managers and returned owned package bytes. Report cleanup
    // failures through the normal build/UI error path before any staged output is committed.
    source.finish(result)
}

fn inspect_snapshot(snapshot: &BatchBuildSnapshot) -> Result<SourceInspection, String> {
    validate_paths(
        &snapshot.request.package_directory,
        &snapshot.request.staging_root,
    )?;
    let mut stock_patches = BTreeMap::<u16, u16>::new();
    let mut authored = Vec::new();

    for entry in fs::read_dir(&snapshot.request.package_directory).map_err(|error| {
        format!(
            "Could not list {}: {error}",
            snapshot.request.package_directory.display()
        )
    })? {
        let entry = entry.map_err(|error| format!("Could not read a package entry: {error}"))?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
            .is_file()
            || !has_pkg_extension(&path)
        {
            continue;
        }
        let Some(header) = read_relevant_header(&path)? else {
            continue;
        };
        if header.build_signature == SUNDIAL_BUILD_SIGNATURE {
            let profile = authored_package(header.package_id).ok_or_else(|| {
                format!(
                    "Authored package {} uses unrecognized package id {:04X}",
                    path.display(),
                    header.package_id
                )
            })?;
            let name = file_name(&path)?;
            if header.patch_id != profile.patch_id || name != profile.file_name {
                return Err(format!(
                    "Authored package {} has identity {:04X}/{}; expected {} at patch {}",
                    path.display(),
                    header.package_id,
                    header.patch_id,
                    profile.file_name,
                    profile.patch_id
                ));
            }
            authored.push(name);
        } else if header.package_id == PARHELION_ASSET_PACKAGE_ID {
            return Err(format!(
                "Package id {PARHELION_ASSET_PACKAGE_ID:04X} is reserved for Parhelion assets but is occupied by {}",
                path.display()
            ));
        } else {
            stock_patches
                .entry(header.package_id)
                .and_modify(|patch| *patch = (*patch).max(header.patch_id))
                .or_insert(header.patch_id);
        }
    }

    validate_stock_patches(&stock_patches)?;
    authored.sort();
    validate_authored_set(
        snapshot.request.ignore_installed_authored_overlays,
        &authored,
    )?;
    Ok(SourceInspection {
        ignored_authored_files: authored,
    })
}

#[cfg(test)]
fn build_and_stage_snapshot(snapshot: &BatchBuildSnapshot) -> Result<BuildReport, String> {
    build_and_stage_snapshot_with_progress(snapshot, |_| {})
}

pub fn build_and_stage_snapshot_with_progress(
    snapshot: &BatchBuildSnapshot,
    mut progress: impl FnMut(BuildProgress),
) -> Result<BuildReport, String> {
    let planned = plan_snapshot(snapshot, &mut progress)?;
    let source_inspection = planned.source_inspection;
    let source_artifacts = planned.source_artifacts;
    let bundle = planned.bundle;
    fs::create_dir_all(&snapshot.request.staging_root).map_err(|error| {
        format!(
            "Could not create staging root {}: {error}",
            snapshot.request.staging_root.display()
        )
    })?;
    let slug = if snapshot.request.recipes.len() == 1 {
        snapshot.request.recipes[0].slug()
    } else {
        "parhelion-project".to_owned()
    };
    let staged_run = staging_retention::StagedRun::begin(&snapshot.request.staging_root, &slug)?;
    let run_directory = staged_run.directory().to_owned();
    let result = (|| {
        progress(BuildProgress::phase(BuildPhase::WritingPackages, 0, 1));
        let paths = bundle
            .write_new(&run_directory)
            .map_err(|error| format!("Could not write staged packages: {error}"))?;
        progress(BuildProgress::phase(BuildPhase::WritingPackages, 1, 1));
        let artifacts = artifact_reports_with_progress(&paths, &mut progress)?;
        validate_outputs(&artifacts)?;
        let staged_recipe_paths =
            stage_recipe_snapshot_with_progress(&run_directory, snapshot, &mut progress)?;
        let staged_recipe_files = staged_recipe_paths
            .iter()
            .map(|path| {
                path.strip_prefix(&run_directory)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();

        let source_package_directory = fs::canonicalize(&snapshot.request.package_directory)
            .map_err(|error| {
                format!(
                    "Could not canonicalize source package directory {}: {error}",
                    snapshot.request.package_directory.display()
                )
            })?;
        let manifest_path = run_directory.join(MANIFEST_FILE_NAME);
        progress(BuildProgress::artifact(
            BuildPhase::WritingManifest,
            MANIFEST_FILE_NAME,
            0,
            1,
        ));
        let manifest = ManifestDocument {
            schema: MANIFEST_SCHEMA,
            source_package_directory: source_package_directory.display().to_string(),
            source_artifacts: source_artifacts.clone(),
            ignored_authored_files: source_inspection.ignored_authored_files.clone(),
            selection_fingerprint: snapshot.fingerprint.clone(),
            selected_recipe_files: staged_recipe_files,
            project: ManifestProject::from_build(
                &snapshot.request.recipes,
                &bundle.plan.weapons,
                &bundle.plan.sunrise,
            )?,
            artifacts: artifacts.clone(),
        };
        write_manifest(&manifest_path, &manifest)?;
        progress(BuildProgress::artifact(
            BuildPhase::WritingManifest,
            MANIFEST_FILE_NAME,
            1,
            1,
        ));

        let report = BuildReport {
            weapons: snapshot
                .request
                .recipes
                .iter()
                .zip(&bundle.plan.weapons)
                .map(|(recipe, plan)| WeaponBuildReport {
                    name: recipe.name.clone(),
                    namespace: recipe.namespace.clone(),
                    item_hash: plan.item_hash,
                    icon_definition_hash: u32::from(plan.icon_definition_tag),
                    item_index: plan.item_index,
                    collectible_hash: plan.collectible_hash,
                    collectible_index: plan.collectible_index,
                    unlock_hash: plan.unlock_hash,
                    unlock_definition_index: plan.unlock_definition_index,
                    unlock_bank: plan.unlock_bank,
                    unlock_slot: plan.unlock_slot,
                })
                .collect(),
            run_directory,
            manifest_path,
            artifacts,
            selection_fingerprint: snapshot.fingerprint.clone(),
            staged_recipe_paths,
        };
        Ok(report)
    })();
    let report = staged_run.finish(result)?;
    progress(BuildProgress::phase(BuildPhase::Complete, 1, 1));
    Ok(report)
}

fn project_spec(snapshot: &BatchBuildSnapshot) -> Result<WeaponProjectSpec, String> {
    snapshot
        .request
        .recipes
        .iter()
        .map(|recipe| {
            recipe
                .to_spec()
                .map_err(|error| format!("Recipe {:?} is invalid: {error}", recipe.name))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|weapons| WeaponProjectSpec { weapons })
}

#[cfg(test)]
fn stage_recipe_snapshot(
    run_directory: &Path,
    snapshot: &BatchBuildSnapshot,
) -> Result<Vec<PathBuf>, String> {
    stage_recipe_snapshot_with_progress(run_directory, snapshot, &mut |_| {})
}

fn stage_recipe_snapshot_with_progress(
    run_directory: &Path,
    snapshot: &BatchBuildSnapshot,
    progress: &mut impl FnMut(BuildProgress),
) -> Result<Vec<PathBuf>, String> {
    let recipe_directory = run_directory.join("recipes");
    fs::create_dir(&recipe_directory).map_err(|error| {
        format!(
            "Could not create staged recipe directory {}: {error}",
            recipe_directory.display()
        )
    })?;
    let mut paths = Vec::with_capacity(snapshot.request.recipes.len());
    let total = snapshot.request.recipes.len();
    for (index, recipe) in snapshot.request.recipes.iter().enumerate() {
        progress(BuildProgress::artifact(
            BuildPhase::StagingRecipes,
            recipe.name.clone(),
            index,
            total,
        ));
        let slug = recipe.slug();
        let mut suffix = 0u16;
        let path = loop {
            let file_name = if suffix == 0 {
                format!("{slug}.parhelion.json")
            } else {
                format!("{slug}-{suffix}.parhelion.json")
            };
            let candidate = recipe_directory.join(file_name);
            if !candidate.exists() {
                break candidate;
            }
            suffix = suffix.checked_add(1).ok_or_else(|| {
                format!("Could not allocate staged filename for {:?}", recipe.name)
            })?;
        };
        write_staged_recipe(recipe, &path).map_err(|error| {
            format!(
                "Could not stage normalized recipe {}: {error}",
                path.display()
            )
        })?;
        paths.push(path);
        progress(BuildProgress::artifact(
            BuildPhase::StagingRecipes,
            recipe.name.clone(),
            index + 1,
            total,
        ));
    }
    Ok(paths)
}

fn write_staged_recipe(recipe: &WeaponRecipe, path: &Path) -> Result<(), String> {
    let mut encoded = recipe.to_json_pretty().map_err(|error| error.to_string())?;
    encoded.push('\n');
    // A staging run is new and exclusively leased until its completion marker is written.
    // Writing the final owned filename avoids abandoned replacement-temp files after a crash.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn validate_paths(package_directory: &Path, staging_root: &Path) -> Result<(), String> {
    if !package_directory.is_dir() {
        return Err(format!(
            "Package directory does not exist: {}",
            package_directory.display()
        ));
    }
    let package_directory = fs::canonicalize(package_directory).map_err(|error| {
        format!(
            "Could not resolve package directory {}: {error}",
            package_directory.display()
        )
    })?;
    let staging_root = resolve_path_for_comparison(staging_root)
        .map_err(|error| format!("Could not resolve {}: {error}", staging_root.display()))?;
    if path_is_within(&staging_root, &package_directory) {
        return Err("Staging must be outside the live packages directory".to_owned());
    }
    Ok(())
}

fn validate_stock_patches(stock_patches: &BTreeMap<u16, u16>) -> Result<(), String> {
    for profile in CANONICAL_PACKAGES {
        match stock_patches.get(&profile.package_id) {
            Some(&patch) if patch == profile.stock_patch_id => {}
            Some(patch) => {
                return Err(format!(
                    "Package {:04X} has stock patch {patch}; this profile requires patch {}",
                    profile.package_id, profile.stock_patch_id
                ));
            }
            None => {
                return Err(format!(
                    "Package {:04X} has no stock package chain",
                    profile.package_id
                ));
            }
        }
    }
    Ok(())
}

fn validate_authored_set(
    ignore_installed_authored_overlays: bool,
    authored: &[String],
) -> Result<(), String> {
    if authored.is_empty() {
        return Ok(());
    }
    if !ignore_installed_authored_overlays {
        return Err(format!(
            "The source contains Parhelion-authored packages: {}. Enable the stock-view option or choose a clean package directory",
            authored.join(", ")
        ));
    }

    // `inspect_snapshot` has already authenticated every entry in this list by build
    // signature, package id, patch id, and canonical file name. A prior generation can
    // legitimately be partial (for example, after adding a new output profile), and each
    // recognized overlay can be independently omitted from the filtered stock view. Do
    // not require an obsolete all-or-nothing artifact count here.
    Ok(())
}

fn read_relevant_header(path: &Path) -> Result<Option<PackageHeaderPrefix>, String> {
    let mut prefix = [0u8; PACKAGE_HEADER_PREFIX_SIZE];
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    file.read_exact(&mut prefix)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let header = PackageHeaderPrefix::parse(&prefix);
    if header.version != SHADOWKEEP_HEADER_VERSION {
        return Ok(None);
    }
    if canonical_package(header.package_id).is_none()
        && authored_package(header.package_id).is_none()
    {
        return Ok(None);
    }
    Ok(Some(header))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("Package filename is not valid Unicode: {}", path.display()))
}

fn create_unique_run_directory(root: &Path, slug: &str) -> Result<PathBuf, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))?
        .as_secs();
    // Retention removes old directories, so a first-free numeric suffix could reuse an old
    // report's path. A fresh random identity keeps stale requests from targeting a later run.
    tempfile::Builder::new()
        .prefix(&format!("{slug}-{seconds}-"))
        .rand_bytes(12)
        .tempdir_in(root)
        .map(tempfile::TempDir::keep)
        .map_err(|error| format!("Could not allocate a unique staging run directory: {error}"))
}

enum PackageSource {
    Direct(PathBuf),
    Filtered(FilteredPackageView),
}

impl PackageSource {
    fn prepare(
        package_directory: &Path,
        ignored_authored_files: &[String],
    ) -> Result<Self, String> {
        if ignored_authored_files.is_empty() {
            return Ok(Self::Direct(package_directory.to_path_buf()));
        }
        FilteredPackageView::create(package_directory, ignored_authored_files).map(Self::Filtered)
    }

    fn path(&self) -> &Path {
        match self {
            Self::Direct(path) => path,
            Self::Filtered(view) => &view.package_directory,
        }
    }

    fn finish<T>(self, result: Result<T, String>) -> Result<T, String> {
        let cleanup = match self {
            Self::Direct(_) => Ok(()),
            Self::Filtered(view) => view.close(),
        };
        package_views::finish_with_cleanup(result, cleanup)
    }
}

pub(crate) struct FilteredPackageView {
    temporary: Option<tempfile::TempDir>,
    lease: Option<package_views::ViewLease>,
    package_directory: PathBuf,
}

impl FilteredPackageView {
    pub(crate) fn path(&self) -> &Path {
        &self.package_directory
    }

    pub(crate) fn add_overlay(&self, source: &Path) -> Result<(), String> {
        let target = self.package_directory.join(file_name(source)?);
        if target.exists() {
            return Err(format!(
                "Package view already contains {}",
                target.display()
            ));
        }
        // Staging can be on another volume; only the small authored output needs copying.
        if fs::hard_link(source, &target).is_err() {
            fs::copy(source, &target).map_err(|error| {
                format!(
                    "Could not add {} to the read-only package view: {error}",
                    source.display()
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn create(source: &Path, ignored: &[String]) -> Result<Self, String> {
        let install_root = source
            .parent()
            .ok_or_else(|| format!("Package directory has no parent: {}", source.display()))?;
        let view_root = package_view_root(source, install_root)?;
        package_views::prune_stale_views(&view_root)?;
        Self::create_in_root(source, ignored, &view_root)
    }

    fn create_in_root(source: &Path, ignored: &[String], view_root: &Path) -> Result<Self, String> {
        package_views::initialize_source_decoder(source)?;
        let install_root = source
            .parent()
            .ok_or_else(|| format!("Package directory has no parent: {}", source.display()))?;
        let temporary = tempfile::Builder::new()
            .prefix(".parhelion-package-view-")
            .tempdir_in(view_root)
            .map_err(|error| {
                format!(
                    "Could not create a temporary stock package view in {}: {error}",
                    view_root.display()
                )
            })?;
        let package_directory = temporary.path().join("packages");
        let lease = package_views::ViewLease::create(temporary.path())?;
        let view = Self {
            temporary: Some(temporary),
            lease: Some(lease),
            package_directory,
        };
        view.populate(source, ignored, install_root)?;
        Ok(view)
    }

    fn populate(
        &self,
        source: &Path,
        ignored: &[String],
        install_root: &Path,
    ) -> Result<(), String> {
        let package_directory = &self.package_directory;
        fs::create_dir(package_directory).map_err(|error| {
            format!(
                "Could not create temporary package view {}: {error}",
                package_directory.display()
            )
        })?;

        let ignored = ignored.iter().map(String::as_str).collect::<BTreeSet<_>>();
        for entry in fs::read_dir(source)
            .map_err(|error| format!("Could not list {}: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| format!("Could not read package entry: {error}"))?;
            let path = entry.path();
            if !entry
                .file_type()
                .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
                .is_file()
                || !has_pkg_extension(&path)
            {
                continue;
            }
            let name = file_name(&path)?;
            if ignored.contains(name.as_str()) {
                continue;
            }
            let target = package_directory.join(&name);
            fs::hard_link(&path, &target).map_err(|error| {
                format!(
                    "Could not hard-link {} into the temporary stock view: {error}",
                    path.display()
                )
            })?;
        }
        copy_oodle_runtime(install_root, package_directory.parent().unwrap())?;
        Ok(())
    }

    pub(crate) fn close(mut self) -> Result<(), String> {
        self.cleanup()
    }

    pub(crate) fn finish<T>(self, result: Result<T, String>) -> Result<T, String> {
        package_views::finish_with_cleanup(result, self.close())
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let Some(temporary) = self.temporary.take() else {
            return Ok(());
        };
        let path = temporary.keep();
        let lease = self
            .lease
            .take()
            .expect("an owned package view has a lease");
        package_views::remove_owned_view(&path, lease)
    }
}

impl Drop for FilteredPackageView {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            // Normal builds use explicit close and report this through the UI. Unwinding and
            // test-only consumers still retain a marked view for a later safe cleanup attempt.
            eprintln!("{error}");
        }
    }
}

fn package_view_root(source: &Path, install_root: &Path) -> Result<PathBuf, String> {
    let preferred = default_data_root().join("package-views");
    match supports_package_hard_links(source, &preferred)? {
        true => Ok(preferred),
        false => {
            let parent = install_root.parent().ok_or_else(|| {
                format!(
                    "The install root {} has no writable sibling for package views",
                    install_root.display()
                )
            })?;
            let same_volume = parent.join(".sundial-parhelion").join("package-views");
            if supports_package_hard_links(source, &same_volume)? {
                Ok(same_volume)
            } else {
                Err(format!(
                    "Could not create a same-volume package view outside {}",
                    install_root.display()
                ))
            }
        }
    }
}

fn supports_package_hard_links(source: &Path, candidate_root: &Path) -> Result<bool, String> {
    fs::create_dir_all(candidate_root).map_err(|error| {
        format!(
            "Could not create package-view root {}: {error}",
            candidate_root.display()
        )
    })?;
    let source_package = first_package_file(source)?;
    let probe = tempfile::Builder::new()
        .prefix(".parhelion-link-probe-")
        .tempdir_in(candidate_root)
        .map_err(|error| {
            format!(
                "Could not create a package-view probe in {}: {error}",
                candidate_root.display()
            )
        })?;
    match fs::hard_link(&source_package, probe.path().join("probe.pkg")) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => Ok(false),
        Err(error) => Err(format!(
            "Could not verify package hard links from {} into {}: {error}",
            source.display(),
            candidate_root.display()
        )),
    }
}

fn first_package_file(directory: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| {
                format!(
                    "Could not read an entry in {}: {error}",
                    directory.display()
                )
            })?
            .path();
        if !has_pkg_extension(&path) {
            continue;
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if metadata.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "Package directory contains no .pkg files: {}",
        directory.display()
    ))
}

fn copy_oodle_runtime(install_root: &Path, view_root: &Path) -> Result<(), String> {
    let source_directory = install_root.join("bin").join("x64");
    let target_directory = view_root.join("bin").join("x64");
    for name in ["oo2core_3_win64.dll", "oo2core_9_win64.dll"] {
        let source = source_directory.join(name);
        if !source.is_file() {
            continue;
        }
        fs::create_dir_all(&target_directory).map_err(|error| {
            format!(
                "Could not create temporary Oodle directory {}: {error}",
                target_directory.display()
            )
        })?;
        fs::copy(&source, target_directory.join(name))
            .map_err(|error| format!("Could not stage {}: {error}", source.display()))?;
    }
    Ok(())
}

fn artifact_reports_with_progress(
    paths: &[PathBuf],
    progress: &mut impl FnMut(BuildProgress),
) -> Result<Vec<ArtifactMetadata>, String> {
    let mut reports = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let artifact_name = file_name(path)?;
        progress(BuildProgress::artifact(
            BuildPhase::ValidatingPackages,
            artifact_name.clone(),
            index,
            paths.len(),
        ));
        fs::metadata(path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        let digest = digest_file(path)
            .map_err(|error| format!("Could not hash {}: {error}", path.display()))?;
        reports.push(ArtifactMetadata {
            file_name: artifact_name.clone(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
        });
        progress(BuildProgress::artifact(
            BuildPhase::ValidatingPackages,
            artifact_name,
            index + 1,
            paths.len(),
        ));
    }
    reports.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(reports)
}

fn source_artifact_reports(package_directory: &Path) -> Result<Vec<ArtifactMetadata>, String> {
    let mut reports = Vec::new();
    for profile in CANONICAL_PACKAGES {
        let mut has_latest = false;
        for patch_id in 0..=profile.stock_patch_id {
            let file_name = profile.stock_file_name(patch_id);
            let path = package_directory.join(&file_name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!("Could not inspect {}: {error}", path.display()));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "Stock source package is not a regular file: {}",
                    path.display()
                ));
            }
            let header = read_relevant_header(&path)?.ok_or_else(|| {
                format!(
                    "Stock source package {} is not a supported Shadowkeep investment package",
                    path.display()
                )
            })?;
            if header.package_id != profile.package_id
                || header.patch_id != patch_id
                || header.build_signature == SUNDIAL_BUILD_SIGNATURE
            {
                return Err(format!(
                    "Stock source package {} has an incompatible identity",
                    path.display()
                ));
            }
            has_latest |= patch_id == profile.stock_patch_id;
            let digest = digest_file(&path)
                .map_err(|error| format!("Could not hash {}: {error}", path.display()))?;
            reports.push(ArtifactMetadata {
                file_name,
                byte_length: digest.byte_length,
                sha256: digest.sha256,
            });
        }
        if !has_latest {
            return Err(format!(
                "Package {:04X} has no stock patch {} source",
                profile.package_id, profile.stock_patch_id
            ));
        }
    }
    Ok(reports)
}

fn validate_source_artifacts_unchanged(
    before: &[ArtifactMetadata],
    after: &[ArtifactMetadata],
) -> Result<(), String> {
    if before != after {
        return Err(
            "Stock source packages changed while the weapon project was being compiled; discard this staging run and build again"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_outputs(artifacts: &[ArtifactMetadata]) -> Result<(), String> {
    let names = artifacts
        .iter()
        .map(|artifact| artifact.file_name.as_str())
        .collect::<Vec<_>>();
    authored_packages_for_file_names(names).map_err(|error| {
        format!("Compiler emitted an invalid recipe-selected package set: {error}")
    })?;
    Ok(())
}

fn write_manifest(path: &Path, manifest: &ManifestDocument) -> Result<(), String> {
    manifest
        .validate()
        .map_err(|error| format!("Could not validate build manifest: {error}"))?;
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("Could not encode build manifest: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("Could not sync {}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
