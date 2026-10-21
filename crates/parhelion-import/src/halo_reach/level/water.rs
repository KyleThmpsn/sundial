//! MCC water's body coordinates are float pairs per triangle corner, separate from wave UVs.
use super::*;

const WATER: u16 = 1;
const COORDINATE_SLOT: usize = 7;
const DECLARATION: u32 = 0x0018_001b;

pub(super) struct Coordinates {
    starts: BTreeMap<usize, usize>,
    bytes: Vec<u8>,
}

impl Coordinates {
    pub(super) fn read(
        cache: &Cache,
        pages: &mut Pages,
        geometry: &Geometry,
        mesh: usize,
        parts: &[usize],
    ) -> Result<Option<Self>> {
        let parts = parts
            .iter()
            .copied()
            .map(|part| Ok((part, cache.u16(part + 18)? & WATER != 0)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(part, water)| water.then_some(part))
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Ok(None);
        }
        let row = *geometry
            .sections
            .get(mesh)
            .context("Water mesh outside geometry")?;
        let starts = cache.block(row + 64, 2)?;
        ensure!(
            starts.len() == parts.len(),
            "Water part coordinate ranges differ"
        );
        let buffer = geometry
            .section_buffer(cache, mesh, COORDINATE_SLOT)?
            .context("Water mesh has no body-coordinate stream")?;
        let (_, declaration, bytes) = geometry.vertex_buffer(cache, pages, buffer)?;
        ensure!(
            declaration == DECLARATION,
            "Unsupported water-coordinate declaration {declaration:#x}"
        );
        // The declaration's 24-byte stride describes a triangle's three pairs. The source
        // allocation can include unused capacity, so part ranges bound the consumed prefix.
        let starts = parts
            .into_iter()
            .zip(starts)
            .map(|(part, start)| Ok((part, usize::from(cache.u16(start)?))))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Some(Self { starts, bytes }))
    }

    pub(super) fn apply(
        &self,
        cache: &Cache,
        part: usize,
        topology: u8,
        indices: &[u32],
        primitive: &mut Primitive,
    ) -> Result<()> {
        let Some(&start) = self.starts.get(&part) else {
            return Ok(());
        };
        ensure!(topology == 3, "Water coordinates require a triangle list");
        let first = usize::try_from(cache.i32(part + 4)?)?;
        let count = usize::try_from(cache.i32(part + 8)?)?;
        ensure!(
            count.is_multiple_of(3) && start.is_multiple_of(3),
            "Incomplete water triangle range"
        );
        let source = indices
            .get(
                first
                    ..first
                        .checked_add(count)
                        .context("Water index range overflow")?,
            )
            .context("Water draw outside index buffer")?;
        let end = start
            .checked_add(count)
            .and_then(|end| end.checked_mul(8))
            .context("Water coordinate range overflow")?;
        let bytes = self
            .bytes
            .get(start * 8..end)
            .context("Water coordinates outside stream")?;
        let original = primitive
            .vertices
            .iter()
            .map(|vertex| (vertex.source, vertex))
            .collect::<BTreeMap<_, _>>();
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut shared = BTreeMap::new();
        for (face, values) in source.chunks_exact(3).zip(bytes.chunks_exact(24)) {
            if face[0] == face[1] || face[0] == face[2] || face[1] == face[2] {
                continue;
            }
            for (&index, pair) in face.iter().zip(values.chunks_exact(8)) {
                let uv = std::array::from_fn(|lane| {
                    f32::from_le_bytes(pair[lane * 4..lane * 4 + 4].try_into().unwrap())
                });
                ensure!(
                    uv.iter().all(|value| value.is_finite()),
                    "Nonfinite water coordinates"
                );
                let key = (index, uv.map(f32::to_bits));
                let target = match shared.get(&key) {
                    Some(&target) => target,
                    None => {
                        let mut vertex = (*original
                            .get(&index)
                            .context("Water corner outside decoded part")?)
                        .clone();
                        vertex.water_uv = Some(uv);
                        let target = u32::try_from(vertices.len())?;
                        vertices.push(vertex);
                        shared.insert(key, target);
                        target
                    }
                };
                triangles.push(target);
            }
        }
        primitive.vertices = vertices;
        primitive.indices = triangles;
        Ok(())
    }
}
