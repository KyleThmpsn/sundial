use super::*;

pub(super) fn validate_sunrise_build_cache(
    target_packages_directory: &Path,
) -> Result<ValidatedSunriseCache, InstallError> {
    let game_root = target_packages_directory.parent().ok_or_else(|| {
        InstallError::validation(format!(
            "Target packages directory has no game-root parent: {}",
            target_packages_directory.display()
        ))
    })?;
    let game_root = fs::canonicalize(game_root).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve game root {} while locating the Sunrise cache: {error}",
            game_root.display()
        ))
    })?;
    let paths = std::array::from_fn(|index| {
        let mut path = game_root.clone();
        for component in SUNRISE_BUILD_CACHE_LAYOUTS[index] {
            path.push(component);
        }
        path
    });
    let candidates = SunriseCacheCandidates { game_root, paths };
    let file = discover_sunrise_build_cache(&candidates).map_err(InstallError::validation)?;
    Ok(ValidatedSunriseCache { candidates, file })
}

pub(super) fn validate_package_header_caches(
    target_packages_directory: &Path,
) -> Result<ValidatedPackageHeaderCaches, InstallError> {
    let game_root = target_packages_directory.parent().ok_or_else(|| {
        InstallError::validation(format!(
            "Target packages directory has no game-root parent: {}",
            target_packages_directory.display()
        ))
    })?;
    let game_root = fs::canonicalize(game_root).map_err(|error| {
        InstallError::validation(format!(
            "Could not resolve game root {} while locating package-header caches: {error}",
            game_root.display()
        ))
    })?;
    let files = discover_package_header_caches(&game_root).map_err(InstallError::validation)?;
    Ok(ValidatedPackageHeaderCaches { game_root, files })
}

pub(super) fn discover_package_header_caches(
    game_root: &Path,
) -> Result<Vec<SunriseCacheFile>, String> {
    let mut files = Vec::new();
    let entries = fs::read_dir(game_root).map_err(|error| {
        format!(
            "Could not scan game root {} for package-header caches: {error}",
            game_root.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect a package-header cache candidate in {}: {error}",
                game_root.display()
            )
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_package_header_cache_name(file_name) {
            continue;
        }
        let path = entry.path();
        reject_regular_cache_file(&path, "Client package-header cache")?;
        let canonical_path = fs::canonicalize(&path).map_err(|error| {
            format!(
                "Could not resolve client package-header cache {}: {error}",
                path.display()
            )
        })?;
        if canonical_path
            .parent()
            .is_none_or(|parent| !paths_equal(parent, game_root))
        {
            return Err(format!(
                "Client package-header cache escaped the validated game root: {}",
                canonical_path.display()
            ));
        }
        let digest = digest_file(&canonical_path).map_err(|error| {
            format!(
                "Could not verify client package-header cache {}: {error}",
                canonical_path.display()
            )
        })?;
        files.push(SunriseCacheFile {
            path: canonical_path,
            parent: game_root.to_path_buf(),
            digest,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub(super) fn is_package_header_cache_name(file_name: &str) -> bool {
    file_name
        .strip_prefix(PACKAGE_HEADER_CACHE_PREFIX)
        .and_then(|name| name.strip_suffix(PACKAGE_HEADER_CACHE_SUFFIX))
        .is_some_and(|identity| {
            !identity.is_empty() && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

pub(super) fn discover_sunrise_build_cache(
    candidates: &SunriseCacheCandidates,
) -> Result<Option<SunriseCacheFile>, String> {
    let mut existing = Vec::with_capacity(candidates.paths.len());
    for path in &candidates.paths {
        let metadata = match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Could not inspect Sunrise build-data cache {}: {error}",
                    path.display()
                ));
            }
            Ok(metadata) => metadata,
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "The Sunrise build-data cache must be a regular file: {}",
                path.display()
            ));
        }
        let parent_path = path.parent().ok_or_else(|| {
            format!(
                "Sunrise build-data cache has no parent directory: {}",
                path.display()
            )
        })?;
        let parent = fs::canonicalize(parent_path).map_err(|error| {
            format!(
                "Could not resolve Sunrise cache directory {}: {error}",
                parent_path.display()
            )
        })?;
        let canonical_path = fs::canonicalize(path).map_err(|error| {
            format!(
                "Could not resolve Sunrise build-data cache {}: {error}",
                path.display()
            )
        })?;
        if !path_is_within(&parent, &candidates.game_root)
            || !path_is_within(&canonical_path, &candidates.game_root)
        {
            return Err(format!(
                "Sunrise build-data cache resolves outside the validated game root {}: {}",
                candidates.game_root.display(),
                canonical_path.display()
            ));
        }
        if canonical_path
            .parent()
            .is_none_or(|canonical_parent| !paths_equal(canonical_parent, &parent))
        {
            return Err(format!(
                "Sunrise build-data cache parent changed while it was being resolved: {}",
                path.display()
            ));
        }
        let digest = digest_file(&canonical_path).map_err(|error| {
            format!(
                "Could not verify Sunrise build-data cache {}: {error}",
                canonical_path.display()
            )
        })?;
        existing.push(SunriseCacheFile {
            path: canonical_path,
            parent,
            digest,
        });
    }

    match existing.len() {
        0 => Ok(None),
        1 => Ok(existing.pop()),
        _ => Err(format!(
            "Both supported Sunrise build-data cache locations exist; remove the inactive cache or select an unambiguous Sunrise installation: {} and {}",
            candidates.paths[0].display(),
            candidates.paths[1].display()
        )),
    }
}

pub(super) fn backup_sunrise_cache(
    validated: &ValidatedSunriseCache,
    backup_directory: &Path,
) -> Result<OriginalSunriseCache, String> {
    verify_sunrise_cache_snapshot(&validated.candidates, validated.file.as_ref())?;
    let Some(expected) = &validated.file else {
        return Ok(OriginalSunriseCache {
            candidates: validated.candidates.clone(),
            file: None,
            backup_path: None,
        });
    };

    reject_regular_cache_file(&expected.path, "Sunrise build-data cache")?;
    let cache_backup_directory = backup_directory.join(SUNRISE_CACHE_BACKUP_DIRECTORY);
    fs::create_dir(&cache_backup_directory).map_err(|error| {
        format!(
            "Could not create Sunrise cache backup directory {}: {error}",
            cache_backup_directory.display()
        )
    })?;
    let backup_path = cache_backup_directory.join("build_data.bin");
    let copied = copy_file_create_new(&expected.path, &backup_path).map_err(|error| {
        format!(
            "Could not back up Sunrise build-data cache {}: {error}",
            expected.path.display()
        )
    })?;
    if copied != expected.digest {
        return Err(format!(
            "Sunrise build-data cache {} changed while it was being backed up",
            expected.path.display()
        ));
    }
    verify_sunrise_cache_snapshot(&validated.candidates, Some(expected))?;
    Ok(OriginalSunriseCache {
        candidates: validated.candidates.clone(),
        file: Some(expected.clone()),
        backup_path: Some(backup_path),
    })
}

pub(super) fn backup_package_header_caches(
    validated: &ValidatedPackageHeaderCaches,
    backup_directory: &Path,
) -> Result<OriginalPackageHeaderCaches, String> {
    verify_package_header_cache_snapshot(&validated.game_root, &validated.files)?;
    if validated.files.is_empty() {
        return Ok(OriginalPackageHeaderCaches {
            game_root: validated.game_root.clone(),
            files: Vec::new(),
        });
    }

    let cache_backup_directory = backup_directory.join(PACKAGE_HEADER_CACHE_BACKUP_DIRECTORY);
    fs::create_dir(&cache_backup_directory).map_err(|error| {
        format!(
            "Could not create package-header cache backup directory {}: {error}",
            cache_backup_directory.display()
        )
    })?;
    let mut files = Vec::with_capacity(validated.files.len());
    for expected in &validated.files {
        reject_regular_cache_file(&expected.path, "Client package-header cache")?;
        let file_name = expected.path.file_name().ok_or_else(|| {
            format!(
                "Client package-header cache has no filename: {}",
                expected.path.display()
            )
        })?;
        let backup_path = cache_backup_directory.join(file_name);
        let copied = copy_file_create_new(&expected.path, &backup_path).map_err(|error| {
            format!(
                "Could not back up client package-header cache {}: {error}",
                expected.path.display()
            )
        })?;
        if copied != expected.digest {
            return Err(format!(
                "Client package-header cache {} changed while it was being backed up",
                expected.path.display()
            ));
        }
        files.push((expected.clone(), backup_path));
    }
    verify_package_header_cache_snapshot(&validated.game_root, &validated.files)?;
    Ok(OriginalPackageHeaderCaches {
        game_root: validated.game_root.clone(),
        files,
    })
}

pub(super) fn reject_regular_cache_file(path: &Path, description: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Could not inspect {description} {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "The {description} must be a regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn verify_sunrise_cache_unchanged(
    original: &OriginalSunriseCache,
) -> Result<(), String> {
    verify_sunrise_cache_snapshot(&original.candidates, original.file.as_ref())
}

pub(super) fn verify_sunrise_cache_snapshot(
    candidates: &SunriseCacheCandidates,
    expected: Option<&SunriseCacheFile>,
) -> Result<(), String> {
    let current = discover_sunrise_build_cache(candidates)?;
    match (expected, current.as_ref()) {
        (None, None) => Ok(()),
        (None, Some(current)) => Err(format!(
            "Sunrise build-data cache {} appeared after preflight",
            current.path.display()
        )),
        (Some(expected), None) => Err(format!(
            "Sunrise build-data cache {} disappeared after backup",
            expected.path.display()
        )),
        (Some(expected), Some(current))
            if expected.digest == current.digest
                && paths_equal(&expected.path, &current.path)
                && paths_equal(&expected.parent, &current.parent) =>
        {
            Ok(())
        }
        (Some(expected), Some(_)) => Err(format!(
            "Sunrise build-data cache {} changed after backup",
            expected.path.display()
        )),
    }
}

pub(super) fn verify_package_header_caches_unchanged(
    original: &OriginalPackageHeaderCaches,
) -> Result<(), String> {
    let expected = original
        .files
        .iter()
        .map(|(file, _)| file.clone())
        .collect::<Vec<_>>();
    verify_package_header_cache_snapshot(&original.game_root, &expected)
}

pub(super) fn verify_package_header_cache_snapshot(
    game_root: &Path,
    expected: &[SunriseCacheFile],
) -> Result<(), String> {
    let current = discover_package_header_caches(game_root)?;
    if current == expected {
        Ok(())
    } else {
        Err("Client package-header cache set changed after preflight".to_owned())
    }
}

pub(super) fn invalidate_sunrise_cache(
    original: &OriginalSunriseCache,
    cache_ops: CacheInvalidationOps,
) -> Result<Option<InvalidatedSunriseCache>, String> {
    verify_sunrise_cache_unchanged(original)?;
    let (file, backup_path) = match (&original.file, &original.backup_path) {
        (None, None) => return Ok(None),
        (Some(file), Some(backup_path)) => (file, backup_path),
        _ => {
            return Err(
                "Internal error: Sunrise cache snapshot and backup state disagree".to_owned(),
            );
        }
    };
    let backup_digest = digest_file(backup_path).map_err(|error| {
        format!(
            "Could not verify Sunrise cache backup {} before invalidation: {error}",
            backup_path.display()
        )
    })?;
    if backup_digest != file.digest {
        return Err(format!(
            "Sunrise cache backup {} no longer matches the original cache",
            backup_path.display()
        ));
    }
    reject_regular_cache_file(&file.path, "Sunrise build-data cache")?;
    let current_parent = fs::canonicalize(&file.parent).map_err(|error| {
        format!(
            "Could not recheck Sunrise cache directory {}: {error}",
            file.parent.display()
        )
    })?;
    if !paths_equal(&current_parent, &file.parent)
        || !path_is_within(&current_parent, &original.candidates.game_root)
    {
        return Err(format!(
            "Sunrise cache directory changed or escaped the validated game root: {}",
            file.parent.display()
        ));
    }

    let quarantine_directory =
        create_cache_quarantine_directory(&file.parent, &original.candidates.game_root)?;
    if let Err(error) = verify_sunrise_cache_unchanged(original) {
        let _ = fs::remove_dir(&quarantine_directory);
        return Err(error);
    }
    let quarantined_cache_path = quarantine_directory.join("build_data.bin");
    if let Err(error) = (cache_ops.rename)(&file.path, &quarantined_cache_path) {
        let _ = fs::remove_dir(&quarantine_directory);
        return Err(format!(
            "Could not atomically quarantine Sunrise build-data cache {}: {error}",
            file.path.display()
        ));
    }

    // The successful same-directory rename is the cache transaction's commit boundary. Cleanup is
    // deliberately best-effort: no later failure may roll packages back without restoring cache.
    let retained_quarantine_path =
        (cache_ops.cleanup)(&quarantined_cache_path, &quarantine_directory);
    Ok(Some(InvalidatedSunriseCache {
        cache_path: file.path.clone(),
        backup_path: backup_path.clone(),
        byte_length: file.digest.byte_length,
        sha256: file.digest.sha256.clone(),
        retained_quarantine_path,
    }))
}

pub(super) fn invalidate_package_header_caches(
    original: &OriginalPackageHeaderCaches,
    cache_ops: CacheInvalidationOps,
) -> Result<Vec<InvalidatedPackageHeaderCache>, String> {
    verify_package_header_caches_unchanged(original)?;
    for (file, backup_path) in &original.files {
        let backup_digest = digest_file(backup_path).map_err(|error| {
            format!(
                "Could not verify package-header cache backup {} before invalidation: {error}",
                backup_path.display()
            )
        })?;
        if backup_digest != file.digest {
            return Err(format!(
                "Package-header cache backup {} no longer matches the original cache",
                backup_path.display()
            ));
        }
        reject_regular_cache_file(&file.path, "Client package-header cache")?;
        let current_digest = digest_file(&file.path).map_err(|error| {
            format!(
                "Could not recheck client package-header cache {}: {error}",
                file.path.display()
            )
        })?;
        if current_digest != file.digest {
            return Err(format!(
                "Client package-header cache {} changed before invalidation",
                file.path.display()
            ));
        }
        let current_parent = fs::canonicalize(&file.parent).map_err(|error| {
            format!(
                "Could not recheck package-header cache directory {}: {error}",
                file.parent.display()
            )
        })?;
        if !paths_equal(&current_parent, &original.game_root) {
            return Err(format!(
                "Package-header cache directory changed after preflight: {}",
                file.parent.display()
            ));
        }
    }

    let mut invalidated = Vec::with_capacity(original.files.len());
    for (file, backup_path) in &original.files {
        let quarantine_directory =
            create_cache_quarantine_directory(&file.parent, &original.game_root)?;
        let file_name = file.path.file_name().ok_or_else(|| {
            format!(
                "Client package-header cache has no filename: {}",
                file.path.display()
            )
        })?;
        let quarantined_cache_path = quarantine_directory.join(file_name);
        if let Err(error) = (cache_ops.rename)(&file.path, &quarantined_cache_path) {
            let _ = fs::remove_dir(&quarantine_directory);
            return Err(format!(
                "Could not atomically quarantine client package-header cache {}: {error}",
                file.path.display()
            ));
        }
        let retained_quarantine_path =
            (cache_ops.cleanup)(&quarantined_cache_path, &quarantine_directory);
        invalidated.push(InvalidatedPackageHeaderCache {
            cache_path: file.path.clone(),
            backup_path: backup_path.clone(),
            byte_length: file.digest.byte_length,
            sha256: file.digest.sha256.clone(),
            retained_quarantine_path,
        });
    }
    Ok(invalidated)
}

pub(super) fn create_cache_quarantine_directory(
    cache_parent: &Path,
    game_root: &Path,
) -> Result<PathBuf, String> {
    for attempt in 0..128u64 {
        let candidate = cache_parent.join(format!(
            "{SUNRISE_CACHE_QUARANTINE_PREFIX}{}-{attempt}",
            unique_token()
        ));
        let create_result = create_private_directory(&candidate);
        match create_result {
            Ok(()) => {
                let canonical = fs::canonicalize(&candidate).map_err(|error| {
                    format!(
                        "Could not resolve Sunrise cache quarantine {}: {error}",
                        candidate.display()
                    )
                })?;
                if !path_is_within(&canonical, game_root)
                    || canonical
                        .parent()
                        .is_none_or(|parent| !paths_equal(parent, cache_parent))
                {
                    let _ = fs::remove_dir(&canonical);
                    return Err(format!(
                        "Sunrise cache quarantine escaped its validated parent: {}",
                        canonical.display()
                    ));
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create adjacent Sunrise cache quarantine {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
    Err("Could not allocate a unique adjacent Sunrise cache quarantine".to_owned())
}

pub(super) fn rename_cache_into_quarantine(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

pub(super) fn cleanup_cache_quarantine(
    quarantined_cache_path: &Path,
    quarantine_directory: &Path,
) -> Option<PathBuf> {
    match fs::remove_file(quarantined_cache_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Some(quarantined_cache_path.to_path_buf()),
    }
    match fs::remove_dir(quarantine_directory) {
        Ok(()) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => Some(quarantine_directory.to_path_buf()),
    }
}
