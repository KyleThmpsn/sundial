//! Cooked MCC U13 convex geometry and explicit collection and MOPP child indices.
use super::*;

pub(super) fn read(cache: &Cache, at: usize) -> Result<Value> {
    let capsules = cache
        .block(at + 0x8c, 0x70)?
        .into_iter()
        .map(|row| {
            let a = cache.floats::<3>(row + 0x50)?;
            let b = cache.floats::<3>(row + 0x60)?;
            let radius = cache.f32(row + 0x40)?;
            ensure!(
                a.iter().chain(&b).all(|v| v.is_finite()) && radius.is_finite() && radius > 0.,
                "Invalid source collision capsule"
            );
            Ok(json!({"a_source":a,"b_source":b,"radius_source":radius,
            "material_index":cache.u8(row+4)? as i8}))
        })
        .collect::<Result<Vec<_>>>()?;
    let boxes = cache.block(at + 0x98, 0xe0)?.into_iter().map(|row| {
        let half = cache.floats::<3>(row + 0x50)?;
        let radius = cache.f32(row + 0x40)?;
        let transform = cache.floats::<16>(row + 0xa0)?;
        ensure!(half.iter().all(|v| v.is_finite() && *v > 0.)
            && radius.is_finite() && radius >= 0.
            && transform.iter().all(|v| v.is_finite()), "Invalid source collision box");
        Ok(json!({"half_extents_source":half,"radius_source":radius,"transform_source":transform,
            "material_index":cache.u8(row+4)? as i8}))
    }).collect::<Result<Vec<_>>>()?;
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
            "radius_source":cache.f32(row+0x40)?,"material_index":cache.u8(row+4)? as i8}));
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
    let mopps = cache
        .block(at + 0xf8, 0x80)?
        .into_iter()
        .map(|row| {
            Ok(json!({"type":cache.u32(row+0x18)?,
                "shape":[cache.i16(row+0x58)?,cache.i16(row+0x5a)?],
                "code_size":cache.i32(row+0x38)?,
                "code_info":cache.floats::<4>(row+0x40)?,
                "tail":(0x70..0x80).step_by(4).map(|offset| cache.u32(row+offset))
                    .collect::<Result<Vec<_>>>()?}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"polyhedra":polyhedra,"boxes":boxes,"capsules":capsules,"lists":lists,"mopps":mopps}))
}
