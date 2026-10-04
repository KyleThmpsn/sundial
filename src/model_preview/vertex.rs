//! Native vertex declarations, shared by entity, static and terrain geometry.
//! The render globals describe semantics and formats separately for each stream.
//! Reference: alkahest's prebl-0.5 render_globals and GPU input-layout tables.
use super::*;
use std::collections::BTreeMap;
mod formats;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    semantic: u8,
    index: u8,
    format: u8,
    stream: usize,
    offset: usize,
}

#[derive(Default)]
pub(super) struct Layouts(BTreeMap<u16, Vec<Element>>);

impl Layouts {
    pub fn read(manager: &PackageManager) -> Result<Self, String> {
        let mut result = Self::default();
        result.builtins();
        let mut roots = manager.get_all_by_reference(0x8080_72A6);
        roots.sort_by_key(|(tag, _)| tag.0);
        for (tag, _) in roots {
            let root = checked(manager, tag.0, 0x8080_72A6)?;
            let elements = checked(manager, u32_at(&root, 0xC)?, 0x8080_72AD)?;
            let mapping = checked(manager, u32_at(&root, 0x28)?, 0x8080_72A9)?;
            result.merge(&elements, &mapping)?;
        }
        Ok(result)
    }

    fn merge(&mut self, elements: &[u8], mapping: &[u8]) -> Result<(), String> {
        let (set_count, sets) = table(elements, 8, 0x8080_72AF, 16, 256)?;
        let (count, rows) = table(mapping, 8, 0x8080_72AC, 0x1C, 256)?;
        for row in (0..count).map(|i| rows + i * 0x1C) {
            let id = u16::from(mapping[row]);
            let mut layout = Vec::new();
            for stream in 0..4 {
                let set = u32_at(mapping, row + 8 + stream * 4)?;
                if set == u32::MAX {
                    continue;
                }
                if set as usize >= set_count {
                    return Err("A vertex declaration names a missing element set".into());
                }
                let (count, rows) = table(elements, sets + set as usize * 16, 0x8080_72B2, 3, 32)?;
                let mut offset = 0;
                for at in (0..count).map(|i| rows + i * 3) {
                    let format = elements[at + 2];
                    layout.push(Element {
                        semantic: elements[at],
                        index: elements[at + 1],
                        format,
                        stream,
                        offset,
                    });
                    offset += formats::size(format)?;
                }
            }
            if self.0.get(&id).is_some_and(|old| *old != layout) {
                return Err(format!(
                    "Conflicting native vertex declarations for layout {id}"
                ));
            }
            self.0.insert(id, layout);
        }
        Ok(())
    }

    fn builtins(&mut self) {
        let declarations: &[&[(u8, u8)]] = &[
            &[(0, 3)],
            &[(0, 3)],
            &[(0, 2), (5, 2), (8, 5)],
            &[(0, 3), (5, 2), (8, 5)],
            &[(0, 3), (8, 5)],
            &[(0, 2), (5, 2)],
            &[(0, 3), (3, 3), (6, 4), (5, 2)],
        ];
        for (id, declaration) in declarations.iter().enumerate() {
            let mut offset = 0;
            let elements = declaration
                .iter()
                .map(|&(semantic, format)| {
                    let element = Element {
                        semantic,
                        index: 0,
                        format,
                        stream: 0,
                        offset,
                    };
                    offset += formats::size(format).expect("built-in format");
                    element
                })
                .collect();
            self.0.insert(id as u16, elements);
        }
    }

    pub fn read_vertices(
        &self,
        manager: &PackageManager,
        id: u16,
        tags: [u32; 4],
        budget: usize,
    ) -> Result<Vertices, String> {
        let layout = self
            .0
            .get(&id)
            .ok_or_else(|| format!("Unsupported native vertex layout {id}"))?;
        let position = layout
            .iter()
            .find(|e| e.semantic == 0 && e.index == 0)
            .ok_or("The vertex declaration has no position attribute")?;
        let mut streams = Streams {
            manager,
            tags,
            buffers: BTreeMap::new(),
        };
        let positions = streams.attribute(position, None, budget)?;
        let count = positions.len();
        let mut notices = Vec::new();
        let uv = layout
            .iter()
            .filter(|e| e.semantic == 5)
            .min_by_key(|e| e.index);
        let uvs = optional_attribute(&mut streams, uv, count, &mut notices, "Texture coordinates");
        let has_uv = uvs.is_some();
        let uvs: Vec<[f32; 2]> = uvs
            .unwrap_or_else(|| vec![[0.0; 4]; count])
            .into_iter()
            .map(|v| [v[0], v[1]])
            .collect();
        let detail = layout.iter().find(|e| e.semantic == 5 && e.index == 2);
        let detail_scales = optional_attribute(
            &mut streams,
            detail,
            count,
            &mut notices,
            "Secondary texture coordinates",
        )
        .unwrap_or_else(|| vec![[1.0; 4]; count])
        .into_iter()
        .map(|v| [v[0], v[1]])
        .collect();
        let normal = layout.iter().find(|e| e.semantic == 3 && e.index == 0);
        let normals = optional_attribute(&mut streams, normal, count, &mut notices, "Normals")
            .unwrap_or_else(|| vec![[0.0; 4]; count])
            .into_iter()
            .map(|v| {
                // Byte-normalized normal declarations encode signed directions in [0, 1].
                std::array::from_fn(|i| {
                    if normal.is_some_and(|e| matches!(e.format, 5 | 17)) {
                        v[i] * 2.0 - 1.0
                    } else {
                        v[i]
                    }
                })
            })
            .collect();
        let weights = skin_weights(&mut streams, layout, count, &mut notices);
        let tangent = layout.iter().find(|e| e.semantic == 6 && e.index == 0);
        let tangents = optional_attribute(&mut streams, tangent, count, &mut notices, "Tangents")
            .unwrap_or_else(|| vec![[0.0, 0.0, 0.0, 1.0]; count])
            .into_iter()
            .map(|v| {
                if tangent.is_some_and(|e| matches!(e.format, 5 | 17)) {
                    v.map(|v| v * 2.0 - 1.0)
                } else {
                    v
                }
            })
            .collect();
        let color = layout.iter().find(|e| e.semantic == 8 && e.index == 0);
        let colors = optional_attribute(&mut streams, color, count, &mut notices, "Vertex colors")
            .unwrap_or_else(|| vec![[1.0; 4]; count]);
        Ok(Vertices {
            positions,
            normals,
            uvs,
            detail_uvs: Vec::new(),
            detail_scales,
            weights,
            tangents,
            colors,
            has_uv,
            notices,
        })
    }
}

pub(super) struct Vertices {
    pub positions: Vec<[f32; 4]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub detail_uvs: Vec<[f32; 2]>,
    detail_scales: Vec<[f32; 2]>,
    pub weights: Vec<Option<animation::Weights>>,
    pub has_uv: bool,
    pub notices: Vec<String>,
}

impl Vertices {
    pub fn transform(
        &mut self,
        scale: [f32; 3],
        offset: [f32; 3],
        uv: [f32; 4],
    ) -> Result<(), String> {
        if scale
            .iter()
            .chain(&offset)
            .chain(&uv)
            .any(|v| !v.is_finite())
        {
            return Err("Invalid model transform".into());
        }
        for point in &mut self.positions {
            for i in 0..3 {
                point[i] = point[i] * scale[i] + offset[i];
            }
            if point.iter().any(|v| !v.is_finite()) {
                return Err("The model contains invalid positions".into());
            }
        }
        for normal in &mut self.normals {
            for i in 0..3 {
                if scale[i].abs() > 1e-8 {
                    normal[i] /= scale[i];
                }
            }
            *normal = super::shader::normal::normalize(*normal).unwrap_or([0.0; 3]);
        }
        for tangent in &mut self.tangents {
            let direction = std::array::from_fn(|i| tangent[i] * scale[i]);
            let direction = super::shader::normal::normalize(direction).unwrap_or([0.0; 3]);
            tangent[..3].copy_from_slice(&direction);
            tangent[3] *= (scale[0] * scale[1] * scale[2]).signum();
        }
        for value in &mut self.uvs {
            *value = [value[0] * uv[0] + uv[2], value[1] * uv[1] + uv[3]];
            if value.iter().any(|v| !v.is_finite()) {
                return Err("Invalid texture coordinates".into());
            }
        }
        self.detail_uvs = self
            .uvs
            .iter()
            .zip(&self.detail_scales)
            .map(|(uv, scale)| [uv[0] * scale[0], uv[1] * scale[1]])
            .collect();
        if self.detail_uvs.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid secondary texture coordinates".into());
        }
        Ok(())
    }
}

/// New formats require the actual array class, in addition to bounded row storage.
pub(super) fn table(
    bytes: &[u8],
    at: usize,
    class: u32,
    stride: usize,
    limit: usize,
) -> Result<(usize, usize), String> {
    // Empty native declarations use a null relative pointer and own no row header.
    if u64_at(bytes, at)? == 0 && i64_at(bytes, at + 8)? == 0 {
        return Ok((0, at + 16));
    }
    let (_, _, _, actual) = native_array_at(bytes, at)?;
    if actual != class {
        return Err(format!(
            "Unexpected model table class {actual:08X}, expected {class:08X}"
        ));
    }
    array(bytes, at, class, stride, limit)
}

pub(super) fn buffer(
    manager: &PackageManager,
    tag: u32,
    subtype: u8,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| format!("Model buffer 0x{tag:08X} is missing"))?;
    if entry.file_type != 32 || entry.file_subtype != subtype {
        return Err("Unsupported model buffer type".into());
    }
    let data = manager
        .get_entry(entry.reference)
        .ok_or("The model buffer payload is missing")?;
    if data.file_size > 32 * 1024 * 1024 {
        return Err("This buffer exceeds the preview size budget.".into());
    }
    Ok((manager.read_tag(tag)?, manager.read_tag(entry.reference)?))
}

pub(super) fn indices(manager: &PackageManager, tag: u32) -> Result<(usize, Vec<u8>), String> {
    let (header, data) = buffer(manager, tag, 6)?;
    let width = if bool_at(&header, 1)? { 4 } else { 2 };
    if u64_at(&header, 8)? != data.len() as u64 || data.len() % width != 0 {
        return Err("The model index buffer has an invalid size".into());
    }
    Ok((width, data))
}

struct Stream {
    data: Vec<u8>,
    stride: usize,
}
struct Streams<'a> {
    manager: &'a PackageManager,
    tags: [u32; 4],
    buffers: BTreeMap<usize, Stream>,
}
impl Streams<'_> {
    fn attribute(
        &mut self,
        element: &Element,
        count: Option<usize>,
        budget: usize,
    ) -> Result<Vec<[f32; 4]>, String> {
        if let std::collections::btree_map::Entry::Vacant(slot) = self.buffers.entry(element.stream)
        {
            let (header, data) = buffer(self.manager, self.tags[element.stream], 4)?;
            let stride = usize::from(u16_at(&header, 4)?);
            if stride == 0
                || data.is_empty()
                || data.len() % stride != 0
                || u32_at(&header, 0)? as usize != data.len()
            {
                return Err("The model vertex buffer has an invalid size or stride".into());
            }
            slot.insert(Stream { data, stride });
        }
        let stream = &self.buffers[&element.stream];
        let actual = stream.data.len() / stream.stride;
        if actual > budget {
            return Err("This model exceeds the preview vertex budget.".into());
        }
        if count.is_some_and(|count| count != actual) {
            return Err("The vertex attribute count does not match the positions".into());
        }
        if element.offset + formats::size(element.format)? > stream.stride {
            return Err(format!(
                "Vertex format {} at offset {} exceeds the {}-byte stream",
                element.format, element.offset, stream.stride
            ));
        }
        stream
            .data
            .chunks_exact(stream.stride)
            .map(|row| formats::read(element.format, &row[element.offset..]))
            .collect()
    }
}

fn optional_attribute(
    streams: &mut Streams<'_>,
    element: Option<&Element>,
    count: usize,
    notices: &mut Vec<String>,
    label: &str,
) -> Option<Vec<[f32; 4]>> {
    let element = element?;
    match streams.attribute(element, Some(count), count) {
        Ok(values) => Some(values),
        Err(error) => {
            notices.push(format!("{label}: {error}"));
            None
        }
    }
}

fn skin_weights(
    streams: &mut Streams<'_>,
    layout: &[Element],
    count: usize,
    notices: &mut Vec<String>,
) -> Vec<Option<animation::Weights>> {
    let indices = layout.iter().find(|e| e.semantic == 2 && e.index == 0);
    let weights = layout.iter().find(|e| e.semantic == 1 && e.index == 0);
    let Some(bones) = optional_attribute(streams, indices, count, notices, "Skin weights") else {
        return (0..count).map(|_| None).collect();
    };
    if let Some(values) = optional_attribute(streams, weights, count, notices, "Skin weights") {
        return bones
            .into_iter()
            .zip(values)
            .map(|(b, w)| {
                if b.iter().any(|v| *v < 0.0 || *v > 255.0 || v.fract() != 0.0) {
                    return None;
                }
                Some(animation::Weights {
                    bones: b.map(|v| v as u8),
                    values: w.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
                })
            })
            .collect();
    }
    // Shadowkeep's two-influence declaration packs [bone0,bone1,weight0,weight1]
    // into BLENDINDICES R8G8B8A8_UINT. Wider or generated-stream formats differ.
    if weights.is_none() && indices.is_some_and(|e| e.format == 6) {
        return bones
            .into_iter()
            .map(|v| {
                Some(animation::Weights {
                    bones: [v[0] as u8, v[1] as u8, 0, 0],
                    values: [v[2] as u8, v[3] as u8, 0, 0],
                })
            })
            .collect();
    }
    notices.push("This skin-weight declaration is shown in its stored pose.".into());
    (0..count).map(|_| None).collect()
}
