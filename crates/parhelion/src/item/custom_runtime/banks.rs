//! Patches of an ability bank's rows: property values and the glyph rows that name the
//! tile's glyph.
use super::*;

/// The patches that write each bank value into the bank of the ability entity `source`, each
/// through the entity's binding of the bank whose resource starts nearest before the value. The
/// graph tree copies the bank they patch, so the copy carries a private bank. Refuses a value
/// whose row and lane the stock bank does not hold.
pub(in crate::item) fn bank_value_patches(
    manager: &PackageManager,
    source: TagHash,
    values: &[crate::subclass::BankValue],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let entity = read_tag(manager, source, "ability entity")?;
    let bank = sundial::package_authoring::ability_modifier::entity_bank(&entity)
        .map_err(invalid)?
        .ok_or_else(|| invalid(format!("Ability entity {source} has no bank to change")))?;
    let payload = read_tag(manager, TagHash(bank), "ability bank")?;
    sundial::package_authoring::ability_movement::validate_bank_context(manager, &entity, &payload)
        .map_err(invalid)?;
    let places = BankPlaces::of(&entity, (source, bank))?;
    values
        .iter()
        .map(|value| {
            let context = |error: String| invalid(format!("Ability bank 0x{bank:08X}: {error}"));
            use sundial::package_authoring::ability_movement::{
                PARAMETER_RESET, parameter_lane, row_lane,
            };
            let lane = if value.parameter {
                if value.lane != PARAMETER_RESET {
                    return Err(context(format!(
                        "parameter 0x{:08X} has no lane +0x{:X}",
                        value.key, value.lane
                    )));
                }
                parameter_lane(&payload, (value.key, value.row))
            } else {
                row_lane(&payload, (value.key, value.row, value.lane))
            }
            .map_err(context)?;
            if !lane.unit.value(value.bits).is_finite()
                || (lane.unit == sundial::package_authoring::ability_movement::Unit::Mode
                    && value.bits > 1)
            {
                return Err(context(format!("{} has an invalid value", lane.label)));
            }
            places.patch(
                (value.row, lane.offset),
                value.bits.to_le_bytes()[..lane.unit.width()].to_vec(),
            )
        })
        .collect()
}

/// The patches that make each row of `source`'s bank naming a HUD glyph while one of `keys`
/// applies name `glyph`, as the entity's energy controller does. The tile shows the glyph of the
/// last such row, so a glyph written to the controller alone stays hidden under it. The graph
/// tree copies the bank they patch, so the copy carries a private bank.
pub(in crate::item) fn bank_glyph_patches(
    manager: &PackageManager,
    source: TagHash,
    keys: &[u32],
    glyph: u32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let entity = read_tag(manager, source, "ability entity")?;
    let Some(bank) =
        sundial::package_authoring::ability_modifier::entity_bank(&entity).map_err(invalid)?
    else {
        return Ok(Vec::new());
    };
    let payload = read_tag(manager, TagHash(bank), "ability bank")?;
    let variants = sundial::package_authoring::ability_hud::variant_glyphs(&payload, keys)
        .map_err(|error| invalid(format!("Ability bank 0x{bank:08X}: {error}")))?;
    if variants.is_empty() {
        return Ok(Vec::new());
    }
    let places = BankPlaces::of(&entity, (source, bank))?;
    variants
        .into_iter()
        .map(|variant| places.patch((variant.row, variant.offset), glyph.to_le_bytes().to_vec()))
        .collect()
}

/// Preserve each attached variant's selection gates and map its original glyph to a private row.
pub(in crate::item) fn attached_glyph_patches(
    manager: &PackageManager,
    source: TagHash,
    glyphs: &[(u32, u32)],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let entity = read_tag(manager, source, "attached ability")?;
    let Some(bank) =
        sundial::package_authoring::ability_modifier::entity_bank(&entity).map_err(invalid)?
    else {
        return Ok(Vec::new());
    };
    let payload = read_tag(manager, TagHash(bank), "attached ability bank")?;
    let variants =
        sundial::package_authoring::ability_hud::all_variant_glyphs(&payload).map_err(invalid)?;
    if variants.is_empty() {
        return Ok(Vec::new());
    }
    let places = BankPlaces::of(&entity, (source, bank))?;
    variants
        .into_iter()
        .map(|variant| {
            let glyph = glyphs
                .iter()
                .find(|(original, _)| *original == variant.glyph)
                .map(|(_, glyph)| *glyph)
                .ok_or_else(|| {
                    invalid(format!(
                        "An attached ability's bank selects unplanned HUD glyph {:08X}",
                        variant.glyph
                    ))
                })?;
            places.patch((variant.row, variant.offset), glyph.to_le_bytes().to_vec())
        })
        .collect()
}

/// The resources through which an ability entity binds its bank, each by binding, index and
/// start in the bank.
pub(super) struct BankPlaces {
    pub(super) source: TagHash,
    pub(super) bank: u32,
    pub(super) places: Vec<(u32, usize, u64)>,
}

impl BankPlaces {
    pub(super) fn of(entity: &[u8], (source, bank): (TagHash, u32)) -> AuthoringResult<Self> {
        use sundial::package_authoring::entity::weapon_component_binding_hashes;
        let mut places = Vec::new();
        for binding in weapon_component_binding_hashes(entity).map_err(invalid)? {
            for resource in weapon_component_bindings(entity, binding).map_err(invalid)? {
                if resource.owner_tag == bank {
                    places.push((binding, resource.resource_index, resource.resource_offset));
                }
            }
        }
        Ok(Self {
            source,
            bank,
            places,
        })
    }

    /// The patch writing `bytes` at bank offset `at`, part of row `row`, through the binding
    /// whose resource starts nearest before it.
    pub(super) fn patch(
        &self,
        (row, at): (u16, usize),
        bytes: Vec<u8>,
    ) -> AuthoringResult<WeaponRuntimeResourcePatch> {
        let context = |error: String| invalid(format!("Ability bank 0x{:08X}: {error}", self.bank));
        let &(binding, index, start) = self
            .places
            .iter()
            .filter(|(_, _, start)| usize::try_from(*start).is_ok_and(|s| s <= at))
            .max_by_key(|(_, _, start)| *start)
            .ok_or_else(|| {
                context(format!(
                    "{} binds no resource before row {row}",
                    self.source
                ))
            })?;
        let offset = u32::try_from(at - start as usize)
            .map_err(|_| context("the row lies too far from the bound resource".into()))?;
        Ok(WeaponRuntimeResourcePatch {
            binding_hash: binding,
            resource_index: u16::try_from(index)
                .map_err(|_| context("the bound resource index is too large".into()))?,
            offset,
            bytes,
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        })
    }
}
