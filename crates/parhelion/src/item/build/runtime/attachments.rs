//! Prepared effect groups shared by weapon perks and subclass entry perks.
//! Subclasses need no item runtime entity, so their groups are allocated before perk compilation.
//! So are an item's when its runtime entity was not authored, as armor has none.
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

/// Allocates the effect groups items without an authored runtime entity name, and subclasses',
/// and authors their HUD images. An item in `failed` is skipped. An item whose groups fail is
/// added to `failed` with everything it extended left as it was, and the other items continue.
pub(super) fn author_groups(
    manager: &PackageManager,
    resolved: &[resolve::ResolvedWeapon],
    assets: &mut RuntimeAssets<'_>,
    layers: &mut BTreeMap<(String, u32), TagHash>,
    failed: &mut BTreeMap<usize, crate::AuthoringError>,
) -> AuthoringResult<Vec<sundial::package_authoring::sandbox_perk::program::HudStatus>> {
    // Weapon runtimes bound the statuses of the groups they allocated. Their images go first.
    let mut statuses = std::mem::take(&mut *assets.hud_statuses);
    author_images(manager, &statuses, assets.packages, layers)?;
    // Entries these groups hold in common across items, such as the effect of one perk on
    // several armor pieces, are allocated once.
    let mut shared = super::cache::AnimationCache::new();
    for (ordinal, donor) in resolved.iter().enumerate() {
        if failed.contains_key(&ordinal) {
            continue;
        }
        let restore = super::Restore::take(assets, ordinal);
        let before = (layers.clone(), shared.clone(), statuses.len());
        let allocation = crate::appended_tags::checkpoint();
        match item_groups(
            manager,
            (donor, ordinal),
            (&mut *assets, &mut shared),
            layers,
            &mut statuses,
        ) {
            Ok(()) => allocation.commit(),
            Err(error) => {
                drop(allocation);
                restore.apply(assets);
                *layers = before.0;
                shared = before.1;
                statuses.truncate(before.2);
                let error = if error.recipe_namespace().is_some() {
                    error
                } else {
                    donor.weapon.in_recipe(error)
                };
                failed.insert(ordinal, error);
            }
        }
    }
    Ok(statuses)
}

/// Loads and allocates the effect groups of one item at `ordinal`.
fn item_groups(
    manager: &PackageManager,
    (donor, ordinal): (&resolve::ResolvedWeapon, usize),
    (assets, shared): (&mut RuntimeAssets<'_>, &mut super::cache::AnimationCache),
    layers: &mut BTreeMap<(String, u32), TagHash>,
    statuses: &mut Vec<sundial::package_authoring::sandbox_perk::program::HudStatus>,
) -> AuthoringResult<()> {
    let mut directories = BTreeSet::new();
    let mut particles = None;
    let Some(list) = donor.subclass_list() else {
        // The runtime author allocates an item's groups with its entity. One without an
        // authored entity, such as armor, allocates the groups its private perks name here.
        if assets.particle_symbols.contains_key(&ordinal) {
            return Ok(());
        }
        for imported in donor
            .weapon
            .overrides
            .socket_plug_variants
            .iter()
            .flat_map(|variant| &variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_ref())
            .flat_map(sundial::package_authoring::sandbox_perk::program::Program::owned_programs)
            .flat_map(|program| &program.imported_assets)
        {
            load(imported, &mut directories, &mut particles)
                .map_err(|error| donor.weapon.in_recipe(error))?;
        }
        if let Some(particles) = particles {
            allocate(
                manager,
                particles,
                (donor, ordinal),
                (assets, shared),
                layers,
                statuses,
            )?;
        }
        return Ok(());
    };
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
    for entry in &list.entries {
        let Some(entity) = &entry.entity else {
            continue;
        };
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
        let keys = entity
            .hud_keys
            .iter()
            .copied()
            .chain(
                list.entries
                    .iter()
                    .flat_map(|source| &source.modifiers)
                    .filter(|modifier| modifier.target == entry.entry)
                    .map(|modifier| modifier.key),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        particles
            .as_ref()
            .ok_or_else(|| invalid("Imported ability has no asset group"))?
            .ability(manager, &imported.symbol, entity.source, &keys)
            .map_err(|error| donor.weapon.in_recipe(error))?;
        if let Some(glyph) = entity.hud_glyph {
            particles
                .as_mut()
                .ok_or_else(|| invalid("Imported ability has no asset group"))?
                .ability_hud(manager, &imported.symbol, &keys, glyph)
                .map_err(|error| donor.weapon.in_recipe(error))?;
        }
    }
    if let Some(particles) = particles {
        allocate(
            manager,
            particles,
            (donor, ordinal),
            (assets, shared),
            layers,
            statuses,
        )?;
    }
    Ok(())
}

/// Every imported ability's allocated root entity, by the source game and its identity there.
/// Perk conditions that name an imported ability's Shadowkeep version accept these entities too,
/// whichever item's perk names it. An ability without a recorded source joins no condition, and
/// a build without the ability adds none, so a gate then accepts its Shadowkeep version alone.
pub(super) fn imported_abilities(
    resolved: &[resolve::ResolvedWeapon],
    particle_symbols: &BTreeMap<usize, BTreeMap<String, TagHash>>,
) -> AuthoringResult<BTreeMap<(String, u32), BTreeSet<u32>>> {
    let mut found = BTreeMap::<(String, u32), BTreeSet<u32>>::new();
    for (ordinal, donor) in resolved.iter().enumerate() {
        let Some(list) = donor.subclass_list() else {
            continue;
        };
        for entity in list
            .entries
            .iter()
            .filter_map(|entry| entry.entity.as_ref())
        {
            let Some(imported) = &entity.imported else {
                continue;
            };
            let Some(source) = &imported.source else {
                continue;
            };
            // A gate only widens to imported copies, so one without a root leaves the gate on
            // its Shadowkeep version rather than failing the build.
            let Some(root) = particle_symbols.get(&ordinal).and_then(|symbols| {
                symbols.get(&particles::ability_symbol(
                    &imported.symbol,
                    entity.hud_glyph,
                ))
            }) else {
                continue;
            };
            found
                .entry((source.source.clone(), source.identity))
                .or_default()
                .insert(root.0);
        }
    }
    Ok(found)
}

/// Authors the image of each imported HUD status that `layers` doesn't hold yet, and records it.
fn author_images(
    manager: &PackageManager,
    displays: &[sundial::package_authoring::sandbox_perk::program::HudStatus],
    packages: &mut crate::asset_packages::AssetPackages,
    layers: &mut BTreeMap<(String, u32), TagHash>,
) -> AuthoringResult<()> {
    let missing = displays
        .iter()
        .filter(|display| {
            display
                .image
                .as_ref()
                .zip(display.icon)
                .is_some_and(|(image, icon)| !layers.contains_key(&(image.clone(), icon)))
        })
        .cloned()
        .collect::<Vec<_>>();
    let authored = crate::item::hud_status::imported_images(manager, &missing, packages)?;
    for (key, tag) in authored {
        if layers.insert(key, tag).is_some() {
            return Err(invalid("Imported HUD image was allocated twice"));
        }
    }
    Ok(())
}

/// Binds a group's HUD statuses, authors their images and allocates the group under `ordinal`.
/// A node whose final bytes match an entry an earlier item's group recorded in `shared` takes that
/// entry.
fn allocate(
    manager: &PackageManager,
    mut particles: ImportedParticles,
    (donor, ordinal): (&resolve::ResolvedWeapon, usize),
    (assets, shared): (&mut RuntimeAssets<'_>, &mut super::cache::AnimationCache),
    layers: &mut BTreeMap<(String, u32), TagHash>,
    statuses: &mut Vec<sundial::package_authoring::sandbox_perk::program::HudStatus>,
) -> AuthoringResult<()> {
    let displays = particles.bind_hud(manager)?;
    author_images(manager, &displays, assets.packages, layers)?;
    statuses.extend(displays);
    // These groups are read and allocated on every build, outside entity cache replay, so their
    // shared entries are recorded apart from the record cached runtimes replay against. Audio and
    // shard bindings name the item, so those nodes and their users stay the item's own.
    let (symbols, eager) = particles
        .author(
            manager,
            (&mut *assets.packages, shared),
            donor.weapon.identity.item_hash,
        )
        .map_err(|error| donor.weapon.in_recipe(error))?;
    if assets.particle_symbols.insert(ordinal, symbols).is_some() {
        return Err(donor.weapon.in_recipe(invalid(
            "Attachment assets conflict with an item runtime asset group",
        )));
    }
    assets.placed.extend(eager);
    Ok(())
}
