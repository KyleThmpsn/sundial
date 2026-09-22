use super::*;
use crate::tag_payload::{relative_target, write_u64};
fn array(data: &[u8], o: usize) -> AuthoringResult<(usize, usize, usize, u32)> {
    sundial::package_authoring::native_payload::native_array_at(data, o).map_err(invalid)
}
pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    symbols: &BTreeMap<String, TagHash>,
    graph: &Value,
    previous: &BTreeMap<u32, Vec<u8>>,
) -> AuthoringResult<Option<ReplacementSpec>> {
    let Some(dyes) = graph["dyes"].as_array() else {
        return Ok(None);
    };
    let tag = TagHash(0x81613D24);
    if tag.pkg_id() != emission.item_string_table_tag.pkg_id() {
        return Err(invalid("Dye table moved to another package"));
    }
    let mut table = match previous.get(&tag.0) {
        Some(data) => data.clone(),
        None => manager.read_tag(tag).map_err(|e| invalid(e.to_string()))?,
    };
    let (count, header, rows, _) = array(&table, 8)?;
    if rows + count * 8 != table.len() {
        return Err(invalid("Dye table is not terminal"));
    }
    let mut assignments = emission.entity_assignments.clone();
    let mut locked = vec![];
    for (i, d) in dyes.iter().enumerate() {
        let key = d["manifest"].as_u64().ok_or_else(|| invalid("Dye key"))? as u32;
        let parent = symbols[d["parent"].as_str().ok_or_else(|| invalid("Dye parent"))?];
        assignments = sundial::package_authoring::weapon_entity::append_weapon_entity_assignment(
            assignments,
            key,
            parent.0,
        )
        .map_err(invalid)?;
        table.extend_from_slice(&key.to_le_bytes());
        table.extend_from_slice(&key.to_le_bytes());
        locked.push(WeaponDyeReferenceOverride {
            channel_index: d["channel"]
                .as_i64()
                .ok_or_else(|| invalid("Dye channel"))? as i8,
            dye_reference_index: u16::try_from(count + i)
                .map_err(|_| invalid("Dye index overflow"))?,
        });
    }
    write_u64(&mut table, 8, (count + dyes.len()) as u64)?;
    write_u64(&mut table, header, (count + dyes.len()) as u64)?;
    let size = table.len();
    write_u64(&mut table, 0, size as u64)?;
    emission.entity_assignments = assignments;
    let target = graph["item_hash"]
        .as_u64()
        .ok_or_else(|| invalid("Target item missing"))? as u32;
    let ordinal = super::definition_ordinal(emission, target)?;
    let definition = &mut emission.host_new_tags[ordinal].payload;
    // Locked source dyes match the modern exotic's authored presentation.
    set_weapon_render_dye_rows(definition, &[vec![], vec![], locked])?;
    let _ = relative_target(definition, 0x88)?;
    Ok(Some(ReplacementSpec {
        tag,
        payload: table,
    }))
}
