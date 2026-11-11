//! Cutout sheets with authored reverse faces use the ordinary back-face rasterizer.
//! Their opposing copies can carry different normals and baked light. Drawing both at the same
//! depth lets the back copy replace the visible face. Single-sheet foliage keeps both-sided drawing.
use crate::halo_reach::level::Level;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type Point = [u32; 3];
type Face = [Point; 3];

fn canonical(points: Face) -> Face {
    let first = (0..3).min_by_key(|&i| points[i]).unwrap_or(0);
    std::array::from_fn(|i| points[(first + i) % 3])
}

/// Only materials whose every nondegenerate triangle has a coincident reverse triangle in the
/// same BSP loading set qualify. Exact source positions avoid treating nearby sheets as twins.
/// Mixed paired and single-sheet geometry conservatively retains two-sided rasterization.
/// Call before T-junction subdivision, whose opposite winding can round a new centroid differently.
pub(super) fn paired(level: &Level) -> BTreeSet<u32> {
    let mut faces = BTreeMap::<u32, HashMap<u32, HashSet<Face>>>::new();
    for primitive in &level.primitives {
        let Some(key) = primitive.material.and_then(|key| u32::try_from(key).ok()) else {
            continue;
        };
        if !level
            .materials
            .get(key as usize)
            .is_some_and(|material| material.alpha_mode() == "MASK" && !material.decal())
        {
            continue;
        }
        let bucket = faces
            .entry(key)
            .or_default()
            .entry(primitive.bsps)
            .or_default();
        for triangle in primitive.indices.chunks_exact(3) {
            let points = [triangle[0], triangle[1], triangle[2]].map(|index| {
                primitive.vertices[index as usize]
                    .position
                    .map(|value| if value == 0. { 0 } else { value.to_bits() })
            });
            if points[0] != points[1] && points[1] != points[2] && points[2] != points[0] {
                bucket.insert(canonical(points));
            }
        }
    }
    faces
        .into_iter()
        .filter_map(|(key, groups)| {
            let paired = groups.values().all(|faces| {
                !faces.is_empty()
                    && faces
                        .iter()
                        .all(|&[a, b, c]| faces.contains(&canonical([a, c, b])))
            });
            paired.then_some(key)
        })
        .collect()
}
