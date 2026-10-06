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
    let globals = manager
        .read_tag(
            sundial::package_authoring::resolve_live_named_tag(manager, "investment_globals", None)
                .map_err(invalid)?,
        )
        .map_err(|e| invalid(e.to_string()))?;
    let tag = TagHash(read_u32(&globals, 16 + 67 * 16)?);
    if tag.pkg_id() != emission.table_tags.item_string_table_tag.pkg_id() {
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
    let mut allocated = vec![];
    for (i, d) in dyes.iter().enumerate() {
        let key = d["manifest"]
            .as_u64()
            .and_then(|key| u32::try_from(key).ok())
            .ok_or_else(|| invalid("Dye key"))?;
        let parent = *symbols
            .get(d["parent"].as_str().ok_or_else(|| invalid("Dye parent"))?)
            .ok_or_else(|| invalid("Dye parent asset missing"))?;
        assignments = sundial::package_authoring::entity::append_weapon_entity_assignment(
            assignments,
            key,
            parent.0,
        )
        .map_err(invalid)?;
        table.extend_from_slice(&key.to_le_bytes());
        table.extend_from_slice(&key.to_le_bytes());
        allocated.push(WeaponDyeReferenceOverride {
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
    if graph["kind"] == "shader" {
        let mut rows = super::super::super::gear::shader_dye_rows(definition)?;
        let allocated = allocated
            .iter()
            .map(|r| (r.channel_index, r.dye_reference_index))
            .collect::<BTreeMap<_, _>>();
        for row in rows.iter_mut().flatten() {
            if let Some(channel) = crate::shader::source_channel(row.dye_reference_index) {
                row.dye_reference_index = *allocated
                    .get(&channel)
                    .ok_or_else(|| invalid("Source shader channel was not allocated"))?;
            } else if usize::from(row.dye_reference_index) >= count {
                return Err(invalid("Copied shader dye is outside the native dye table"));
            }
        }
        // The emitted definition carries source rows and any explicitly copied native channels.
        super::super::super::gear::set_shader_dye_rows(definition, &rows)?;
    } else if graph.get("gear_art").is_some() {
        let source = graph["dye_rows"]
            .as_array()
            .filter(|rows| rows.len() == 3)
            .ok_or_else(|| invalid("Gear dye layers missing"))?;
        let mut rows: [Vec<WeaponDyeReferenceOverride>; 3] = std::array::from_fn(|_| Vec::new());
        for (layer, values) in source.iter().enumerate() {
            for value in values.as_array().ok_or_else(|| invalid("Gear dye layer"))? {
                let index = value["dye"]
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| invalid("Gear dye reference"))?;
                let allocated = allocated
                    .get(index)
                    .ok_or_else(|| invalid("Gear dye was not allocated"))?;
                if value["channel"].as_i64() != Some(i64::from(allocated.channel_index)) {
                    return Err(invalid("Gear dye channel differs from its material"));
                }
                rows[layer].push(*allocated);
            }
        }
        set_base_dyes(definition, rows)?;
    } else {
        // Legacy weapon graphs have no layer descriptors. Their source colors
        // are the imported model's baseline, not a lock on later shader choices.
        set_base_dyes(definition, [vec![], allocated, vec![]])?;
    }
    let _ = relative_target(definition, 0x88)?;
    Ok(Some(ReplacementSpec {
        tag,
        payload: table,
    }))
}

fn set_base_dyes(
    definition: &mut Vec<u8>,
    rows: [Vec<WeaponDyeReferenceOverride>; 3],
) -> AuthoringResult<()> {
    // Preserve the effective unshaded appearance, including overlapping source
    // custom and locked channels. All imported base colors become defaults so
    // a shader equipped later in game can replace them. Shader plugs themselves
    // retain their layers through the separate branch above.
    let mut defaults = BTreeMap::new();
    for layer in [1, 0, 2] {
        for row in &rows[layer] {
            defaults.insert(row.channel_index, *row);
        }
    }
    set_weapon_render_dye_rows(
        definition,
        &[vec![], defaults.into_values().collect(), vec![]],
    )
}
