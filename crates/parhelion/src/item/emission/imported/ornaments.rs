//! Ornament targets are private plug definitions selected by one authored weapon.
use super::*;
use crate::tag_payload::{array_at, read_u16, relative_target, write_u16, write_u64};
pub(super) fn apply(emission: &mut PackageEmission, graph: &Value) -> AuthoringResult<()> {
    let Some(link) = graph.get("ornament") else {
        return Ok(());
    };
    let hash = |key: &str| {
        link[key]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid(format!("Ornament {key} missing")))
    };
    let owner = hash("target_weapon")?;
    let socket = hash("target_socket")? as usize;
    let target = graph["item_hash"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("Ornament target hash"))?;
    if !emission.plans.iter().any(|p| p.item_hash == owner) || owner == target {
        return Err(invalid("Ornament owner must be a separate authored weapon"));
    }
    let ordinal = definition_ordinal(emission, owner)?;
    let definition = &emission.host_new_tags[ordinal].payload;
    let resource = relative_target(definition, 0x68)?;
    let (count, _, rows, class) = array_at(definition, resource)?;
    if class != 0x808077C4 || socket >= count {
        return Err(invalid("Ornament socket layout"));
    }
    let row = rows + socket * 0x50;
    let (item_count, _, item_rows, _) = array_at(&emission.item_table, 8)?;
    let resolve = |index: u16| -> AuthoringResult<u32> {
        if index as usize >= item_count {
            return Err(invalid("Ornament choice outside item table"));
        }
        read_u32(&emission.item_table, item_rows + index as usize * 24)
    };
    if resolve(read_u16(definition, row + 2)?)? != 0xAEBAE371 {
        return Err(invalid(
            "Ornament socket must retain the native default-appearance reset",
        ));
    }
    let (n, _, choices, class) = array_at(definition, row + 0x40)?;
    if class != 0x80802E03 {
        return Err(invalid("Ornament choices have an unknown class"));
    }
    let mut found = 0;
    for i in 0..n {
        if resolve(read_u16(definition, choices + i * 0x20)?)? == target {
            found += 1;
        }
    }
    if found != 1 {
        return Err(invalid(
            "Imported ornament must occur exactly once in its owner's socket",
        ));
    }
    // The source recipe moved its ornament lane. Disable only the explicitly
    // nominated native duplicate after verifying its category and reset choice.
    if let Some(disable) = link["disable_duplicate_socket"].as_u64() {
        let disable = usize::try_from(disable).map_err(|_| invalid("Duplicate socket overflow"))?;
        if disable == socket || disable >= count {
            return Err(invalid("Invalid duplicate ornament socket"));
        }
        let old = rows + disable * 0x50;
        if read_u16(definition, old)? == u16::MAX
            && read_u16(definition, old + 2)? == u16::MAX
            && crate::tag_payload::read_u64(definition, old + 0x40)? == 0
        {
            return Ok(());
        }
        if read_u16(definition, old)? != read_u16(definition, row)?
            || resolve(read_u16(definition, old + 2)?)? != 0xAEBAE371
        {
            return Err(invalid("Refusing to disable a non-ornament socket"));
        }
        let definition = &mut emission.host_new_tags[ordinal].payload;
        for offset in [0, 2, 0x0C, 0x20] {
            write_u16(definition, old + offset, u16::MAX)?;
        }
        for offset in [0x10, 0x40] {
            write_u64(definition, old + offset, 0)?;
            write_u64(definition, old + offset + 8, 0)?;
        }
    }
    Ok(())
}
