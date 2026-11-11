//! Restrict prepared material edits to checked ancestor references in response groups.
use super::*;
use crate::tag_payload::{array_at, relative_target};

fn fields(owner: &[u8]) -> AuthoringResult<BTreeSet<usize>> {
    let definition = relative_target(owner, 24)?;
    if u32_at(owner, definition.saturating_sub(4))? != 0x8080388F {
        return Err(invalid("Prepared projectile movement definition differs"));
    }
    let (count, _, rows, class) = array_at(owner, definition + 0x218)?;
    if class != 0x808037CD || count > owner.len().saturating_sub(rows) / 88 {
        return Err(invalid("Prepared projectile response groups differ"));
    }
    let mut fields = BTreeSet::new();
    for i in 0..count {
        let group = rows + i * 88;
        fields.insert(group + 0x30);
        if u64_at(owner, group + 0x18)? == 0 {
            continue;
        }
        let (count, _, rows, class) = array_at(owner, group + 0x18)?;
        if class != 0x80809FC7 || count > owner.len().saturating_sub(rows) / 16 {
            return Err(invalid(
                "Prepared projectile material list exceeds its owner",
            ));
        }
        fields.extend((0..count).map(|i| rows + i * 16 + 8));
    }
    Ok(fields)
}

fn ancestor(
    manager: &PackageManager,
    mut from: u32,
    to: u32,
    checked: &BTreeSet<u32>,
) -> AuthoringResult<()> {
    let mut seen = BTreeSet::new();
    while from != u32::MAX {
        if !seen.insert(from)
            || !checked.contains(&from)
            || manager
                .get_entry(TagHash(from))
                .is_none_or(|e| e.reference != 0x80808BCB)
        {
            return Err(invalid(
                "Prepared projectile material ancestry is unchecked",
            ));
        }
        if from == to {
            return Ok(());
        }
        let payload = manager
            .read_tag(TagHash(from))
            .map_err(|e| invalid(e.to_string()))?;
        from = u32_at(&payload, 12)?;
    }
    Err(invalid(
        "Prepared projectile material is not an ancestor of its carrier",
    ))
}

pub(super) fn validate(
    manager: &PackageManager,
    owner: &[u8],
    resource: usize,
    patches: &[NativeAssetResourcePatch],
    checked: &BTreeSet<u32>,
) -> AuthoringResult<()> {
    let allowed = fields(owner)?;
    let mut seen = BTreeSet::new();
    for patch in patches {
        let offset = resource
            .checked_add(patch.offset as usize)
            .ok_or_else(|| invalid("Projectile material offset overflowed"))?;
        if patch.binding_hash != 0x0437756D
            || patch.resource_index != 0
            || patch.expected.len() != 4
            || patch.bytes.len() != 4
            || patch.imported_particle.is_some()
            || !allowed.contains(&offset)
            || !seen.insert(offset)
        {
            return Err(invalid(
                "Prepared projectile has an invalid material reference",
            ));
        }
        let from = u32_at(owner, offset)?;
        let to = u32::from_le_bytes(patch.bytes[..].try_into().unwrap());
        if from.to_le_bytes().as_slice() != patch.expected {
            return Err(invalid("Prepared projectile material reference changed"));
        }
        ancestor(manager, from, to, checked)?;
    }
    Ok(())
}
