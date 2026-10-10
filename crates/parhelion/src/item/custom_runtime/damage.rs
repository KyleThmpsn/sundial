//! A damage type: a private copy of each damage profile the graphs below a root name, with the
//! damage type changed, and the patches that make each graph of the tree name the copies. A
//! profile shared with other graphs stays as it is for them.
use super::*;
use sundial::package_authoring::ability_damage::{references, retype};
use sundial::package_authoring::ability_palette::Graphs;

/// The private copies of damage profiles made so far, by stock profile and damage type.
type Copies = BTreeMap<(u32, u8), TagHash>;

/// Copies every damage profile the graphs below `source` name, each once, with damage type
/// `mode`, and returns the patches that make each graph name the copies, by graph. The trees of
/// `swapped`, projectiles a swap fires in place of stock ones, take their own damage type, or
/// the ability's when they name none, and their patches come back by projectile. A profile that
/// already deals a tree's type is left alone, and one that two trees change to the same type is
/// copied once.
pub(in crate::item) fn author(
    manager: &PackageManager,
    (source, mode): (TagHash, Option<u8>),
    swapped: &[(u32, Option<u8>)],
    graphs: &mut Graphs<'_>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<(
    palettes::ColorPatches,
    BTreeMap<u32, palettes::ColorPatches>,
)> {
    let roots = std::iter::once((source.0, mode))
        .chain(swapped.iter().map(|&(root, own)| (root, own.or(mode))))
        .collect::<Vec<_>>();
    let mut copies = Copies::new();
    let mut by_root = Vec::with_capacity(roots.len());
    for (root, mode) in roots {
        by_root.push(match mode {
            Some(mode) => {
                tree_patches(manager, (root, mode), graphs, &mut copies, allocator, tags)?
            }
            None => palettes::ColorPatches::new(),
        });
    }
    let mut roots = by_root.into_iter();
    let own = roots.next().unwrap_or_default();
    let swapped = swapped.iter().map(|&(root, _)| root).zip(roots).collect();
    Ok((own, swapped))
}

/// The patches that make every graph below `root` name a private copy of each damage profile it
/// names, dealing `mode`, by graph: one projectile or asset retyped on its own.
pub(in crate::item) fn retyped(
    manager: &PackageManager,
    (root, mode): (u32, u8),
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<palettes::ColorPatches> {
    let mut graphs = Graphs::new(manager, SPAWN_DEPTH);
    tree_patches(
        manager,
        (root, mode),
        &mut graphs,
        &mut Copies::new(),
        allocator,
        tags,
    )
}

fn tree_patches(
    manager: &PackageManager,
    (root, mode): (u32, u8),
    graphs: &mut Graphs<'_>,
    copies: &mut Copies,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<palettes::ColorPatches> {
    let mut patches = palettes::ColorPatches::new();
    let tree = graphs.get(root).map_err(invalid)?;
    for (graph, payload) in tree.iter() {
        for (place, profile) in references(manager, *graph, payload).map_err(invalid)? {
            if profile.mode == mode {
                continue;
            }
            let copy = match copies.get(&(place.graph, mode)) {
                Some(copy) => *copy,
                None => {
                    let copy = allocator.assigned_tag(
                        tags.len(),
                        "Private damage profile",
                        "damage profile",
                    )?;
                    let mut payload = read_tag(manager, TagHash(place.graph), "damage profile")?;
                    retype(&mut payload, profile, copy.0, mode).map_err(invalid)?;
                    tags.push(NewTagSpec {
                        template_tag: TagHash(place.graph),
                        payload,
                        storage: crate::NewTagStorageMode::InheritTemplate,
                    });
                    copies.insert((place.graph, mode), copy);
                    copy
                }
            };
            let graph_patches = patches.entry(*graph).or_default();
            if graph_patches.iter().any(|patch| {
                (patch.binding_hash, patch.resource_index, patch.offset)
                    == (place.binding_hash, place.resource_index, place.offset)
            }) {
                continue;
            }
            graph_patches.push(WeaponRuntimeResourcePatch {
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
