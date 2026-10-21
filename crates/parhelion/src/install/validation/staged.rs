//! Content pins for reusing a completed package and recipe validation.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::install) struct Inputs {
    stage: PathBuf,
    target: PathBuf,
    files: Vec<ArtifactMetadata>,
}

impl Inputs {
    pub(super) fn new(
        stage: &Path,
        target: &Path,
        files: Vec<ArtifactMetadata>,
    ) -> Result<Self, InstallError> {
        Ok(Self {
            stage: canonical_directory(stage, "staged run")?,
            target: canonical_packages_directory(target)?,
            files,
        })
    }

    pub(in crate::install) fn file(&self, name: &str) -> Option<&ArtifactMetadata> {
        self.files.iter().find(|file| file.file_name == name)
    }

    /// The bytes whose decoded meaning the installer reuses, including map recipes and the
    /// manifest. This is also checked after account review, before preparing any publication.
    pub(in crate::install) fn verify(
        &self,
        stage: &Path,
        target: &Path,
    ) -> Result<(), InstallError> {
        if !paths_equal(&canonical_directory(stage, "staged run")?, &self.stage)
            || !paths_equal(&canonical_packages_directory(target)?, &self.target)
        {
            return Err(InstallError::validation(
                "The staging run or target installation changed since review. Review installation again",
            ));
        }
        for file in &self.files {
            verify_staged_file(stage, file)?;
        }
        Ok(())
    }
}

pub(super) fn metadata(name: &str, bytes: &[u8]) -> ArtifactMetadata {
    let digest = crate::artifact::digest_bytes(bytes);
    ArtifactMetadata {
        file_name: name.to_owned(),
        byte_length: digest.byte_length,
        sha256: digest.sha256,
    }
}

fn verify_staged_file(stage: &Path, file: &ArtifactMetadata) -> Result<(), InstallError> {
    let path = stage.join(&file.file_name);
    sundial::package_authoring::mod_content::checked_path(stage, &path)
        .map_err(InstallError::validation)?;
    loader::verify_file(&path, &TransactionDigest::from(file))
}

pub(super) fn packages(
    artifacts: &[ArtifactMetadata],
    mods: &[crate::mod_family::Member],
) -> Vec<ArtifactMetadata> {
    artifacts
        .iter()
        .filter(|artifact| {
            !mods.iter().any(|member| {
                member.kept
                    && member
                        .receipt
                        .artifacts
                        .iter()
                        .any(|a| a.path.rsplit('/').next() == Some(artifact.file_name.as_str()))
            })
        })
        .cloned()
        .collect()
}

/// Reuse only results for the same content at the same installation. Package structure and
/// recipe meaning are immutable for these hashes. Stock chains, runtime and installed state
/// are checked separately by every install, and verified publication still hashes copied bytes.
pub(super) fn verify(
    validated: &ValidatedManifest,
    stage: &Path,
    target: &Path,
) -> Result<(), InstallError> {
    validated.inputs.verify(stage, target)?;
    check_layout(target, &validated.artifacts)?;
    if !validated.mods.is_empty() && !uses_mod_loader(target) {
        return Err(InstallError::validation(
            "An importer mod family requires Sunrise 0.6 or later",
        ));
    }
    validate_direct_package_set(stage, &packages(&validated.artifacts, &validated.mods))?;
    for artifact in validated
        .artifacts
        .iter()
        .chain(&validated.merge_data)
        .chain(validated.map_diagnostics.iter())
    {
        // Kept members resolve only while their installed receipt and loader manifest match.
        let path = crate::mod_family::source(&validated.mods, stage, target, &artifact.file_name)
            .map_err(InstallError::validation)?;
        if path == stage.join(&artifact.file_name) {
            verify_staged_file(stage, artifact)?;
        } else {
            loader::verify_file(&path, &TransactionDigest::from(artifact))?;
        }
    }
    Ok(())
}
