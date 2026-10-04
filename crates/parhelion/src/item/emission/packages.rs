use super::*;

pub(super) struct Packages<'a, 'p> {
    pub directory: &'a Path,
    pub progress: &'a mut build::Progress<'p>,
    /// The package directory, read once for every package this emission writes.
    pub chains: Option<crate::chain::PatchChains>,
}

impl<'a, 'p> Packages<'a, 'p> {
    pub(super) fn overlay(
        &mut self,
        id: u16,
        replacements: &[ReplacementSpec],
        tags: &[NewTagSpec],
        references: &[crate::NewTagReferenceOverride],
    ) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
        let (chains, progress) = self.scanned()?;
        package(progress, id, |report| {
            crate::extend::build_extended_overlay_with_chains(
                chains,
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
        let profile = crate::package_profile::authored_package(package.id)
            .ok_or_else(|| invalid("An authored asset package has no registered profile"))?;
        let (chains, progress) = self.scanned()?;
        self::package(progress, package.id, |report| {
            crate::extend::build_standalone_package_with_chains(
                chains,
                package.id,
                profile.file_name,
                &package.tags,
                &package.references,
                report,
            )
        })
    }

    /// Scans the directory on first use and hands out the scan beside the progress reporter,
    /// as two borrows the borrow checker can tell apart.
    fn scanned(
        &mut self,
    ) -> AuthoringResult<(&crate::chain::PatchChains, &mut build::Progress<'p>)> {
        if self.chains.is_none() {
            self.chains = Some(crate::chain::PatchChains::scan(self.directory)?);
        }
        let Packages {
            chains, progress, ..
        } = self;
        Ok((chains.as_ref().expect("scanned above"), progress))
    }
}

fn package(
    progress: &mut build::Progress<'_>,
    id: u16,
    build: impl FnOnce(&mut dyn FnMut(&str)) -> AuthoringResult<crate::ExtendedOverlayArtifact>,
) -> AuthoringResult<crate::ExtendedOverlayArtifact> {
    let profile = crate::package_profile::authored_package(id)
        .ok_or_else(|| invalid("An emitted package has no registered profile"))?;
    let mut current = profile.file_name.to_owned();
    let artifact = build(&mut |stage| {
        current = format!("{stage}: {}", profile.file_name);
        progress.start(&current);
    })
    .map_err(|error| error.context(current.clone()))?;
    progress.finish(&current);
    Ok(artifact)
}
