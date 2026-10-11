use super::{
    cache::{Cache, Tag},
    material::{self, Material},
    resource::Pages,
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
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}
#[derive(Clone, Debug, Serialize)]
pub struct Primitive {
    pub section: usize,
    pub part: usize,
    pub material: Option<usize>,
    pub flags: u16,
    pub instance: Option<String>,
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

fn nullable(value: u8) -> Option<usize> {
    (value != 255).then_some(value as usize)
}

pub(super) fn read_bones(cache: &Cache, at: usize) -> Result<Vec<Bone>> {
    let mut bones = Vec::new();
    for row in cache.block(at + 48, 96)? {
        let inverse_scale = cache.f32(row + 40)?;
        ensure!(
            (inverse_scale - 1.).abs() < 0.001,
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

fn decode_vertices(
    data: &[u8],
    format: u8,
    flags: u16,
    rigid: usize,
    bounds: &[f32; 10],
    palette: &[u8],
    bone_count: usize,
) -> Result<Vec<Vertex>> {
    let stride = if format == 2 { 44 } else { 36 };
    let mut vertices = Vec::with_capacity(data.len() / stride);
    for bytes in data.chunks_exact(stride) {
        let f = |p| f32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
        let unorm = |p| u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) as f32 / 65535.;
        let snorm =
            |p| (i16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) as f32 / 32767.).max(-1.);
        let position = std::array::from_fn(|a| {
            let v = f(a * 4);
            (if flags & 256 != 0 || format == 0 {
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
            } else {
                bounds[6 + a * 2] + unorm(16 + a * 2) * (bounds[7 + a * 2] - bounds[6 + a * 2])
            }
        });
        ensure!(
            position.iter().chain(uv.iter()).all(|v| v.is_finite()),
            "Nonfinite model vertex"
        );
        let normal = rig::normalize([snorm(20), snorm(22), snorm(24)])?;
        let t = rig::normalize([snorm(28), snorm(30), snorm(32)])?;
        let tangent = [t[0], t[1], t[2], if snorm(34) < 0. { -1. } else { 1. }];
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
            position,
            normal,
            tangent,
            uv,
            joints,
            weights,
        });
    }
    Ok(vertices)
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
    let at = tag.address()?;
    ensure!(tag.group == "mode", "Expected render model");
    cache.meta(at, 252)?;
    let bones = read_bones(cache, at)?;
    let markers = read_markers(cache, at, bones.len())?;
    let sections = cache.block(at + 104, 92)?;
    let Selection {
        regions,
        selected,
        mut selected_sections,
        instance_mask,
    } = select(cache, at, &sections, choices)?;
    let materials = cache
        .block(at + 72, 44)?
        .into_iter()
        .map(|r| material::read(cache, cache.reference(r, None)?))
        .collect::<Result<Vec<_>>>()?;
    let bounds = cache.block(at + 116, 52)?;
    let bounds = cache.floats::<10>(*bounds.first().context("Missing compression bounds")? + 4)?;
    ensure!(
        bounds.chunks_exact(2).all(|b| b[1] >= b[0]),
        "Inverted compression bounds"
    );
    let node_maps = cache.block(at + 176, 12)?;
    let resource = cache.resource(tag, cache.u32(at + 248)?)?;
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
        let row = sections[si];
        let vb = usize::try_from(cache.i16(row + 24)?)?;
        ensure!(vb < vertex_buffers, "Vertex buffer outside fixup");
        let count = usize::try_from(cache.i32(fixup + vb * 28)?)?;
        if count == 0 {
            continue;
        }
        let length = usize::try_from(cache.i32(fixup + vb * 28 + 8)?)?;
        let format = cache.u8(row + 47)?;
        let stride = match format {
            0 | 1 => 36,
            2 => 44,
            n => anyhow::bail!(
                "Unsupported Reach vertex format {n} in {} section {si}",
                tag.path
            ),
        };
        ensure!(
            count.checked_mul(stride) == Some(length),
            "Vertex stride or buffer length differs"
        );
        let vb_offset = *resource
            .fixups
            .get(vb)
            .context("Missing vertex resource fixup")?;
        let data = pages.bytes(cache, &resource, vb_offset, length)?;
        let flags = cache.u16(row + 44)?;
        let palette = if let Some(&map) = node_maps.get(si) {
            cache
                .block(map, 1)?
                .into_iter()
                .map(|p| cache.u8(p))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let rigid = nullable(cache.u8(row + 46)?).unwrap_or(0);
        let vertices =
            decode_vertices(&data, format, flags, rigid, &bounds, &palette, bones.len())?;
        let ib = cache.i16(row + 40)?;
        let topology = cache.u8(row + 50)?;
        let (indices, restart) = if ib == -1 || flags & 16 != 0 {
            ((0..u32::try_from(count)?).collect::<Vec<_>>(), u32::MAX)
        } else {
            let ib = usize::try_from(ib)?;
            ensure!(ib < index_buffers, "Index buffer outside fixup");
            let length = usize::try_from(cache.i32(fixup + vertex_buffers * 40 + ib * 28 + 8)?)?;
            let offset = *resource
                .fixups
                .get(vertex_buffers * 2 + ib)
                .context("Missing index resource fixup")?;
            let data = pages.bytes(cache, &resource, offset, length)?;
            let width = if count > 65535 { 4 } else { 2 };
            ensure!(length.is_multiple_of(width), "Partial index");
            (
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
            )
        };
        let parts = cache.block(row, 24)?;
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
fn draw(
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
        instance: None,
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
