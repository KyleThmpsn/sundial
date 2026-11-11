//! Subclass damage-type gates on private perk programs. A gated program fires while a subclass of
//! one damage type is equipped, which native roster conditions express by naming each super
//! ability entity such a subclass equips. Before compiling, every gate becomes the roster
//! conditions of the stock subclasses of its damage type. A subclass an earlier build installed
//! lives in the authored overlay the build replaces, so only stock subclasses take part, read
//! from the installed catalog.
use super::*;

/// Expands every gated program the specs compile: the sandbox perks of each plug variant and the
/// custom perks of each authored subclass entry, at any nesting depth. The catalog is read only
/// when a program has a gate.
pub(crate) fn expand_subclass_gates(
    install_directory: &Path,
    weapons: &mut [WeaponCloneSpec],
) -> AuthoringResult<()> {
    if !weapons.iter().any(has_gate) {
        return Ok(());
    }
    let catalog = sundial::investment::InvestmentCatalog::load(install_directory, false, |_| {})
        .map_err(|error| invalid(format!("Could not load the installed subclasses: {error}")))?;
    let supers = catalog.subclass_super_entities(crate::package_profile::is_stock_item_definition);
    for weapon in weapons {
        let name = weapon.text.name.clone();
        for program in weapon
            .overrides
            .socket_plug_variants
            .iter_mut()
            .flat_map(|variant| &mut variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_mut())
        {
            program
                .expand_subclass_gates(&supers)
                .map_err(|error| invalid(error).context(name.clone()))?;
        }
        if let Some(abilities) = &mut weapon.overrides.subclass_abilities {
            abilities
                .expand_subclass_gates(&supers)
                .map_err(|error| invalid(error).context(name.clone()))?;
        }
    }
    Ok(())
}

fn has_gate(weapon: &WeaponCloneSpec) -> bool {
    weapon
        .overrides
        .socket_plug_variants
        .iter()
        .flat_map(|variant| &variant.sandbox_perks)
        .filter_map(|perk| perk.program.as_ref())
        .any(sundial::package_authoring::sandbox_perk::program::Program::has_subclass_gate)
        || weapon
            .overrides
            .subclass_abilities
            .as_ref()
            .is_some_and(crate::subclass::SubclassAbilities::has_subclass_gate)
}
