//! Advisory projection of authored effects into Sunrise's replicated weapon bank.
use crate::recipe::WeaponRecipe;
use sundial::investment::{WeaponDamageCarrierFamily, WeaponDamageProfile, WeaponDonor};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Projection {
    pub default_count: usize,
    pub maximum_count: usize,
    pub omitted: Vec<String>,
    pub wave_frame: bool,
}

/// Counts one equipped choice per socket. Private clones replace source entries.
/// This models replication only, not whether an effect works on the weapon.
pub(crate) fn project(
    recipe: &WeaponRecipe,
    donor: &WeaponDonor,
    mut perks: impl FnMut(u32) -> Vec<u16>,
) -> Projection {
    let mut base = recipe
        .overrides
        .base_sandbox_perks
        .as_deref()
        .unwrap_or(&donor.base_sandbox_perks)
        .to_vec();
    // Fixed damage is applied after authored base rows and before the four-row
    // projection. Socket-driven damage keeps its existing plug contribution.
    if let Some(damage) = recipe.overrides.modern_damage_type
        && !donor.sockets.iter().any(|socket| socket.socket_type == 68)
    {
        let marker = base
            .iter()
            .position(|p| super::item_fields::fixed_damage_perk(*p).is_some());
        let family = match donor.summary.damage_profile {
            WeaponDamageProfile::LegacyFixed(_) => WeaponDamageCarrierFamily::LegacyFixed,
            _ => WeaponDamageCarrierFamily::ModernFixed,
        };
        let damage = super::ModernDamageType::from(damage);
        match (marker, family.base_sandbox_perk_index(damage.shared())) {
            (Some(index), Some(perk)) => base[index] = perk,
            (None, Some(perk)) => base.push(perk),
            (Some(index), None) => {
                base.remove(index);
            }
            (None, None) => {}
        }
    }
    let active = |rows: Vec<u16>| {
        rows.into_iter()
            .filter(|p| *p != u16::MAX)
            .take(4)
            .collect::<Vec<_>>()
    };
    let base = active(base);
    let mut wave_frame = base.contains(&1778);
    let mut defaults = base
        .into_iter()
        .map(|p| format!("Base effect {p}"))
        .collect::<Vec<_>>();
    let mut maximum_count = defaults.len();
    for lane in 0..donor
        .sockets
        .len()
        .max(recipe.overrides.socket_columns.len())
    {
        let socket = donor.sockets.get(lane);
        let column = recipe
            .overrides
            .socket_columns
            .get(lane)
            .and_then(Option::as_ref);
        let socket_type = column
            .and_then(|c| c.socket_type)
            .or_else(|| socket.map(|s| s.socket_type));
        if socket_type == Some(u16::MAX) {
            continue;
        }
        let choices = if let Some(column) = column {
            column
                .choices
                .iter()
                .map(|h| h.parse_u32().unwrap_or(0))
                .collect::<Vec<_>>()
        } else {
            let mut choices = socket
                .and_then(|s| s.native_default)
                .into_iter()
                .collect::<Vec<_>>();
            if let Some(socket) = socket {
                for &hash in &socket.ordered_embedded_choices {
                    if !choices.contains(&hash) {
                        choices.push(hash);
                    }
                }
            }
            choices
        };
        let mut maximum = 0;
        for (choice, hash) in choices.into_iter().enumerate() {
            let variant = recipe.overrides.socket_plug_variants.iter().find(|v| {
                usize::from(v.socket_index) == lane
                    && usize::from(v.choice_index) == choice
                    && v.source_plug_hash.parse_u32() == Ok(hash)
            });
            let mut rows = perks(hash);
            if let Some(variant) = variant {
                rows = variant.effect_indices(&rows);
            }
            let rows = active(rows);
            wave_frame |= rows.contains(&1778);
            maximum = maximum.max(rows.len());
            if choice == 0 {
                defaults.extend(
                    rows.into_iter()
                        .map(|p| format!("Socket {} effect {p}", lane + 1)),
                );
            }
        }
        maximum_count += maximum;
    }
    Projection {
        default_count: defaults.len(),
        maximum_count,
        omitted: defaults.into_iter().skip(16).collect(),
        wave_frame,
    }
}
