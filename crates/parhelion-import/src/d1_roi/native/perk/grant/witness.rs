//! A source ability with no native assignment can still share one property row with a native root
//! bank. Defender's Ward of Dawn, pattern `5E4C7A19`, has no native pattern, while Shadowkeep's
//! Sentinel Shield bank `80BC9C02`, which also carries Ward of Dawn, keeps Starless Night's row
//! unchanged: key `7F4BA6E0` sets script parameter `7417499F` from 0 to 1, replacing it. Applying
//! that existing native key on the bank's slot then performs the source row without a private one.
//!
//! The join holds only for an ungated script parameter row that is its source ability's only row
//! under the key, when exactly one native root bank holds the key, in one ungated script parameter
//! row whose parameters equal the source's in name, reset, value and operation.
use super::*;
use sundial::package_authoring::ability_bank::{
    AbilityTarget, PropertyRow, SCRIPT_DEFINITION_CLASS, SLOT_BANKS, property_rows, row_modifiers,
};

/// The parameters of an ungated, driverless script parameter row that is its ability's only row
/// under its key, as name, reset bits, applied bits and operation (1 adds). `None` for any other
/// row.
pub(crate) fn script_parameters(
    data: &Payload,
    rows: &[usize],
    row: usize,
) -> Result<Option<Vec<ModifierWitness>>> {
    let key = data.u32(row)?;
    if rows
        .iter()
        .filter(|&&at| data.u32(at).ok() == Some(key))
        .count()
        != 1
    {
        return Ok(None);
    }
    row_parameters(data, row)
}

/// The parameters of an ungated, driverless script parameter row, whatever else its key holds,
/// as [`script_parameters`] gives them.
pub(crate) fn row_parameters(data: &Payload, row: usize) -> Result<Option<Vec<ModifierWitness>>> {
    let modifier = data.pointer(row + 32)?;
    if data.u32(modifier - 4)? != 0x80802974
        || data.u64(row + 24)? != 0
        || data.bytes::<4>(row + 8)? != [255, 0, 0, 0]
        || data.u32(row + 20)? != 0
        || data.bytes::<16>(modifier)? != [0; 16]
    {
        return Ok(None);
    }
    let settings = data.pointer(24)?;
    let table = data.array(settings + 0xB0, 16, Some(0x80802122))?;
    let start = usize::try_from(data.u32(modifier + 16)?)?;
    let count = usize::try_from(data.u32(modifier + 20)?)?;
    let Some(parameters) = table.get(start..start.saturating_add(count)) else {
        return Ok(None);
    };
    let source = parameters
        .iter()
        .map(|&at| {
            Ok((
                data.u32(at)?,
                data.u32(at + 4)?,
                data.u32(at + 8)?,
                data.u32(at + 12)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((!source.is_empty() && source.iter().all(|parameter| parameter.3 <= 1)).then_some(source))
}

/// The native bank, its slot and the key to apply, when one native row is the source row.
pub(crate) fn find(
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
    row: usize,
) -> Result<Option<(u32, AbilityTarget, u32)>> {
    let key = data.u32(row)?;
    let Some(source) = script_parameters(data, rows, row)? else {
        return Ok(None);
    };
    let mut holders = Vec::new();
    for &(slot, banks) in &SLOT_BANKS {
        for &bank in banks {
            let payload = native.tag(bank, Some(0x80809C36))?;
            let rows = property_rows(&payload.0).map_err(anyhow::Error::msg)?;
            let gated = row_modifiers(&payload.0).map_err(anyhow::Error::msg)?;
            let matching = rows
                .iter()
                .zip(&gated)
                .filter(|(candidate, _)| candidate.key == key)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            let parameters = |row: &PropertyRow| {
                row.parameters
                    .iter()
                    .map(|p| {
                        (
                            p.name,
                            p.reset.to_bits(),
                            p.applied.to_bits(),
                            u32::from(p.add),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let same = matches!(matching.as_slice(), [(candidate, (_, false))]
                if candidate.modifier_class == SCRIPT_DEFINITION_CLASS
                    && parameters(candidate) == source);
            holders.push((bank, slot, same));
        }
    }
    Ok(match holders.as_slice() {
        [(bank, slot, true)] => Some((*bank, *slot, key)),
        _ => None,
    })
}

type ModifierWitness = (u32, u32, u32, u32);
