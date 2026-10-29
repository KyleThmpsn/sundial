//! Register every imported gear art variant without weapon-only placement assumptions.
use super::*;
use crate::tag_payload::{write_i64, write_u64};

fn key(item: u32, role: &str) -> u32 {
    let value = fnv1_name_hash(&format!("parhelion/imported-gear/{item:08X}/{role}"));
    if [0, u32::MAX, 0x811C9DC5].contains(&value) {
        value ^ 0x10000
    } else {
        value
    }
}

fn word(value: &Value) -> AuthoringResult<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("Invalid gear assignment"))
}

pub(super) fn bind(
    graph: &mut Value,
    spec: &WeaponCloneSpec,
    emission: &PackageEmission,
) -> AuthoringResult<()> {
    let matching_kind = if spec.kind == ItemKind::Weapon {
        graph["kind"] == "weapon"
    } else {
        crate::imported::kind(spec.kind).is_some_and(|kind| graph["kind"] == kind)
    };
    // A weapon reimport keeps its authored gameplay base and saves the import's carrier as the
    // appearance donor. Gear recipes cannot set one, so their base is the carrier.
    let carrier = spec
        .presentation_donor
        .as_ref()
        .map_or(spec.donor_item_hash, |donor| donor.item_hash);
    if !matching_kind || graph["native_item"].as_u64() != Some(u64::from(carrier)) {
        return Err(invalid(
            "Imported gear requires its matching native slot and runtime template",
        ));
    }
    let item = if let Some(link) = graph.get("ornament") {
        use crate::tag_payload::{array_at, read_u16, relative_target};
        let socket = word(&link["target_socket"])? as usize;
        let choice = word(&link["choice_index"])? as usize;
        let source = word(&link["native_plug"])?;
        // Armor and Rise of Iron weapon ornaments carry gear art of their own.
        if !matches!(spec.kind, ItemKind::Armor | ItemKind::Weapon)
            || !spec.overrides.socket_plug_variants.iter().any(|v| {
                usize::from(v.socket_index) == socket
                    && usize::from(v.choice_index) == choice
                    && v.source_plug_hash == source
            })
        {
            return Err(invalid("Ornament has no matching private socket choice"));
        }
        let owner = definition_ordinal(emission, spec.identity.item_hash)?;
        let data = &emission.host_new_tags[owner].payload;
        let (count, _, rows, _) = array_at(data, relative_target(data, 0x68)?)?;
        if socket >= count {
            return Err(invalid("Ornament socket is absent"));
        }
        let (count, _, rows, _) = array_at(data, rows + socket * 0x50 + 0x40)?;
        if choice >= count {
            return Err(invalid("Ornament choice is absent"));
        }
        let ordinal = usize::from(read_u16(data, rows + choice * 0x20)?);
        let (count, _, rows, _) = array_at(&emission.item_table, 8)?;
        if ordinal >= count {
            return Err(invalid("Ornament item is absent"));
        }
        let item = read_u32(&emission.item_table, rows + ordinal * 24)?;
        definition_ordinal(emission, item)?;
        graph["ornament"]["target_item"] = spec.identity.item_hash.into();
        item
    } else {
        spec.identity.item_hash
    };
    graph["item_hash"] = item.into();
    let mut keys = BTreeSet::new();
    for (index, part) in graph["gear_art"]["parts"]
        .as_array_mut()
        .ok_or_else(|| invalid("Imported art parts missing"))?
        .iter_mut()
        .enumerate()
    {
        let value = key(item, &format!("art-{index}"));
        if !keys.insert(value) {
            return Err(invalid("Imported gear art identity collision"));
        }
        part["key"] = value.into();
    }
    for (index, dye) in graph["dyes"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let value = key(item, &format!("dye-{index}"));
        if !keys.insert(value) {
            return Err(invalid("Imported gear dye identity collision"));
        }
        dye["manifest"] = value.into();
    }
    Ok(())
}

/// Explicit source policy applies only to the freshly authored collectible.
pub(super) fn collections(
    emission: &mut PackageEmission,
    graph: &Value,
    spec: Option<&WeaponCloneSpec>,
) -> AuthoringResult<()> {
    if graph["collections_unconditional"] != true || graph.get("ornament").is_some() {
        return Ok(());
    }
    let spec =
        spec.ok_or_else(|| invalid("Unconditional Collections policy has no item recipe"))?;
    if !matches!(
        spec.kind,
        ItemKind::Weapon | ItemKind::Armor | ItemKind::Sparrow
    ) {
        return Err(invalid(
            "Unconditional imported Collections policy requires a weapon, armor or Sparrow",
        ));
    }
    use crate::progression::*;
    use crate::tag_payload::{array_at, read_u16, write_relative_pointer, write_u16, write_u32};
    let item = word(&graph["item_hash"])?;
    if item != spec.identity.item_hash {
        return Err(invalid(
            "Unconditional Collections item differs from its private recipe",
        ));
    }
    definition_ordinal(emission, item)?;
    let (count, _, rows, _) = array_at(&emission.item_table, 8)?;
    let ordinal = (0..count)
        .find(|i| read_u32(&emission.item_table, rows + i * 24).ok() == Some(item))
        .ok_or_else(|| invalid("Imported item ordinal"))?;
    let (count, _, rows, _) = array_at(&emission.collectibles, 8)?;
    let matches = (0..count)
        .map(|i| rows + i * COLLECTIBLE_ROW_SIZE)
        .filter(|r| {
            read_u16(&emission.collectibles, r + COLLECTIBLE_ITEM_INDEX_OFFSET).ok()
                == Some(ordinal as u16)
        })
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid("Imported collectible is absent or ambiguous"));
    };
    let row = *row;
    for field in COLLECTIBLE_SECONDARY_CONDITION_OFFSETS {
        emission.collectibles[row + field..row + field + 16].fill(0);
    }
    let data = &mut emission.collectibles;
    while !(data.len() + 4).is_multiple_of(16) {
        data.push(0);
    }
    data.extend(0x80809FBDu32.to_le_bytes());
    let header = data.len();
    data.extend(1u64.to_le_bytes());
    data.extend(u64::from(NUMERIC_PROGRAM_ROW_CLASS).to_le_bytes());
    data.extend([11, 0, 0, 0, 1, 0, 0, 0]);
    while !(data.len() + NESTED_ARRAY_TRAILER.len()).is_multiple_of(16) {
        data.push(0);
    }
    data.extend(NESTED_ARRAY_TRAILER);
    write_u64(data, row + COLLECTIBLE_CONDITION_OFFSET, 1)?;
    write_relative_pointer(data, row + COLLECTIBLE_CONDITION_OFFSET + 8, header)?;
    write_u16(
        data,
        row + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
        COLLECTIBLE_REACQUISITION_ENABLED,
    )?;
    // Claim cost is independent of acquisition. Only the authored row opts out
    // of its donor's material requirements, leaving the shared set unchanged.
    if graph["collections_no_cost"] == true {
        write_u16(data, row + COLLECTIBLE_MATERIAL_SET_OFFSET, u16::MAX)?;
    }
    let length = data.len() as u64;
    write_u64(data, 0, length)?;
    // Clear any inherited display condition using the same parallel row ordinal.
    let index = (row - rows) / COLLECTIBLE_ROW_SIZE;
    let (count, _, rows, _) = array_at(&emission.collectible_displays, 8)?;
    if index >= count {
        return Err(invalid("Imported collectible display is absent"));
    }
    let at = rows + index * COLLECTIBLE_DISPLAY_ROW_SIZE + COLLECTIBLE_DISPLAY_CONDITION_OFFSET;
    emission.collectible_displays[at..at + 16].fill(0);
    // Native literal operands are dwords. Keep this write explicit at the storage boundary.
    write_u32(data, header + 20, 1)?;
    Ok(())
}

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    symbols: &BTreeMap<String, TagHash>,
    graph: &Value,
    previous: &BTreeMap<u32, Vec<u8>>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    // Saved D2 gear graphs predate the explicit format field. Reach rows already use
    // native placements and must never pass through modern selector translation.
    let modern = match graph["gear_art"].get("selector_format") {
        Some(Value::String(format)) if format == "modern" => true,
        Some(Value::String(format)) if format == "shadowkeep" => false,
        None if graph["source"] == "halo_reach" => false,
        None if graph["source_item"].as_u64().is_some() => true,
        _ => return Err(invalid("Imported gear art selector format is unknown")),
    };
    let item = word(&graph["item_hash"])?;
    let parts = graph["gear_art"]["parts"]
        .as_array()
        .ok_or_else(|| invalid("Imported art parts missing"))?;
    let mut replacements = BTreeMap::new();
    let mut assignments = Vec::new();
    for part in parts {
        let source = word(&part["source_assignment"])?;
        let key = word(&part["key"])?;
        let symbol = part["parent"]
            .as_str()
            .ok_or_else(|| invalid("Gear parent missing"))?;
        let tag = symbols
            .get(symbol)
            .copied()
            .ok_or_else(|| invalid("Gear parent was not allocated"))?;
        if replacements.insert(source, key).is_some() {
            return Err(invalid("Duplicate source art assignment"));
        }
        assignments.push((key, tag));
    }
    let remap = |value: &Value| -> AuthoringResult<u32> {
        let source = word(value)?;
        if [0, u32::MAX, 0x811C9DC5].contains(&source) {
            return Ok(source);
        }
        replacements
            .get(&source)
            .copied()
            .ok_or_else(|| invalid("Unconverted source art assignment"))
    };
    let rows = graph["gear_art"]["rows"]
        .as_array()
        .ok_or_else(|| invalid("Imported art rows missing"))?;
    let mut selectors = Vec::new();
    let mut owned_rows = BTreeSet::new();
    for (ordinal, source) in rows.iter().enumerate() {
        let metadata_key = key(item, &format!("row-{ordinal}"));
        if !owned_rows.insert(metadata_key) {
            return Err(invalid("Gear art row identity collision"));
        }
        let template = usize::try_from(word(&source["template_index"])?)
            .map_err(|_| invalid("Gear template index"))?;
        let (count, _, start, _) =
            sundial::package_authoring::native_payload::native_array_at(&emission.item_metadata, 8)
                .map_err(invalid)?;
        if (0..count)
            .any(|i| read_u32(&emission.item_metadata, start + i * 32).ok() == Some(metadata_key))
        {
            return Err(invalid(
                "Imported art metadata identity collides with an existing row",
            ));
        }
        let index = art::prepare_row_at(emission, metadata_key, template)?;
        let (_, _, start, _) =
            sundial::package_authoring::native_payload::native_array_at(&emission.item_metadata, 8)
                .map_err(invalid)?;
        let singles = source["singles"]
            .as_array()
            .filter(|v| v.len() == 2)
            .ok_or_else(|| invalid("Gear direct assignments missing"))?;
        let slots = source["slots"]
            .as_array()
            .ok_or_else(|| invalid("Gear selectors missing"))?
            .iter()
            .map(|slot| {
                let selector = slot["selector"]
                    .as_u64()
                    .ok_or_else(|| invalid("Gear selector missing"))?;
                let selector = if modern {
                    parhelion_import::d2_mot::artwork::native_selector(selector)
                        .map_err(|error| invalid(error.to_string()))?
                } else {
                    selector
                };
                let keys = slot["assignments"]
                    .as_array()
                    .ok_or_else(|| invalid("Gear alternatives missing"))?
                    .iter()
                    .map(&remap)
                    .collect::<AuthoringResult<Vec<_>>>()?;
                Ok((selector, keys))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        art::set_layout(
            &mut emission.item_metadata,
            start + index * 32,
            [remap(&singles[0])?, remap(&singles[1])?],
            slots,
        )?;
        let class = source["class"]
            .as_i64()
            .filter(|v| (-1..=2).contains(v))
            .ok_or_else(|| invalid("Gear class selector"))? as i8;
        let flags = u8::try_from(word(&source["flags"])?).map_err(|_| invalid("Gear art flags"))?;
        selectors.extend_from_slice(&[class as u8, flags]);
        selectors.extend_from_slice(
            &u16::try_from(index)
                .map_err(|_| invalid("Gear art index overflow"))?
                .to_le_bytes(),
        );
    }
    let ordinal = definition_ordinal(emission, item)?;
    let definition = &mut emission.host_new_tags[ordinal].payload;
    let translation = crate::tag_payload::relative_target(definition, 0x88)?;
    let header = art::append(definition, 0x808077B5, rows.len(), &selectors);
    write_u64(definition, translation, rows.len() as u64)?;
    write_i64(
        definition,
        translation + 8,
        header as i64 - (translation + 8) as i64,
    )?;
    let size = definition.len();
    write_u64(definition, 0, size as u64)?;
    arrays::add_array_markers(definition).map_err(invalid)?;
    let tag = art::ASSIGNMENT_TABLE;
    let old = match previous.get(&tag.0) {
        Some(bytes) => bytes.clone(),
        None => manager
            .read_tag(tag)
            .map_err(|error| invalid(error.to_string()))?,
    };
    let mut result = vec![art::insert_assignments(&old, &assignments)?];
    if let Some(dyes) = dyes::apply(manager, emission, symbols, graph, previous)? {
        result.push(dyes);
    }
    Ok(result)
}
