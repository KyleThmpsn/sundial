//! Recolored effects of an authored ability. Each edited palette gets a private copy with its own
//! or another stock palette's pixels, recolored, and each edited tint changes the color constants
//! that hold it. Every material that binds such a palette or holds such a constant gets a copy with
//! the change, its external constant buffer a copy where the constant sits there, and every
//! particle system drawing with a changed material a copy naming it. The copies take an asset
//! group of their own and are placed for the runtime dependency index. The ability's graph copies
//! then name the private systems through resource patches, so the stock ability and every other
//! effect drawing with the same colors keep them. A grade over every effect gives each particle
//! effect's material a private copy of its pixel program that grades the color it draws.
use super::*;
use crate::subclass::{EffectGrade, PaletteEdit, TintEdit};
use sundial::package_authoring::ability_materials::Routes;
use sundial::package_authoring::ability_palette::{
    self, Graphs, MATERIAL_CLASS, ParticleSite, SYSTEM_MATERIAL, binding_tag_offset,
    palettes_in_graphs, particle_sites,
};
use sundial::package_authoring::ability_tint::{ConstantStore, tints_in_graphs};

/// Where a material names its pixel program, and its external constant buffer.
const PIXEL_PROGRAM: usize = 0x2C8;
const EXTERNAL_CONSTANTS: usize = PIXEL_PROGRAM + 0x84;
/// A pixel program's header: 40 bytes, the bytecode's length at `+8`, no large buffer at `+12`.
const PROGRAM_HEADER_SIZE: usize = 40;
/// Where a material selects its render states. The low byte, with its high bit set, selects a
/// blend state by its index in the client's table of 90.
const STATES: usize = 0x20;
/// Blend states whose render target 0 multiplies, subtracts or takes the smaller of the drawn
/// color and what is behind it, where white or gray means no change: `DEST_COLOR` with `ZERO`,
/// `SRC_COLOR` or `ONE`, `MIN` and `REV_SUBTRACT`. Decals of class `80806E53` use 3 and 76. From
/// Alkahest prebl-0.5 `BLEND_STATE_DESCS` and a 2026-10-05 survey of the stock abilities'
/// materials.
const NEUTRAL_BLENDS: [u8; 7] = [3, 4, 9, 11, 76, 77, 89];
/// The dual-source blend state, whose second output weighs the first rather than drawing a
/// color, and which the grade leaves.
const DUAL_SOURCE_BLEND: u8 = 86;

/// One private copy: its stock template, payload, the copy its package entry names, and the
/// places in its payload that name another copy.
struct Node {
    template: u32,
    payload: Vec<u8>,
    reference: Option<usize>,
    patches: Vec<(usize, usize)>,
}

/// The copies being made, each stock tag once.
#[derive(Default)]
struct Copies {
    nodes: Vec<Node>,
    by_stock: BTreeMap<u32, usize>,
}

impl Copies {
    /// The copy of `stock`, made from its stock payload the first time it is asked for.
    fn of(&mut self, manager: &PackageManager, stock: u32, what: &str) -> AuthoringResult<usize> {
        if let Some(index) = self.by_stock.get(&stock) {
            return Ok(*index);
        }
        let payload = read_tag(manager, TagHash(stock), what)?;
        Ok(self.insert(stock, payload))
    }

    /// Keep any earlier palette or tint edits when the source payload is already available.
    fn insert(&mut self, stock: u32, payload: Vec<u8>) -> usize {
        if let Some(index) = self.by_stock.get(&stock) {
            return *index;
        }
        self.nodes.push(Node {
            template: stock,
            payload,
            reference: None,
            patches: Vec::new(),
        });
        self.by_stock.insert(stock, self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// A header and data pair copied together, naming each other. Returns the header's copy.
    fn pair(
        &mut self,
        manager: &PackageManager,
        (header, data): (u32, u32),
        what: &str,
    ) -> AuthoringResult<usize> {
        if let Some(index) = self.by_stock.get(&header) {
            return Ok(*index);
        }
        let data_copy = self.of(manager, data, what)?;
        let header_copy = self.of(manager, header, what)?;
        self.nodes[data_copy].reference = Some(header_copy);
        self.nodes[header_copy].reference = Some(data_copy);
        Ok(header_copy)
    }

    /// A header and data pair copied together, naming each other, apart from any other copy of
    /// them, as each variant of a graded program is. Returns the header's copy.
    fn fresh_pair(&mut self, header: (u32, Vec<u8>), data: (u32, Vec<u8>)) -> usize {
        for (template, payload) in [data, header] {
            self.nodes.push(Node {
                template,
                payload,
                reference: None,
                patches: Vec::new(),
            });
        }
        let header_copy = self.nodes.len() - 1;
        let data_copy = header_copy - 1;
        self.nodes[data_copy].reference = Some(header_copy);
        self.nodes[header_copy].reference = Some(data_copy);
        header_copy
    }

    fn patch(&mut self, node: usize, offset: usize, target: usize) {
        if !self.nodes[node].patches.iter().any(|(at, _)| *at == offset) {
            self.nodes[node].patches.push((offset, target));
        }
    }
}

/// Writes `rgb` over the x, y and z of the constant at `offset`.
fn write_rgb(payload: &mut [u8], offset: usize, rgb: [f32; 3]) -> AuthoringResult<()> {
    for (channel, value) in rgb.into_iter().enumerate() {
        if !value.is_finite() {
            return Err(invalid("A recolored tint is not a finite color"));
        }
        write_u32(payload, offset + channel * 4, value.to_bits())?;
    }
    Ok(())
}

/// The copy of pixel program `pixel`'s header, naming a copy of its bytecode graded by
/// `program`. None for a program the grade leaves as it is, or whose header and bytecode are not
/// the pair this reads.
fn graded_program(
    manager: &PackageManager,
    copies: &mut Copies,
    pixel: u32,
    program: crate::dxbc::grade::Grade,
) -> AuthoringResult<Option<usize>> {
    if [0, u32::MAX].contains(&pixel) {
        return Ok(None);
    }
    let Some(data) = manager
        .get_entry(TagHash(pixel))
        .filter(|entry| entry.file_type == 33 && entry.file_subtype == 0)
        .map(|entry| entry.reference)
    else {
        return Ok(None);
    };
    if manager.get_entry(TagHash(data)).is_none_or(|entry| {
        entry.file_type != 41 || entry.file_subtype != 0 || entry.reference != pixel
    }) {
        return Ok(None);
    }
    let mut header = read_tag(manager, TagHash(pixel), "effect pixel program")?;
    let code = read_tag(manager, TagHash(data), "effect pixel bytecode")?;
    if header.len() != PROGRAM_HEADER_SIZE
        || read_u64(&header, 0)? != PROGRAM_HEADER_SIZE as u64
        || read_u32(&header, 8)? as usize != code.len()
        || read_u32(&header, 12)? != u32::MAX
        || header[16..].iter().any(|byte| *byte != 0)
    {
        return Ok(None);
    }
    let Some(code) = crate::dxbc::grade::grade(&code, program)
        .map_err(|error| error.context(format!("Pixel program 0x{pixel:08X}")))?
    else {
        return Ok(None);
    };
    let length =
        u32::try_from(code.len()).map_err(|_| invalid("A graded pixel program is too large"))?;
    write_u32(&mut header, 8, length)?;
    Ok(Some(copies.fresh_pair((pixel, header), (data, code))))
}

/// The resource patches that make each graph of a tree name private copies, by graph.
pub(in crate::item) type ColorPatches = BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>;

/// A place in a graph's bound resources: its binding, resource index and byte offset.
type Place = (u32, u16, u32);

/// Each changed material with the particle site that reaches it: root, graph, site, material.
type Reached = Vec<(usize, u32, ParticleSite, u32)>;

/// Each place a route to a changed material starts: root, graph, place, first copy.
type Routed = Vec<(usize, u32, Place, usize)>;

/// Authors a recolored copy of each palette `palettes` changes below `source` and points every
/// material drawing with it at the copy, recording the materials it reaches.
fn author_palettes(
    manager: &PackageManager,
    source: TagHash,
    palettes: &[PaletteEdit],
    graphs: &[(u32, Vec<u8>)],
    (copies, reached): (&mut Copies, &mut Reached),
) -> AuthoringResult<()> {
    let found = palettes_in_graphs(manager, graphs).map_err(invalid)?;
    let mut headers = BTreeMap::<u32, usize>::new();
    for edit in palettes {
        let palette = found
            .iter()
            .find(|palette| palette.header == edit.palette)
            .ok_or_else(|| {
                invalid(format!(
                    "Its Effect Colors change palette 0x{:08X}, which no effect of {source} draws. Remove the change on the Visuals tab.",
                    edit.palette
                ))
            })?;
        // A taken palette must be one too, so its pixels fit the copy's header.
        let mut pixels =
            ability_palette::palette_pixels(manager, edit.source()).map_err(invalid)?;
        edit.apply(&mut pixels);
        let header = copies.pair(manager, (palette.header, palette.data), "palette texture")?;
        let data = copies.nodes[header]
            .reference
            .ok_or_else(|| validation("A palette copy lost the data it names"))?;
        copies.nodes[data].payload = pixels;
        headers.insert(palette.header, header);
        for palette_use in &palette.uses {
            let material = copies.of(manager, palette_use.material, "effect material")?;
            let payload = &copies.nodes[material].payload;
            let at = binding_tag_offset(payload, palette_use.binding).map_err(invalid)?;
            let stock_header = read_u32(payload, at)?;
            let target = *headers
                .get(&stock_header)
                .ok_or_else(|| validation("A material binds a palette the build did not copy"))?;
            copies.patch(material, at, target);
            reached.push((0, palette_use.graph, palette_use.site, palette_use.material));
        }
    }
    Ok(())
}

/// Writes each tint `tints` changes below `source` into copies of the materials or constant
/// buffers holding it, recording the materials it reaches.
fn author_tints(
    manager: &PackageManager,
    source: TagHash,
    tints: &[TintEdit],
    graphs: &[(u32, Vec<u8>)],
    (copies, reached): (&mut Copies, &mut Reached),
) -> AuthoringResult<()> {
    let found = tints_in_graphs(manager, graphs).map_err(invalid)?;
    for edit in tints {
        let tint = found
            .iter()
            .find(|tint| edit.starts_from(tint.rgb))
            .ok_or_else(|| {
                invalid(format!(
                    "Its Effect Colors change a color no effect of {source} draws. Remove the change on the Visuals tab."
                ))
            })?;
        let rgb = edit.apply(tint.rgb);
        for tint_use in &tint.uses {
            let material = copies.of(manager, tint_use.material, "effect material")?;
            match tint_use.store {
                ConstantStore::Inline => {
                    write_rgb(
                        &mut copies.nodes[material].payload,
                        tint_use.constant.offset,
                        rgb,
                    )?;
                }
                ConstantStore::External { header, data } => {
                    let buffer = copies.pair(manager, (header, data), "effect constant buffer")?;
                    let data = copies.nodes[buffer].reference.ok_or_else(|| {
                        validation("A constant buffer copy lost the data it names")
                    })?;
                    write_rgb(
                        &mut copies.nodes[data].payload,
                        tint_use.constant.offset,
                        rgb,
                    )?;
                    copies.patch(material, EXTERNAL_CONSTANTS, buffer);
                }
            }
            reached.push((0, tint_use.graph, tint_use.site, tint_use.material));
        }
    }
    Ok(())
}

/// Grades materials for `grade`, each pixel program once for each kind of blend, since many
/// materials share one.
struct Grader<'a> {
    grade: EffectGrade,
    graded: BTreeMap<(u32, bool), Option<usize>>,
    materials: BTreeMap<u32, Option<usize>>,
    systems: BTreeMap<u32, u32>,
    routes: Routes<'a>,
}

impl Grader<'_> {
    /// The graded copy of `material`, or `None` for one that is not an effect material or whose
    /// blend or program cannot be graded.
    fn material(
        &mut self,
        manager: &PackageManager,
        copies: &mut Copies,
        material: u32,
    ) -> AuthoringResult<Option<usize>> {
        if let Some(copy) = self.materials.get(&material) {
            return Ok(*copy);
        }
        let copy = self.copy_material(manager, copies, material)?;
        self.materials.insert(material, copy);
        Ok(copy)
    }

    fn copy_material(
        &mut self,
        manager: &PackageManager,
        copies: &mut Copies,
        material: u32,
    ) -> AuthoringResult<Option<usize>> {
        if manager
            .get_entry(TagHash(material))
            .is_none_or(|entry| entry.reference != MATERIAL_CLASS)
        {
            return Ok(None);
        }
        let payload = read_tag(manager, TagHash(material), "effect material")?;
        let pixel = read_u32(&payload, PIXEL_PROGRAM)?;
        let blend = payload
            .get(STATES)
            .filter(|byte| **byte & 0x80 != 0)
            .map(|byte| byte & 0x7F);
        if blend == Some(DUAL_SOURCE_BLEND) {
            return Ok(None);
        }
        let neutral = blend.is_some_and(|blend| NEUTRAL_BLENDS.contains(&blend));
        let header = match self.graded.get(&(pixel, neutral)) {
            Some(header) => *header,
            None => {
                let program = if neutral {
                    self.grade.neutral_program()
                } else {
                    self.grade.program()
                };
                let header = graded_program(manager, copies, pixel, program)?;
                self.graded.insert((pixel, neutral), header);
                header
            }
        };
        let Some(header) = header else {
            return Ok(None);
        };
        let copy = copies.insert(material, payload);
        copies.patch(copy, PIXEL_PROGRAM, header);
        Ok(Some(copy))
    }

    /// Grades what `graph`, in the tree of root `root`, draws: its particle systems' materials,
    /// recorded in `reached`, and the materials its models, lights, decals and other resources
    /// name, recorded in `routed`. Each resource on a route gets a copy naming the next.
    fn graph(
        &mut self,
        manager: &PackageManager,
        (root, graph, payload): (usize, u32, &[u8]),
        (copies, reached, routed): (&mut Copies, &mut Reached, &mut Routed),
    ) -> AuthoringResult<()> {
        for site in particle_sites(manager, payload).map_err(invalid)? {
            let material = match self.systems.get(&site.system) {
                Some(material) => *material,
                None => {
                    let system = read_tag(manager, TagHash(site.system), "effect particle system")?;
                    let material = read_u32(&system, SYSTEM_MATERIAL)?;
                    self.systems.insert(site.system, material);
                    material
                }
            };
            if self.material(manager, copies, material)?.is_some() {
                reached.push((root, graph, site, material));
            }
        }
        for route in self.routes.get(graph, payload).map_err(invalid)? {
            let Some(mut next) = self.material(manager, copies, route.material())? else {
                continue;
            };
            for (resource, offsets) in route.chain.iter().rev().skip(1) {
                let copy = copies.of(manager, *resource, "effect resource")?;
                for offset in offsets {
                    copies.patch(copy, *offset, next);
                }
                next = copy;
            }
            let place = (route.binding_hash, route.resource_index, route.offset);
            routed.push((root, graph, place, next));
        }
        Ok(())
    }
}

/// Authors the private copies `palettes`, `tints` and `grade` need below `source`, the
/// ability's entity, and returns the resource patches that make each graph of its tree name
/// them. The grade also reaches the trees of `swapped`, projectiles a swap fires in place of
/// stock ones, whose patches come back by projectile.
pub(in crate::item) fn author(
    manager: &PackageManager,
    source: TagHash,
    (palettes, tints, grade): (&[PaletteEdit], &[TintEdit], Option<EffectGrade>),
    swapped: &[u32],
    graphs: &mut Graphs<'_>,
    (packages, placed): (&mut crate::asset_packages::AssetPackages, &mut Vec<TagHash>),
) -> AuthoringResult<(ColorPatches, BTreeMap<u32, ColorPatches>)> {
    if palettes.is_empty() && tints.is_empty() && grade.is_none() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    let mut copies = Copies::default();
    // Each changed material with the particle sites that reach it, by the root whose tree it is
    // in: the ability's own, then each swapped projectile's.
    let mut reached = Reached::new();
    // Each place a route to a changed material starts, with the copy of its first resource.
    let mut routed = Routed::new();
    if !palettes.is_empty() {
        author_palettes(
            manager,
            source,
            palettes,
            &graphs.get(source.0).map_err(invalid)?,
            (&mut copies, &mut reached),
        )?;
    }
    if !tints.is_empty() {
        author_tints(
            manager,
            source,
            tints,
            &graphs.get(source.0).map_err(invalid)?,
            (&mut copies, &mut reached),
        )?;
    }
    if let Some(grade) = grade {
        let mut grader = Grader {
            grade,
            graded: BTreeMap::new(),
            materials: BTreeMap::new(),
            systems: BTreeMap::new(),
            routes: Routes::new(manager),
        };
        let roots = std::iter::once(source.0).chain(swapped.iter().copied());
        for (root, tag) in roots.enumerate() {
            let tree = graphs.get(tag).map_err(invalid)?;
            for (graph, payload) in tree.iter() {
                grader.graph(
                    manager,
                    (root, *graph, payload),
                    (&mut copies, &mut reached, &mut routed),
                )?;
            }
        }
    }
    // Each system drawing with a changed material, named by the material's copy.
    let mut systems = BTreeMap::<u32, usize>::new();
    for (_, _, site, material) in &reached {
        if systems.contains_key(&site.system) {
            continue;
        }
        let system = copies.of(manager, site.system, "effect particle system")?;
        if read_u32(&copies.nodes[system].payload, SYSTEM_MATERIAL)? != *material {
            return Err(invalid(format!(
                "Particle system 0x{:08X} no longer draws with material 0x{material:08X}",
                site.system
            )));
        }
        let copy = copies.by_stock[material];
        copies.patch(system, SYSTEM_MATERIAL, copy);
        systems.insert(site.system, system);
    }
    let Copies { nodes, .. } = copies;
    let index = packages.reserve_group(nodes.iter().map(|node| node.payload.len()))?;
    let package = &mut packages.packages[index];
    let allocator = AppendedTagAllocator::new(package.id, 0);
    let base = package.tags.len();
    let tags = (0..nodes.len())
        .map(|ordinal| allocator.assigned_tag(base + ordinal, "Effect color copy", "effect asset"))
        .collect::<AuthoringResult<Vec<_>>>()?;
    for (ordinal, mut node) in nodes.into_iter().enumerate() {
        for (offset, target) in &node.patches {
            write_u32(&mut node.payload, *offset, tags[*target].0)?;
        }
        if let Some(target) = node.reference {
            package.references.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: base + ordinal,
                reference: crate::NewTagReference::Appended(tags[target].entry_index() as usize),
            });
        }
        package.tags.push(NewTagSpec {
            template_tag: TagHash(node.template),
            payload: node.payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        placed.push(tags[ordinal]);
    }
    // Each root's graphs name the copies at their places, each place once.
    let mut by_root = vec![ColorPatches::new(); 1 + swapped.len()];
    let places = reached
        .into_iter()
        .map(|(root, graph, site, _)| {
            let place = (site.binding_hash, site.resource_index, site.offset);
            (root, graph, place, systems[&site.system])
        })
        .chain(routed);
    for (root, graph, (binding_hash, resource_index, offset), copy) in places {
        let graph = by_root[root].entry(graph).or_default();
        if graph.iter().any(|patch| {
            (patch.binding_hash, patch.resource_index, patch.offset)
                == (binding_hash, resource_index, offset)
        }) {
            continue;
        }
        graph.push(WeaponRuntimeResourcePatch {
            binding_hash,
            resource_index,
            offset,
            bytes: tags[copy].0.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    let mut roots = by_root.into_iter();
    let own = roots.next().unwrap_or_default();
    let swapped = swapped.iter().copied().zip(roots).collect();
    Ok((own, swapped))
}
