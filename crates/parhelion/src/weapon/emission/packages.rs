use super::*;

pub(super) struct Packages<'a, 'p> {
    pub directory: &'a Path,
    pub progress: &'a mut build::Progress<'p>,
}

impl Packages<'_, '_> {
    pub(super) fn overlay(
        &mut self,
        id: u16,
        replacements: &[ReplacementSpec],
        tags: &[NewTagSpec],
        references: &[crate::NewTagReferenceOverride],
    ) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
        let directory = self.directory;
        self.package(id, |report| {
            crate::extend::build_extended_overlay_with_progress(
                directory,
                id,
                replacements,
                tags,
                references,
                report,
            )
        })
    }

    pub(super) fn standalone(
        &mut self,
        package: &crate::asset_packages::AssetPackage,
    ) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
        let directory = self.directory;
        let profile = crate::package_profile::authored_package(package.id)
            .ok_or_else(|| invalid("An authored asset package has no registered profile"))?;
        self.package(package.id, |report| {
            crate::extend::build_standalone_package_with_progress(
                directory,
                package.id,
                profile.file_name,
                &package.tags,
                &package.references,
                report,
            )
        })
    }

    fn package(
        &mut self,
        id: u16,
        build: impl FnOnce(&mut dyn FnMut(&str)) -> AuthoringResult<crate::ExtendedOverlayArtifact>,
    ) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
        let profile = crate::package_profile::authored_package(id)
            .ok_or_else(|| invalid("An emitted package has no registered profile"))?;
        let mut current = profile.file_name.to_owned();
        let artifact = build(&mut |stage| {
            current = format!("{stage}: {}", profile.file_name);
            self.progress.start(&current);
        })
        .map_err(|error| error.context(current.clone()))?;
        self.progress.finish(&current);
        Ok(artifact)
    }
}
