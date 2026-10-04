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

pub(super) fn bind(graph: &mut Value, spec: &WeaponCloneSpec) -> AuthoringResult<()> {
    if crate::imported::kind(spec.kind).is_none_or(|kind| graph["kind"] != kind)
        || graph["native_item"].as_u64() != Some(u64::from(spec.donor_item_hash))
    {
        return Err(invalid(
            "Imported gear requires its matching native slot and runtime template",
        ));
    }
    let item = spec.identity.item_hash;
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

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    symbols: &BTreeMap<String, TagHash>,
    graph: &Value,
    previous: &BTreeMap<u32, Vec<u8>>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
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
    arrays::repair(definition).map_err(invalid)?;
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
