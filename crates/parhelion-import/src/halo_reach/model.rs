use super::{
    cache::{Cache, Tag},
    material::{self, Material},
    resource::{Pages, Resource},
    rig::{self, Bone, METRES_PER_UNIT},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
    /// Water body and global-shape coordinates, indexed by source triangle corner.
    #[serde(skip)]
    pub water_uv: Option<[f32; 2]>,
    pub joints: [u16; 4],
    pub weights: [f32; 4],
    /// The vertex's index in its section's vertex buffer, which other per-vertex streams share.
    #[serde(skip)]
    pub source: u32,
    /// Lightmap texture coordinates, which a level fills where its lightmap covers the vertex.
    #[serde(skip)]
    pub lightmap: Option<[f32; 2]>,
    /// Source per-vertex lobe, ambient and direction. The fourth lanes hold bandwidth and sun.
    #[serde(skip)]
    pub lighting: Option<[[f32; 4]; 3]>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Primitive {
    pub section: usize,
    pub part: usize,
    pub material: Option<usize>,
    pub flags: u16,
    /// MCC structure subparts and instance colorings select analytical lights separately from
    /// their baked VMF samples. Zero disables the analytical term. None has no source selection.
    pub analytical_light: Option<u32>,
    pub instance: Option<String>,
    /// In a level, the scenario's structure BSPs whose loading draws it, bit `n` for the `n`th.
    /// Zero draws it wherever the level draws.
    pub bsps: u32,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    pub name: String,
    pub region: Option<usize>,
    pub permutation: Option<usize>,
    pub bone: Option<usize>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
}
#[derive(Clone, Debug, Serialize)]
pub struct Permutation {
    pub name: String,
    pub sections: Vec<usize>,
    pub instance_mask: [u32; 4],
}
#[derive(Clone, Debug, Serialize)]
pub struct Region {
    pub name: String,
    pub permutations: Vec<Permutation>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Model {
    pub tag: Tag,
    pub bones: Vec<Bone>,
    pub markers: Vec<Marker>,
    pub regions: Vec<Region>,
    pub selected: BTreeMap<String, String>,
    pub materials: Vec<Material>,
    pub primitives: Vec<Primitive>,
}

impl Model {
    pub(super) fn marker_selected(&self, marker: &Marker) -> Result<bool> {
        let Some(region) = marker.region else {
            return Ok(true);
        };
        let region = self.regions.get(region).context("Marker region")?;
        let selected = self.selected.get(&region.name);
        if let Some(permutation) = marker.permutation {
            let permutation = region
                .permutations
                .get(permutation)
                .context("Marker permutation")?;
            Ok(selected == Some(&permutation.name))
        } else {
            Ok(selected.is_some_and(|name| !name.is_empty()))
        }
    }
}

fn nullable(value: u8) -> Option<usize> {
    (value != 255).then_some(value as usize)
}

pub(super) fn read_bones(cache: &Cache, at: usize) -> Result<Vec<Bone>> {
    model_bones(cache, at, true)
}

/// The render model's bones. With `posed`, a model that is posed by its bones must have unit
/// inverse scales and a default pose that matches its bind pose. A model drawn where it stands, in
/// the pose its vertices are stored in, needs neither.
fn model_bones(cache: &Cache, at: usize, posed: bool) -> Result<Vec<Bone>> {
    let mut bones = Vec::new();
    for row in cache.block(at + 48, 96)? {
        let inverse_scale = cache.f32(row + 40)?;
        ensure!(
            !posed || (inverse_scale - 1.).abs() < 0.001,
            "Nonunit inverse bone scale requires explicit conversion"
        );
        let mut inverse_bind = rig::IDENTITY;
        for column in 0..4 {
            for axis in 0..3 {
                inverse_bind[column * 4 + axis] = cache.f32(row + 44 + column * 12 + axis * 4)?
                    * if column == 3 { METRES_PER_UNIT } else { 1. };
            }
        }
        let parent = cache.i16(row + 4)?;
        ensure!(parent >= -1, "Invalid bone parent");
        bones.push(Bone {
            name: cache.string_id(cache.u32(row)?)?,
            parent: (parent >= 0).then_some(parent as usize),
            translation: cache.floats::<3>(row + 12)?.map(|v| v * METRES_PER_UNIT),
            rotation: rig::quaternion(cache.floats(row + 24)?)?,
            inverse_bind,
        });
    }
    if bones.is_empty() {
        bones.push(Bone {
            name: "root".into(),
            parent: None,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            inverse_bind: rig::IDENTITY,
        });
    }
    if !posed {
        return Ok(bones);
    }
    let worlds = rig::world(&bones)?;
    for (bone, world) in bones.iter().zip(&worlds) {
        let check = rig::multiply(world, &bone.inverse_bind);
        ensure!(
            check
                .iter()
                .zip(rig::IDENTITY)
                .all(|(a, b)| (a - b).abs() < 0.025),
            "Local and inverse bind transforms disagree for {}",
            bone.name
        );
    }
    Ok(bones)
}

struct Selection {
    regions: Vec<Region>,
    selected: BTreeMap<String, String>,
    selected_sections: BTreeSet<usize>,
    instance_mask: [u32; 4],
}
fn select(
    cache: &Cache,
    at: usize,
    sections: &[usize],
    choices: &BTreeMap<String, String>,
) -> Result<Selection> {
    let mut regions = Vec::new();
    let mut selected = BTreeMap::new();
    let mut selected_sections = BTreeSet::new();
    let mut instance_mask = [0u32; 4];
    for row in cache.block(at + 12, 16)? {
        let name = cache.string_id(cache.u32(row)?)?;
        let mut permutations = Vec::new();
        for p in cache.block(row + 4, 24)? {
            let start = cache.i16(p + 4)?;
            let count = cache.i16(p + 6)?;
            ensure!(
                count >= 0
                    && (count == 0
                        || start >= 0 && start as usize + count as usize <= sections.len()),
                "Permutation outside sections"
            );
            permutations.push(Permutation {
                name: cache.string_id(cache.u32(p)?)?,
                sections: if count == 0 {
                    Vec::new()
                } else {
                    (start as usize..start as usize + count as usize).collect()
                },
                instance_mask: [
                    cache.u32(p + 8)?,
                    cache.u32(p + 12)?,
                    cache.u32(p + 16)?,
                    cache.u32(p + 20)?,
                ],
            });
        }
        // An empty HLMT permutation names a hidden region, including damage decals
        // on an intact vehicle. It must not select the first damaged draw instead.
        if choices.get(&name).is_some_and(String::is_empty)
            && !permutations.iter().any(|p| p.name.is_empty())
        {
            selected.insert(name.clone(), String::new());
            regions.push(Region { name, permutations });
            continue;
        }
        let chosen = if let Some(choice) = choices.get(&name) {
            permutations
                .iter()
                .find(|p| &p.name == choice)
                .with_context(|| format!("Missing permutation {name}/{choice}"))?
        } else {
            permutations.first().context("Empty model region")?
        };
        selected.insert(name.clone(), chosen.name.clone());
        selected_sections.extend(chosen.sections.iter().copied());
        for (m, v) in instance_mask.iter_mut().zip(chosen.instance_mask) {
            *m |= v;
        }
        regions.push(Region { name, permutations });
    }
    ensure!(
        choices.keys().all(|name| selected.contains_key(name)),
        "Requested region does not exist"
    );
    if regions.is_empty() {
        selected_sections.extend(0..sections.len());
    }
    Ok(Selection {
        regions,
        selected,
        selected_sections,
        instance_mask,
    })
}

/// The vertex format of decorator models: 32 bytes of floats, a position from 0 to 1 across the
/// compression bounds, texture coordinates likewise, and a unit normal, as Reach's
/// `decorator_mesh` vertex program reads them (`POSITION0`, `TEXCOORD0` and `NORMAL0`, the first
/// two scaled and offset by its vertex compression constants).
const DECORATOR_FORMAT: u8 = 15;

/// The bytes a vertex of `format` takes.
fn vertex_stride(format: u8) -> Result<usize> {
    Ok(match format {
        0 | 1 => 36,
        2 => 44,
        DECORATOR_FORMAT => 32,
        n => anyhow::bail!("Unsupported Reach vertex format {n}"),
    })
}

fn decode_vertices(
    data: &[u8],
    format: u8,
    rigid: usize,
    bounds: &[f32; 10],
    palette: &[u8],
    bone_count: usize,
) -> Result<Vec<Vertex>> {
    let stride = vertex_stride(format)?;
    let mut vertices = Vec::with_capacity(data.len() / stride);
    for (source, bytes) in data.chunks_exact(stride).enumerate() {
        let f = |p| f32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
        let unorm = |p| u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) as f32 / 65535.;
        let snorm =
            |p| (i16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) as f32 / 32767.).max(-1.);
        let position = std::array::from_fn(|a| {
            // Compressed formats store positions from 0 to 1 across the compression bounds,
            // even when mesh flag bit 8 claims uncompressed positions.
            let v = f(a * 4);
            (if format == 0 {
                v
            } else {
                bounds[a * 2] + v * (bounds[a * 2 + 1] - bounds[a * 2])
            }) * METRES_PER_UNIT
        });
        let uv = std::array::from_fn(|a| {
            if format == 0 {
                half::f16::from_bits(u16::from_le_bytes(
                    bytes[16 + a * 2..18 + a * 2].try_into().unwrap(),
                ))
                .to_f32()
            } else if format == DECORATOR_FORMAT {
                bounds[6 + a * 2] + f(12 + a * 4) * (bounds[7 + a * 2] - bounds[6 + a * 2])
            } else {
                bounds[6 + a * 2] + unorm(16 + a * 2) * (bounds[7 + a * 2] - bounds[6 + a * 2])
            }
        });
        ensure!(
            position.iter().chain(uv.iter()).all(|v| v.is_finite()),
            "Nonfinite model vertex"
        );
        let (normal, tangent) = if format == DECORATOR_FORMAT {
            // A decorator vertex keeps no tangent, so any direction across its normal serves.
            let normal = rig::normalize([f(20), f(24), f(28)])?;
            let across = if normal[2].abs() < 0.9 {
                [0., 0., 1.]
            } else {
                [1., 0., 0.]
            };
            let t = rig::normalize([
                across[1] * normal[2] - across[2] * normal[1],
                across[2] * normal[0] - across[0] * normal[2],
                across[0] * normal[1] - across[1] * normal[0],
            ])?;
            (normal, [t[0], t[1], t[2], 1.])
        } else {
            let normal = rig::normalize([snorm(20), snorm(22), snorm(24)])?;
            let t = rig::normalize([snorm(28), snorm(30), snorm(32)])?;
            (
                normal,
                [t[0], t[1], t[2], if snorm(34) < 0. { -1. } else { 1. }],
            )
        };
        let mut joints = [u16::try_from(rigid)?; 4];
        let mut weights = [1., 0., 0., 0.];
        if format == 2 {
            let sum = bytes[40..44].iter().map(|&w| u16::from(w)).sum::<u16>();
            ensure!(
                sum > 0 && (i32::from(sum) - 255).abs() <= 2,
                "Invalid source skin weights"
            );
            for lane in 0..4 {
                let b = bytes[36 + lane];
                joints[lane] = if palette.is_empty() {
                    u16::from(b)
                } else if bytes[40 + lane] == 0 && b as usize >= palette.len() {
                    0
                } else {
                    u16::from(
                        *palette
                            .get(b as usize)
                            .context("Bone outside section palette")?,
                    )
                };
                weights[lane] = bytes[40 + lane] as f32 / sum as f32;
                if weights[lane] == 0. && joints[lane] as usize >= bone_count {
                    joints[lane] = 0;
                }
            }
        }
        ensure!(
            joints.iter().all(|&b| (b as usize) < bone_count),
            "Skin bone outside skeleton"
        );
        vertices.push(Vertex {
            water_uv: None,
            position,
            normal,
            tangent,
            uv,
            joints,
            weights,
            source: u32::try_from(source)?,
            lightmap: None,
            lighting: None,
        });
    }
    Ok(vertices)
}

/// A render geometry block: the struct a render model holds at `+0x64` and a structure BSP at
/// `+0x44C`. Its sections, compression bounds, node maps and buffer resource sit at the same
/// offsets from its start.
pub(super) struct Geometry {
    pub(super) sections: Vec<usize>,
    /// Compression bounds: one for the whole model, or one for each mesh of a level.
    bounds: Vec<[f32; 10]>,
    node_maps: Vec<usize>,
    resource: Resource,
    vertex_buffers: usize,
    index_buffers: usize,
}

/// One section's decoded buffers and the parts that draw them.
pub(super) struct Section {
    pub(super) row: usize,
    pub(super) vertices: Vec<Vertex>,
    pub(super) indices: Vec<u32>,
    pub(super) restart: u32,
    pub(super) topology: u8,
    pub(super) parts: Vec<usize>,
}

impl Geometry {
    /// The block at `base` of `tag`.
    pub(super) fn read(cache: &Cache, tag: &Tag, base: usize) -> Result<Self> {
        let sections = cache.block(base + 4, 92)?;
        let bounds = cache
            .block(base + 16, 52)?
            .into_iter()
            .map(|row| cache.floats::<10>(row + 4))
            .collect::<Result<Vec<_>>>()?;
        // A block whose vertices are all stored uncompressed, as a BSP's decal geometry is, holds
        // no bounds. Decoding a compressed section then fails.
        ensure!(
            bounds
                .iter()
                .all(|bounds| bounds.chunks_exact(2).all(|b| b[1] >= b[0])),
            "Inverted compression bounds"
        );
        let node_maps = cache.block(base + 76, 12)?;
        let resource = cache.resource(tag, cache.u32(base + 148)?)?;
        ensure!(resource.fixup_size >= 24, "Short geometry fixup");
        let fixup = resource.fixup_offset;
        let vertex_buffers = usize::try_from(cache.i32(fixup + resource.fixup_size - 24)?)?;
        let index_buffers = usize::try_from(cache.i32(fixup + resource.fixup_size - 12)?)?;
        ensure!(
            vertex_buffers <= 65536
                && index_buffers <= 65536
                && vertex_buffers * 40 + index_buffers * 28 <= resource.fixup_size,
            "Geometry buffer table outside fixup"
        );
        Ok(Self {
            sections,
            bounds,
            node_maps,
            resource,
            vertex_buffers,
            index_buffers,
        })
    }

    /// The compression bounds of section `si`, none when the block holds none. A render model has
    /// one entry for every section. A level has one per compressed mesh, in mesh order, and its
    /// uncompressed world meshes follow them.
    fn bounds_for(&self, si: usize) -> Option<&[f32; 10]> {
        if self.bounds.len() > 1 {
            self.bounds.get(si).or(self.bounds.first())
        } else {
            self.bounds.first()
        }
    }

    /// The primary vertex buffer, or none for a fully empty section placeholder.
    fn primary_buffer(&self, cache: &Cache, si: usize) -> Result<Option<usize>> {
        let Some(buffer) = self.section_buffer(cache, si, 0)? else {
            let row = self.sections[si];
            ensure!(
                cache.block(row, 24)?.is_empty()
                    && cache.block(row + 12, 16)?.is_empty()
                    && cache.i16(row + 40)? == -1
                    && (1..8).all(|slot| cache.i16(row + 24 + slot * 2).ok() == Some(-1)),
                "Section {si} has draw data without a primary vertex buffer"
            );
            return Ok(None);
        };
        Ok(Some(buffer))
    }

    /// Section `si`'s vertices and indices, or none for an empty section or buffer.
    pub(super) fn section(
        &self,
        cache: &Cache,
        pages: &mut Pages,
        tag: &Tag,
        si: usize,
        bone_count: usize,
    ) -> Result<Option<Section>> {
        let row = *self.sections.get(si).context("Section outside geometry")?;
        let fixup = self.resource.fixup_offset;
        let Some(vb) = self.primary_buffer(cache, si)? else {
            return Ok(None);
        };
        ensure!(vb < self.vertex_buffers, "Vertex buffer outside fixup");
        let count = usize::try_from(cache.i32(fixup + vb * 28)?)?;
        if count == 0 {
            return Ok(None);
        }
        let length = usize::try_from(cache.i32(fixup + vb * 28 + 8)?)?;
        let format = cache.u8(row + 47)?;
        let stride = vertex_stride(format).with_context(|| format!("{} section {si}", tag.path))?;
        ensure!(
            count.checked_mul(stride) == Some(length),
            "Vertex stride or buffer length differs"
        );
        let vb_offset = *self
            .resource
            .fixups
            .get(vb)
            .context("Missing vertex resource fixup")?;
        let data = pages.bytes(cache, &self.resource, vb_offset, length)?;
        let palette = if let Some(&map) = self.node_maps.get(si) {
            cache
                .block(map, 1)?
                .into_iter()
                .map(|p| cache.u8(p))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let rigid = nullable(cache.u8(row + 46)?).unwrap_or(0);
        // World vertices (format 0) are stored uncompressed and read no bounds.
        let bounds = match self.bounds_for(si) {
            Some(bounds) => *bounds,
            None if format == 0 => [0.; 10],
            None => anyhow::bail!("Missing compression bounds for {} section {si}", tag.path),
        };
        let vertices = decode_vertices(&data, format, rigid, &bounds, &palette, bone_count)?;
        let (indices, restart) = self.indices(cache, pages, row, count)?;
        Ok(Some(Section {
            row,
            vertices,
            indices,
            restart,
            topology: cache.u8(row + 50)?,
            parts: cache.block(row, 24)?,
        }))
    }

    /// Section `si`'s vertex buffer bytes, vertex format, index list, restart value and
    /// topology, with its vertices undecoded, or none for an empty section or buffer.
    pub(super) fn raw_section(
        &self,
        cache: &Cache,
        pages: &mut Pages,
        si: usize,
    ) -> Result<Option<RawSection>> {
        let row = *self.sections.get(si).context("Section outside geometry")?;
        let Some(vb) = self.primary_buffer(cache, si)? else {
            return Ok(None);
        };
        let (count, _, data) = self.vertex_buffer(cache, pages, vb)?;
        if count == 0 {
            return Ok(None);
        }
        let (indices, restart) = self.indices(cache, pages, row, count)?;
        Ok(Some(RawSection {
            vertices: data,
            count,
            format: cache.u8(row + 47)?,
            indices,
            restart,
            topology: cache.u8(row + 50)?,
        }))
    }

    /// The index list of the section at `row` over `count` vertices, and its restart value. A
    /// section without an index buffer draws its vertices in order.
    fn indices(
        &self,
        cache: &Cache,
        pages: &mut Pages,
        row: usize,
        count: usize,
    ) -> Result<(Vec<u32>, u32)> {
        let ib = cache.i16(row + 40)?;
        if ib == -1 || cache.u16(row + 44)? & 16 != 0 {
            return Ok(((0..u32::try_from(count)?).collect(), u32::MAX));
        }
        let ib = usize::try_from(ib)?;
        ensure!(ib < self.index_buffers, "Index buffer outside fixup");
        let fixup = self.resource.fixup_offset;
        let length = usize::try_from(cache.i32(fixup + self.vertex_buffers * 40 + ib * 28 + 8)?)?;
        let offset = *self
            .resource
            .fixups
            .get(self.vertex_buffers * 2 + ib)
            .context("Missing index resource fixup")?;
        let data = pages.bytes(cache, &self.resource, offset, length)?;
        let width = if count > 65535 { 4 } else { 2 };
        ensure!(length.is_multiple_of(width), "Partial index");
        Ok((
            data.chunks_exact(width)
                .map(|b| {
                    if width == 2 {
                        u32::from(u16::from_le_bytes(b.try_into().unwrap()))
                    } else {
                        u32::from_le_bytes(b.try_into().unwrap())
                    }
                })
                .collect(),
            if width == 2 { 65535 } else { u32::MAX },
        ))
    }
}

/// A section's buffers with its vertices undecoded.
pub(super) struct RawSection {
    pub(super) vertices: Vec<u8>,
    pub(super) count: usize,
    pub(super) format: u8,
    pub(super) indices: Vec<u32>,
    pub(super) restart: u32,
    pub(super) topology: u8,
}

impl Geometry {
    /// The vertex buffer section `si` names in `slot`, one of its eight vertex buffer slots.
    pub(super) fn section_buffer(
        &self,
        cache: &Cache,
        si: usize,
        slot: usize,
    ) -> Result<Option<usize>> {
        ensure!(slot < 8, "Vertex buffer slot outside section");
        let row = *self.sections.get(si).context("Section outside geometry")?;
        let vb = cache.i16(row + 24 + slot * 2)?;
        ensure!(
            vb >= -1,
            "Invalid vertex buffer sentinel {vb} in section {si} slot {slot}"
        );
        Ok(usize::try_from(vb).ok())
    }

    /// Vertex buffer `vb`'s vertex count, declaration word (stride above type) and bytes.
    pub(super) fn vertex_buffer(
        &self,
        cache: &Cache,
        pages: &mut Pages,
        vb: usize,
    ) -> Result<(usize, u32, Vec<u8>)> {
        ensure!(vb < self.vertex_buffers, "Vertex buffer outside fixup");
        let fixup = self.resource.fixup_offset;
        let count = usize::try_from(cache.i32(fixup + vb * 28)?)?;
        let declaration = cache.u32(fixup + vb * 28 + 4)?;
        let length = usize::try_from(cache.i32(fixup + vb * 28 + 8)?)?;
        let offset = *self
            .resource
            .fixups
            .get(vb)
            .context("Missing vertex resource fixup")?;
        let data = pages.bytes(cache, &self.resource, offset, length)?;
        Ok((count, declaration, data))
    }
}

fn read_markers(cache: &Cache, at: usize, bone_count: usize) -> Result<Vec<Marker>> {
    let mut markers = Vec::new();
    for group in cache.block(at + 60, 16)? {
        let name = cache.string_id(cache.u32(group)?)?;
        for row in cache.block(group + 4, 48)? {
            let bone = nullable(cache.u8(row + 2)?);
            ensure!(
                bone.is_none_or(|n| n < bone_count),
                "Marker bone outside skeleton"
            );
            markers.push(Marker {
                name: name.clone(),
                region: nullable(cache.u8(row)?),
                permutation: nullable(cache.u8(row + 1)?),
                bone,
                translation: cache.floats::<3>(row + 4)?.map(|v| v * METRES_PER_UNIT),
                rotation: rig::quaternion(cache.floats(row + 16)?)?,
                scale: cache.f32(row + 32)?,
            });
        }
    }
    Ok(markers)
}

pub fn read(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    choices: &BTreeMap<String, String>,
) -> Result<Model> {
    read_permutations(cache, pages, tag, choices, &BTreeMap::new())
}

/// Decodes `tag` as an object placed in a level draws it, where it stands in the pose its vertices
/// are stored in. Nothing poses it by its bones, so their default pose need not match their bind
/// pose, as it does not for some light fixtures.
pub(super) fn read_static(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    choices: &BTreeMap<String, String>,
) -> Result<Model> {
    decode(cache, pages, tag, choices, &BTreeMap::new(), false)
}

/// Decode the selected pose together with explicitly requested alternate draws.
/// Markers and physics still use `choices` as their initial selection.
pub(super) fn read_permutations(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    choices: &BTreeMap<String, String>,
    additional: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Model> {
    decode(cache, pages, tag, choices, additional, true)
}

/// Decodes `tag` with the selected and requested permutations, checking its bones for posing when
/// `posed`.
fn decode(
    cache: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    choices: &BTreeMap<String, String>,
    additional: &BTreeMap<String, BTreeSet<String>>,
    posed: bool,
) -> Result<Model> {
    let at = tag.address()?;
    ensure!(tag.group == "mode", "Expected render model");
    cache.meta(at, 252)?;
    let bones = model_bones(cache, at, posed)?;
    let markers = read_markers(cache, at, bones.len())?;
    let geometry = Geometry::read(cache, tag, at + 100)?;
    let sections = &geometry.sections;
    let Selection {
        regions,
        selected,
        mut selected_sections,
        mut instance_mask,
    } = select(cache, at, sections, choices)?;
    for (name, requested) in additional {
        let region = regions
            .iter()
            .find(|region| &region.name == name)
            .with_context(|| format!("Additional model region {name} is absent"))?;
        for name in requested {
            let permutation = region
                .permutations
                .iter()
                .find(|p| &p.name == name)
                .with_context(|| format!("Additional model permutation {name} is absent"))?;
            selected_sections.extend(permutation.sections.iter().copied());
            for (mask, extra) in instance_mask.iter_mut().zip(permutation.instance_mask) {
                *mask |= extra;
            }
        }
    }
    let materials = cache
        .block(at + 72, 44)?
        .into_iter()
        .map(|r| material::read(cache, cache.reference(r, None)?))
        .collect::<Result<Vec<_>>>()?;
    let instances = cache.block(at + 32, 60)?;
    let instance_section = cache.i32(at + 28)?;
    if !instances.is_empty() {
        ensure!(
            instance_section >= 0 && (instance_section as usize) < sections.len(),
            "Invalid instance section"
        );
        selected_sections.insert(instance_section as usize);
    }
    let mut primitives = Vec::new();
    for si in selected_sections {
        let Some(Section {
            row,
            vertices,
            indices,
            restart,
            topology,
            parts,
        }) = geometry.section(cache, pages, tag, si, bones.len())?
        else {
            continue;
        };
        if si as i32 == instance_section && !instances.is_empty() {
            let subsets = cache.block(row + 12, 16)?;
            ensure!(subsets.len() >= instances.len(), "Missing instance subset");
            for (ii, &instance) in instances.iter().enumerate() {
                ensure!(ii < 128, "Instance mask exceeds supported format");
                if !regions.is_empty() && instance_mask[ii / 32] & (1 << (ii % 32)) == 0 {
                    continue;
                }
                let subset = subsets[ii];
                let part_index = cache.u16(subset + 8)? as usize;
                let part = *parts
                    .get(part_index)
                    .context("Instance material part outside table")?;
                let mut primitive = draw(
                    cache,
                    si,
                    part_index,
                    part,
                    subset,
                    topology,
                    &indices,
                    restart,
                    &vertices,
                    materials.len(),
                )?;
                place_instance(cache, instance, bones.len(), &mut primitive)?;
                if !primitive.indices.is_empty() {
                    primitives.push(primitive);
                }
            }
        } else {
            for (pi, &part) in parts.iter().enumerate() {
                let primitive = draw(
                    cache,
                    si,
                    pi,
                    part,
                    part + 4,
                    topology,
                    &indices,
                    restart,
                    &vertices,
                    materials.len(),
                )?;
                if !primitive.indices.is_empty() {
                    primitives.push(primitive);
                }
            }
        }
    }
    ensure!(
        !primitives.is_empty(),
        "Selected source variant has no triangles"
    );
    Ok(Model {
        tag: tag.clone(),
        bones,
        markers,
        regions,
        selected,
        materials,
        primitives,
    })
}

fn place_instance(
    cache: &Cache,
    instance: usize,
    bone_count: usize,
    primitive: &mut Primitive,
) -> Result<()> {
    let scale = cache.f32(instance + 8)?;
    ensure!(scale.is_finite() && scale > 0., "Invalid instance scale");
    let mut transform = rig::IDENTITY;
    for c in 0..4 {
        for a in 0..3 {
            transform[c * 4 + a] = cache.f32(instance + 12 + c * 12 + a * 4)?
                * if c == 3 { METRES_PER_UNIT } else { scale };
        }
    }
    let bone = usize::try_from(cache.i32(instance + 4)?)?;
    ensure!(bone < bone_count, "Instance bone outside skeleton");
    for v in &mut primitive.vertices {
        v.position = rig::point(&transform, v.position, true);
        v.normal = rig::normalize(rig::point(&transform, v.normal, false))?;
        let t = rig::normalize(rig::point(
            &transform,
            [v.tangent[0], v.tangent[1], v.tangent[2]],
            false,
        ))?;
        v.tangent[..3].copy_from_slice(&t);
        v.joints = [u16::try_from(bone)?; 4];
        v.weights = [1., 0., 0., 0.];
    }
    primitive.instance = Some(cache.string_id(cache.u32(instance)?)?);
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Source draw fields and their checked owning buffers remain explicit"
)]
pub(super) fn draw(
    c: &Cache,
    section: usize,
    part: usize,
    row: usize,
    range: usize,
    topology: u8,
    indices: &[u32],
    restart: u32,
    vertices: &[Vertex],
    material_count: usize,
) -> Result<Primitive> {
    let start = usize::try_from(c.i32(range)?)?;
    let count = usize::try_from(c.i32(range + 4)?)?;
    ensure!(
        start <= indices.len() && count <= indices.len() - start,
        "Draw outside index buffer"
    );
    let source = &indices[start..start + count];
    let mut triangles = Vec::new();
    match topology {
        3 => {
            ensure!(count.is_multiple_of(3), "Incomplete triangle list");
            for f in source.chunks_exact(3) {
                triangles.push([f[0], f[1], f[2]]);
            }
        }
        5 => {
            let mut previous = Vec::new();
            let mut parity = 0;
            for &v in source {
                if v == restart {
                    previous.clear();
                    parity = 0;
                    continue;
                }
                if previous.len() >= 2 {
                    let f = if parity % 2 == 0 {
                        [
                            previous[previous.len() - 2],
                            previous[previous.len() - 1],
                            v,
                        ]
                    } else {
                        [
                            previous[previous.len() - 1],
                            previous[previous.len() - 2],
                            v,
                        ]
                    };
                    triangles.push(f);
                    parity += 1;
                }
                previous.push(v);
            }
        }
        n => anyhow::bail!("Unsupported source topology {n}"),
    }
    ensure!(
        triangles
            .iter()
            .flatten()
            .all(|&i| (i as usize) < vertices.len()),
        "Triangle outside vertex buffer"
    );
    triangles.retain(|f| f[0] != f[1] && f[0] != f[2] && f[1] != f[2]);
    let used = triangles.iter().flatten().copied().collect::<BTreeSet<_>>();
    let mapping = used
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, i as u32))
        .collect::<BTreeMap<_, _>>();
    let material = c.i16(row)?;
    ensure!(
        material >= -1 && (material < 0 || (material as usize) < material_count),
        "Material outside model"
    );
    Ok(Primitive {
        section,
        part,
        material: (material >= 0).then_some(material as usize),
        flags: c.u16(row + 18)?,
        analytical_light: None,
        instance: None,
        bsps: 0,
        vertices: used
            .into_iter()
            .map(|v| vertices[v as usize].clone())
            .collect(),
        indices: triangles
            .into_iter()
            .flatten()
            .map(|v| mapping[&v])
            .collect(),
    })
}
