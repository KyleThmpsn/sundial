//! Private override arrays preserve other translation keys while releasing source stat inputs.
use super::*;
use crate::item::RuntimeResourceAppend;
use crate::tag_payload::{append_native_array, array_at, relative_target};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    offset: u32,
    rows: Vec<[u8; 8]>,
}

type Plans = (Vec<RuntimeResourcePatch>, Vec<RuntimeResourceAppend>);

pub(super) fn plans(owner: &[u8], resource: usize, changes: &[Change]) -> AuthoringResult<Plans> {
    if changes.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let definition = relative_target(owner, 24)?;
    let (count, _, start, class) = array_at(owner, definition + 0x238)?;
    if class != 0x808038C7
        || count == 0
        || count
            .checked_mul(40)
            .and_then(|size| start.checked_add(size))
            .is_none_or(|end| end > owner.len())
    {
        return Err(invalid("Imported stat override keys are invalid"));
    }
    let allowed = std::iter::once(definition + 0x228)
        .chain((0..count).map(|i| start + i * 40))
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen = std::collections::BTreeSet::new();
    let mut patches = Vec::new();
    let mut appends = Vec::new();
    for change in changes {
        let at = resource
            .checked_add(change.offset as usize)
            .ok_or_else(|| invalid("Imported stat override offset overflow"))?;
        if !allowed.contains(&at)
            || !seen.insert(at)
            || change.rows.iter().any(|row| {
                u32::from_le_bytes(row[..4].try_into().unwrap()) > 18
                    || !f32::from_le_bytes(row[4..].try_into().unwrap()).is_finite()
            })
        {
            return Err(invalid("Imported stat override array is invalid"));
        }
        if change.rows.is_empty() {
            patches.push(RuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset: change.offset,
                bytes: vec![0; 16],
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        } else {
            let mut bytes = vec![0; 16];
            let rows = change.rows.iter().flatten().copied().collect::<Vec<_>>();
            append_native_array(&mut bytes, 0, 0x808038C9, change.rows.len(), &rows)?;
            let (_, header, _, _) = array_at(&bytes, 0)?;
            appends.push(RuntimeResourceAppend {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                bytes,
                arrays: vec![(change.offset, header, change.rows.len() as u64)],
                slots: Vec::new(),
                pointers: Vec::new(),
                references: Vec::new(),
                retained_pointers: Vec::new(),
                retained_bytes: Vec::new(),
            });
        }
    }
    Ok((patches, appends))
}
