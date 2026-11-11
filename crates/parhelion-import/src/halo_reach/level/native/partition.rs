//! The level's render triangles grouped into the pieces' meshes: by the areas that draw them,
//! inside or outside the play area, by span band, period and tiling class, then split at the
//! median until each group fits one model.
use super::{
    Loading, MAX_PIECE_EXTENT, MAX_VERTICES, PlayArea, Timing, Triangle, WIDE_PIECE_TRIANGLES,
    bounds, period_range, spatial_coordinates, split, tiling_class, triangle, triangle_span,
    uv_band, uv_period, uv_range, uv_ranges, uv_tiling,
};
use crate::halo_reach::level::Level;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Every render triangle of the level's primitives, and each primitive's texture coordinate
/// period ([`uv_period`]).
pub(super) fn triangles(level: &Level) -> Result<(Vec<Triangle>, Vec<[f32; 2]>)> {
    let periods = level
        .primitives
        .iter()
        .map(|primitive| uv_period(primitive.material.and_then(|m| level.materials.get(m))))
        .collect::<Vec<_>>();
    let tilings = level
        .primitives
        .iter()
        .map(|primitive| uv_tiling(primitive.material.and_then(|m| level.materials.get(m))))
        .collect::<Vec<_>>();
    let mut triangles = Vec::new();
    for (index, primitive) in level.primitives.iter().enumerate() {
        for face in primitive.indices.chunks_exact(3) {
            triangles.push(triangle(
                level,
                u32::try_from(index)?,
                [face[0], face[1], face[2]],
                periods[index],
                tilings[index],
            ));
        }
    }
    Ok((triangles, periods))
}

/// Each material's period, triangles, the range its shifted texture coordinates cover and its
/// widest triangle's span, for the report. `periods` are the primitives' ([`triangles`]).
pub(super) fn uv_report(level: &Level, triangles: &[Triangle], periods: &[[f32; 2]]) -> Vec<Value> {
    let mut uv_materials = BTreeMap::<u32, (Value, usize, [f32; 2], [f32; 2], f32)>::new();
    for t in triangles {
        let primitive = &level.primitives[t.primitive as usize];
        let key = primitive.material.map_or(u32::MAX, |m| m as u32);
        let entry = uv_materials.entry(key).or_insert_with(|| {
            (
                json!(periods[t.primitive as usize]),
                0,
                [f32::INFINITY; 2],
                [f32::NEG_INFINITY; 2],
                0.,
            )
        });
        entry.1 += 1;
        for axis in 0..2 {
            entry.2[axis] = entry.2[axis].min(t.uv[0][axis]);
            entry.3[axis] = entry.3[axis].max(t.uv[1][axis]);
        }
        entry.4 = entry.4.max(triangle_span(t));
    }
    uv_materials
        .iter()
        .map(|(&key, (period, triangles, low, high, widest))| {
            json!({
                "material": level
                    .materials
                    .get(key as usize)
                    .and_then(|material| material.tag.as_ref())
                    .map(|tag| tag.path.clone()),
                "period": period,
                "triangles": triangles,
                "range": [high[0] - low[0], high[1] - low[1]],
                "widest": widest,
            })
        })
        .collect()
}

/// The render triangles grouped into the pieces' meshes.
pub(super) struct Renders {
    /// Each piece's triangles.
    pub groups: Vec<Vec<Triangle>>,
    /// The areas that draw each of `groups`.
    pub areas: Vec<u64>,
    /// Each band's areas, class, triangles, pieces and refused splits, for the report.
    pub bands: Vec<Value>,
}

/// Groups `triangles`, in Reach's space, into the pieces' meshes, play-area pieces first. The play
/// area is in the map's space, so each triangle's center moves by `offset` to test it.
pub(super) fn renders(
    level: &Level,
    triangles: Vec<Triangle>,
    play: &PlayArea,
    loading: &Loading,
    offset: [f32; 3],
    timing: &mut Timing,
) -> Result<Renders> {
    let triangle_center = |t: &Triangle| -> [f32; 3] {
        let primitive = &level.primitives[t.primitive as usize];
        let points = t.corners.map(|c| primitive.vertices[c as usize].position);
        std::array::from_fn(|k| (points[0][k] + points[1][k] + points[2][k]) / 3.)
    };
    // Texture coordinates are 16-bit steps over a piece's range, so a triangle spanning a sliver
    // of that range collapses to one texel and streaks. Each triangle shifts its coordinates by
    // whole periods toward zero (`triangle`), and triangles draw in bands of similar span
    // (`uv_band`), each band's pieces holding a range that every one of its triangles spans
    // thousands of steps of. Shifting and grouping whole surfaces left Breakneck's and Countdown's
    // floors and catwalks streaked: one piece held triangles spanning 2,700 and 160,000 units
    // beside triangles spanning a fraction of one, steps of 0.04 and 2.7. Splitting by span with
    // whole-surface shifts took 871 and 503 pieces, past a package's entries.
    let fits_model = |group: &[Triangle], limit: f32| {
        // Three corners each is the most a group can use.
        let vertices = group.len() * 3 <= MAX_VERTICES || {
            let mut seen = std::collections::HashSet::new();
            group.iter().all(|t| {
                let shift = t.shift.map(f32::to_bits);
                t.corners.iter().for_each(|&c| {
                    seen.insert((t.primitive, c, shift));
                });
                seen.len() <= MAX_VERTICES
            })
        };
        vertices && uv_range(group) <= limit
    };
    // A play-area piece also stays within MAX_PIECE_EXTENT, so its positions keep POSITION_STEP.
    let compact = |group: &[Triangle]| {
        group.len() < WIDE_PIECE_TRIANGLES || {
            let (low, high) = bounds(group.iter().flat_map(|t| {
                let primitive = &level.primitives[t.primitive as usize];
                t.corners.map(|c| primitive.vertices[c as usize].position)
            }));
            (0..3).all(|k| high[k] - low[k] <= MAX_PIECE_EXTENT)
        }
    };
    let mut renders = Vec::new();
    // The areas that draw each of `renders`.
    let mut render_areas = Vec::new();
    let mut band_report = Vec::new();
    // The level's triangles are in Reach's space and the collision faces already moved. Taking the
    // triangles unmoved put Reflection's arena, moved 735 m down, wholly outside the play area, so
    // its pieces took the scenery's coarser positions and no extent cap.
    let (inside, scenery): (Vec<_>, Vec<_>) = triangles.into_iter().partition(|t| {
        let center = triangle_center(t);
        play.contains(std::array::from_fn(|k| center[k] + offset[k]))
    });
    timing.lap("collision_faces_and_play_partition");
    // Triangles also group by how often their base maps repeat ([`tiling_class`]). A piece's range
    // counts as its most repeated base map gives, so a surface repeating once in 200 units beside
    // one repeating every unit would hold the other to its own range and split pieces they shared.
    // They group first by the areas that draw them, so each piece serves the same areas.
    for (region, in_play) in [(inside, true), (scenery, false)] {
        let mut bands = BTreeMap::<(u64, u32, u32, [u32; 2]), Vec<Triangle>>::new();
        for triangle in region {
            bands
                .entry((
                    loading.membership(level.primitives[triangle.primitive as usize].bsps),
                    uv_band(triangle_span(&triangle)),
                    triangle.period,
                    triangle.tiling.map(tiling_class),
                ))
                .or_default()
                .push(triangle);
        }
        // Each band splits on its own, so bands split in parallel and their pieces follow in band
        // order.
        let bands = bands
            .into_iter()
            .map(|(key, group)| (key, std::sync::Mutex::new(group)))
            .collect::<Vec<_>>();
        timing.lap(if in_play {
            "render_bands_play"
        } else {
            "render_bands_scenery"
        });
        let split_bands = crate::cancellation::parallel_map(&bands, |(key, group)| {
            let (_, band, period, _) = *key;
            let group = std::mem::take(&mut *group.lock().unwrap_or_else(|e| e.into_inner()));
            let limit = period_range(period, band);
            // Why groups failed to fit, for the report: too many vertices, too wide a texture
            // coordinate range, or too wide a play-area piece.
            let refused: [std::sync::atomic::AtomicUsize; 3] = Default::default();
            let fits = |group: &[Triangle]| {
                let model = fits_model(group, limit);
                let near = !in_play || compact(group);
                if !(model && near) {
                    let reason = if !model && uv_range(group) > limit {
                        1
                    } else if !model {
                        0
                    } else {
                        2
                    };
                    refused[reason].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                model && near
            };
            let triangles = group.len();
            let mut pieces = Vec::new();
            let coordinates = |group: &[Triangle]| {
                let ranges = uv_ranges(group);
                let axis = usize::from(ranges[1] > ranges[0]);
                if ranges[axis] > limit {
                    // Spatially overlapping surfaces can have unrelated UV origins. Splitting
                    // their positions does not separate those ranges and fragments the same
                    // geometry into thousands of tiny resources. Preserve every UV and split
                    // by its midpoint until the existing precision bound is satisfied.
                    group
                        .iter()
                        .map(|triangle| (triangle.uv[0][axis] + triangle.uv[1][axis]) * 0.5)
                        .collect()
                } else {
                    spatial_coordinates(group, triangle_center)
                }
            };
            split(group, &coordinates, &fits, &mut pieces);
            (
                pieces,
                triangles,
                refused.map(std::sync::atomic::AtomicUsize::into_inner),
            )
        })?;
        for (((areas, band, period, class), _), (pieces, triangles, refused)) in
            bands.into_iter().zip(split_bands)
        {
            band_report.push(json!({
                "areas": areas,
                "play": in_play,
                "band": band,
                "period": period,
                "tiling": class,
                "triangles": triangles,
                "pieces": pieces.len(),
                "refused": refused,
            }));
            renders.extend(pieces);
            render_areas.resize(renders.len(), areas);
        }
        timing.lap(if in_play {
            "render_split_play"
        } else {
            "render_split_scenery"
        });
    }
    Ok(Renders {
        groups: renders,
        areas: render_areas,
        bands: band_report,
    })
}
