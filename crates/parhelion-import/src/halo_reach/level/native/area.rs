//! Each connected area's row: its own static compound placing the collision meshes it holds, its
//! own static instance group placing its pieces, and the props, spawns, ambience, visibility,
//! decorators and interiors of what it loads ([`Area`]).
use super::{
    Area, Donor, Interior, Loading, PACKFILE_SHAPE, Piece, Prop, Spawn, Templates, Visible, World,
    decorators, statics, world,
};
use crate::{
    presentation::Graph,
    tiger::{
        physics::{Instance, static_compound},
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Each area's row of the conversion `areas` and its report. Every input holds one entry per
/// area, apart from `pieces`, drawn by the areas `render_areas`, `meshes`, held by the areas of
/// [`world::Meshes::areas`], and `props`, each loaded with its BSPs.
#[allow(clippy::too_many_arguments)]
pub(super) fn rows(
    native: &mut Reader,
    graph: &mut Graph,
    templates: &Templates,
    donor: &Donor,
    areas: &[Vec<usize>],
    loading: &Loading,
    pieces: &[Piece],
    render_areas: &[u64],
    meshes: &world::Meshes,
    decorated: &decorators::Added,
    props: &[Prop],
    spawns: &[Vec<Spawn>],
    ambience: &[Option<String>],
    interiors: &[Vec<Interior>],
    open_seams: &[BTreeSet<i16>],
) -> Result<(Vec<Area>, Vec<Value>)> {
    // Each area's static compound places its own meshes, numbered in its own order.
    let area_shapes = (0..areas.len())
        .map(|area| {
            (0..meshes.instances.len())
                .filter(|&shape| meshes.areas[shape] & 1u64 << area != 0)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // A compound encodes every mesh it places again, so the areas' compounds encode in parallel.
    let compounds = crate::cancellation::parallel_map(&area_shapes, |shapes| {
        if shapes.is_empty() {
            return Ok(None);
        }
        let placed = shapes
            .iter()
            .enumerate()
            .map(|(local, &shape)| {
                let instance = &meshes.instances[shape];
                Ok(Instance {
                    translation: instance.translation,
                    bounds: instance.bounds,
                    user_data: PACKFILE_SHAPE | u64::try_from(local)?,
                    triangles: instance.triangles.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        static_compound(&donor.world_compound, &donor.world_mesh, &placed).map(Some)
    })?;
    let mut area_rows = Vec::with_capacity(areas.len());
    let mut area_report = Vec::with_capacity(areas.len());
    for ((area, zone_sets), (shapes, compound)) in areas
        .iter()
        .enumerate()
        .zip(area_shapes.iter().zip(compounds))
    {
        let bit = 1u64 << area;
        let area_pieces = (0..pieces.len())
            .filter(|&piece| render_areas[piece] & bit != 0)
            .collect::<Vec<_>>();
        ensure!(
            !shapes.is_empty() && !area_pieces.is_empty(),
            "Area {area} converted to no pieces or no collision"
        );
        let symbol = if area == 0 {
            meshes.world.compound.clone()
        } else {
            format!("{}-{area}", meshes.world.compound)
        };
        let compound = compound?.context("Area compound")?;
        graph.add(
            &symbol,
            templates.world_compound,
            &compound,
            None,
            Vec::new(),
        )?;
        let placed = area_pieces
            .iter()
            .map(|&piece| statics::Instance {
                mesh: &pieces[piece].mesh,
                position: pieces[piece].position,
                minimum: pieces[piece].minimum,
                maximum: pieces[piece].maximum,
            })
            .collect::<Vec<_>>();
        let group = if area == 0 {
            "statics".to_owned()
        } else {
            format!("statics-{area}")
        };
        let statics = statics::group(native, graph, &templates.statics, &placed, &group)?;
        let (decorator_records, decorator_clusters) = &decorated.areas[area];
        let visibility = statics::identifiers(&group, placed.len())
            .into_iter()
            .zip(&placed)
            .map(|(id, instance)| Visible {
                id,
                minimum: instance.minimum,
                maximum: instance.maximum,
                upper: false,
            })
            .chain(decorator_clusters.iter().cloned())
            .collect();
        area_report.push(json!({
            "zone_sets": zone_sets,
            "bsps": loading.groups[area].iter().map(|&bsp| loading.tags[bsp].path.as_str()).collect::<Vec<_>>(),
            "pieces": area_pieces.len(),
            "collision_meshes": shapes.len(),
            "spawns": spawns[area].len(),
            "open_seams": open_seams[area],
            "interiors": interiors[area].len(),
            "decorator_records": decorator_records.len(),
            "decorator_clusters": decorator_clusters.len(),
        }));
        area_rows.push(Area {
            zone_sets: zone_sets.clone(),
            props: props
                .iter()
                .filter(|prop| loading.membership(prop.bsps) & bit != 0)
                .cloned()
                .collect(),
            pieces: area_pieces,
            world: World {
                compound: symbol,
                meshes: shapes
                    .iter()
                    .map(|&shape| meshes.world.meshes[shape].clone())
                    .collect(),
            },
            statics,
            spawns: spawns[area].clone(),
            ambience: ambience[area].clone(),
            visibility,
            decorators: decorator_records.clone(),
            interiors: interiors[area].clone(),
        });
    }
    Ok((area_rows, area_report))
}
