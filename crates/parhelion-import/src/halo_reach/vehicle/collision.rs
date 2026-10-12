//! Cooked MCC U13 convex geometry and explicit compound-child indices.
use super::*;

pub(super) fn read(cache: &Cache, at: usize) -> Result<Value> {
    let vectors = cache.block(at + 0xbc, 0x30)?;
    let planes = cache.block(at + 0xc8, 16)?;
    let mut vector_cursor = 0;
    let mut plane_cursor = 0;
    let mut polyhedra = Vec::new();
    for row in cache.block(at + 0xb0, 0xb0)? {
        let groups = usize::try_from(cache.i32(row + 0x78)?)?;
        let count = usize::try_from(cache.i32(row + 0x80)?)?;
        let plane_count = usize::try_from(cache.i32(row + 0x98)?)?;
        ensure!(
            (4..=1024).contains(&count) && groups == count.div_ceil(4),
            "Source collision vertex count differs from its four-vector storage"
        );
        let selected = vectors
            .get(vector_cursor..vector_cursor + groups)
            .context("Source collision vector pool is short")?;
        let mut vertices = Vec::with_capacity(count);
        for &group in selected {
            let values = cache.floats::<12>(group)?;
            for lane in 0..4 {
                if vertices.len() < count {
                    vertices.push([values[lane], values[lane + 4], values[lane + 8]]);
                }
            }
        }
        let equations = planes
            .get(plane_cursor..plane_cursor + plane_count)
            .context("Source collision plane pool is short")?
            .iter()
            .map(|&at| cache.floats::<4>(at))
            .collect::<Result<Vec<_>>>()?;
        vector_cursor += groups;
        plane_cursor += plane_count;
        polyhedra.push(json!({"vertices_source":vertices,"planes_source":equations,
            "radius_source":cache.f32(row+0x40)?}));
    }
    ensure!(
        vector_cursor == vectors.len() && plane_cursor == planes.len(),
        "Source collision pools contain unassigned geometry"
    );
    let children = cache.block(at + 0xec, 0x20)?;
    let mut child_cursor = 0;
    let mut lists = Vec::new();
    for row in cache.block(at + 0xe0, 0x90)? {
        let count = usize::try_from(cache.i32(row + 0x38)?)?;
        ensure!(
            (1..=256).contains(&count),
            "Invalid source collision list size"
        );
        lists.push(
            children
                .get(child_cursor..child_cursor + count)
                .context("Source collision child pool is short")?
                .iter()
                .map(|&at| Ok([cache.i16(at)?, cache.i16(at + 2)?]))
                .collect::<Result<Vec<_>>>()?,
        );
        child_cursor += count;
    }
    ensure!(
        child_cursor == children.len(),
        "Unassigned collision children"
    );
    Ok(json!({"polyhedra":polyhedra,"lists":lists}))
}
