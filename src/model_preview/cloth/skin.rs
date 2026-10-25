use super::{
    Point,
    graph::{Graph, MAX_PARTICLES},
    math::*,
    pack::Pack,
};

pub(super) struct Skin {
    pub output: usize,
    pub set: usize,
    pub subset: Vec<usize>,
    pub binds: Vec<Matrix>,
    vertices: Vec<Vertex>,
}
struct Vertex {
    index: usize,
    bones: Vec<(usize, f32)>,
    point: Point,
}

impl Skin {
    pub fn read(p: &Pack<'_>, at: usize, g: &Graph) -> Result<Self, String> {
        p.expect(at, "hclObjectSpaceSkinPNTOperator", 0xc0)?;
        let output = p.u32(at + 0x40)?;
        let count = g.buffer(output)?;
        let set = p.u32(at + 0x44)?;
        let transforms = g
            .transforms
            .get(set)
            .ok_or("Missing cloth skin transform set")?;
        let binds = p
            .array(at + 0x20, 64, 512)?
            .into_iter()
            .map(|row| p.vector(row))
            .collect::<Result<Vec<Matrix>, _>>()?;
        let mut subset = p
            .array(at + 0x30, 2, 512)?
            .into_iter()
            .map(|row| p.u16(row))
            .collect::<Result<Vec<_>, _>>()?;
        if subset.is_empty() {
            subset = (0..transforms.len()).collect();
        }
        if subset.len() != binds.len() || subset.iter().any(|i| *i >= transforms.len()) {
            return Err("Cloth skin bind count does not match its transform set".into());
        }
        let deform = at + 0x48;
        let entries = [0xe0, 0xb0, 0x80, 0x40]
            .into_iter()
            .enumerate()
            .map(|(i, size)| p.array(deform + i * 16, size, MAX_PARTICLES / 16 + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let controls = p.array(deform + 0x40, 1, MAX_PARTICLES / 16 + 1)?;
        let packed = p.array(at + 0xa0, 0x180, MAX_PARTICLES / 16 + 1)?;
        let unpacked = p.array(at + 0xb0, 0x300, MAX_PARTICLES / 16 + 1)?;
        let (blocks, is_packed) = if packed.is_empty() {
            (&unpacked, false)
        } else {
            if !unpacked.is_empty() {
                return Err("Ambiguous packed and unpacked cloth skin storage".into());
            }
            (&packed, true)
        };
        if blocks.len() != controls.len() {
            return Err("Cloth skin blocks do not match their control stream".into());
        }
        let first = p.u16(deform + 0x50)?;
        let last = p.u16(deform + 0x52)?;
        if first > last || last >= count {
            return Err("Cloth skin range exceeds its output".into());
        }
        let mut cursors = [0; 4];
        let mut vertices = Vec::new();
        for (control, block) in controls.into_iter().zip(blocks) {
            let kind = p.u8(control)?;
            if kind >= 4 {
                return Err("Unsupported cloth skin control byte".into());
            }
            let entry = *entries[kind]
                .get(cursors[kind])
                .ok_or("Missing cloth skin influence block")?;
            cursors[kind] += 1;
            let influences = 4 - kind;
            for lane in 0..16 {
                let index = p.u16(entry + lane * 2)?;
                if index < first || index > last {
                    return Err("Cloth skin vertex exceeds its declared range".into());
                }
                let mut bones = Vec::new();
                for influence in 0..influences {
                    let slot = lane * influences + influence;
                    let bone = p.u16(entry + 0x20 + slot * 2)?;
                    if bone >= binds.len() {
                        return Err("Cloth skin references a missing bind".into());
                    }
                    let weight = if influences == 1 {
                        1.
                    } else {
                        p.u8(entry + 0x20 + influences * 32 + slot)? as f32 * (1. / 255.)
                    };
                    bones.push((bone, weight));
                }
                let value = |component: usize| -> Result<Vector, String> {
                    if !is_packed {
                        return p.vector(block + component * 0x100 + lane * 16);
                    }
                    let row = block + component * 0x80 + lane * 8;
                    // hkPackedVector3 stores signed mantissas and a shared float
                    // exponent word. This is not normalized int16 vertex storage.
                    let scale = f32::from_bits((p.u16(row + 6)? as u32) << 16) * 65536.;
                    if !scale.is_finite() {
                        return Err("Invalid packed cloth vector exponent".into());
                    }
                    let mut result = [0.; 3];
                    for (axis, out) in result.iter_mut().enumerate() {
                        *out = (p.u16(row + axis * 2)? as u16 as i16) as f32 * scale;
                    }
                    Ok(result)
                };
                vertices.push(Vertex {
                    index,
                    bones,
                    point: Point {
                        position: value(0)?,
                        normal: value(1)?,
                        tangent: value(2)?,
                    },
                });
            }
        }
        if entries
            .iter()
            .zip(cursors)
            .any(|(entries, used)| entries.len() != used)
        {
            return Err("Unconsumed cloth skin influence blocks".into());
        }
        Ok(Self {
            output,
            set,
            subset,
            binds,
            vertices,
        })
    }

    pub fn apply(
        &self,
        transforms: &[Vec<Matrix>],
        buffers: &mut [Vec<Point>],
        written: &mut [Vec<bool>],
    ) {
        let matrices: Vec<_> = self
            .subset
            .iter()
            .zip(&self.binds)
            .map(|(bone, bind)| compose(&transforms[self.set][*bone], bind))
            .collect();
        for vertex in &self.vertices {
            let mut result = Point::default();
            for &(bone, weight) in &vertex.bones {
                let matrix = &matrices[bone];
                result.position = add(
                    result.position,
                    mul(point(matrix, vertex.point.position), weight),
                );
                result.normal = add(
                    result.normal,
                    mul(direction(matrix, vertex.point.normal), weight),
                );
                result.tangent = add(
                    result.tangent,
                    mul(direction(matrix, vertex.point.tangent), weight),
                );
            }
            buffers[self.output][vertex.index] = result;
            written[self.output][vertex.index] = true;
        }
    }
}
