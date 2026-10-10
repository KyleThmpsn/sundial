use super::{
    Point,
    graph::{Graph, MAX_PARTICLES},
    math::*,
    pack::Pack,
};

pub(super) struct Frames {
    buffer: usize,
    involved: Vec<usize>,
    selected: Vec<usize>,
    normal_ids: Vec<usize>,
    triangles: Vec<[usize; 3]>,
    flips: Vec<u8>,
    references: Vec<usize>,
    cosine: Vec<f32>,
    sine: Vec<f32>,
    normal_count: usize,
    update_normals: bool,
    update_tangents: bool,
}

pub(super) fn flip_bytes(p: &Pack<'_>, at: usize, triangles: usize) -> Result<Vec<u8>, String> {
    let words = p.array(at, 4, MAX_PARTICLES * 4)?;
    let mut bytes = Vec::new();
    for row in words {
        bytes.extend_from_slice(p.span(row, 4)?);
    }
    if bytes.len() * 8 < triangles {
        return Err("Cloth triangle flip mask is incomplete".into());
    }
    Ok(bytes)
}

pub(super) fn face(points: &[Point], tri: [usize; 3], flips: &[u8], index: usize) -> Vector {
    let [a, b, c] = tri.map(|i| points[i].position);
    let sign = if flips[index / 8] & (1 << (7 - index % 8)) != 0 {
        -1.
    } else {
        1.
    };
    mul(cross(sub(b, a), sub(c, a)), sign)
}

impl Frames {
    pub fn read(p: &Pack<'_>, at: usize, g: &Graph) -> Result<Self, String> {
        p.expect(at, "hclUpdateSomeVertexFramesOperator", 0xc0)?;
        let buffer = p.u32(at + 0xb0)?;
        let count = g.buffer(buffer)?;
        let indices = |offset: usize, limit: usize| -> Result<Vec<usize>, String> {
            p.array(at + offset, 2, MAX_PARTICLES)?
                .into_iter()
                .map(|row| {
                    let index = p.u16(row)?;
                    if index >= limit {
                        return Err("Cloth frame references a missing vertex".into());
                    }
                    Ok(index)
                })
                .collect()
        };
        let involved = indices(0x30, count)?;
        let selected = indices(0x40, involved.len())?;
        let update_normals = p.u8(at + 0xb8)? != 0;
        let update_tangents = p.u8(at + 0xb9)? != 0;
        let normal_count = p.u32(at + 0xb4)?;
        if normal_count > MAX_PARTICLES {
            return Err("Cloth normal budget exceeded".into());
        }
        let normal_ids = indices(0x50, normal_count)?;
        let triangles = p
            .array(at + 0x20, 6, MAX_PARTICLES * 8)?
            .into_iter()
            .map(|row| {
                let t = [p.u16(row)?, p.u16(row + 2)?, p.u16(row + 4)?];
                if t.iter().any(|i| *i >= involved.len()) {
                    return Err("Cloth frame triangle exceeds its vertex map".into());
                }
                Ok(t)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let flips = flip_bytes(p, at + 0x60, triangles.len())?;
        let references = indices(0x70, count)?;
        let values = |offset: usize| -> Result<Vec<f32>, String> {
            p.array(at + offset, 4, MAX_PARTICLES)?
                .into_iter()
                .map(|row| p.f32(row))
                .collect()
        };
        let cosine = values(0x80)?;
        let sine = values(0x90)?;
        if update_normals && normal_ids.len() != involved.len() {
            return Err("Incomplete cloth normal sharing map".into());
        }
        if update_tangents
            && [references.len(), cosine.len(), sine.len()]
                .iter()
                .any(|n| *n != selected.len())
        {
            return Err("Incomplete cloth tangent reference map".into());
        }
        if p.u8(at + 0xba)? != 0 {
            return Err("Separate cloth bitangent buffers are not supported".into());
        }
        Ok(Self {
            buffer,
            involved,
            selected,
            normal_ids,
            triangles,
            flips,
            references,
            cosine,
            sine,
            normal_count,
            update_normals,
            update_tangents,
        })
    }
    pub fn apply(&self, buffers: &mut [Vec<Point>]) {
        let points = &mut buffers[self.buffer];
        if self.update_normals {
            let mut normals = vec![[0.; 3]; self.normal_count];
            for (i, triangle) in self.triangles.iter().enumerate() {
                let n = face(points, triangle.map(|v| self.involved[v]), &self.flips, i);
                for &v in triangle {
                    let id = self.normal_ids[v];
                    normals[id] = add(normals[id], n);
                }
            }
            for n in &mut normals {
                *n = normal(*n);
            }
            for &v in &self.selected {
                points[self.involved[v]].normal = normals[self.normal_ids[v]];
            }
        }
        if self.update_tangents {
            for (i, &v) in self.selected.iter().enumerate() {
                let target = self.involved[v];
                let n = points[target].normal;
                let e = sub(points[self.references[i]].position, points[target].position);
                let e = normal(sub(e, mul(n, dot(e, n))));
                points[target].tangent =
                    add(mul(e, self.cosine[i]), mul(cross(n, e), self.sine[i]));
            }
        }
    }
}
