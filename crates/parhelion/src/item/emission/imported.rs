//! Link explicitly selected imported assets into private package allocations.
use super::*;
use parhelion_import::d2_mot::arrays;
use serde_json::Value;
mod crosshair;
mod dyes;
mod gear;
mod loading;
mod ornament_collections;
mod ornament_icon;
mod ornaments;
mod shader;

/// An independently placed private part, either source geometry or marker-only geometry.
struct Kept {
    assignment: u32,
    key: u32,
    parent: String,
    source_region: Option<(u64, usize)>,
}

fn kept_parts(graph: &Value) -> AuthoringResult<Vec<Kept>> {
    graph["kept_parts"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(graph["source_parts"].as_array().into_iter().flatten())
        .map(|part| {
            let word = |name: &str| {
                part[name]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| invalid(format!("Kept part {name} missing")))
            };
            Ok(Kept {
                assignment: word("assignment")?,
                key: word("key")?,
                parent: part["parent"]
                    .as_str()
                    .ok_or_else(|| invalid("Kept part parent missing"))?
                    .to_owned(),
                source_region: if part["source_region"] == true {
                    Some((
                        part["selector"]
                            .as_u64()
                            .ok_or_else(|| invalid("Source art selector missing"))?,
                        usize::try_from(
                            part["position"]
                                .as_u64()
                                .ok_or_else(|| invalid("Source art position missing"))?,
                        )
                        .map_err(|_| invalid("Source art position overflow"))?,
                    ))
                } else {
                    None
                },
            })
        })
        .collect()
}

/// Bind the assembled graph to its body slot, replacing the native carrier's assignment, and
/// each kept donor part to its private copy.
fn replace(
    data: &mut Vec<u8>,
    row: usize,
    key: u32,
    donor_hash: u32,
    source_key: u32,
    kept: &[Kept],
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
                } else if let Some(part) = kept
                    .iter()
                    .find(|part| part.source_region.is_none() && part.assignment == *slot_key)
                {
                    *slot_key = part.key;
                }
            }
        }
        // The source assignment identifies the carrier, not its final placement.
        if !singles.contains(&source_key) && !slots.iter().any(|(_, keys)| keys.contains(&key)) {
            return Err(invalid("Selected native assignment missing"));
        }
        let keys = kept.iter().map(|part| part.key).collect::<Vec<_>>();
        parhelion_import::d2_mot::artwork::assembled(singles, slots, key, &keys)
            .map_err(|error| invalid(error.to_string()))?;
        let mut regions = BTreeMap::<u64, BTreeMap<usize, u32>>::new();
        for part in kept {
            if let Some((selector, position)) = part.source_region
                && regions
                    .entry(selector)
                    .or_default()
                    .insert(position, part.key)
                    .is_some()
            {
                return Err(invalid("Duplicate source art position"));
            }
        }
        for (selector, positions) in regions {
            if !positions.keys().copied().eq(0..positions.len()) {
                return Err(invalid("Source art alternatives are discontinuous"));
            }
            let keys = positions.into_values().collect();
            if let Some(slot) = slots.iter_mut().find(|slot| slot.0 == selector) {
                slot.1 = keys;
            } else {
                slots.push((selector, keys));
            }
        }
        Ok(())
    })
}
/// Checks every imported graph against the fingerprint taken when it was selected. Each
/// graph hashes tens of megabytes of its own files, so the graphs are checked side by side.
fn validate_graphs(weapons: &[WeaponCloneSpec]) -> AuthoringResult<()> {
    let graphs = weapons
        .iter()
        .filter_map(|weapon| {
            weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(|graph| (weapon.kind, weapon.identity.item_hash, graph))
        })
        .collect::<Vec<_>>();
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8);
    let results = std::thread::scope(|scope| {
        graphs
            .chunks(graphs.len().div_ceil(workers).max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(kind, item, graph)| {
                            if let Some(kind) = crate::imported::kind(*kind) {
                                graph.validate_reusable(kind)
                            } else {
                                graph.validate(*item)
                            }
                            .map_err(|e| e.to_string())
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join())
            .collect::<Vec<_>>()
    });
    // Report the first failing graph in recipe order, as a sequential check would.
    for chunk in results {
        for result in chunk.map_err(|_| invalid("An imported graph check stopped unexpectedly"))? {
            result.map_err(invalid)?;
        }
    }
    Ok(())
}

pub(super) fn apply(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<Vec<ReplacementSpec>> {
    validate_graphs(weapons)?;
    for spec in weapons {
        if let Some(reference) = &spec.overrides.imported_graph {
            let graph: Value = serde_json::from_slice(
                &fs::read(reference.directory.join("asset-graph.json"))
                    .map_err(|e| invalid(e.to_string()))?,
            )
            .map_err(|e| invalid(e.to_string()))?;
            if (graph["kind"] == "shader") != (spec.kind == ItemKind::Shader) {
                return Err(invalid(
                    "Imported asset kind does not match its item recipe",
                ));
            }
            if let Some(kind) = crate::imported::kind(spec.kind) {
                if graph["kind"] != kind {
                    return Err(invalid("Imported asset kind does not match its recipe"));
                }
            } else if graph.get("gear_art").is_some() {
                return Err(invalid("Imported gear cannot supply a weapon model"));
            }
        }
    }
    let folders = weapons
        .iter()
        .filter_map(|weapon| {
            weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(|graph| (weapon, graph))
        })
        .flat_map(|(weapon, graph)| {
            std::iter::once((graph.directory.as_path(), Some(weapon))).chain(
                graph
                    .attachments
                    .iter()
                    .map(|attachment| (attachment.directory.as_path(), None)),
            )
        })
        .collect::<Vec<_>>();
    if folders.is_empty() {
        return Ok(Vec::new());
    }
    let mut companions = linking::Companions::new();
    let mut replacements = BTreeMap::new();
    // Custom shader dyes already extend the dye table, so imported dyes follow them.
    if let Some(table) = &emission.dye_table {
        replacements.insert(table.tag.0, table.payload.clone());
    }
    for (folder, spec) in folders {
        for replacement in apply_one(
            directory,
            emission,
            folder,
            &replacements,
            manager,
            &mut companions,
            spec,
        )
        .map_err(|error| error.context(format!("Imported graph {}", folder.display())))?
        {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
    }
    // The files must still be the ones selected now that linking has read them.
    validate_graphs(weapons)?;
    Ok(replacements
        .into_iter()
        .map(|(tag, payload)| ReplacementSpec {
            tag: TagHash(tag),
            payload,
        })
        .collect())
}
/// One node of an imported asset graph, with its payload read from `folder` and prepared:
/// model draw indices declared and moved markers applied.
fn graph_node(
    folder: &Path,
    n: &Value,
    spec: Option<&WeaponCloneSpec>,
    repaired: &mut BTreeMap<String, Vec<u8>>,
) -> AuthoringResult<linking::Node> {
    let name = n["symbol"]
        .as_str()
        .ok_or_else(|| invalid("Asset symbol missing"))?;
    let template = n["template"]
        .as_u64()
        .ok_or_else(|| invalid("Template missing"))? as u32;
    let is_companion = name == "parent-companion" || n["shared_owner"].is_string();
    let mut payload = if is_companion {
        Vec::new()
    } else if let Some(bytes) = repaired.remove(name) {
        bytes
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
    if name == "model" || n["model"] == true {
        parhelion_import::d2_mot::audit::draws::declare_model_draw_indices(&mut payload)
            .map_err(|e| invalid(format!("Imported model draw indices: {e}")))?;
    }
    // Moved markers move in the imported model's own marker sets, matched by name.
    if let Some(spec) = spec
        && !spec.overrides.marker_offsets.is_empty()
        && sundial::package_authoring::gear_markers::is_marker_set(&payload)
    {
        sundial::package_authoring::gear_markers::offset_markers(
            &mut payload,
            &spec.overrides.marker_offsets,
        )
        .map_err(|error| invalid(format!("Imported marker set {name}: {error}")))?;
    }
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
        let mut companion = linking::Companion::new(
            n["shared_owner"].as_str().unwrap_or("parent"),
            n["source_parent"].as_u64().unwrap_or(0x80EC272A) as u32,
        );
        for parent in n["inherited"].as_array().into_iter().flatten() {
            companion.inherited.push(
                parent
                    .as_u64()
                    .and_then(|tag| u32::try_from(tag).ok())
                    .ok_or_else(|| invalid("Inherited loading owner is not a tag"))?,
            );
        }
        node.companion = Some(companion);
    }
    Ok(node)
}

fn apply_one(
    directory: &Path,
    emission: &mut PackageEmission,
    folder: &Path,
    previous: &BTreeMap<u32, Vec<u8>>,
    manager: &sundial::package_authoring::PackageManager,
    companions: &mut linking::Companions,
    spec: Option<&WeaponCloneSpec>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let mut graph: Value = serde_json::from_slice(
        &fs::read(folder.join("asset-graph.json")).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    if graph["kind"] == "shader" {
        shader::bind(
            &mut graph,
            spec.ok_or_else(|| invalid("Source shader recipe missing"))?,
        )?;
    } else if graph.get("gear_art").is_some() {
        gear::bind(
            &mut graph,
            spec.ok_or_else(|| invalid("Imported gear recipe missing"))?,
        )?;
    }
    let json_nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| invalid("Asset nodes missing"))?;
    let mut nodes = Vec::with_capacity(json_nodes.len());
    let mut repaired = parhelion_import::d2_mot::native::vertex_input::repair(&graph, folder)
        .map_err(|e| invalid(format!("Imported vertex inputs: {e:#}")))?;
    for n in json_nodes {
        nodes.push(graph_node(folder, n, spec, &mut repaired)?);
    }
    let extra_bounds = if graph["ornament_icon_png"].is_string() {
        8
    } else {
        0
    };
    let shader = graph["kind"] == "shader";
    let borrowed = if shader {
        shader::edit(
            &mut nodes,
            &graph,
            spec.ok_or_else(|| invalid("Source shader recipe missing"))?,
            manager,
        )?
    } else {
        BTreeMap::new()
    };
    let primary = if shader {
        graph["dyes"]
            .as_array()
            .and_then(|dyes| dyes.first())
            .and_then(|dye| dye["parent"].as_str())
            .ok_or_else(|| invalid("Imported shader has no dye parent"))?
    } else {
        graph["parent"].as_str().unwrap_or("parent")
    };
    let linked = linking::link(
        directory,
        emission,
        manager,
        nodes,
        primary,
        companions,
        extra_bounds,
        |symbols, groups| {
            loading::include_native_resources(manager, folder, json_nodes, symbols, groups)?;
            for (scope, resources) in &borrowed {
                let tag = symbols
                    .get(scope)
                    .ok_or_else(|| invalid("Source shader scope was not allocated"))?;
                for group in groups.values_mut().filter(|g| g.contains(tag)) {
                    group.extend(resources.iter().copied());
                    group.sort();
                    group.dedup();
                }
            }
            Ok(())
        },
        Some(&folder.join("linked")),
    )?;
    let linking::Linked {
        symbols,
        package_index,
    } = linked;
    if shader {
        let replacement = dyes::apply(manager, emission, &symbols, &graph, previous)?
            .ok_or_else(|| invalid("Imported shader has no dye rows"))?;
        return Ok(vec![replacement]);
    }
    if graph.get("gear_art").is_some() {
        return gear::apply(manager, emission, &symbols, &graph, previous);
    }
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
    let kept = kept_parts(&graph)?;
    replace(
        &mut emission.item_metadata,
        row,
        key,
        native_art_item,
        source_key,
        &kept,
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
    let mut assignments = vec![(key, symbol("parent")?)];
    for part in &kept {
        assignments.push((part.key, symbol(&part.parent)?));
    }
    let table = art::insert_assignments(&original, &assignments)?;
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
    if let Some(replacement) =
        crosshair::apply(manager, emission, &symbols, &graph, previous, item)?
    {
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
