//! Prepared effect groups shared by weapon perks and subclass entry perks.
//! Subclasses need no item runtime entity, so their groups are allocated before perk compilation.
use super::*;
use crate::item::custom_runtime::particles::{self, ImportedParticles};
use sundial::package_authoring::sandbox_perk::program::ImportedAsset;

pub(super) fn load(
    imported: &ImportedAsset,
    directories: &mut BTreeSet<PathBuf>,
    particles: &mut Option<ImportedParticles>,
) -> AuthoringResult<()> {
    load_group(
        &imported.directory,
        &imported.sha256,
        &imported.symbol,
        directories,
        particles,
    )
}

fn load_group(
    directory: &Path,
    fingerprint: &str,
    symbol: &str,
    directories: &mut BTreeSet<PathBuf>,
    particles: &mut Option<ImportedParticles>,
) -> AuthoringResult<()> {
    let directory = fs::canonicalize(directory)
        .map_err(|error| invalid(format!("Imported attachment {symbol}: {error}")))?;
    let check = || -> AuthoringResult<()> {
        let actual = parhelion_import::d2_mot::native::attachment::fingerprint(&directory)
            .map_err(|error| invalid(format!("Imported attachment {symbol}: {error}")))?;
        if actual != fingerprint {
            return Err(invalid(
                "Imported assets changed after preparation. Prepare the import again.",
            ));
        }
        Ok(())
    };
    check()?;
    if directories.insert(directory.clone()) {
        let inputs = custom_runtime::imports::Inputs::open_directory(&directory)?;
        let group = particles::load(&inputs)?
            .ok_or_else(|| invalid("Imported attachment group has no assets"))?;
        group.attachment(symbol)?;
        if let Some(assets) = particles.as_mut() {
            assets.merge(group)?;
        } else {
            *particles = Some(group);
        }
    }
    particles
        .as_ref()
        .ok_or_else(|| invalid("Imported attachment has no asset group"))?
        .attachment(symbol)?;
    check()
}

pub(super) fn author_subclasses(
    manager: &PackageManager,
    resolved: &[resolve::ResolvedWeapon],
    assets: &mut RuntimeAssets<'_>,
) -> AuthoringResult<()> {
    for (ordinal, donor) in resolved.iter().enumerate() {
        let Some(list) = &donor.subclass_list else {
            continue;
        };
        let mut directories = BTreeSet::new();
        let mut particles = None;
        for imported in list
            .entries
            .iter()
            .flat_map(|entry| &entry.custom_perks)
            .flat_map(|perk| &perk.effects)
            .filter_map(|effect| effect.program.as_ref())
            .flat_map(sundial::package_authoring::sandbox_perk::program::Program::owned_programs)
            .flat_map(|program| &program.imported_assets)
        {
            load(imported, &mut directories, &mut particles)
                .map_err(|error| donor.weapon.in_recipe(error))?;
        }
        for entity in list
            .entries
            .iter()
            .filter_map(|entry| entry.entity.as_ref())
        {
            let Some(imported) = &entity.imported else {
                continue;
            };
            load_group(
                &imported.directory,
                &imported.sha256,
                &imported.symbol,
                &mut directories,
                &mut particles,
            )
            .map_err(|error| donor.weapon.in_recipe(error))?;
            particles
                .as_ref()
                .ok_or_else(|| invalid("Imported ability has no asset group"))?
                .ability(manager, &imported.symbol, entity.source, &entity.hud_keys)
                .map_err(|error| donor.weapon.in_recipe(error))?;
        }
        if let Some(particles) = particles {
            // These groups are read and allocated on every build, outside entity cache replay.
            // A source root symbol therefore resolves in this subclass's own allocation.
            let (symbols, eager) = particles
                .author(manager, assets.packages, donor.weapon.identity.item_hash)
                .map_err(|error| donor.weapon.in_recipe(error))?;
            if assets.particle_symbols.insert(ordinal, symbols).is_some() {
                return Err(donor.weapon.in_recipe(invalid(
                    "Subclass attachment assets conflict with an item runtime asset group",
                )));
            }
            assets.placed.extend(eager);
        }
    }
    Ok(())
}
