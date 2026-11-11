//! The native donor entities a weapon's runtime entity is composed from: the pattern donor's
//! entity, the appearance's rig and runtime components grafted onto it, the content groups the
//! runtime edits select by, and the splice, action and stat sources those edits read.
use super::*;
use crate::item::{ResolvedRuntimeComponentDonor, ResolvedSandboxPatternSource};

/// Reads donor runtime entities through the stock pattern and assignment tables.
#[derive(Clone, Copy)]
pub(super) struct Stock<'s> {
    pub manager: &'s PackageManager,
    pub patterns: &'s [u8],
    pub assignments: &'s [u8],
}

impl Stock<'_> {
    fn entity(&self, item_hash: u32, description: &str) -> AuthoringResult<(TagHash, Vec<u8>)> {
        resolve_runtime_weapon_entity(
            self.manager,
            self.patterns,
            self.assignments,
            item_hash,
            description,
        )
    }
}

/// The runtime component donors other than the pattern donor itself, in binding order.
pub(super) fn component_donors(
    donor: &resolve::ResolvedWeapon,
) -> Vec<&ResolvedRuntimeComponentDonor> {
    let pattern_source_hash = donor
        .runtime_pattern_source
        .as_ref()
        .map(|source| source.item_hash);
    let mut component_donors = donor
        .donors
        .runtime_components
        .iter()
        .filter(|component| Some(component.pattern_item_hash) != pattern_source_hash)
        .collect::<Vec<_>>();
    component_donors.sort_by_key(|component| component.binding_hash);
    component_donors
}

/// The pattern donor's entity and its tag, or the summoned vehicle's entity in their place.
pub(super) fn pattern_entity(
    stock: &Stock<'_>,
    donor: &resolve::ResolvedWeapon,
    pattern_source: ResolvedSandboxPatternSource,
) -> AuthoringResult<(TagHash, Vec<u8>)> {
    let (tag, entity) = stock.entity(pattern_source.item_hash, "weapon-pattern donor entity")?;
    crate::vehicle::authoring::select(
        stock.manager,
        donor.weapon.overrides.sparrow.as_ref(),
        tag,
        entity,
    )
}

/// Moves the appearance's rig and animations onto `entity`, and returns the marker set appends
/// that keep the base's own markers. This runs before any component donor, so the appearance's
/// rig is promoted onto exactly the entity the resolver tested. A donor that then conflicts with
/// it is reported against that donor rather than silently dropping the appearance's animations.
pub(super) fn graft_rig(
    stock: &Stock<'_>,
    donor: &resolve::ResolvedWeapon,
    entity: &mut Vec<u8>,
) -> AuthoringResult<Vec<WeaponRuntimeResourceAppend>> {
    let Some(item_hash) = donor.donors.appearance_rig else {
        return Ok(Vec::new());
    };
    let (_, appearance_entity) = stock.entity(item_hash, "appearance runtime entity")?;
    let base_entity = entity.clone();
    crate::weapon::rig::graft_presentation(entity, &appearance_entity).map_err(|error| {
        invalid(format!(
            "Could not move the appearance's rig and animations onto this runtime: {error}"
        ))
    })?;
    crate::weapon::rig::marker_set_appends(stock.manager, &base_entity, entity)
}

/// Grafts each runtime component donor's binding onto `entity`, rewiring a binding the donor
/// shapes differently.
pub(super) fn graft_components(
    stock: &Stock<'_>,
    components: &[&ResolvedRuntimeComponentDonor],
    entity: &mut Vec<u8>,
) -> AuthoringResult<()> {
    let manager = stock.manager;
    let mut component_entities = Vec::with_capacity(components.len());
    for component in components {
        let (_, component_entity) = stock.entity(
            component.pattern_item_hash,
            "runtime-component donor weapon entity",
        )?;
        component_entities.push((component.binding_hash, component_entity));
    }
    let component_grafts = component_entities
        .iter()
        .map(|(binding_hash, entity)| (*binding_hash, entity.as_slice()))
        .collect::<Vec<_>>();
    graft_weapon_component_bindings_or_rewire(entity, &component_grafts, &|tag| {
        manager.read_tag(tag)
    })
    .map_err(invalid)?;
    Ok(())
}

/// The content groups the runtime edits select blocks by, read from the composed `entity` and
/// the donors that lend animations, type markers, behavior and hold. The pattern row selects the
/// appearance's block when it supplies the row, and the base weapon's own block keeps its
/// behavior there.
pub(super) fn content_groups(
    stock: &Stock<'_>,
    donor: &resolve::ResolvedWeapon,
    entity: &[u8],
) -> AuthoringResult<crate::weapon::behavior::ContentGroups> {
    let manager = stock.manager;
    let donors = &donor.donors;
    Ok(crate::weapon::behavior::ContentGroups {
        selected: donor
            .gear_art_pattern_source
            .or(donor.runtime_pattern_source)
            .map(|source| source.weapon_content_group_hash),
        own: donor
            .runtime_pattern_source
            .map(|source| source.weapon_content_group_hash),
        animations: donors
            .animation
            .map(|source| {
                let (_, entity) = stock.entity(source.item_hash, "animation donor entity")?;
                crate::weapon::animations::profile(
                    manager,
                    &entity,
                    Some(source.weapon_content_group_hash),
                )
            })
            .transpose()?,
        kind: donors
            .type_marker
            .map(|source| source.weapon_content_group_hash),
        // The pattern row's identity can differ from the investment item a runtime is loaded by.
        behavior: donors
            .behavior
            .zip(donor.weapon.overrides.behavior_donor)
            .map(|(source, item)| (source.weapon_content_group_hash, item)),
        hold: donors
            .pinned_appearance
            .map(|source| {
                let (_, entity) = stock.entity(source.item_hash, "pinned appearance entity")?;
                crate::weapon::animations::hold(manager, &entity)
            })
            .transpose()?,
        // A moved rig's attachment row is the appearance's, so the block takes its hold class.
        stance: donors
            .appearance_rig
            .map(|_| {
                crate::weapon::animations::stance(
                    manager,
                    entity,
                    donor
                        .gear_art_pattern_source
                        .map(|source| source.weapon_content_group_hash),
                )
            })
            .transpose()?,
    })
}

/// The stat translator patches the native donors need on `entity`. A moved rig makes the pattern
/// row name the appearance's weapon type, and the stat translator converts by that type. The base
/// weapon's conversion is put back, so the appearance changes how the weapon looks and not its
/// fire rate. Bullets per Shot is the translator column every Barrel bullet input copies, in the
/// gameplay pattern's table, whose arrays a moved rig's row is pointed at too.
pub(super) fn stat_patches(
    manager: &PackageManager,
    entity: &[u8],
    donor: &resolve::ResolvedWeapon,
    pattern_source: ResolvedSandboxPatternSource,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = match (
        donor.donors.appearance_rig,
        donor.gear_art_pattern_source,
        donor.runtime_pattern_source,
    ) {
        (Some(_), Some(row), Some(own)) => crate::weapon::rig::stat_table_patches(
            manager,
            entity,
            row.weapon_translation_group_hash,
            own.weapon_translation_group_hash,
        )?,
        _ => Vec::new(),
    };
    if let Some(bullets) = donor
        .weapon
        .overrides
        .barrel
        .as_ref()
        .and_then(|barrel| barrel.bullets_per_shot)
    {
        patches.extend(crate::weapon::burst::patches(
            manager,
            entity,
            pattern_source.weapon_translation_group_hash,
            bullets,
        )?);
    }
    Ok(patches)
}

/// Each component splice's donor entity, by binding. A donor item's selector can name a shared
/// pattern with a different item identity.
pub(super) fn splices(
    stock: &Stock<'_>,
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<Vec<(u32, Vec<u8>)>> {
    donor
        .donors
        .component_splices
        .iter()
        .map(|(binding, source)| {
            let (_, entity) = stock.entity(source.item_hash, "component donor entity")?;
            Ok((*binding, entity))
        })
        .collect()
}

/// Each single action's donor, read as the animation profile its own row plays.
pub(super) fn actions(
    stock: &Stock<'_>,
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<
    Vec<(
        crate::recipe::AnimationAction,
        crate::weapon::animations::Profile,
    )>,
> {
    donor
        .donors
        .animation_actions
        .iter()
        .map(|(action, source)| {
            let (_, entity) = stock.entity(source.item_hash, "animation action donor entity")?;
            Ok((
                *action,
                crate::weapon::animations::profile(
                    stock.manager,
                    &entity,
                    Some(source.weapon_content_group_hash),
                )?,
            ))
        })
        .collect()
}
