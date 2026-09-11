//! Prepare verified payloads and recheck original targets before committing.
use super::*;

#[cfg(test)]
pub(super) fn prepare_temporary_files(
    validated: &ValidatedRun,
) -> Result<Vec<PreparedArtifact>, String> {
    prepare_temporary_files_inner(validated, None)
}

#[cfg(test)]
pub(super) fn prepare_temporary_files_inner(
    validated: &ValidatedRun,
    fail_before_allocation: Option<usize>,
) -> Result<Vec<PreparedArtifact>, String> {
    prepare_temporary_files_with_progress(validated, fail_before_allocation, &mut |_| {})
}

pub(super) fn prepare_temporary_files_with_progress(
    validated: &ValidatedRun,
    fail_before_allocation: Option<usize>,
    progress: Observer<'_>,
) -> Result<Vec<PreparedArtifact>, String> {
    let mut prepared = Vec::with_capacity(validated.artifacts.len());
    for (index, artifact) in validated.artifacts.iter().enumerate() {
        progress(InstallProgress::item(
            InstallPhase::Preparing,
            &artifact.file_name,
            index,
            validated.artifacts.len(),
        ));
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
        if let Err(error) = validate_authored_package_file(
            &temporary_path,
            profile,
            &validated.target_packages_directory,
        ) {
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
        progress(InstallProgress::item(
            InstallPhase::Preparing,
            &artifact.file_name,
            index + 1,
            validated.artifacts.len(),
        ));
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

pub(super) fn verify_targets_unchanged(originals: &[OriginalArtifact]) -> Result<(), String> {
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
