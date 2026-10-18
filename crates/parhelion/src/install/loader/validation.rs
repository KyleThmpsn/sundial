use super::*;

pub(super) fn validate_packages(
    target: &Path,
    sources: &[(ArtifactMetadata, PathBuf)],
) -> Result<(), InstallError> {
    authored_packages_for_file_names(sources.iter().map(|(a, _)| a.file_name.as_str()))
        .map_err(InstallError::validation)?;
    for (artifact, path) in sources {
        let profile = authored_package_for_file_name(&artifact.file_name)
            .ok_or_else(|| InstallError::validation("Unknown managed package"))?;
        mod_content::checked_path(
            path.parent()
                .ok_or_else(|| InstallError::validation("Package has no parent"))?,
            path,
        )
        .map_err(InstallError::validation)?;
        validate_authored_package_file(path, profile, target)?;
        verify_file(path, &TransactionDigest::from(artifact))?;
        let bytes = fs::read(path).map_err(failure)?;
        let layout = PackageLayout::parse(&bytes).map_err(failure)?;
        verify_block_sources(target, path, &layout, &bytes)?;
        if let Some(stock) = canonical_package(profile.package_id) {
            if profile.patch_id != stock.stock_patch_id + 1 {
                return Err(InstallError::validation(
                    "A loader patch must immediately follow its stock generation",
                ));
            }
            let baseline = fs::read(target.join(stock.stock_file_name(stock.stock_patch_id)))
                .map_err(failure)?;
            let original = PackageLayout::parse(&baseline).map_err(failure)?;
            // Sunrise merges mod patches entry by entry and refuses one whose entry table
            // differs in length from its base.
            if layout.entry_count != original.entry_count {
                return Err(InstallError::validation(format!(
                    "{} has {} entries where its stock generation has {}. Sunrise refuses a patch that adds entries, so build again for this installation",
                    artifact.file_name, layout.entry_count, original.entry_count
                )));
            }
            let size = original
                .block_count
                .checked_mul(crate::format::BLOCK_HEADER_SIZE)
                .ok_or_else(|| InstallError::validation("Block table length overflow"))?;
            if layout.block_count < original.block_count
                || bytes.get(layout.block_table_offset..layout.block_table_offset + size)
                    != baseline.get(original.block_table_offset..original.block_table_offset + size)
            {
                return Err(InstallError::validation(format!(
                    "{} changes the inherited stock block table",
                    artifact.file_name
                )));
            }
        } else if layout.patch_id != 0 {
            return Err(InstallError::validation(
                "A new managed package must begin at generation zero",
            ));
        }
    }
    Ok(())
}

/// Refuses packages Sunrise would not load beside what is installed. That is a new package whose
/// id an installed file already uses, or a patch when the installation holds a newer generation
/// than the stock one it extends. Only new content is checked, so a conflicting file another tool
/// adds later never blocks removing the installed mod. The `retired` packages an earlier layout
/// left are gone before Sunrise next loads, so they never conflict.
pub(super) fn reject_installed_conflicts(
    target: &Path,
    sources: &[(ArtifactMetadata, PathBuf)],
    retired: &[ArtifactMetadata],
) -> Result<(), InstallError> {
    let mut installed = installed_generations(target)?;
    for files in installed.values_mut() {
        files.retain(|(name, _)| !retired.iter().any(|old| old.file_name == *name));
    }
    for (artifact, _) in sources {
        let profile = authored_package_for_file_name(&artifact.file_name)
            .ok_or_else(|| InstallError::validation("Unknown managed package"))?;
        // Parhelion's own file of the same name is the legacy layout, which the upgrade owns.
        let mut others = installed
            .get(&profile.package_id)
            .into_iter()
            .flatten()
            .filter(|(name, _)| *name != artifact.file_name);
        match canonical_package(profile.package_id) {
            Some(stock) => {
                if let Some((name, _)) =
                    others.find(|(_, generation)| u16::from(*generation) > stock.stock_patch_id)
                {
                    return Err(InstallError::validation(format!(
                        "{name} is newer than the stock generation {} patches. Sunrise loads a patch only as the next generation of the installed newest",
                        artifact.file_name
                    )));
                }
            }
            None => {
                if let Some((name, _)) = others.next() {
                    return Err(InstallError::validation(format!(
                        "{name} already uses package id {:04X}. Build again to choose free package ids",
                        profile.package_id
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Another mod in the loader's folder, as installation reads it.
struct OtherMod {
    id: String,
    incompatible: bool,
    packages: Vec<PathBuf>,
    patches: Vec<PathBuf>,
}

/// A missing package folder is empty. An unreadable folder is not evidence that its IDs are free.
fn mod_packages(install: &Path, directory: &Path) -> Result<Vec<PathBuf>, InstallError> {
    mod_content::checked_path(install, directory).map_err(InstallError::validation)?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(InstallError::validation(format!(
                "Could not inspect mod packages at {}: {error}",
                directory.display()
            )));
        }
    };
    let mut packages = Vec::new();
    for entry in entries {
        let entry = entry.map_err(failure)?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
        {
            continue;
        }
        mod_content::checked_path(install, &path).map_err(InstallError::validation)?;
        if !entry.file_type().map_err(failure)?.is_file() {
            return Err(InstallError::validation(format!(
                "Mod package is not a regular file: {}",
                path.display()
            )));
        }
        packages.push(path);
    }
    packages.sort();
    Ok(packages)
}

fn other_mods(install: &Path) -> Result<Vec<OtherMod>, InstallError> {
    let mods = mod_content::runtime_directory(install).join("mods");
    let entries = match fs::read_dir(&mods) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(failure(error)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(failure)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with('.')
            || name == mod_content::ID
            || !entry.file_type().map_err(failure)?.is_dir()
        {
            continue;
        }
        mod_content::checked_path(install, &entry.path()).map_err(InstallError::validation)?;
        let manifest_path = entry.path().join(mod_content::MANIFEST);
        mod_content::checked_path(install, &manifest_path).map_err(InstallError::validation)?;
        let manifest = mod_content::read_json::<serde_json::Value>(&manifest_path).ok();
        let field = |key: &str| manifest.as_ref().and_then(|value| value.get(key));
        let incompatible = field("incompatible")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(mod_content::ID)));
        let id = field("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&name)
            .to_owned();
        found.push(OtherMod {
            id,
            incompatible,
            packages: mod_packages(install, &entry.path().join("packages"))?,
            patches: mod_packages(install, &entry.path().join("patches"))?,
        });
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

fn package_id(path: &Path) -> Option<u16> {
    let parsed = tiger_pkg::manager::PackagePath::parse(path.to_str()?)?;
    u16::from_str_radix(&parsed.id, 16).ok()
}

/// Another mod may change the entries Parhelion changes only where Sunrise merges them intact:
/// both mods describe the entries in merge data, no stock bytes they both change keep only one
/// mod's value, and Parhelion's own rows keep their numbers. The loader is simulated with this
/// build in place of the installed one, so a refusal comes before any content changes. Returns
/// the shared entries Sunrise merges.
pub(super) fn check_other_mods(
    target: &Path,
    sources: &[(ArtifactMetadata, PathBuf)],
    retired: &[ArtifactMetadata],
) -> Result<Vec<String>, InstallError> {
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    let others = other_mods(install)?;
    if let Some(other) = others.iter().find(|other| other.incompatible) {
        return Err(InstallError::validation(format!(
            "Mod {} declares Parhelion incompatible. Installing this build would cause Sunrise to refuse that mod. Resolve the incompatibility before installing",
            other.id
        )));
    }
    let total = sources.len()
        + others
            .iter()
            .map(|other| other.packages.len() + other.patches.len())
            .sum::<usize>();
    if total > 1024 {
        return Err(InstallError::validation(format!(
            "The installed mods and this build would add {total} package files. Sunrise loads at most 1,024 from all mods together"
        )));
    }
    let mut incoming = mod_content::Incoming {
        retired: retired.iter().map(|old| old.file_name.clone()).collect(),
        ..mod_content::Incoming::default()
    };
    for (artifact, path) in sources {
        let profile = authored_package_for_file_name(&artifact.file_name)
            .ok_or_else(|| InstallError::validation("Unknown managed package"))?;
        if canonical_package(profile.package_id).is_some() {
            incoming.patches.push(path.clone());
            continue;
        }
        if let Some(other) = others.iter().find(|other| {
            other
                .packages
                .iter()
                .any(|path| package_id(path) == Some(profile.package_id))
        }) {
            return Err(InstallError::validation(format!(
                "Mod {} already adds package {:04X}. Build again to choose free package ids",
                other.id, profile.package_id
            )));
        }
        incoming.packages.push(path.clone());
    }
    let coexistence =
        mod_content::coexistence(install, &incoming).map_err(InstallError::validation)?;
    if let Some(problem) = coexistence.problems.first() {
        return Err(InstallError::validation(problem.trim_end_matches('.')));
    }
    Ok(coexistence.merged)
}

/// The authored ids a build for this installation may use. On Sunrise's mod loader they leave
/// out every id an installed package or another mod's new package holds, since Sunrise refuses
/// a mod package whose id is taken. Parhelion's own files never count, so a build keeps its ids
/// while nothing else claims them.
pub(crate) fn package_window(
    target: &Path,
) -> Result<crate::package_profile::Window, InstallError> {
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    if !uses_mod_loader(target) {
        return Ok(crate::package_profile::Window::FULL);
    }
    let mut taken = installed_generations(target)?
        .into_iter()
        .filter(|(_, files)| {
            files
                .iter()
                .any(|(name, _)| authored_package_for_file_name(name).is_none())
        })
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    for other in other_mods(install)? {
        taken.extend(other.packages.iter().filter_map(|path| package_id(path)));
    }
    crate::package_profile::Window::avoiding(&taken).ok_or_else(|| {
        InstallError::validation(format!(
            "Installed packages and other mods hold every package id from {:04X} to {:04X}",
            crate::package_profile::MIN_AUTHORED_STANDALONE_PACKAGE_ID,
            crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID,
        ))
    })
}

/// Whether the installation's runtime loads Sunrise mods. Installation reads the runtime again
/// and refuses one it cannot read.
pub(crate) fn uses_mod_loader(target: &Path) -> bool {
    target
        .parent()
        .and_then(|install| sundial::package_authoring::installed_runtime(install).ok())
        .and_then(|runtime| runtime.uses_mod_loader().ok())
        .unwrap_or(false)
}

/// The installation's own package files by id, with their generation.
fn installed_generations(target: &Path) -> Result<BTreeMap<u16, Vec<(String, u8)>>, InstallError> {
    let mut installed = BTreeMap::<u16, Vec<(String, u8)>>::new();
    for entry in fs::read_dir(target).map_err(failure)? {
        let name = entry.map_err(failure)?.file_name();
        // Startup packages such as `_unp1_` carry no hexadecimal id and are never authored.
        let Some(parsed) = name
            .to_str()
            .filter(|name| name.ends_with(".pkg"))
            .and_then(tiger_pkg::manager::PackagePath::parse)
        else {
            continue;
        };
        if let Ok(id) = u16::from_str_radix(&parsed.id, 16) {
            installed
                .entry(id)
                .or_default()
                .push((parsed.filename, parsed.patch));
        }
    }
    Ok(installed)
}

fn verify_block_sources(
    target: &Path,
    authored: &Path,
    layout: &PackageLayout,
    bytes: &[u8],
) -> Result<(), InstallError> {
    let mut lengths = BTreeMap::new();
    for index in 0..layout.block_count {
        let row = layout.block_table_offset + index * crate::format::BLOCK_HEADER_SIZE;
        let offset = u32::from_le_bytes(bytes[row..row + 4].try_into().unwrap());
        let size = u32::from_le_bytes(bytes[row + 4..row + 8].try_into().unwrap());
        let patch = u16::from_le_bytes(bytes[row + 8..row + 10].try_into().unwrap());
        if size == 0 {
            continue;
        }
        let length = match lengths.entry(patch) {
            std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let path = if patch == layout.patch_id {
                    authored.to_path_buf()
                } else {
                    let profile = canonical_package(layout.package_id).ok_or_else(|| {
                        InstallError::validation(
                            "Standalone package references an unavailable predecessor",
                        )
                    })?;
                    target.join(profile.stock_file_name(patch))
                };
                reject_symlink(&path, "block source package")?;
                *entry.insert(fs::metadata(&path).map_err(failure)?.len())
            }
        };
        if u64::from(offset) + u64::from(size) > length {
            return Err(InstallError::validation(format!(
                "Package {:04X} block {index} extends beyond generation {patch}",
                layout.package_id
            )));
        }
    }
    Ok(())
}

/// The mod folder's manifest and receipt, and at most 1,024 packages, each with its merge data.
pub(super) const FOLDER_FILE_LIMIT: usize = 2 + 2 * 1024;

/// A package's path in the mod folder, or the path of the merge data beside it.
pub(super) fn package_path(artifact: &ArtifactMetadata) -> Result<String, InstallError> {
    let package = match artifact.file_name.strip_suffix(".merge") {
        Some(stem) => format!("{stem}.pkg"),
        None => artifact.file_name.clone(),
    };
    let profile = authored_package_for_file_name(&package)
        .ok_or_else(|| InstallError::validation("Unknown managed package"))?;
    Ok(format!(
        "{}/{}",
        if canonical_package(profile.package_id).is_some() {
            "patches"
        } else {
            "packages"
        },
        artifact.file_name
    ))
}

pub(super) fn managed(target: &Path) -> Result<Option<Folder>, InstallError> {
    let install = target
        .parent()
        .ok_or_else(|| InstallError::validation("Missing game root"))?;
    mod_content::ensure_readable(install).map_err(InstallError::validation)?;
    let directory = mod_content::directory(install);
    if !directory.try_exists().map_err(failure)? {
        return Ok(None);
    }
    mod_content::files(install).map_err(InstallError::validation)?;
    let receipt: mod_content::Receipt =
        mod_content::read_json(&directory.join(mod_content::RECEIPT))
            .map_err(InstallError::validation)?;
    let sources = receipt
        .artifacts
        .iter()
        .filter(|a| a.path.ends_with(".pkg"))
        .map(|a| {
            let path = directory.join(&a.path);
            let artifact = ArtifactMetadata {
                file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
                byte_length: a.byte_length,
                sha256: a.sha256.clone(),
            };
            (artifact, path)
        })
        .collect::<Vec<_>>();
    validate_packages(target, &sources)?;
    Ok(Some(snapshot_folder(&directory)?))
}

pub(super) fn artifacts(folder: &Folder) -> Vec<ArtifactMetadata> {
    folder
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".pkg"))
        .map(|(name, digest)| ArtifactMetadata {
            file_name: Path::new(name)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            byte_length: digest.byte_length,
            sha256: digest.sha256.clone(),
        })
        .collect()
}

pub(super) fn snapshot_folder(folder: &Path) -> Result<Folder, InstallError> {
    mod_content::checked_path(
        folder
            .parent()
            .ok_or_else(|| InstallError::validation("Mod folder has no parent"))?,
        folder,
    )
    .map_err(InstallError::validation)?;
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(folder).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        mod_content::checked_path(folder, &entry.path()).map_err(InstallError::validation)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| InstallError::validation("Non-Unicode mod filename"))?;
        if matches!(name.as_str(), mod_content::MANIFEST | mod_content::RECEIPT)
            && entry.file_type().map_err(failure)?.is_file()
        {
            files.insert(
                name,
                TransactionDigest::from(&digest_file(&entry.path()).map_err(failure)?),
            );
        } else if matches!(name.as_str(), "packages" | "patches")
            && entry.file_type().map_err(failure)?.is_dir()
        {
            for package in fs::read_dir(entry.path()).map_err(failure)? {
                let package = package.map_err(failure)?;
                mod_content::checked_path(folder, &package.path())
                    .map_err(InstallError::validation)?;
                if !package.file_type().map_err(failure)?.is_file() {
                    return Err(InstallError::validation("Unexpected managed mod content"));
                }
                let file_name = package
                    .file_name()
                    .into_string()
                    .map_err(|_| InstallError::validation("Non-Unicode package filename"))?;
                files.insert(
                    format!("{name}/{file_name}"),
                    TransactionDigest::from(&digest_file(&package.path()).map_err(failure)?),
                );
            }
        } else {
            return Err(InstallError::validation(format!(
                "Unrecognized content in the managed mod: {name}"
            )));
        }
    }
    if files.len() > FOLDER_FILE_LIMIT {
        return Err(InstallError::validation(
            "Managed mod exceeds its file limit",
        ));
    }
    Ok(Folder { files })
}

pub(super) fn verify_folder(path: &Path, expected: Option<&Folder>) -> Result<(), InstallError> {
    match expected {
        Some(expected) if snapshot_folder(path)? != *expected => Err(InstallError::validation(
            format!("Managed content changed after review: {}", path.display()),
        )),
        Some(_) => Ok(()),
        None if path.try_exists().map_err(failure)? => Err(InstallError::validation(format!(
            "Content appeared after review: {}",
            path.display()
        ))),
        None => Ok(()),
    }
}

pub(super) fn verify_file(path: &Path, expected: &TransactionDigest) -> Result<(), InstallError> {
    reject_symlink(path, "managed package")?;
    if *expected != digest_file(path).map_err(failure)? {
        return Err(InstallError::validation(format!(
            "File changed after review: {}",
            path.display()
        )));
    }
    Ok(())
}
