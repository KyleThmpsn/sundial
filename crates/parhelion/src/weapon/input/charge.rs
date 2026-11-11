//! Charge Time and Hold Time bases on a native Charge component whose stat translator copies no
//! value into them. The Charge definition, class `80803ABE` and `0x418` bytes, names each of its
//! three inputs in `m_stat_collection` at `+10`, rows of `0x20` bytes. A row's `+00` is a
//! reference relative to the row to the input's numeric member (`808045C2`), whose signed kind
//! byte at `+00` selects how component initialization (`F3F8F0`) reads it. Kind 0 reads the
//! Float32 at `+04`. The stat translator writes only the inputs it routes, so an unrouted input
//! starts from that literal, and component modifiers still act on it. Devil's Ruin's Charge
//! Time and Line in the Sand's Hold Time are such literals.
use super::Input;
use crate::{
    AuthoringResult,
    error::invalid,
    item::WeaponRuntimeResourcePatch,
    tag_payload::{read_i64, read_u8, read_u32, read_u64},
};
use sundial::package_authoring::{
    PackageManager,
    entity::{WEAPON_TRIGGER_CHARGE_COMPONENT_KEY, weapon_component_bindings},
};
use tiger_pkg::TagHash;

/// The Charge definition's class word, which precedes the definition the resource names.
const DEFINITION: u32 = 0x8080_3ABE;
const DEFINITION_SIZE: usize = 0x418;
/// `m_stat_collection`, one row per input.
const COLLECTION: usize = 0x10;
const ROW: usize = 0x20;
const INPUTS: u32 = 3;
/// The literal kind that holds a Float32 at member `+04`.
const FLOAT_LITERAL: u8 = 0;

/// The base literal of each Charge input in `values`, written on the effective entity's one
/// Charge component.
pub(super) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    values: &[&Input],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let bindings = weapon_component_bindings(entity, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY).map_err(
        |error| {
            invalid(format!(
                "Imported charge inputs need a native Charge component: {error}"
            ))
        },
    )?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "Imported charge inputs need one native Charge component",
        ));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let start = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Native Charge resource offset overflow"))?;
    if read_u32(&owner, start)? != binding.owner_tag {
        return Err(invalid("Native Charge resource prefix differs"));
    }
    // A component resource prefix names its definition by an absolute offset in the owner.
    let definition = usize::try_from(read_u64(&owner, start + 8)?)
        .map_err(|_| invalid("Native Charge definition offset overflow"))?;
    if definition < 4
        || definition % 8 != 0
        || definition
            .checked_add(DEFINITION_SIZE)
            .is_none_or(|end| end > owner.len())
        || read_u32(&owner, definition - 4)? != DEFINITION
        || read_u32(&owner, definition)? != binding.owner_tag
        || read_u32(&owner, definition + 4)? != binding.concrete_class
    {
        return Err(invalid("Native Charge definition layout differs"));
    }
    let resource_index = u16::try_from(binding.resource_index)
        .map_err(|_| invalid("Native Charge resource index overflow"))?;
    values
        .iter()
        .map(|value| {
            if value.component != 4 || value.input >= INPUTS {
                return Err(invalid("Imported input is not a native Charge input"));
            }
            let row = definition + COLLECTION + value.input as usize * ROW;
            let relative = read_i64(&owner, row)?;
            if relative == 0 {
                return Err(invalid(format!(
                    "The native Charge definition names no member for input {}",
                    value.input
                )));
            }
            let literal = isize::try_from(relative)
                .ok()
                .and_then(|relative| row.checked_add_signed(relative))
                .filter(|&at| at > definition && at + 8 <= definition + DEFINITION_SIZE)
                .ok_or_else(|| invalid("Native Charge input member lies outside its definition"))?;
            if read_u8(&owner, literal)? != FLOAT_LITERAL {
                return Err(invalid(format!(
                    "Native Charge input {} does not start from a Float32 literal",
                    value.input
                )));
            }
            let offset = (literal + 4)
                .checked_sub(start)
                .and_then(|offset| u32::try_from(offset).ok())
                .ok_or_else(|| invalid("Native Charge literal precedes its resource"))?;
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
                resource_index,
                offset,
                bytes: value.value.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            })
        })
        .collect()
}
