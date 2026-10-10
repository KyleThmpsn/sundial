//! A projectile swap: the patches at every place a graph names the stock projectile,
//! each naming one private copy of the replacement.
use super::*;

/// Most levels of graphs a copy follows below its source: the graphs an ability spawns, and
/// theirs.
pub(super) const SPAWN_DEPTH: usize = crate::subclass::SPAWN_DEPTH;

/// Where an entity graph keeps the client's object type, and the type of a projectile.
pub(super) const OBJECT_TYPE: usize = 0x96;
pub(super) const PROJECTILE: u8 = 18;

/// The patches that make each swap's graph spawn a private copy of its replacement wherever it
/// spawned the replaced projectile, by graph. Each replacement is copied once, with the graphs
/// below it that `colors` recolors and `values` changes for it. Refuses a swap whose graph spawns
/// no such projectile, and one between graphs that are not both projectiles.
pub(in crate::item) fn swap_patches(
    manager: &PackageManager,
    swaps: &[crate::subclass::SpawnSwap],
    (colors, values): (
        &BTreeMap<u32, palettes::ColorPatches>,
        &BTreeMap<u32, Vec<WeaponRuntimeValueOverride>>,
    ),
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>> {
    let mut patches = BTreeMap::<u32, Vec<WeaponRuntimeResourcePatch>>::new();
    let mut copies = BTreeMap::<u32, TagHash>::new();
    for swap in swaps {
        for tag in [swap.replaced, swap.replacement] {
            let payload = read_tag(manager, TagHash(tag), "swapped projectile")?;
            if payload.get(OBJECT_TYPE) != Some(&PROJECTILE) {
                return Err(invalid(format!("Graph 0x{tag:08X} is not a projectile")));
            }
        }
        let payload = read_tag(manager, TagHash(swap.graph), "spawning graph")?;
        let places =
            sundial::package_authoring::ability_spawns::spawns(manager, swap.graph, &payload)
                .map_err(invalid)?
                .into_iter()
                .filter(|place| place.graph == swap.replaced)
                .collect::<Vec<_>>();
        if places.is_empty() {
            return Err(invalid(format!(
                "Graph 0x{:08X} spawns no projectile 0x{:08X}",
                swap.graph, swap.replaced
            )));
        }
        let copy = match copies.get(&swap.replacement) {
            Some(copy) => *copy,
            None => {
                let values = values.get(&swap.replacement).map_or(&[][..], Vec::as_slice);
                let colors = colors
                    .get(&swap.replacement)
                    .filter(|each| !each.is_empty());
                let copy = if values.is_empty() && colors.is_none() {
                    append_private_patched_graph(
                        manager,
                        TagHash(swap.replacement),
                        &[],
                        &[],
                        &[],
                        allocator,
                        tags,
                    )?
                } else {
                    append_private_graph_tree(
                        manager,
                        TagHash(swap.replacement),
                        values,
                        colors.unwrap_or(&BTreeMap::new()),
                        &BTreeMap::new(),
                        allocator,
                        tags,
                    )?
                };
                copies.insert(swap.replacement, copy);
                copy
            }
        };
        for place in places {
            patches
                .entry(swap.graph)
                .or_default()
                .push(WeaponRuntimeResourcePatch {
                    binding_hash: place.binding_hash,
                    resource_index: place.resource_index,
                    offset: place.offset,
                    bytes: copy.0.to_le_bytes().to_vec(),
                    graph_values: Vec::new(),
                    graph_removals: Vec::new(),
                    graph_trajectories: None,
                });
        }
    }
    Ok(patches)
}
