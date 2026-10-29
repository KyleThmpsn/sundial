//! Shadowkeep Havok 2012.2 packfiles containing convex collision compounds, compressed triangle
//! meshes, the static compound of a world's collision, or a cloth solver.
use super::payload::Payload;
use crate::presentation::put;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub mod cloth;
mod compound;
mod mesh;
pub(crate) use compound::{Instance, static_compound};
pub(crate) use mesh::bare_mesh;

pub(crate) struct Hull {
    pub vertices: Vec<[f32; 3]>,
    pub planes: Vec<[f32; 4]>,
    pub radius: f32,
}

/// Convex leaves admitted by native dynamic-body assembly.
pub(crate) enum Shape {
    Hull(Hull),
    Capsule {
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    },
}

impl Shape {
    fn bounds(&self) -> Result<([f32; 3], [f32; 3])> {
        match self {
            Self::Hull(hull) => {
                let (low, high) = hull.bounds()?;
                Ok((low.map(|v| v - hull.radius), high.map(|v| v + hull.radius)))
            }
            Self::Capsule { a, b, radius } => {
                ensure!(
                    a.iter().chain(b).all(|v| v.is_finite())
                        && radius.is_finite()
                        && *radius > 0.
                        && a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>() > 1e-12,
                    "Invalid collision capsule"
                );
                let low = std::array::from_fn(|i| a[i].min(b[i]) - radius);
                let high = std::array::from_fn(|i| a[i].max(b[i]) + radius);
                ensure!(
                    low.iter().chain(&high).all(|v| v.is_finite()),
                    "Nonfinite capsule bounds"
                );
                Ok((low, high))
            }
        }
    }
}

impl Hull {
    fn bounds(&self) -> Result<([f32; 3], [f32; 3])> {
        ensure!(
            (4..=1024).contains(&self.vertices.len())
                && self.planes.len() >= 4
                && self.radius.is_finite()
                && (0.0..=1.0).contains(&self.radius),
            "Invalid collision hull"
        );
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for vertex in &self.vertices {
            for axis in 0..3 {
                ensure!(vertex[axis].is_finite(), "Nonfinite collision vertex");
                minimum[axis] = minimum[axis].min(vertex[axis]);
                maximum[axis] = maximum[axis].max(vertex[axis]);
            }
        }
        for axis in 0..3 {
            ensure!(
                maximum[axis] > minimum[axis],
                "Degenerate collision hull bounds"
            );
        }
        Ok((minimum, maximum))
    }
}

/// Whether `bytes` is a native Havok 2012.2 packfile whose first object is of class `root`.
pub(crate) fn rooted(bytes: &[u8], root: &str) -> bool {
    Template::read(bytes, root).is_ok()
}

struct Template {
    prefix: Vec<u8>,
    bytes: Vec<u8>,
    /// Each class's first object in the data section.
    objects: BTreeMap<String, usize>,
    names: BTreeMap<String, u32>,
    data: usize,
}

impl Template {
    /// A packfile whose first object is of class `root`.
    fn read(bytes: &[u8], root: &str) -> Result<Self> {
        let p = Payload(bytes.to_vec());
        ensure!(
            p.u32(0)? == 0x57e0e057
                && p.u32(4)? == 0x10c0c010
                && p.u32(12)? == 9
                && p.u32(16)? == 0x01000108
                && p.u32(20)? == 3
                && p.u32(24)? == 2
                && p.u32(28)? == 0
                && p.u32(32)? == 0
                && bytes.get(40..54) == Some(b"hk_2012.2.0-r1"),
            "Collision template is not a native Havok 2012.2 packfile"
        );
        for (index, name) in [b"__classnames__".as_slice(), b"__types__", b"__data__"]
            .into_iter()
            .enumerate()
        {
            let at = 0x40 + index * 0x30;
            ensure!(
                bytes.get(at..at + name.len()) == Some(name),
                "Havok section order differs"
            );
        }
        let names_base = p.u32(0x54)? as usize;
        let data = p.u32(0xb4)? as usize;
        let finish = data + p.u32(0xc0)? as usize;
        let finish_end = data + p.u32(0xc4)? as usize;
        ensure!(
            data <= bytes.len() && finish <= finish_end && finish_end <= bytes.len(),
            "Havok section bounds differ"
        );
        let mut names = BTreeMap::new();
        let mut objects = BTreeMap::new();
        let mut at = finish;
        while at + 4 <= finish_end && p.u32(at)? != u32::MAX {
            ensure!(
                at + 12 <= finish_end && p.u32(at + 4)? == 0,
                "Havok finish fixup differs"
            );
            let offset = p.u32(at + 8)?;
            let start = names_base + offset as usize;
            let suffix = bytes.get(start..data).context("Havok class name bounds")?;
            let length = suffix
                .iter()
                .position(|&b| b == 0)
                .context("Havok class name terminator")?;
            let name = std::str::from_utf8(&suffix[..length])?.to_owned();
            names.insert(name.clone(), offset);
            objects.entry(name).or_insert(data + p.u32(at)? as usize);
            at += 12;
        }
        ensure!(objects.get(root) == Some(&data), "Havok root is not {root}");
        // The class name section also names classes the packfile has no object of, as a world
        // collision's main packfile names the shapes its instances bind to: each entry is a
        // signature, a 9, then the name.
        let names_end = names_base + p.u32(0x58)? as usize;
        let mut entry = names_base;
        while entry + 5 < names_end && p.u32(entry)? != u32::MAX {
            let start = entry + 5;
            let suffix = bytes
                .get(start..names_end)
                .context("Havok class name bounds")?;
            let length = suffix
                .iter()
                .position(|&b| b == 0)
                .context("Havok class name terminator")?;
            let name = std::str::from_utf8(&suffix[..length])?.to_owned();
            names
                .entry(name)
                .or_insert(u32::try_from(start - names_base)?);
            entry = start + length + 1;
        }
        Ok(Self {
            prefix: bytes[..data].to_vec(),
            bytes: bytes.to_vec(),
            objects,
            names,
            data,
        })
    }

    /// The first `size` bytes of the template's first object of class `name`.
    fn object(&self, name: &str, size: usize) -> Result<Vec<u8>> {
        let start = *self
            .objects
            .get(name)
            .with_context(|| format!("Collision template lacks {name}"))?;
        Ok(self
            .bytes
            .get(start..start + size)
            .context("Havok object extent")?
            .to_vec())
    }

    /// Extend only the class-name section. Existing name offsets and relative fixups stay valid.
    fn add_class(&mut self, name: &str, signature: u32) -> Result<()> {
        let p = Payload(self.prefix.clone());
        let base = p.u32(0x54)? as usize;
        if let Some(&offset) = self.names.get(name) {
            ensure!(
                p.u32(base + offset as usize - 5)? == signature,
                "Havok class signature differs"
            );
            return Ok(());
        }
        let old_size = p.u32(0x58)? as usize;
        for field in (0x58..=0x6c).step_by(4) {
            ensure!(
                p.u32(field)? as usize == old_size,
                "Class-name section contains fixups"
            );
        }
        let mut used = 0;
        for &offset in self.names.values() {
            let start = base + offset as usize;
            let length = self
                .prefix
                .get(start..base + old_size)
                .context("Havok class name extent")?
                .iter()
                .position(|&b| b == 0)
                .context("Havok class name terminator")?;
            used = used.max(offset as usize + length + 1);
        }
        let mut section = self.prefix[base..base + used].to_vec();
        section.extend(signature.to_le_bytes());
        section.push(9);
        self.names
            .insert(name.into(), u32::try_from(section.len())?);
        section.extend(name.as_bytes());
        section.push(0);
        let size = section.len().div_ceil(16) * 16;
        section.resize(size, 0xff);
        let growth = size
            .checked_sub(old_size)
            .context("Havok name section shrank")?;
        self.prefix.splice(base..base + old_size, section);
        for field in (0x58..=0x6c).step_by(4) {
            put(&mut self.prefix, field, &u32::try_from(size)?.to_le_bytes())?;
        }
        for field in [0x84, 0xb4] {
            put(
                &mut self.prefix,
                field,
                &u32::try_from(p.u32(field)? as usize + growth)?.to_le_bytes(),
            )?;
        }
        // Original object bytes keep their original offsets in `bytes`.
        self.data += growth;
        Ok(())
    }
}

struct Writer {
    data: Vec<u8>,
    local: Vec<(u32, u32)>,
    references: Vec<(u32, u32)>,
    objects: Vec<(u32, String)>,
}

impl Writer {
    fn append(&mut self, bytes: &[u8]) -> Result<usize> {
        let at = self.data.len().div_ceil(16) * 16;
        ensure!(
            at + bytes.len() < u32::MAX as usize,
            "Havok data exceeds its offset space"
        );
        self.data.resize(at, 0);
        self.data.extend_from_slice(bytes);
        Ok(at)
    }

    fn object(&mut self, bytes: &[u8], name: &str) -> Result<usize> {
        let at = self.append(bytes)?;
        self.objects.push((u32::try_from(at)?, name.into()));
        Ok(at)
    }

    fn array(&mut self, field: usize, rows: &[u8], stride: usize) -> Result<usize> {
        ensure!(
            rows.len().is_multiple_of(stride),
            "Havok array stride differs"
        );
        let count = u32::try_from(rows.len() / stride)?;
        put(&mut self.data, field, &[0; 8])?;
        put(&mut self.data, field + 8, &count.to_le_bytes())?;
        put(
            &mut self.data,
            field + 12,
            &(count | 0x80000000).to_le_bytes(),
        )?;
        if count == 0 {
            return Ok(0);
        }
        let at = self.append(rows)?;
        self.local.push((u32::try_from(field)?, u32::try_from(at)?));
        Ok(at)
    }

    fn vector(&mut self, field: usize, values: [f32; 3], w: f32) -> Result<()> {
        for (axis, value) in values.into_iter().chain([w]).enumerate() {
            put(&mut self.data, field + axis * 4, &value.to_le_bytes())?;
        }
        Ok(())
    }

    fn finish(mut self, template: Template) -> Result<Vec<u8>> {
        let mut offsets = Vec::new();
        for part in 0..3 {
            let at = self.data.len().div_ceil(16) * 16;
            self.data.resize(at, 0xff);
            offsets.push(u32::try_from(at)?);
            match part {
                0 => {
                    for &(from, to) in &self.local {
                        self.data.extend(from.to_le_bytes());
                        self.data.extend(to.to_le_bytes());
                    }
                }
                1 => {
                    for &(from, to) in &self.references {
                        self.data.extend(from.to_le_bytes());
                        self.data.extend(2u32.to_le_bytes());
                        self.data.extend(to.to_le_bytes());
                    }
                }
                _ => {
                    for (from, name) in &self.objects {
                        self.data.extend(from.to_le_bytes());
                        self.data.extend(0u32.to_le_bytes());
                        self.data.extend(template.names[name].to_le_bytes());
                    }
                }
            }
        }
        let end = self.data.len().div_ceil(16) * 16;
        self.data.resize(end, 0xff);
        offsets.extend([u32::try_from(end)?; 3]);
        let mut out = template.prefix;
        ensure!(out.len() == template.data, "Havok prefix length changed");
        for (index, value) in offsets.into_iter().enumerate() {
            put(&mut out, 0xb8 + index * 4, &value.to_le_bytes())?;
        }
        out.extend(self.data);
        Ok(out)
    }
}

fn convex(
    w: &mut Writer,
    template: &[u8],
    hull: &Hull,
    minimum: [f32; 3],
    maximum: [f32; 3],
) -> Result<usize> {
    let shape = w.object(template, "hkpConvexVerticesShape")?;
    put(&mut w.data, shape, &[0; 16])?;
    put(&mut w.data, shape + 24, &[0; 8])?;
    put(&mut w.data, shape + 32, &hull.radius.to_le_bytes())?;
    w.vector(
        shape + 48,
        std::array::from_fn(|i| (maximum[i] - minimum[i]) * 0.5),
        0.,
    )?;
    w.vector(
        shape + 64,
        std::array::from_fn(|i| (maximum[i] + minimum[i]) * 0.5),
        0.,
    )?;
    let mut vectors = Vec::new();
    for chunk in hull.vertices.chunks(4) {
        for axis in [0, 1, 2] {
            for lane in 0..4 {
                vectors.extend(chunk[lane.min(chunk.len() - 1)][axis].to_le_bytes());
            }
        }
    }
    w.array(shape + 80, &vectors, 48)?;
    put(
        &mut w.data,
        shape + 96,
        &u32::try_from(hull.vertices.len())?.to_le_bytes(),
    )?;
    put(&mut w.data, shape + 100, &[0; 4])?;
    let mut planes = Vec::new();
    for plane in &hull.planes {
        ensure!(
            plane.iter().all(|v| v.is_finite()),
            "Nonfinite collision plane"
        );
        for value in plane {
            planes.extend(value.to_le_bytes());
        }
    }
    w.array(shape + 104, &planes, 16)?;
    put(&mut w.data, shape + 120, &[0; 8])?;
    Ok(shape)
}

/// Solid convex parts followed by separate triangle meshes for native hit bodies.
pub(crate) fn vehicle_shapes(
    template: &[u8],
    hulls: &[Shape],
    groups: &[Vec<usize>],
    meshes: &[Vec<[[f32; 3]; 3]>],
) -> Result<Vec<u8>> {
    ensure!(
        !hulls.is_empty()
            && hulls.len() <= 1024
            && (1..=256).contains(&(groups.len() + meshes.len())),
        "Collision compound has an invalid body or hull count"
    );
    for group in groups {
        ensure!(
            !group.is_empty()
                && group.len() <= 256
                && group.iter().all(|&index| index < hulls.len()),
            "Invalid collision hull selection"
        );
    }
    let mut template = Template::read(template, "s_physics_component_havok_data")?;
    let root = template.object("s_physics_component_havok_data", 64)?;
    let convex_template = template.object("hkpConvexVerticesShape", 128)?;
    let list_shape = template.object("hkpListShape", 144)?;
    if hulls
        .iter()
        .any(|shape| matches!(shape, Shape::Capsule { .. }))
    {
        // Native 2012.2 reflection at 020C84D0, with inherited fields included.
        template.add_class("hkpCapsuleShape", 0xb58289ff)?;
    }
    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    w.object(&root, "s_physics_component_havok_data")?;
    for field in [0, 16, 48] {
        w.array(field, &[], 8)?;
    }
    let slots = w.array(32, &vec![0; (groups.len() + meshes.len()) * 8], 8)?;
    let mut shapes = Vec::new();
    let mut bounds = Vec::new();
    for hull in hulls {
        bounds.push(hull.bounds()?);
        let shape = match hull {
            Shape::Hull(hull) => {
                let (minimum, maximum) = hull.bounds()?;
                convex(&mut w, &convex_template, hull, minimum, maximum)?
            }
            Shape::Capsule { a, b, radius } => {
                // Common hkpConvexShape header, followed by two hkVector4 endpoints.
                let mut bytes = vec![0; 0x50];
                bytes[0x10..0x18].copy_from_slice(&convex_template[0x10..0x18]);
                bytes[0x10] = 4;
                let shape = w.object(&bytes, "hkpCapsuleShape")?;
                put(&mut w.data, shape + 0x20, &radius.to_le_bytes())?;
                // The collision-sphere consumer copies both vectors including their radius lane.
                w.vector(shape + 0x30, *a, *radius)?;
                w.vector(shape + 0x40, *b, *radius)?;
                shape
            }
        };
        shapes.push(shape);
    }
    for (index, group) in groups.iter().enumerate() {
        let shape = if group.len() == 1 {
            shapes[group[0]]
        } else {
            let selected = group.iter().map(|&k| shapes[k]).collect::<Vec<_>>();
            let selected_bounds = group.iter().map(|&k| bounds[k]).collect::<Vec<_>>();
            list(&mut w, &list_shape, &selected, &selected_bounds)?
        };
        w.references
            .push((u32::try_from(slots + index * 8)?, u32::try_from(shape)?));
    }
    if !meshes.is_empty() {
        let shape_bytes = template.object("hkpBvCompressedMeshShape", 0x110)?;
        for (index, triangles) in meshes.iter().enumerate() {
            let (shape, _) = mesh::mesh_shape(&mut w, &shape_bytes, triangles)?;
            w.references.push((
                u32::try_from(slots + (groups.len() + index) * 8)?,
                u32::try_from(shape)?,
            ));
        }
    }
    w.finish(template)
}

/// An `hkpListShape` over the written `shapes`, whose box encloses their `bounds`.
fn list(
    w: &mut Writer,
    template: &[u8],
    shapes: &[usize],
    bounds: &[([f32; 3], [f32; 3])],
) -> Result<usize> {
    ensure!(
        !shapes.is_empty() && shapes.len() == bounds.len() && shapes.len() <= 256,
        "Invalid list shape children"
    );
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for (minimum, maximum) in bounds {
        for axis in 0..3 {
            low[axis] = low[axis].min(minimum[axis]);
            high[axis] = high[axis].max(maximum[axis]);
        }
    }
    let list = w.object(template, "hkpListShape")?;
    put(&mut w.data, list, &[0; 16])?;
    put(&mut w.data, list + 24, &[0; 24])?;
    put(&mut w.data, list + 64, &[0; 16])?;
    put(&mut w.data, list + 112, &[0xff; 32])?;
    w.vector(
        list + 80,
        std::array::from_fn(|i| (high[i] - low[i]) * 0.5),
        0.,
    )?;
    w.vector(
        list + 96,
        std::array::from_fn(|i| (high[i] + low[i]) * 0.5),
        1.,
    )?;
    // Native A2AC40 treats zero as inheritance from the root collidable.
    // Saved body filters omit the live contact-enable bits and cannot
    // be copied to this runtime filter field.
    let children = vec![0; shapes.len() * 32];
    let start = w.array(list + 48, &children, 32)?;
    for (i, &shape) in shapes.iter().enumerate() {
        w.references
            .push((u32::try_from(start + i * 32)?, u32::try_from(shape)?));
    }
    Ok(list)
}

/// The most prisms a volume keeps in a list shape. Stock volume packfiles hold up to three
/// prisms in a list shape and more in a static compound.
const LIST_PRISMS: usize = 3;

/// A volume packfile: a shape array whose entries each hold one volume's convex prisms, in
/// a list shape for up to three and a static compound of them beyond, as stock Crucible volume
/// packfiles do. A template without a list shape, such as a stock environment volume packfile
/// (Distant Shore's `80B8C78B` holds only convex shapes and static compounds), takes a static
/// compound for every volume. The stock template must cover the shape classes actually emitted.
pub(crate) fn volume_shapes(template: &[u8], volumes: &[Vec<Hull>]) -> Result<Vec<u8>> {
    ensure!(
        !volumes.is_empty()
            && volumes.len() <= 1024
            && volumes.iter().all(|hulls| !hulls.is_empty()),
        "Volume packfile has an invalid volume or prism count"
    );
    let template = Template::read(template, "s_hkpShape_array_data")?;
    let root = template.object("s_hkpShape_array_data", 16)?;
    let convex_template = template.object("hkpConvexVerticesShape", 128)?;
    let list_shape = template
        .objects
        .contains_key("hkpListShape")
        .then(|| template.object("hkpListShape", 144))
        .transpose()?;
    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    w.object(&root, "s_hkpShape_array_data")?;
    let slots = w.array(0, &vec![0; volumes.len() * 8], 8)?;
    for (index, hulls) in volumes.iter().enumerate() {
        let mut shapes = Vec::with_capacity(hulls.len());
        let mut bounds = Vec::with_capacity(hulls.len());
        for hull in hulls {
            let (minimum, maximum) = hull.bounds()?;
            bounds.push((
                minimum.map(|v| v - hull.radius),
                maximum.map(|v| v + hull.radius),
            ));
            shapes.push(convex(&mut w, &convex_template, hull, minimum, maximum)?);
        }
        let shape = match &list_shape {
            Some(list_shape) if hulls.len() <= LIST_PRISMS => {
                list(&mut w, list_shape, &shapes, &bounds)?
            }
            _ => {
                let compound_shape =
                    template.object("hkpStaticCompoundShape", compound::SHAPE_SIZE)?;
                compound::convex_compound(&mut w, &compound_shape, &shapes, &bounds)?
            }
        };
        w.references
            .push((u32::try_from(slots + index * 8)?, u32::try_from(shape)?));
    }
    w.finish(template)
}
