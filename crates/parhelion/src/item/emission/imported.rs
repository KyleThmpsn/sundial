//! Link explicitly selected imported assets into private package allocations.
use super::*;
use parhelion_import::d2_mot::arrays;
use serde_json::Value;
mod character;
mod crosshair;
mod dyes;
mod gear;
mod loading;
mod ornament_collections;
mod ornament_icon;
mod ornaments;
mod shader;

struct Shared {
    companions: linking::Companions,
    characters: BTreeMap<(u32, u32), [u8; 32]>,
    timings: Timings,
}

/// Where linking the imported graphs spent its time, summed over every graph.
#[derive(Default)]
pub(super) struct Timings {
    graphs: usize,
    /// Reading and parsing each graph's description.
    read: std::time::Duration,
    /// Binding materials and reading and preparing each node's payload.
    nodes: std::time::Duration,
    /// Replication companions, ornament icon bounds and shader edits.
    companions: std::time::Duration,
    /// Allocating, converting and linking the nodes.
    link: std::time::Duration,
    /// Everything else: the item tables each graph edits afterwards.
    tables: std::time::Duration,
}

impl std::fmt::Display for Timings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = |duration: std::time::Duration| duration.as_secs_f64();
        write!(
            f,
            "{} graphs: read {:.3}s, nodes {:.3}s, companions {:.3}s, link {:.3}s, tables {:.3}s",
            self.graphs,
            seconds(self.read),
            seconds(self.nodes),
            seconds(self.companions),
            seconds(self.link),
            seconds(self.tables)
        )
    }
}

/// An independently placed private part, either source geometry or marker-only geometry.
struct Kept {
    assignment: u32,
    key: u32,
    parent: String,
    region: Option<Region>,
}

struct Region {
    selector: u64,
    position: usize,
    native: bool,
}

fn kept_region(part: &Value) -> AuthoringResult<Option<Region>> {
    let native = part["native_region"] == true;
    if !native && part["source_region"] != true {
        return Ok(None);
    }
    let selector = part["selector"]
        .as_u64()
        .ok_or_else(|| invalid("Kept art selector missing"))?;
    let selector = if native {
        selector
    } else {
        parhelion_import::d2_mot::artwork::native_selector(selector)
            .map_err(|error| invalid(error.to_string()))?
    };
    let position = usize::try_from(
        part["position"]
            .as_u64()
            .ok_or_else(|| invalid("Kept art position missing"))?,
    )
    .map_err(|_| invalid("Kept art position overflow"))?;
    Ok(Some(Region {
        selector,
        position,
        native,
    }))
}

fn place_region(
    slot: &mut Vec<u32>,
    positions: BTreeMap<usize, u32>,
    native: bool,
) -> AuthoringResult<()> {
    if native {
        // A retained native alternative can occupy any source position.
        // Empty positions remain empty and existing selector widths survive.
        let width = positions
            .last_key_value()
            .unwrap()
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Kept art position overflow"))?;
        slot.try_reserve(width.saturating_sub(slot.len()))
            .map_err(|_| invalid("Kept art alternatives exceed available capacity"))?;
        slot.resize(
            slot.len().max(width),
            parhelion_import::d2_mot::artwork::EMPTY,
        );
        for (position, key) in positions {
            slot[position] = key;
        }
    } else {
        if !positions.keys().copied().eq(0..positions.len()) {
            return Err(invalid("Source art alternatives are discontinuous"));
        }
        *slot = positions.into_values().collect();
    }
    Ok(())
}

fn place_regions(slots: &mut Vec<(u64, Vec<u32>)>, kept: &[Kept]) -> AuthoringResult<()> {
    struct Group {
        native: bool,
        positions: BTreeMap<usize, u32>,
    }
    let mut regions = BTreeMap::<u64, Group>::new();
    for part in kept {
        let Some(region) = &part.region else {
            continue;
        };
        let group = regions.entry(region.selector).or_insert_with(|| Group {
            native: region.native,
            positions: BTreeMap::new(),
        });
        if group.native != region.native
            || group.positions.insert(region.position, part.key).is_some()
        {
            return Err(invalid("Conflicting kept art position"));
        }
    }
    for (selector, group) in regions {
        let slot = if let Some(index) = slots.iter().position(|slot| slot.0 == selector) {
            &mut slots[index].1
        } else {
            slots.push((selector, Vec::new()));
            &mut slots.last_mut().unwrap().1
        };
        place_region(slot, group.positions, group.native)?;
    }
    Ok(())
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
                region: kept_region(part)?,
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
                    .find(|part| part.region.is_none() && part.assignment == *slot_key)
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
        place_regions(slots, kept)
    })
}
/// Checks every imported graph against the fingerprint taken when it was selected. Each
/// graph hashes tens of megabytes of its own files, so the graphs are checked side by side.
/// The build checks before [`apply`] links them and again after.
pub(super) fn validate_graphs(weapons: &[WeaponCloneSpec]) -> AuthoringResult<()> {
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
) -> AuthoringResult<(Vec<ReplacementSpec>, Timings)> {
    let mut timings = Timings::default();
    let started = std::time::Instant::now();
    // Native runtime cache replay can append whole packages without reserving new groups.
    // Claim that complete native prefix before any importer or map chooses its package IDs.
    if crate::package_profile::host().is_standalone() {
        for package in &emission.asset_packages.packages {
            crate::mod_family::build::native(package.id);
        }
    }
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
            } else if graph.get("gear_art").is_some()
                && !(spec.kind == ItemKind::Weapon && graph["kind"] == "weapon")
            {
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
                graph.attachments.iter().map(move |attachment| {
                    (
                        attachment.directory.as_path(),
                        (weapon.kind == ItemKind::Armor).then_some(weapon),
                    )
                }),
            )
        })
        .collect::<Vec<_>>();
    timings.read = started.elapsed();
    if folders.is_empty() {
        return Ok((Vec::new(), timings));
    }
    let mut shared = Shared {
        companions: linking::Companions::new(),
        characters: BTreeMap::new(),
        timings,
    };
    let mut replacements = BTreeMap::new();
    // Custom shader dyes already extend the dye table, so imported dyes follow them.
    if let Some(table) = &emission.dye_table {
        replacements.insert(table.tag.0, table.payload.clone());
    }
    let accounted =
        |timings: &Timings| timings.read + timings.nodes + timings.companions + timings.link;
    for (folder, spec) in folders {
        let started = std::time::Instant::now();
        let before = accounted(&shared.timings);
        let applied = apply_one(
            directory,
            emission,
            folder,
            &replacements,
            manager,
            &mut shared,
            spec,
        )
        .map_err(|error| error.context(format!("Imported graph {}", folder.display())))?;
        for replacement in applied {
            replacements.insert(replacement.tag.0, replacement.payload);
        }
        let staged = accounted(&shared.timings) - before;
        shared.timings.tables += started.elapsed().saturating_sub(staged);
        shared.timings.graphs += 1;
    }
    let replacements = replacements
        .into_iter()
        .map(|(tag, payload)| ReplacementSpec {
            tag: TagHash(tag),
            payload,
        })
        .collect();
    Ok((replacements, shared.timings))
}
/// One node of an imported asset graph, with its payload read from `folder` and prepared:
/// model draw indices declared and moved markers applied.
fn graph_node(
    folder: &Path,
    n: &Value,
    model: bool,
    spec: Option<&WeaponCloneSpec>,
    bound: &mut BTreeMap<String, Vec<u8>>,
    manager: &sundial::package_authoring::PackageManager,
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
    } else if let Some(bytes) = bound.remove(name) {
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
    if model {
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
    let mut patches = n["patches"]
        .as_array()
        .ok_or_else(|| invalid("Fixups missing"))?
        .clone();
    if !is_companion
        && manager
            .get_entry(TagHash(template))
            .is_some_and(|entry| entry.reference == 0x80809c36)
        && let Some(fixed) = parhelion_import::tiger::instance::upgrade_owner(&payload, &patches)
            .map_err(|error| invalid(format!("Imported component {name}: {error:#}")))?
    {
        payload = fixed.payload;
        patches = fixed.patches;
    }
    let mut node = linking::Node::new(name, template, payload);
    node.audio = serde_json::from_value(n.clone())
        .map_err(|error| invalid(format!("Imported graph audio: {error}")))?;
    node.storage = match n["storage"].as_str() {
        None => crate::NewTagStorageMode::InheritTemplate,
        Some("audio_media") => crate::NewTagStorageMode::AudioMedia,
        Some("audio_bank") => crate::NewTagStorageMode::AudioBank,
        Some("gpu_hull") => crate::NewTagStorageMode::GpuHull,
        Some("gpu_domain") => crate::NewTagStorageMode::GpuDomain,
        Some(_) => return Err(invalid("Unsupported imported asset storage")),
    };
    for p in &patches {
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
    shared: &mut Shared,
    spec: Option<&WeaponCloneSpec>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let started = std::time::Instant::now();
    let mut graph: Value = serde_json::from_slice(
        &fs::read(folder.join("asset-graph.json")).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    shared.timings.read += started.elapsed();
    if graph["source"] == "d1_roi"
        && graph["kind"] == "armor"
        && graph["converter_revision"].as_u64().unwrap_or(0) < 48
    {
        return Err(invalid(
            "Reimport this D1 armor and its ornaments. Older prepared assets use experimental shader color bindings",
        ));
    }
    if graph["source"] == "halo_reach"
        && graph["animation"]["equipment_skeleton"]["source_parts"]["placement"] == "Trailing"
        && (graph["animation"]["equipment_skeleton"]["source_parts"]["layout"].is_null()
            || graph["animation"]["equipment_skeleton"]["source_parts"]["layout"]
                == "trailing procedural range merged into its FK section")
    {
        return Err(invalid(
            "Import this Reach weapon again. Its donor's moving parts lost their procedural bones, which draws nothing on the inspect screen",
        ));
    }
    if graph["kind"] == "shader" {
        shader::bind(
            &mut graph,
            spec.ok_or_else(|| invalid("Source shader recipe missing"))?,
        )?;
    } else if graph.get("gear_art").is_some() {
        gear::bind(
            &mut graph,
            spec.ok_or_else(|| invalid("Imported gear recipe missing"))?,
            emission,
        )?;
    }
    let json_nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| invalid("Asset nodes missing"))?;
    let branding = crate::branding::Branding::for_packages(directory);
    let shader = graph["kind"] == "shader";
    let origin = crate::package_profile::host()
        .is_standalone()
        .then(|| crate::mod_family::build::graph_origin(&graph, spec))
        .flatten();
    let cache_key = crate::mod_family::build::group_key(folder, spec);
    let reused = origin.and_then(|origin| crate::mod_family::build::group(origin, &cache_key));
    let linked = if let Some(symbols) = reused {
        let owner = origin.unwrap();
        let ids = symbols
            .values()
            .map(|tag| tag.pkg_id())
            .collect::<BTreeSet<_>>();
        for id in ids {
            crate::mod_family::build::use_id(owner, id);
            if !emission
                .asset_packages
                .packages
                .iter()
                .any(|package| package.id == id)
            {
                emission
                    .asset_packages
                    .packages
                    .push(crate::mod_family::build::restore(id).map_err(invalid)?);
            }
        }
        let id = symbols
            .values()
            .next()
            .ok_or_else(|| invalid("Cached imported graph has no symbols"))?
            .pkg_id();
        let package_index = emission
            .asset_packages
            .packages
            .iter()
            .position(|package| package.id == id)
            .unwrap();
        linking::Linked {
            symbols,
            package_index,
        }
    } else {
        emission.asset_packages.owner = origin;
        let linked = (|| {
            let started = std::time::Instant::now();
            let mut nodes = Vec::with_capacity(json_nodes.len());
            let mut bound = parhelion_import::tiger::material::bind(&graph, folder)
                .map_err(|e| invalid(format!("Imported model bindings: {e:#}")))?;
            let models =
                parhelion_import::model_symbols(&graph).map_err(|e| invalid(e.to_string()))?;
            for n in json_nodes {
                let model = n["symbol"]
                    .as_str()
                    .is_some_and(|name| models.contains(name));
                nodes.push(graph_node(folder, n, model, spec, &mut bound, manager)?);
            }
            shared.timings.nodes += started.elapsed();
            let started = std::time::Instant::now();
            loading::replication_companions(
                manager,
                directory,
                &mut nodes,
                &mut shared.companions,
            )?;
            let extra_bounds = if crate::package_profile::host().is_standalone() {
                0
            } else {
                ornament_icon::entry_bound(manager, emission, &graph, folder, branding)?
            };
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
            shared.timings.companions += started.elapsed();
            let started = std::time::Instant::now();
            let linked = linking::link(
                directory,
                emission,
                manager,
                nodes,
                primary,
                &mut shared.companions,
                extra_bounds,
                |symbols, groups| {
                    loading::include_native_resources(
                        manager, folder, json_nodes, symbols, groups,
                    )?;
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
                // Linked payloads are written only on request, to compare with staged packages.
                std::env::var_os("PARHELION_KEEP_LINKED")
                    .is_some()
                    .then(|| folder.join("linked"))
                    .as_deref(),
            )?;
            shared.timings.link += started.elapsed();
            Ok::<_, crate::AuthoringError>(linked)
        })();
        emission.asset_packages.owner = None;
        let linked = linked?;
        if let Some(origin) = origin {
            crate::mod_family::build::remember_group(origin, cache_key, &linked.symbols);
        }
        linked
    };
    let linking::Linked {
        symbols,
        package_index,
    } = linked;
    if shader {
        let replacement = dyes::apply(manager, emission, &symbols, &graph, previous)?
            .ok_or_else(|| invalid("Imported shader has no dye rows"))?;
        return Ok(vec![replacement]);
    }
    gear::collections(emission, &graph, spec)?;
    if graph.get("gear_art").is_some() {
        let mut result = gear::apply(manager, emission, &symbols, &graph, previous)?;
        ornaments::apply(emission, &graph)?;
        ornament_icon::apply(manager, emission, &graph, package_index, folder, branding)?;
        result.extend(character::apply(
            manager,
            emission,
            &graph,
            folder,
            previous,
            &mut shared.characters,
        )?);
        return Ok(result);
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
    if origin.is_none() || !crate::mod_family::build::is_kept(origin.unwrap()) {
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
    }
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
    arrays::add_array_markers(definition).map_err(invalid)?;

    ornaments::apply(emission, &graph)?;
    ornament_icon::apply(manager, emission, &graph, package_index, folder, branding)?;
    ornament_collections::apply(emission, &graph)?;
    result.extend(character::apply(
        manager,
        emission,
        &graph,
        folder,
        previous,
        &mut shared.characters,
    )?);
    Ok(result)
}
