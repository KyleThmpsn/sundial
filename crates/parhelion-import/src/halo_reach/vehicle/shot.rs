//! MCC U13 object collision BSPs used for surface and damage-section hits.
use super::*;
use std::collections::BTreeSet;

pub(super) fn read(cache: &Cache, model: &super::super::cache::Tag) -> Result<Value> {
    let Some(tag) = cache.reference(model.address()? + 0x10, Some("coll"))? else {
        return Ok(Value::Null);
    };
    let at = tag.address()?;
    cache.meta(at, 0x50)?;
    let materials = cache
        .block(at + 0x14, 4)?
        .into_iter()
        .map(|row| cache.string_id(cache.u32(row)?))
        .collect::<Result<Vec<_>>>()?;
    let nodes = cache
        .block(at + 0x44, 12)?
        .into_iter()
        .map(|row| cache.string_id(cache.u32(row)?))
        .collect::<Result<Vec<_>>>()?;
    let regions = cache.block(at + 0x20, 0x10)?.into_iter().map(|region| {
        let permutations = cache.block(region + 4, 0x28)?.into_iter().map(|permutation| {
            let bsps = cache.block(permutation + 4, 0x70)?.into_iter().map(|bsp| {
                let node = usize::try_from(cache.i16(bsp)?)?;
                ensure!(node < nodes.len(), "Collision BSP node exceeds its table");
                let planes = cache.block(bsp + 0x1c, 16)?;
                let edges = cache.block(bsp + 0x58, 12)?;
                let vertices = cache.block(bsp + 0x64, 16)?;
                let mut surfaces = Vec::new();
                for (index, row) in cache.block(bsp + 0x4c, 12)?.into_iter().enumerate() {
                    let flags = cache.u8(row + 0xa)?;
                    if flags & 0x10 != 0 { continue; }
                    let material = usize::try_from(cache.i16(row + 4)?)?;
                    ensure!(material < materials.len(), "Collision surface material exceeds its table");
                    let plane = *planes.get(usize::from(cache.u16(row)? & 0x7fff))
                        .context("Collision surface plane")?;
                    let plane = cache.floats::<4>(plane)?;
                    ensure!(plane.iter().all(|v| v.is_finite()), "Nonfinite collision plane");
                    let first = usize::from(cache.u16(row + 2)?);
                    let mut edge = first;
                    let mut visited = BTreeSet::new();
                    let mut points = Vec::new();
                    loop {
                        ensure!(visited.insert(edge) && visited.len() <= 256, "Collision surface edge cycle differs");
                        let at = *edges.get(edge).context("Collision surface edge")?;
                        let (vertex, next) = if usize::from(cache.u16(at + 8)?) == index {
                            (cache.u16(at)?, cache.u16(at + 4)?)
                        } else {
                            ensure!(usize::from(cache.u16(at + 10)?) == index, "Collision edge does not border its surface");
                            (cache.u16(at + 2)?, cache.u16(at + 6)?)
                        };
                        let point = cache.floats::<3>(*vertices.get(usize::from(vertex))
                            .context("Collision edge vertex")?)?;
                        ensure!(point.iter().all(|v| v.is_finite()), "Nonfinite collision vertex");
                        points.push(point);
                        edge = usize::from(next);
                        if edge == first { break; }
                    }
                    ensure!(points.len() >= 3, "Collision surface has fewer than three vertices");
                    surfaces.push(json!({"material":material,"flags":flags,"plane_source":plane,"points_source":points}));
                }
                Ok(json!({"node":node,"surfaces":surfaces}))
            }).collect::<Result<Vec<_>>>()?;
            Ok(json!({"name":cache.string_id(cache.u32(permutation)?)?,"bsps":bsps}))
        }).collect::<Result<Vec<_>>>()?;
        Ok(json!({"name":cache.string_id(cache.u32(region)?)?,"permutations":permutations}))
    }).collect::<Result<Vec<_>>>()?;
    Ok(json!({"tag":tag,"materials":materials,"nodes":nodes,"regions":regions}))
}
