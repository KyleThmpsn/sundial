//! The pieces' static meshes: each render group's mesh, its positions snapped to the pieces'
//! shared grid, written as a native static mesh copying the donor model's buffers ([`statics`]).
use super::{
    ATTRIBUTE_STRIDE, DETAIL_COORDINATES, Donor, POSITION_STEP, Piece, Triangle, bounds, mesh,
    statics,
};
use crate::{
    halo_reach::{level::Level, material::Material, native::material::vertex_data::Table},
    presentation::{Graph, geometry},
    tiger::reader::Reader,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, HashMap};

/// The position step each piece stores its positions on, and the step each corner takes.
///
/// A piece too wide for POSITION_STEP stores its positions on a coarser power-of-two step, so
/// a corner it shares with a finer piece rounded to two points. Paradiso's play area holds
/// 1/128 m and 1/64 m pieces beside 1/256 m ones, and thin lines of sky showed along its
/// ground. Each corner takes the coarsest step of the pieces that hold it, which a finer piece
/// also stores exactly.
pub(super) struct Steps {
    /// Each piece's own step.
    pieces: Vec<f32>,
    /// Each corner's step, by its position in the map's space on the finest grid ([`corner_key`]).
    corners: HashMap<[i64; 3], f32>,
}

impl Steps {
    /// The steps of the pieces `renders`, whose corners move by `offset` into the map's space.
    pub(super) fn new(level: &Level, renders: &[Vec<Triangle>], offset: [f32; 3]) -> Self {
        let pieces = renders
            .iter()
            .map(|group| {
                let (low, high) = bounds(group.iter().flat_map(|t| {
                    let primitive = &level.primitives[t.primitive as usize];
                    t.corners.map(|c| primitive.vertices[c as usize].position)
                }));
                let half = (0..3).map(|k| (high[k] - low[k]) / 2.).fold(0., f32::max);
                // Snapping moves the extent by up to a step, so two steps of margin keep the
                // encoder from doubling it again.
                let mut step = POSITION_STEP;
                while half + 2. * step > 32767. * step {
                    step *= 2.;
                }
                step
            })
            .collect::<Vec<_>>();
        let mut corners = HashMap::<[i64; 3], f32>::new();
        for (group, &step) in renders.iter().zip(&pieces) {
            for t in group {
                let primitive = &level.primitives[t.primitive as usize];
                for c in t.corners {
                    let position = primitive.vertices[c as usize].position;
                    let key = corner_key(std::array::from_fn(|k| position[k] + offset[k]));
                    let entry = corners.entry(key).or_insert(step);
                    *entry = entry.max(step);
                }
            }
        }
        Self { pieces, corners }
    }
}

fn corner_key(position: [f32; 3]) -> [i64; 3] {
    position.map(|v| (v / POSITION_STEP).round() as i64)
}

/// The pieces' static meshes in the graph.
pub(super) struct Written {
    pub pieces: Vec<Piece>,
    /// Their vertices and triangles.
    pub vertices: usize,
    pub triangles: usize,
    pub seconds: f64,
}

/// Writes each of `renders` as a static mesh ([`statics::mesh`]) drawing with the materials
/// `used`, its positions in the map's space, moved by `offset`, relative to `arena`. Its vertices'
/// detail lanes take `ground`, `neutral`, `placed_from` and `vertex_data` ([`mesh`]). Meshes
/// prepare `workers` at a time.
#[allow(clippy::too_many_arguments)]
pub(super) fn write(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &statics::Templates,
    donor: &Donor,
    level: &Level,
    renders: &[Vec<Triangle>],
    steps: &Steps,
    used: &BTreeMap<u32, Material>,
    offset: [f32; 3],
    arena: [f32; 3],
    ground: &HashMap<u32, [f32; 2]>,
    neutral: [f32; 2],
    placed_from: u32,
    vertex_data: Option<&Table>,
    workers: usize,
    report: &mut dyn FnMut(&str),
) -> Result<Written> {
    let started = std::time::Instant::now();
    let stages = used
        .iter()
        .map(|(&key, material)| {
            (
                key,
                if material.alpha_mode() == "BLEND" {
                    7
                } else {
                    0
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let solid = statics::solid(used);
    let clipped = statics::clipped(used);
    let casters = statics::casters(used);
    let mut pieces = Vec::new();
    let mut vertices = 0usize;
    let mut drawn = 0usize;
    for (batch, groups) in renders.chunks(workers).enumerate() {
        report(&format!(
            "Preparing Render Geometry / Piece {}/{}",
            batch * workers + 1,
            renders.len()
        ));
        let prepared = crate::cancellation::parallel(groups, |group| {
            let (mut mesh, coordinates) = mesh(
                level,
                &group.iter().collect::<Vec<_>>(),
                offset,
                ground,
                neutral,
                placed_from,
                vertex_data,
            )?;
            // Snap in shared world coordinates before subtracting the instances' origin. This
            // also removes half-step rounding differences between individual statics.
            let piece = batch * workers
                + groups
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, group))
                    .context("Render piece outside its batch")?;
            let step = steps.pieces[piece];
            for position in &mut mesh.positions {
                let corner = steps
                    .corners
                    .get(&corner_key(*position))
                    .copied()
                    .unwrap_or(step)
                    .max(step);
                *position = std::array::from_fn(|axis| {
                    (position[axis] / corner).round() * corner - arena[axis]
                });
            }
            let mut streams = geometry::encode_static(&mesh, step)?;
            let minimum = std::array::from_fn(|k| streams.bounds.0[k] + arena[k]);
            let maximum = std::array::from_fn(|k| streams.bounds.1[k] + arena[k]);
            ensure!(
                streams.attributes.len() == coordinates.len() * ATTRIBUTE_STRIDE,
                "Piece has {} attribute bytes for {} lightmap coordinates",
                streams.attributes.len(),
                coordinates.len()
            );
            for (record, lanes) in streams
                .attributes
                .chunks_exact_mut(ATTRIBUTE_STRIDE)
                .zip(&coordinates)
            {
                for (k, bits) in lanes.iter().enumerate() {
                    let at = DETAIL_COORDINATES + k * 2;
                    record[at..at + 2].copy_from_slice(&bits.to_le_bytes());
                }
            }
            let draws = mesh
                .groups
                .iter()
                .map(|(key, _)| statics::Draw {
                    key: *key,
                    stage: stages.get(key).copied().unwrap_or(0),
                    solid: solid.contains(key),
                    clipped: clipped.contains(key),
                    casts: casters.contains(key),
                    glow: graph.contains(&format!("material-{key:08X}-glow")),
                    light: graph.contains(&format!("material-{key:08X}-baked")),
                })
                .collect::<Vec<_>>();
            Ok((
                streams,
                draws,
                minimum,
                maximum,
                mesh.positions.len(),
                mesh.groups
                    .iter()
                    .map(|(_, faces)| faces.len())
                    .sum::<usize>(),
            ))
        })?;
        for (local, (streams, draws, minimum, maximum, count, triangles)) in
            prepared.into_iter().enumerate()
        {
            let part = batch * workers + local;
            report(&format!(
                "Writing Render Geometry / Piece {}/{}",
                part + 1,
                renders.len()
            ));
            let name = format!("piece{part}");
            vertices += count;
            drawn += triangles;
            let mesh = statics::mesh(
                native,
                graph,
                templates,
                &donor.model,
                donor.row,
                &streams,
                &draws,
                &name,
            )?;
            pieces.push(Piece {
                mesh,
                minimum,
                maximum,
                position: arena,
            });
        }
    }
    Ok(Written {
        pieces,
        vertices,
        triangles: drawn,
        seconds: started.elapsed().as_secs_f64(),
    })
}
