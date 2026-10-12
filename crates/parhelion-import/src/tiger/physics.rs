//! Shadowkeep Havok 2012.2 packfiles containing convex collision compounds.
use super::payload::Payload;
use crate::presentation::put;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) struct Hull {
    pub vertices: Vec<[f32; 3]>,
    pub planes: Vec<[f32; 4]>,
    pub radius: f32,
}

struct Template {
    prefix: Vec<u8>,
    root: Vec<u8>,
    convex: Vec<u8>,
    list: Vec<u8>,
    names: BTreeMap<String, u32>,
    data: usize,
}

impl Template {
    fn read(bytes: &[u8]) -> Result<Self> {
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
        let copy = |name: &str, size: usize| -> Result<Vec<u8>> {
            let start = *objects
                .get(name)
                .with_context(|| format!("Collision template lacks {name}"))?;
            Ok(bytes
                .get(start..start + size)
                .context("Havok object extent")?
                .to_vec())
        };
        ensure!(
            objects.get("s_physics_component_havok_data") == Some(&data),
            "Havok root differs"
        );
        Ok(Self {
            prefix: bytes[..data].to_vec(),
            root: copy("s_physics_component_havok_data", 64)?,
            convex: copy("hkpConvexVerticesShape", 128)?,
            list: copy("hkpListShape", 144)?,
            names,
            data,
        })
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

/// Each carrier body retains its filter and points to the same source collision
/// hulls. Compound children use that body's filter, never a scenery-template one.
pub(crate) fn compound(template: &[u8], hulls: &[Hull], filters: &[u32]) -> Result<Vec<u8>> {
    ensure!(
        !hulls.is_empty() && hulls.len() <= 256 && !filters.is_empty() && filters.len() <= 256,
        "Collision compound has an invalid body or hull count"
    );
    let template = Template::read(template)?;
    let mut w = Writer {
        data: Vec::new(),
        local: Vec::new(),
        references: Vec::new(),
        objects: Vec::new(),
    };
    w.object(&template.root, "s_physics_component_havok_data")?;
    for field in [0, 16, 48] {
        w.array(field, &[], 8)?;
    }
    let slots = w.array(32, &vec![0; filters.len() * 8], 8)?;
    let mut shapes = Vec::new();
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for hull in hulls {
        ensure!(
            (4..=1024).contains(&hull.vertices.len())
                && hull.planes.len() >= 4
                && hull.radius.is_finite()
                && (0.0..=1.0).contains(&hull.radius),
            "Invalid collision hull"
        );
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for vertex in &hull.vertices {
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
            low[axis] = low[axis].min(minimum[axis] - hull.radius);
            high[axis] = high[axis].max(maximum[axis] + hull.radius);
        }
        let shape = w.object(&template.convex, "hkpConvexVerticesShape")?;
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
        shapes.push(shape);
    }
    for (index, &filter) in filters.iter().enumerate() {
        let shape = if shapes.len() == 1 {
            shapes[0]
        } else {
            let list = w.object(&template.list, "hkpListShape")?;
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
            let mut children = vec![0; shapes.len() * 32];
            for i in 0..shapes.len() {
                put(&mut children, i * 32 + 8, &filter.to_le_bytes())?;
            }
            let start = w.array(list + 48, &children, 32)?;
            for (i, &child) in shapes.iter().enumerate() {
                w.references
                    .push((u32::try_from(start + i * 32)?, u32::try_from(child)?));
            }
            list
        };
        w.references
            .push((u32::try_from(slots + index * 8)?, u32::try_from(shape)?));
    }
    w.finish(template)
}
