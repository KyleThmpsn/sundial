use super::*;

pub(super) mod decoder;
pub(super) mod staged;

#[cfg(test)]
pub(super) fn validate_request(request: &InstallRequest) -> Result<ValidatedRun, InstallError> {
    validate_request_with_progress(request, &mut |_| {})
}

pub(super) fn validate_request_with_progress(
    request: &InstallRequest,
    progress: Observer<'_>,
) -> Result<ValidatedRun, InstallError> {
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Checking Game and Runtime",
        0,
        7,
    ));
    check_game_before_validation(request)?;

    let (staged_run_directory, target_packages_directory, backup_root) =
        validate_request_paths(request)?;
    let target_packages_directory =
        sundial::package_authoring::validate_shadowkeep_packages_directory(
            &target_packages_directory,
        )
        .map_err(InstallError::validation)?;
    (request.runtime_feature_check)(&target_packages_directory).map_err(|error| {
        InstallError::validation(format!(
            "The selected runtime cannot install Parhelion packages: {error}"
        ))
    })?;

    if !(1..=MAX_PACKAGE_BACKUP_RETENTION).contains(&request.package_backup_retention) {
        return Err(InstallError::validation(format!(
            "Package backup retention must be between 1 and {MAX_PACKAGE_BACKUP_RETENTION}"
        )));
    }

    let reviewed = request
        .confirmed_replacement
        .as_ref()
        .and_then(|review| review.validated());
    progress(InstallProgress::item(
        InstallPhase::Checking,
        if reviewed.is_some() {
            "Checking Staged Files for Changes"
        } else {
            "Verifying Staged Packages and Recipes"
        },
        1,
        7,
    ));
    let manifest = if let Some(reviewed) = reviewed {
        staged::verify(reviewed, &staged_run_directory, &target_packages_directory)?;
        std::sync::Arc::clone(reviewed)
    } else {
        std::sync::Arc::new(validate_manifest_and_staged_files(
            &staged_run_directory,
            &target_packages_directory,
        )?)
    };
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Checking Installed Package Headers",
        2,
        7,
    ));
    validate_target_package_chain(&target_packages_directory)?;
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Verifying Source Checksums",
        3,
        7,
    ));
    verify_source_artifacts(&target_packages_directory, &manifest.source_artifacts)?;
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Finding Replaced Packages",
        4,
        7,
    ));
    let obsolete_artifacts =
        find_obsolete_artifacts(&target_packages_directory, &manifest.artifacts)?;
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Checking Runtime Cache",
        5,
        7,
    ));
    let sunrise_build_cache = validate_sunrise_build_cache_with(
        &target_packages_directory,
        request.runtime_snapshot_check,
    )?;
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Checking Package Caches",
        6,
        7,
    ));
    let package_header_caches = validate_package_header_caches(&target_packages_directory)?;
    progress(InstallProgress::item(
        InstallPhase::Checking,
        "Package Checks Complete",
        7,
        7,
    ));
    let replacement_guard = replacement::review_request(
        request,
        &target_packages_directory,
        &staged_run_directory,
        &sunrise_build_cache.runtime,
        &manifest,
        progress,
    )
    .map_err(InstallError::validation)?;
    manifest
        .inputs
        .verify(&staged_run_directory, &target_packages_directory)?;
    let manifest = std::sync::Arc::unwrap_or_clone(manifest);
    Ok(ValidatedRun {
        replacement_guard,
        staged_run_directory,
        target_packages_directory,
        backup_root,
        source_artifacts: manifest.source_artifacts,
        artifacts: manifest.artifacts,
        merge_data: manifest.merge_data,
        map_diagnostics: manifest.map_diagnostics,
        mods: manifest.mods,
        obsolete_artifacts,
        selected_recipe_files: manifest.selected_recipe_files,
        staged_inputs: manifest.inputs,
        authored_unlocks: manifest.authored_unlocks,
        authored_grants: manifest.authored_grants,
        package_backup_retention: request.package_backup_retention,
        limit_package_backups: request.limit_package_backups,
        backup_recipe_snapshots: request.backup_recipe_snapshots,
        sunrise_build_cache,
        package_header_caches,
    })
}

pub(super) fn validate_request_paths(
    request: &InstallRequest,
) -> Result<(PathBuf, PathBuf, PathBuf), InstallError> {
    let staged_run_directory = canonical_directory(&request.staged_run_directory, "staging run")?;
    let target_packages_directory =
        canonical_packages_directory(&request.target_packages_directory)?;
    if path_is_within(&target_packages_directory, &staged_run_directory)
        || path_is_within(&staged_run_directory, &target_packages_directory)
    {
        return Err(InstallError::validation(
            "The staging and target packages directories cannot be equal or nested inside one another",
        ));
    }
    let backup_root = resolve_path_for_comparison(&request.backup_root).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve backup root {}: {error}",
            request.backup_root.display()
        ))
    })?;
    if path_is_within(&backup_root, &target_packages_directory) {
        return Err(InstallError::validation(
            "The backup root must be outside the target packages directory",
        ));
    }
    Ok((staged_run_directory, target_packages_directory, backup_root))
}

fn find_obsolete_artifacts(
    target: &Path,
    incoming: &[ArtifactMetadata],
) -> Result<Vec<ArtifactMetadata>, InstallError> {
    let mut obsolete = Vec::new();
    for profile in all_authored_packages_in(&[target]) {
        if profile.required_output
            || incoming
                .iter()
                .any(|artifact| artifact.file_name == profile.file_name)
        {
            continue;
        }
        // The chain validation has already checked the authoring signature and header.
        // Only known optional authored outputs may be retired, never stock packages.
        let path = target.join(profile.file_name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(InstallError::validation(error.to_string())),
            Ok(_) => {}
        }
        let digest =
            digest_file(&path).map_err(|error| InstallError::validation(error.to_string()))?;
        obsolete.push(ArtifactMetadata {
            file_name: profile.file_name.to_owned(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
        });
    }
    Ok(obsolete)
}

pub(super) fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, InstallError> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve {label} {}: {error}",
            path.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(InstallError::validation(format!(
            "The {label} path is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

pub(super) fn canonical_packages_directory(path: &Path) -> Result<PathBuf, InstallError> {
    sundial::package_authoring::resolve_packages_directory(path).map_err(InstallError::validation)
}

pub(super) fn validate_manifest_and_staged_files(
    staged_run_directory: &Path,
    target_packages_directory: &Path,
) -> Result<ValidatedManifest, InstallError> {
    let manifest_path = staged_run_directory.join(MANIFEST_FILE_NAME);
    reject_symlink(&manifest_path, "manifest")?;
    let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
        InstallError::validation(format!(
            "Could not open staged manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    let manifest: ManifestDocument =
        sundial::package_authoring::read_json(Cursor::new(&manifest_bytes)).map_err(|error| {
            InstallError::validation(format!(
                "Could not parse staged manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
    manifest.validate().map_err(InstallError::validation)?;
    validate_manifest_source(&manifest, target_packages_directory)?;
    let recipes = validate_staged_recipe_snapshots(staged_run_directory, &manifest)?;
    let mut files = recipes.files;
    files.insert(0, staged::metadata(MANIFEST_FILE_NAME, &manifest_bytes));
    let inputs = staged::Inputs::new(staged_run_directory, target_packages_directory, files)?;
    let authored_unlocks = validate_manifest_unlocks(&manifest.project)?;
    let authored_grants = manifest_grants(&manifest.project);
    let source_artifacts = validate_source_artifact_records(manifest.source_artifacts)?;
    let artifacts = validate_artifact_records(manifest.artifacts)?;
    check_layout(target_packages_directory, &artifacts)?;
    let selected_recipe_files = manifest.selected_recipe_files;
    if !manifest.mods.is_empty() && !uses_mod_loader(target_packages_directory) {
        return Err(InstallError::validation(
            "An importer mod family requires Sunrise 0.6 or later",
        ));
    }
    let staged_artifacts = staged::packages(&artifacts, &manifest.mods);
    validate_direct_package_set(staged_run_directory, &staged_artifacts)?;
    validate_staged_packages(
        staged_run_directory,
        &artifacts,
        target_packages_directory,
        &manifest.mods,
    )?;
    let merge_data = validate_merge_data(
        staged_run_directory,
        &artifacts,
        manifest.merge_data,
        target_packages_directory,
        &manifest.mods,
    )?;
    let map_diagnostics = validate_map_diagnostics(staged_run_directory, manifest.map_diagnostics)?;
    Ok(ValidatedManifest {
        inputs,
        source_artifacts,
        artifacts,
        merge_data,
        map_diagnostics,
        mods: manifest.mods,
        selected_recipe_files,
        authored_unlocks,
        authored_grants,
        plug_variants: recipes.variants,
    })
}

fn validate_map_diagnostics(
    directory: &Path,
    metadata: Option<ArtifactMetadata>,
) -> Result<Option<ArtifactMetadata>, InstallError> {
    use sundial::package_authoring::mod_content::{self, map_diagnostics};
    let Some(mut metadata) = metadata else {
        return Ok(None);
    };
    let path = directory.join(map_diagnostics::FILE_NAME);
    mod_content::checked_path(directory, &path).map_err(InstallError::validation)?;
    let length = fs::metadata(&path)
        .map_err(|error| InstallError::validation(error.to_string()))?
        .len();
    if length != metadata.byte_length || length > map_diagnostics::BYTE_LIMIT {
        return Err(InstallError::validation(
            "Map diagnostics size changed after staging",
        ));
    }
    let bytes = fs::read(&path).map_err(|error| InstallError::validation(error.to_string()))?;
    let digest = crate::artifact::digest_bytes(&bytes);
    if !digest.sha256.eq_ignore_ascii_case(&metadata.sha256) {
        return Err(InstallError::validation(
            "Map diagnostics changed after staging",
        ));
    }
    let catalog: map_diagnostics::Catalog = serde_json::from_slice(&bytes)
        .map_err(|error| InstallError::validation(format!("Invalid map diagnostics: {error}")))?;
    catalog.validate().map_err(InstallError::validation)?;
    metadata.sha256 = digest.sha256;
    Ok(Some(metadata))
}

/// Merge data matches its manifest digest and describes the package beside it, since Sunrise binds
/// it to that package's header and ignores it otherwise.
fn validate_merge_data(
    directory: &Path,
    artifacts: &[ArtifactMetadata],
    merge_data: Vec<ArtifactMetadata>,
    target: &Path,
    mods: &[crate::mod_family::Member],
) -> Result<Vec<ArtifactMetadata>, InstallError> {
    use sundial::package_authoring::mod_content::{merge::sidecar::Sidecar, plaintext};
    let mut validated = Vec::with_capacity(merge_data.len());
    for mut data in merge_data {
        validate_plain_file_name(&data.file_name)?;
        if !is_sha256(&data.sha256) {
            return Err(InstallError::validation(format!(
                "Merge data {} has an invalid SHA-256 digest",
                data.file_name
            )));
        }
        data.sha256.make_ascii_uppercase();
        validated.push(data);
    }
    for data in &validated {
        let path = crate::mod_family::source(mods, directory, target, &data.file_name)
            .map_err(InstallError::validation)?;
        loader::verify_file(&path, &TransactionDigest::from(data))?;
    }
    for data in &validated {
        let package = data
            .file_name
            .strip_suffix(".merge")
            .map(|stem| format!("{stem}.pkg"))
            .filter(|name| artifacts.iter().any(|artifact| artifact.file_name == *name))
            .ok_or_else(|| {
                InstallError::validation(format!(
                    "Merge data {} sits beside no staged package",
                    data.file_name
                ))
            })?;
        let package_path = crate::mod_family::source(mods, directory, target, &package)
            .map_err(InstallError::validation)?;
        let data_path = crate::mod_family::source(mods, directory, target, &data.file_name)
            .map_err(InstallError::validation)?;
        let records = plaintext::records(&package_path)
            .map_err(|reason| InstallError::validation(format!("{package} {}", reason.text())))?;
        let bytes = fs::read(data_path).map_err(|error| {
            InstallError::validation(format!("Could not read {}: {error}", data.file_name))
        })?;
        Sidecar::parse(&bytes, records.package, records.patch, &records.digest).map_err(
            |status| {
                InstallError::validation(format!(
                    "Merge data {} doesn't describe {package}: {}",
                    data.file_name,
                    status.text()
                ))
            },
        )?;
    }
    Ok(validated)
}

/// A build keeps its investment records where the runtime it was made for loads them: in a records
/// package of their own for Sunrise's mod loader, and appended to a stock package for Dawn and
/// Sunrise before 0.6, which aren't known to load a records package. The mod loader refuses the
/// appended layout when its patches are checked.
fn check_layout(target: &Path, artifacts: &[ArtifactMetadata]) -> Result<(), InstallError> {
    let mod_layout = artifacts
        .iter()
        .any(|artifact| crate::package_profile::is_records_package(&artifact.file_name));
    if mod_layout && !uses_mod_loader(target) {
        return Err(InstallError::validation(
            "This build was made for Sunrise 0.6's mod folder, which this runtime doesn't load. Build it again to install it here",
        ));
    }
    Ok(())
}

/// The authored items the install adds to the account: each subclass whose class the manifest
/// records, on the characters of that class, and a stack of each shader.
fn manifest_grants(project: &ManifestProject) -> Vec<identities::GrantedItem> {
    project
        .weapons
        .iter()
        .filter_map(|weapon| {
            let class_type = match weapon.kind {
                crate::ItemKind::Subclass => Some(weapon.class_type?),
                crate::ItemKind::Shader => None,
                _ => return None,
            };
            Some(identities::GrantedItem {
                item_hash: weapon.item.hash.get(),
                definition_tag: tiger_pkg::TagHash(weapon.item.definition_tag.get()),
                class_type,
                every_class: weapon.every_class,
            })
        })
        .collect()
}

pub(super) fn validate_manifest_unlocks(
    project: &ManifestProject,
) -> Result<Vec<AuthoredCollectionUnlock>, InstallError> {
    let mut definitions = BTreeSet::new();
    let mut slots = BTreeSet::new();
    let mut unlocks = Vec::with_capacity(project.weapons.len());
    // A subclass has no Collections entry, so it has no unlock to enroll.
    for (weapon, manifest_unlock) in project
        .weapons
        .iter()
        .filter_map(|weapon| Some((weapon, weapon.unlock.as_ref()?)))
    {
        if manifest_unlock.bank != ACCOUNT_UNLOCK_BANK {
            return Err(InstallError::validation(format!(
                "Manifest weapon {:?} uses unlock bank {}; the authored package profile requires bank {ACCOUNT_UNLOCK_BANK}",
                weapon.namespace, manifest_unlock.bank
            )));
        }
        let unlock = AuthoredCollectionUnlock {
            definition_index: manifest_unlock.definition_index,
            bank: manifest_unlock.bank,
            slot: manifest_unlock.slot,
        };
        if !definitions.insert(unlock.definition_index) {
            return Err(InstallError::validation(format!(
                "Manifest repeats authored unlock definition {}",
                unlock.definition_index
            )));
        }
        if !slots.insert((unlock.bank, unlock.slot)) {
            return Err(InstallError::validation(format!(
                "Manifest repeats authored unlock bank {}, slot {}",
                unlock.bank, unlock.slot
            )));
        }
        unlocks.push(unlock);
    }
    Ok(unlocks)
}

pub(super) fn validate_manifest_source(
    manifest: &ManifestDocument,
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    if manifest.source_package_directory.trim().is_empty() {
        return Err(InstallError::validation(
            "The manifest source package directory is empty",
        ));
    }
    let source = Path::new(&manifest.source_package_directory);
    let canonical_source = fs::canonicalize(source).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve manifest source package directory {}: {error}",
            source.display()
        ))
    })?;
    if !paths_equal(&canonical_source, target_packages_directory) {
        return Err(InstallError::validation(format!(
            "The staged build was authored from {}, not the selected target package directory {}",
            canonical_source.display(),
            target_packages_directory.display()
        )));
    }
    Ok(())
}

/// Checks each staged recipe against the manifest and returns the private copies of stock plugs
/// the recipes put in their items' sockets.
fn validate_staged_recipe_snapshots(
    staged_run_directory: &Path,
    manifest: &ManifestDocument,
) -> Result<ValidatedRecipes, InstallError> {
    let mut selection = SelectionFingerprint::default();
    let mut variants = Vec::new();
    let mut files = Vec::new();
    for (index, relative_path) in manifest.selected_recipe_files.iter().enumerate() {
        let path = staged_run_directory.join(relative_path);
        reject_symlink(&path, "staged recipe")?;
        let resolved = fs::canonicalize(&path).map_err(|error| {
            InstallError::validation(format!(
                "Could not resolve staged recipe {}: {error}",
                path.display()
            ))
        })?;
        if !path_is_within(&resolved, staged_run_directory) {
            return Err(InstallError::validation(format!(
                "Staged recipe resolves outside the staging run: {}",
                path.display()
            )));
        }
        // A staged snapshot is its recipe's canonical encoding and a newline, so the selection
        // is fingerprinted from the bytes read rather than by encoding each recipe again.
        let encoded = fs::read_to_string(&resolved).map_err(|error| {
            InstallError::validation(format!(
                "Could not read staged recipe {}: {error}",
                path.display()
            ))
        })?;
        let recipe = WeaponRecipe::from_json_str(&encoded).map_err(|error| {
            InstallError::validation(format!(
                "Could not validate staged recipe {}: {error}",
                path.display()
            ))
        })?;
        selection.add(encoded.strip_suffix('\n').unwrap_or(&encoded));
        files.push(staged::metadata(relative_path, encoded.as_bytes()));
        let weapon = &manifest.project.weapons[index];
        let spec = recipe.to_spec().map_err(|error| {
            InstallError::validation(format!(
                "Could not compile staged recipe {} for manifest validation: {error}",
                path.display()
            ))
        })?;
        let identity = spec.identity;
        let matches_manifest = weapon.namespace == recipe.namespace
            && weapon.name == recipe.name
            && weapon.item.hash.get() == identity.item_hash
            && weapon.collectible.is_none() == !spec.kind.has_collections()
            && weapon
                .collectible
                .as_ref()
                .is_none_or(|collectible| collectible.hash.get() == identity.collectible_hash)
            && weapon
                .unlock
                .as_ref()
                .is_none_or(|unlock| unlock.hash.get() == identity.unlock_hash)
            && weapon.donor.item_hash.get() == spec.donor_item_hash;
        if !matches_manifest {
            return Err(InstallError::validation(format!(
                "Manifest weapon {} does not match staged recipe {}",
                weapon.namespace,
                path.display()
            )));
        }
        // A mod's perk is the mod itself, not a copy placed in one of its sockets.
        let placed = if spec.kind == crate::ItemKind::Mod {
            &[][..]
        } else {
            spec.overrides.socket_plug_variants.as_slice()
        };
        variants.extend(placed.iter().map(|variant| replacement::StockPlugVariant {
            item_hash: identity.item_hash,
            lane: usize::from(variant.socket_index),
            source: variant.source_plug_hash,
        }));
    }

    for relative_path in &manifest.selected_map_files {
        let path = staged_run_directory.join(relative_path);
        reject_symlink(&path, "staged map recipe")?;
        let resolved = fs::canonicalize(&path).map_err(|error| {
            InstallError::validation(format!(
                "Could not resolve staged map {}: {error}",
                path.display()
            ))
        })?;
        if !path_is_within(&resolved, staged_run_directory) {
            return Err(InstallError::validation(format!(
                "Staged map resolves outside the staging run: {}",
                path.display()
            )));
        }
        let encoded = fs::read_to_string(&resolved).map_err(|error| {
            InstallError::validation(format!(
                "Could not read staged map {}: {error}",
                path.display()
            ))
        })?;
        let map: crate::map::MapRecipe = serde_json::from_str(&encoded).map_err(|error| {
            InstallError::validation(format!(
                "Could not parse staged map {}: {error}",
                path.display()
            ))
        })?;
        map.validate()
            .map_err(|error| InstallError::validation(error.to_string()))?;
        selection.add(encoded.strip_suffix('\n').unwrap_or(&encoded));
        files.push(staged::metadata(relative_path, encoded.as_bytes()));
    }
    if selection.finish() != manifest.selection_fingerprint {
        return Err(InstallError::validation(
            "The staged recipe selection does not match the manifest fingerprint",
        ));
    }
    Ok(ValidatedRecipes { variants, files })
}

struct ValidatedRecipes {
    variants: Vec<replacement::StockPlugVariant>,
    files: Vec<ArtifactMetadata>,
}

pub(super) fn validate_artifact_records(
    artifacts: Vec<ArtifactMetadata>,
) -> Result<Vec<ArtifactMetadata>, InstallError> {
    let mut by_name = BTreeMap::new();
    let mut case_folded_names = BTreeSet::new();
    for mut artifact in artifacts {
        validate_plain_file_name(&artifact.file_name)?;
        let folded = artifact.file_name.to_ascii_lowercase();
        if !case_folded_names.insert(folded) {
            return Err(InstallError::validation(format!(
                "The manifest contains duplicate artifact name {:?}",
                artifact.file_name
            )));
        }
        if !is_sha256(&artifact.sha256) {
            return Err(InstallError::validation(format!(
                "Artifact {} has an invalid SHA-256 digest",
                artifact.file_name
            )));
        }
        artifact.sha256.make_ascii_uppercase();
        by_name.insert(artifact.file_name.clone(), artifact);
    }

    let profiles =
        authored_packages_for_file_names(by_name.keys().map(String::as_str)).map_err(|error| {
            InstallError::validation(format!("The manifest artifact set is invalid: {error}"))
        })?;
    profiles
        .into_iter()
        .map(|profile| {
            by_name.remove(profile.file_name).ok_or_else(|| {
                InstallError::validation(format!(
                    "The manifest is missing authored package {}",
                    profile.file_name
                ))
            })
        })
        .collect()
}

pub(super) fn parse_canonical_source_artifact_file_name(file_name: &str) -> Option<(u16, u16)> {
    for profile in CANONICAL_PACKAGES {
        for patch_id in 0..=profile.stock_patch_id {
            if file_name == profile.stock_file_name(patch_id) {
                return Some((profile.package_id, patch_id));
            }
        }
    }
    None
}

pub(super) fn validate_source_artifact_records(
    artifacts: Vec<ArtifactMetadata>,
) -> Result<Vec<ArtifactMetadata>, InstallError> {
    let mut by_name = BTreeMap::new();
    let mut case_folded_names = BTreeSet::new();
    let mut packages_with_latest = BTreeSet::new();
    for mut artifact in artifacts {
        validate_plain_file_name(&artifact.file_name)?;
        let Some((package_id, patch_id)) =
            parse_canonical_source_artifact_file_name(&artifact.file_name)
        else {
            return Err(InstallError::validation(format!(
                "Source artifact {} is not a canonical affected stock package generation",
                artifact.file_name
            )));
        };
        let folded = artifact.file_name.to_ascii_lowercase();
        if !case_folded_names.insert(folded) {
            return Err(InstallError::validation(format!(
                "The manifest contains duplicate source artifact name {:?}",
                artifact.file_name
            )));
        }
        if !is_sha256(&artifact.sha256) {
            return Err(InstallError::validation(format!(
                "Source artifact {} has an invalid SHA-256 digest",
                artifact.file_name
            )));
        }
        artifact.sha256.make_ascii_uppercase();
        if canonical_package(package_id).is_some_and(|profile| patch_id == profile.stock_patch_id) {
            packages_with_latest.insert(package_id);
        }
        by_name.insert(artifact.file_name.clone(), artifact);
    }
    if packages_with_latest != CANONICAL_PACKAGE_IDS.into_iter().collect() {
        return Err(InstallError::validation(
            "The manifest must include the required latest stock patch for every affected package id"
                .to_owned(),
        ));
    }
    Ok(by_name.into_values().collect())
}

pub(super) fn validate_plain_file_name(file_name: &str) -> Result<(), InstallError> {
    let components = Path::new(file_name).components().collect::<Vec<_>>();
    let is_plain = components.len() == 1
        && matches!(components[0], Component::Normal(_))
        && !file_name.contains('/')
        && !file_name.contains('\\')
        && !file_name.contains('\0');
    if !is_plain {
        return Err(InstallError::validation(format!(
            "Manifest artifact name {file_name:?} is not a plain file name",
        )));
    }
    Ok(())
}

pub(super) fn validate_direct_package_set(
    staged_run_directory: &Path,
    artifacts: &[ArtifactMetadata],
) -> Result<(), InstallError> {
    let mut package_names = Vec::new();
    let entries = fs::read_dir(staged_run_directory).map_err(|error| {
        InstallError::validation(format!(
            "Could not inspect staging run {}: {error}",
            staged_run_directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            InstallError::validation(format!("Could not inspect a staged entry: {error}"))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if has_pkg_extension(Path::new(&name)) {
            package_names.push(name);
        }
    }
    package_names.sort();
    let mut expected_names = artifacts
        .iter()
        .map(|artifact| artifact.file_name.as_str())
        .collect::<Vec<_>>();
    expected_names.sort_unstable();
    if package_names.iter().map(String::as_str).collect::<Vec<_>>() != expected_names {
        return Err(InstallError::validation(format!(
            "The staging directory package files do not match the recipe-selected manifest set. Found {}",
            package_names.join(", ")
        )));
    }
    Ok(())
}

pub(super) fn verify_source_artifacts(
    target_packages_directory: &Path,
    artifacts: &[ArtifactMetadata],
) -> Result<(), InstallError> {
    let expected_names = artifacts
        .iter()
        .map(|artifact| artifact.file_name.clone())
        .collect::<BTreeSet<_>>();
    let actual_names = discover_target_source_artifact_names(target_packages_directory)?;
    if actual_names != expected_names {
        let missing = expected_names
            .difference(&actual_names)
            .cloned()
            .collect::<Vec<_>>();
        let added = actual_names
            .difference(&expected_names)
            .cloned()
            .collect::<Vec<_>>();
        return Err(InstallError::validation(format!(
            "The affected stock package-chain set changed since this staged build was authored (missing: {}; added: {})",
            if missing.is_empty() {
                "none".to_owned()
            } else {
                missing.join(", ")
            },
            if added.is_empty() {
                "none".to_owned()
            } else {
                added.join(", ")
            }
        )));
    }
    for artifact in artifacts {
        let path = target_packages_directory.join(&artifact.file_name);
        reject_symlink(&path, "stock source artifact")?;
        let digest = digest_file(&path).map_err(|error| {
            InstallError::validation(format!(
                "Could not verify stock source artifact {}: {error}",
                path.display()
            ))
        })?;
        if digest.byte_length != artifact.byte_length || digest.sha256 != artifact.sha256 {
            return Err(InstallError::validation(format!(
                "Stock source artifact {} changed since this staged build was authored",
                artifact.file_name
            )));
        }
    }
    Ok(())
}

pub(super) fn discover_target_source_artifact_names(
    target_packages_directory: &Path,
) -> Result<BTreeSet<String>, InstallError> {
    let mut names = BTreeSet::new();
    for profile in CANONICAL_PACKAGES {
        for patch_id in 0..=profile.stock_patch_id {
            let file_name = profile.stock_file_name(patch_id);
            let path = target_packages_directory.join(&file_name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(InstallError::validation(format!(
                        "Could not inspect stock source artifact {}: {error}",
                        path.display()
                    )));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(InstallError::validation(format!(
                    "The stock source artifact must be a regular file: {}",
                    path.display()
                )));
            }
            let header = read_package_header(&path).map_err(|error| {
                InstallError::validation(format!(
                    "Could not read stock source artifact header {}: {error}",
                    path.display()
                ))
            })?;
            validate_header(&path, header, profile.package_id, patch_id, None)?;
            if header.build_signature == SUNDIAL_BUILD_SIGNATURE {
                return Err(InstallError::validation(format!(
                    "Stock source artifact unexpectedly has Parhelion's build signature: {}",
                    path.display()
                )));
            }
            names.insert(file_name);
        }
    }
    Ok(names)
}

/// Checks each staged package against its manifest length and SHA-256 and validates its
/// structure, from one read of the file. A build's packages run to gigabytes, so they are checked
/// on several threads, a few files in memory at a time. The first failure in manifest order is
/// returned.
pub(super) fn validate_staged_packages(
    staged_run_directory: &Path,
    artifacts: &[ArtifactMetadata],
    target_packages_directory: &Path,
    mods: &[crate::mod_family::Member],
) -> Result<(), InstallError> {
    let profiles = authored_packages_for_file_names(
        artifacts.iter().map(|artifact| artifact.file_name.as_str()),
    )
    .map_err(InstallError::validation)?;
    let jobs = profiles
        .into_iter()
        .map(|profile| {
            artifacts
                .iter()
                .find(|artifact| artifact.file_name == profile.file_name)
                .map(|artifact| (profile, artifact))
                .ok_or_else(|| {
                    InstallError::validation(format!(
                        "Staged package {} has no manifest record",
                        profile.file_name
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let check = |(profile, artifact): &(AuthoredPackage, &ArtifactMetadata)| {
        let path = crate::mod_family::source(
            mods,
            staged_run_directory,
            target_packages_directory,
            profile.file_name,
        )
        .map_err(InstallError::validation)?;
        reject_symlink(&path, "staged artifact")?;
        let bytes = fs::read(&path).map_err(|error| {
            InstallError::validation(format!(
                "Could not verify staged artifact {}: {error}",
                path.display()
            ))
        })?;
        let digest = crate::artifact::digest_bytes(&bytes);
        if digest.byte_length != artifact.byte_length || digest.sha256 != artifact.sha256 {
            return Err(InstallError::validation(format!(
                "Staged artifact {} does not match its manifest length and SHA-256",
                artifact.file_name
            )));
        }
        validate_authored_package_bytes(&path, bytes, *profile, target_packages_directory)
    };
    // Each worker holds one whole package, and an imported map's run to hundreds of megabytes.
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 4)
        .min(jobs.len());
    if workers <= 1 {
        return jobs.iter().try_for_each(check);
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else {
                            return done;
                        };
                        done.push((index, check(job)));
                    }
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a staged package check panicked"))
            .collect::<Vec<_>>()
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().try_for_each(|(_, result)| result)
}

pub(super) fn validate_authored_package_file(
    path: &Path,
    profile: AuthoredPackage,
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    let bytes = fs::read(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not read authored package {} for structural validation: {error}",
            path.display()
        ))
    })?;
    validate_authored_package_bytes(path, bytes, profile, target_packages_directory)
}

/// Validates `bytes`, the contents of authored package `path`, against `profile`.
pub(super) fn validate_authored_package_bytes(
    path: &Path,
    bytes: Vec<u8>,
    profile: AuthoredPackage,
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    let header = read_package_header(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not read authored package header {}: {error}",
            path.display()
        ))
    })?;
    validate_header(
        path,
        header,
        profile.package_id,
        profile.patch_id,
        Some(SUNDIAL_BUILD_SIGNATURE),
    )?;
    let layout = PackageLayout::parse(&bytes).map_err(|error| {
        InstallError::validation(format!(
            "Authored package {} failed structural validation: {error}",
            path.display()
        ))
    })?;
    if layout.package_id != profile.package_id || layout.patch_id != profile.patch_id {
        return Err(InstallError::validation(format!(
            "Authored package {} has structural identity {:04X}/patch {}; expected {:04X}/patch {}",
            path.display(),
            layout.package_id,
            layout.patch_id,
            profile.package_id,
            profile.patch_id,
        )));
    }

    let compressed = layout
        .has_compressed_blocks(&bytes)
        .map_err(|error| InstallError::validation(error.to_string()))?;
    let package =
        PackageD2PreBL::from_reader(profile.file_name, Cursor::new(bytes)).map_err(|error| {
            InstallError::validation(format!(
                "Authored package {} could not be reopened by tiger-pkg: {error}",
                path.display()
            ))
        })?;
    if package.pkg_id() != profile.package_id
        || package.patch_id() != profile.patch_id
        || package.header.group_id != SUNDIAL_BUILD_SIGNATURE
    {
        return Err(InstallError::validation(format!(
            "Authored package {} reopened with identity {:04X}/patch {}/signature 0x{:016X}; expected {:04X}/patch {}/signature 0x{SUNDIAL_BUILD_SIGNATURE:016X}",
            path.display(),
            package.pkg_id(),
            package.patch_id(),
            package.header.group_id,
            profile.package_id,
            profile.patch_id,
        )));
    }
    let runtime_map_tag =
        tiger_pkg::TagHash(sundial::package_authoring::sandbox_perk::SANDBOX_PERK_RUNTIME_MAP_TAG);
    if profile.package_id == runtime_map_tag.pkg_id() {
        let index = usize::from(runtime_map_tag.entry_index());
        if let Some(entry) = package.entries().get(index) {
            let expected = sundial::package_authoring::sandbox_perk::SANDBOX_PERK_RUNTIME_MAP_CLASS;
            if entry.reference != expected || entry.file_type != 8 {
                return Err(InstallError::validation(format!(
                    "Authored runtime map {runtime_map_tag} has an unexpected entry type"
                )));
            }
            if compressed {
                decoder::ensure_initialized(target_packages_directory)
                    .map_err(InstallError::validation)?;
            }
            let payload = package.read_entry(index).map_err(|error| {
                InstallError::validation(format!(
                    "Could not read authored runtime map {runtime_map_tag}: {error}"
                ))
            })?;
            sundial::package_authoring::sandbox_perk::validate_sandbox_perk_runtime_map(&payload)
                .map_err(|error| {
                InstallError::validation(format!(
                    "Authored runtime map {runtime_map_tag} failed payload validation: {error}"
                ))
            })?;
        }
    }
    Ok(())
}

pub(super) fn validate_target_package_chain(
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    for profile in CANONICAL_PACKAGES {
        let stock_path =
            target_packages_directory.join(profile.stock_file_name(profile.stock_patch_id));
        reject_symlink(&stock_path, "stock chain package")?;
        let stock_header = read_package_header(&stock_path).map_err(|error| {
            InstallError::validation(format!(
                "Could not read stock chain package {}: {error}",
                stock_path.display()
            ))
        })?;
        validate_header(
            &stock_path,
            stock_header,
            profile.package_id,
            profile.stock_patch_id,
            None,
        )?;
        if stock_header.build_signature == SUNDIAL_BUILD_SIGNATURE {
            return Err(InstallError::validation(format!(
                "Stock chain package unexpectedly has Parhelion's build signature: {}",
                stock_path.display()
            )));
        }
    }
    for profile in all_authored_packages_in(&[target_packages_directory]) {
        validate_existing_authored_target(
            &target_packages_directory.join(profile.file_name),
            profile.package_id,
            profile.patch_id,
        )?;
    }
    validate_no_alias_package_chains(target_packages_directory)?;
    Ok(())
}

pub(super) fn validate_no_alias_package_chains(
    target_packages_directory: &Path,
) -> Result<(), InstallError> {
    let entries = fs::read_dir(target_packages_directory).map_err(|error| {
        InstallError::validation(format!(
            "Could not scan target package directory {}: {error}",
            target_packages_directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            InstallError::validation(format!("Could not inspect a target package: {error}"))
        })?;
        let path = entry.path();
        if !has_pkg_extension(&path) {
            continue;
        }
        let header = read_package_header(&path).map_err(|error| {
            InstallError::validation(format!(
                "Could not inspect target package header {}: {error}",
                path.display()
            ))
        })?;
        let actual_name = entry.file_name().to_string_lossy().into_owned();
        // A standalone package may carry any family in its name, as a map's packages carry the
        // map's, so its name must only be a standalone name for its own id.
        let named = authored_package_for_file_name(&actual_name)
            .filter(|profile| profile.package_id == header.package_id);
        if let Some(profile) = authored_package(header.package_id)
            && !profile.stock_overlay
        {
            let profile = named.unwrap_or(profile);
            if actual_name != profile.file_name {
                return Err(InstallError::validation(format!(
                    "Unexpected Parhelion asset package {} aliases {}; remove it before installation",
                    path.display(),
                    profile.file_name
                )));
            }
            validate_header(
                &path,
                header,
                profile.package_id,
                profile.patch_id,
                Some(SUNDIAL_BUILD_SIGNATURE),
            )?;
            continue;
        }
        let Some(profile) = canonical_package(header.package_id) else {
            if header.build_signature == SUNDIAL_BUILD_SIGNATURE {
                return Err(InstallError::validation(format!(
                    "Unexpected package {} uses Parhelion's reserved build signature",
                    path.display()
                )));
            }
            continue;
        };
        let expected_name = if header.patch_id <= profile.stock_patch_id {
            profile.stock_file_name(header.patch_id)
        } else if header.patch_id == profile.authored_patch_id {
            profile.authored_file_name.to_owned()
        } else {
            return Err(InstallError::validation(format!(
                "Affected package {} uses unsupported patch {}; Parhelion installs patch {}",
                path.display(),
                header.patch_id,
                profile.authored_patch_id
            )));
        };
        if actual_name != expected_name {
            return Err(InstallError::validation(format!(
                "Unexpected affected package generation {} aliases {}; remove it before installation",
                path.display(),
                expected_name
            )));
        }
        let expected_signature =
            (header.patch_id == profile.authored_patch_id).then_some(SUNDIAL_BUILD_SIGNATURE);
        validate_header(
            &path,
            header,
            profile.package_id,
            header.patch_id,
            expected_signature,
        )?;
        if header.patch_id <= profile.stock_patch_id
            && header.build_signature == SUNDIAL_BUILD_SIGNATURE
        {
            return Err(InstallError::validation(format!(
                "Stock package generation unexpectedly has Parhelion's build signature: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_existing_authored_target(
    path: &Path,
    package_id: u16,
    patch_id: u16,
) -> Result<(), InstallError> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(InstallError::validation(format!(
                "Could not inspect existing authored package {}: {error}",
                path.display()
            )));
        }
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallError::validation(format!(
            "Existing authored target is not a regular file: {}",
            path.display()
        )));
    }
    let header = read_package_header(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not read existing authored package {}: {error}",
            path.display()
        ))
    })?;
    validate_header(
        path,
        header,
        package_id,
        patch_id,
        Some(SUNDIAL_BUILD_SIGNATURE),
    )
}

pub(super) fn read_package_header(path: &Path) -> io::Result<PackageHeaderPrefix> {
    let mut prefix = [0u8; PACKAGE_HEADER_PREFIX_SIZE];
    File::open(path)?.read_exact(&mut prefix)?;
    Ok(PackageHeaderPrefix::parse(&prefix))
}

pub(super) fn validate_header(
    path: &Path,
    header: PackageHeaderPrefix,
    package_id: u16,
    patch_id: u16,
    build_signature: Option<u64>,
) -> Result<(), InstallError> {
    let signature_matches =
        build_signature.is_none_or(|expected| header.build_signature == expected);
    if header.version != SHADOWKEEP_HEADER_VERSION
        || header.package_id != package_id
        || header.patch_id != patch_id
        || !signature_matches
    {
        let expected_signature = build_signature.map_or_else(
            || "any stock signature".to_owned(),
            |signature| format!("0x{signature:016X}"),
        );
        return Err(InstallError::validation(format!(
            "Package {} has header version {}/{:04X}/patch {}/signature 0x{:016X}; expected {SHADOWKEEP_HEADER_VERSION}/{package_id:04X}/patch {patch_id}/signature {expected_signature}",
            path.display(),
            header.version,
            header.package_id,
            header.patch_id,
            header.build_signature,
        )));
    }
    Ok(())
}

pub(super) fn reject_symlink(path: &Path, label: &str) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        InstallError::validation(format!(
            "Could not inspect {label} {}: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallError::validation(format!(
            "The {label} must be a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}
