//! Link explicitly selected imported assets into private package allocations.
use super::*;
use parhelion_import::d2_mot::arrays;
use serde_json::Value;
mod dyes;
mod loading;
mod ornament_collections;
mod ornament_icon;
mod ornaments;

/// Bind the assembled graph to its body slot, replacing the native carrier's assignment.
fn replace(
    data: &mut Vec<u8>,
    row: usize,
    key: u32,
    donor_hash: u32,
    source_key: u32,
) -> AuthoringResult<()> {
    let (count, _, rows, _) =
        sundial::package_authoring::native_payload::native_array_at(data, 8).map_err(invalid)?;
    let donor = (0..count)
        .map(|i| rows + i * 32)
        .find(|&o| read_u32(data, o).ok() == Some(donor_hash))
        .ok_or_else(|| invalid("Sword art donor missing"))?;
    art::rewrite(data, row, donor, |singles, slots| {
        for (_, keys) in slots.iter_mut() {
            for slot_key in keys {
                if *slot_key == source_key {
                    *slot_key = key;
                }
            }
        }
        // The source assignment identifies the carrier, not its final placement.
        if !singles.contains(&source_key) && !slots.iter().any(|(_, keys)| keys.contains(&key)) {
            return Err(invalid("Selected native assignment missing"));
        }
        parhelion_import::d2_mot::artwork::assembled(singles, slots, key)
            .map_err(|error| invalid(error.to_string()))
    })
}
pub(super) fn apply(
    directory: &Path,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let folders = weapons
        .iter()
        .filter_map(|weapon| {
            weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(|graph| (weapon, graph))
        })
        .map(|(weapon, graph)| {
            graph
                .validate(weapon.identity.item_hash)
                .map_err(|error| invalid(error.to_string()))?;
            Ok(std::iter::once(graph.directory.clone())
                .chain(
                    graph
                        .attachments
                        .iter()
                        .map(|attachment| attachment.directory.clone()),
                )
                .collect::<Vec<_>>())
        })
        .collect::<AuthoringResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if folders.is_empty() {
        return Ok(Vec::new());
    }
    let manager = sundial::package_authoring::PackageManager::new(
        directory,
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let mut companions = linking::Companions::new();
    let mut replacements = BTreeMap::new();
    for folder in folders {
        for replacement in apply_one(
            directory,
            emission,
            &folder,
            &replacements,
            &manager,
            &mut companions,
        )
        .map_err(|error| error.context(format!("Imported graph {}", folder.display())))?
        {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
    }
    for weapon in weapons {
        if let Some(graph) = &weapon.overrides.imported_graph {
            graph
                .validate(weapon.identity.item_hash)
                .map_err(|error| invalid(error.to_string()))?;
        }
    }
    Ok(replacements
        .into_iter()
        .map(|(tag, payload)| ReplacementSpec {
            tag: TagHash(tag),
            payload,
        })
        .collect())
}
fn apply_one(
    directory: &Path,
    emission: &mut PackageEmission,
    folder: &Path,
    previous: &BTreeMap<u32, Vec<u8>>,
    manager: &sundial::package_authoring::PackageManager,
    companions: &mut linking::Companions,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let graph: Value = serde_json::from_slice(
        &fs::read(folder.join("asset-graph.json")).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let json_nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| invalid("Asset nodes missing"))?;
    let mut nodes = Vec::with_capacity(json_nodes.len());
    for n in json_nodes {
        let name = n["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Asset symbol missing"))?;
        let template = n["template"]
            .as_u64()
            .ok_or_else(|| invalid("Template missing"))? as u32;
        let is_companion = name == "parent-companion" || n["shared_owner"].is_string();
        let payload = if is_companion {
            Vec::new()
        } else {
            fs::read(
                folder.join(
                    n["file"]
                        .as_str()
                        .ok_or_else(|| invalid("Payload missing"))?,
                ),
            )
            .map_err(|e| invalid(e.to_string()))?
        };
        let mut node = linking::Node::new(name, template, payload);
        for p in n["patches"]
            .as_array()
            .ok_or_else(|| invalid("Fixups missing"))?
        {
            let offset = p["offset"]
                .as_u64()
                .ok_or_else(|| invalid("Offset missing"))? as usize;
            let target = p["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Fixup symbol missing"))?;
            node.patches.push((offset, target.to_owned()));
        }
        node.reference = n["reference"].as_str().map(str::to_owned);
        if is_companion {
            node.companion = Some(linking::Companion {
                shared_owner: n["shared_owner"].as_str().unwrap_or("parent").to_owned(),
                source_parent: n["source_parent"].as_u64().unwrap_or(0x80EC272A) as u32,
            });
        }
        nodes.push(node);
    }
    let extra_bounds = if graph["ornament_icon_png"].is_string() {
        8
    } else {
        0
    };
    let linked = linking::link(
        directory,
        emission,
        manager,
        nodes,
        "parent",
        companions,
        extra_bounds,
        |symbols, groups| {
            loading::include_native_materials(manager, folder, json_nodes, symbols, groups)
        },
        Some(&folder.join("linked")),
    )?;
    let linking::Linked {
        symbols,
        package_index,
    } = linked;
    let symbol = |s: &str| -> AuthoringResult<TagHash> {
        symbols
            .get(s)
            .copied()
            .ok_or_else(|| invalid(format!("Missing symbol {s}")))
    };
    let nodes = json_nodes;
    let read = |tag| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|e| invalid(e.to_string()))
    };
    let item = graph["item_hash"]
        .as_u64()
        .ok_or_else(|| invalid("Item hash missing"))? as u32;
    let key = graph["art_key"]
        .as_u64()
        .ok_or_else(|| invalid("Art key missing"))? as u32;
    let source_key = graph["native_assignment"].as_u64().unwrap_or(0xCFBE7264) as u32;
    let (row_index, native_art_item) = art::prepare_row(
        manager,
        emission,
        item,
        graph["native_item"].as_u64().unwrap_or(0x02222CBF) as u32,
        source_key,
    )?;
    let (_, _, start, _) =
        sundial::package_authoring::native_payload::native_array_at(&emission.item_metadata, 8)
            .map_err(invalid)?;
    let row = start + row_index * 32;
    replace(
        &mut emission.item_metadata,
        row,
        key,
        native_art_item,
        source_key,
    )?;
    let ordinal = definition_ordinal(emission, item)?;
    let definition = &mut emission
        .host_new_tags
        .get_mut(ordinal)
        .ok_or_else(|| invalid("Authored definition missing"))?
        .payload;
    let translation = crate::tag_payload::relative_target(definition, 0x88)?;
    let (art_count, _, art_start, _) =
        sundial::package_authoring::native_payload::native_array_at(definition, translation)
            .map_err(invalid)?;
    if art_count != 1 {
        return Err(invalid("Expected one class-neutral art row"));
    }
    definition[art_start] = 255;
    definition[art_start + 2..art_start + 4].copy_from_slice(
        &u16::try_from(row_index)
            .map_err(|_| invalid("Art index overflow"))?
            .to_le_bytes(),
    );
    let table_tag = art::ASSIGNMENT_TABLE;
    let original = match previous.get(&table_tag.0) {
        Some(data) => data.clone(),
        None => read(table_tag.0)?,
    };
    let table = art::insert_assignments(&original, &[(key, symbol("parent")?)])?;
    fs::write(
        folder.join("allocated.json"),
        serde_json::to_vec_pretty(
            &symbols
                .iter()
                .map(|(k, v)| (k, v.0))
                .collect::<BTreeMap<_, _>>(),
        )
        .map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    eprintln!(
        "Imported {} private asset tags; art index {}; parent {}",
        nodes.len(),
        row_index,
        symbol("parent")?
    );
    let mut result = vec![table];
    if let Some(replacement) = dyes::apply(manager, emission, &symbols, &graph, previous)? {
        result.push(replacement);
    }
    let ordinal = definition_ordinal(emission, item)?;
    let definition = &mut emission.host_new_tags[ordinal].payload;
    arrays::repair(definition).map_err(invalid)?;

    ornaments::apply(emission, &graph)?;
    ornament_icon::apply(manager, emission, &graph, package_index, folder)?;
    ornament_collections::apply(emission, &graph)?;
    Ok(result)
}
