//! A source grant whose key Bungie's own port applies to the same native ability keeps the native
//! key, when some source row under it has no private translation.
//!
//! The native bank must hold the key, every native row under it must be a script parameter row
//! equal to one of the source's rows under the key, and a stock action must apply the key on the
//! bank's slot (an Ability Property node, kind 7). Applying the native key then performs the matched
//! source rows. The other source rows have no native counterpart and are reported as left out.
//!
//! New Tricks' `776F1AAC` on Skip Grenade holds a script row (`99B1D826`
//! `explosion_radius_scalar`, 1 to 1.1, replacing) and a numeric input 0 row (x1.125) in Rise of
//! Iron. Shadowkeep's bank `80BBD727` keeps the script row only, and Bungie's port, action
//! `80BBC880` of plug "New Tricks", applies `776F1AAC` on the grenade slot. The numeric row's
//! input has no unique native declaration, so it is the row left out.
use super::*;
use sundial::package_authoring::ability_bank::{
    AbilityTarget, SCRIPT_DEFINITION_CLASS, property_rows,
};
use sundial::package_authoring::sandbox_perk::action::decode;

/// The slot to apply `key` on and the evidence, when Bungie's port of the key stands in for the
/// source rows under it.
pub(super) fn witness(
    source: &Source,
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
    key: u32,
    bank: u32,
    gates: &BTreeMap<u32, u32>,
) -> Result<Option<(AbilityTarget, Value)>> {
    let own = rows
        .iter()
        .copied()
        .filter(|&at| data.u32(at).ok() == Some(key))
        .collect::<Vec<_>>();
    if own.is_empty() {
        return Ok(None);
    }
    let Some(slot) = ability_modifier::bank_slot(bank) else {
        return Ok(None);
    };
    let payload = native.tag(bank, Some(0x80809C36))?;
    // Only a key whose rows the private route cannot carry takes the port.
    let mut untranslated = Vec::new();
    for &at in &own {
        let Ok(gate) = validate_row(data, at, gates) else {
            return Ok(None);
        };
        if let Err(error) = private_rows(source, native, data, at, (bank, slot, &payload), gate) {
            untranslated.push((at, format!("{error:#}")));
        }
    }
    if untranslated.is_empty() {
        return Ok(None);
    }
    let native_rows = property_rows(&payload.0).map_err(anyhow::Error::msg)?;
    let native_rows = native_rows
        .iter()
        .filter(|row| row.key == key)
        .collect::<Vec<_>>();
    if native_rows.is_empty() {
        return Ok(None);
    }
    let mut matched = BTreeSet::new();
    for row in &native_rows {
        if row.modifier_class != SCRIPT_DEFINITION_CLASS {
            return Ok(None);
        }
        let parameters = row
            .parameters
            .iter()
            .map(|p| {
                (
                    p.name,
                    p.reset.to_bits(),
                    p.applied.to_bits(),
                    u32::from(p.add),
                )
            })
            .collect::<Vec<_>>();
        let mut found = None;
        for &at in &own {
            if witness::row_parameters(data, at)?.as_ref() == Some(&parameters) {
                found = Some(at);
            }
        }
        let Some(at) = found else {
            return Ok(None);
        };
        matched.insert(at);
    }
    // The untranslated rows are exactly the ones left out.
    if untranslated.iter().any(|(at, _)| matched.contains(at)) {
        return Ok(None);
    }
    let Some(action) = applier(native, key, slot)? else {
        return Ok(None);
    };
    let left_out = own
        .iter()
        .filter(|at| !matched.contains(at))
        .map(|&at| {
            let reason = untranslated
                .iter()
                .find(|(row, _)| *row == at)
                .map(|(_, reason)| reason.clone());
            json!({"source_property_offset":at,"reason":reason})
        })
        .collect::<Vec<_>>();
    Ok(Some((
        slot,
        json!({"source_key":key,"native_bank":bank,"slot":slot.byte(),"property_key":key,
            "route":"native_port_key","native_witness":action,
            "matched_source_rows":matched,"left_out":left_out,"balance":"native"}),
    )))
}

/// A stock action that applies `key` on `slot` through an Ability Property node.
fn applier(native: &mut Reader, key: u32, slot: AbilityTarget) -> Result<Option<u32>> {
    let needle = key.to_le_bytes();
    for tag in native.classes(0x808040B5) {
        let stock = native.tag(tag, Some(0x808040B5))?;
        if !stock.0.windows(4).any(|word| word == needle) {
            continue;
        }
        let Ok(decoded) = decode(&stock.0) else {
            continue;
        };
        if decoded.effects().any(|effect| {
            effect.kind == 7
                && effect.native.get(2) == Some(&slot.byte())
                && effect.native.get(4..8) == Some(&needle[..])
                && effect.native.get(8) == Some(&PropertyOperation::Apply.byte())
        }) {
            return Ok(Some(tag));
        }
    }
    Ok(None)
}
