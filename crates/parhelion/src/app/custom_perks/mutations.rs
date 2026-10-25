//! Recipe mutations shared by socket controls and the custom-perk windows.
//! A socket position alone is not an identity: a changed source plug invalidates
//! the old editor key. Keep lookup, insertion, reset and reconciliation together.
use super::*;

/// Drops each custom perk on `socket_index` whose choice no longer holds its plug. A removed or
/// moved choice carries its own custom perk before this runs, so only stale ones remain.
pub(in crate::app) fn reconcile_socket_plug_variants(
    recipe: &mut WeaponRecipe,
    socket_index: usize,
    choices: &[u32],
) {
    recipe.overrides.socket_plug_variants.retain(|variant| {
        if usize::from(variant.socket_index) != socket_index {
            return true;
        }
        let choice_index = usize::from(variant.choice_index);
        choices.get(choice_index).copied() == variant.source_plug_hash.parse_u32().ok()
    });
}
