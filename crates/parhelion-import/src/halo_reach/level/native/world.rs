//! The level's world collision: the collision polygons of its structure BSPs and the instances
//! they place, the placed objects' faces, their split into meshes of at most
//! [`MAX_COLLISION_TRIANGLES`] triangles, and those meshes in the graph, each placed at its own
//! center ([`super::area`] places each area's in its own static compound).
use super::{
    Donor, Face, Loading, MAX_COLLISION_TRIANGLES, PACKFILE_SHAPE, PlayArea, World, bounds,
    spatial_coordinates, split, sub,
};
use crate::{
    halo_reach::{
        cache::Cache,
        level::{
            Level,
            collision::{self, Polygon},
        },
        resource::Pages,
    },
    presentation::Graph,
    tiger::physics::{Instance, bare_mesh},
};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

/// The collision polygons of the structure BSPs and of the instances they place, in Reach's space.
pub(super) struct Polygons {
    pub polygons: Vec<Polygon>,
    /// The areas whose collision holds each of `polygons`, bit `n` for the `n`th area.
    pub areas: Vec<u64>,
    /// How many of `polygons` are the structures' own.
    pub structure: usize,
    /// Each area's open seams, those joining two BSPs it loads ([`collision::shared_seams`]).
    pub open_seams: Vec<BTreeSet<i16>>,
    /// The structure polygons on a seam that some area loading their BSP leaves open.
    pub open_seam_surfaces: usize,
}

/// Each BSP's collision places that BSP's instances, and each polygon lists the areas whose
/// collision holds it. A seam joining two BSPs an area loads is open in that area
/// ([`collision`]).
pub(super) fn read(
    cache: &Cache,
    pages: &mut Pages,
    loading: &Loading,
    level: &Level,
    report: &mut dyn FnMut(&str),
) -> Result<Polygons> {
    let mut polygons = Vec::new();
    let mut polygon_areas = Vec::new();
    let mut structure = 0;
    let open_seams = loading
        .groups
        .iter()
        .map(|group| {
            let loaded = group
                .iter()
                .map(|&bsp| loading.tags[bsp].clone())
                .collect::<Vec<_>>();
            collision::shared_seams(cache, &loaded)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut open_seam_surfaces = 0;
    for (index, tag) in loading.tags.iter().enumerate() {
        report(&format!(
            "Reading Collision / BSP {}/{} / {}",
            index + 1,
            loading.tags.len(),
            tag.path
        ));
        let collision = collision::read(cache, pages, tag, &BTreeSet::new())?;
        let areas = loading.membership(loading.bits[index]);
        let instanced = level
            .instances
            .iter()
            .filter(|instance| instance.bsp == index)
            .map(|instance| collision.placed(instance))
            .collect::<Result<Vec<_>>>()?;
        for (polygon, seam) in collision.structure.into_iter().zip(collision.seams) {
            let open = seam.map_or(0, |seam| {
                open_seams
                    .iter()
                    .enumerate()
                    .filter(|(_, open)| open.contains(&seam))
                    .fold(0u64, |mask, (area, _)| mask | 1 << area)
            });
            if areas & open != 0 {
                open_seam_surfaces += 1;
            }
            if areas & !open != 0 {
                structure += 1;
                polygons.push(polygon);
                polygon_areas.push(areas & !open);
            }
        }
        for polygon in instanced.into_iter().flatten() {
            polygons.push(polygon);
            polygon_areas.push(areas);
        }
    }
    Ok(Polygons {
        polygons,
        areas: polygon_areas,
        structure,
        open_seams,
        open_seam_surfaces,
    })
}

/// The collision triangles in the map's space, moved by `offset`, with the areas whose collision
/// holds each: every polygon as a fan of triangles, then the placed objects' faces `placed`, whose
/// structure BSPs are `placed_bsps`.
pub(super) fn faces(
    polygons: &Polygons,
    placed: &[Face],
    placed_bsps: &[u32],
    loading: &Loading,
    offset: [f32; 3],
) -> (Vec<Face>, Vec<u64>) {
    let mut faces = Vec::<Face>::new();
    let mut face_areas = Vec::new();
    for (polygon, &areas) in polygons.polygons.iter().zip(&polygons.areas) {
        let points = polygon
            .points
            .iter()
            .map(|p| std::array::from_fn(|k| p[k] + offset[k]))
            .collect::<Vec<[f32; 3]>>();
        for i in 1..points.len().saturating_sub(1) {
            faces.push([points[0], points[i], points[i + 1]]);
            face_areas.push(areas);
        }
    }
    for (face, &bsps) in placed.iter().zip(placed_bsps) {
        faces.push(face.map(|point| std::array::from_fn(|k| point[k] + offset[k])));
        face_areas.push(loading.membership(bsps));
    }
    (faces, face_areas)
}

/// The world collision's meshes before they are written, in the map's space.
pub(super) struct Shapes {
    pub faces: Vec<Vec<Face>>,
    /// The areas whose collision holds each of `faces`.
    pub areas: Vec<u64>,
}

/// Groups `faces` by the areas whose collision holds them, `face_areas`, leaving out those no area
/// holds, then splits each group's faces inside and outside `play` at the median until each part
/// fits one mesh.
pub(super) fn shapes(faces: Vec<Face>, face_areas: Vec<u64>, play: &PlayArea) -> Shapes {
    let mut shapes = Vec::new();
    let mut shape_areas = Vec::new();
    let mut by_areas = BTreeMap::<u64, Vec<Face>>::new();
    for (face, areas) in faces.into_iter().zip(face_areas) {
        if areas != 0 {
            by_areas.entry(areas).or_default().push(face);
        }
    }
    for (areas, faces) in by_areas {
        let (inside, scenery): (Vec<_>, Vec<_>) = faces
            .into_iter()
            .partition(|f| play.contains(face_center(f)));
        for group in [inside, scenery] {
            split(
                group,
                &|group| spatial_coordinates(group, face_center),
                &|group: &[Face]| group.len() <= MAX_COLLISION_TRIANGLES,
                &mut shapes,
            );
            shape_areas.resize(shapes.len(), areas);
        }
    }
    Shapes {
        faces: shapes,
        areas: shape_areas,
    }
}

fn face_center(f: &Face) -> [f32; 3] {
    std::array::from_fn(|k| (f[0][k] + f[1][k] + f[2][k]) / 3.)
}

/// The world collision's meshes in the graph.
pub(super) struct Meshes {
    /// The first area's compound symbol and every mesh packfile, in shape order.
    pub world: World,
    /// Each mesh placed at its own center, its user data its index in `world.meshes`.
    pub instances: Vec<Instance>,
    /// The areas whose collision holds each of `instances` ([`Shapes::areas`]).
    pub areas: Vec<u64>,
    /// The meshes' triangles.
    pub triangles: usize,
    pub seconds: f64,
}

/// Writes each of `shapes` as a bare compressed mesh packfile copying the donor's, about the
/// middle of its bounds. Meshes encode `workers` at a time.
pub(super) fn write(
    graph: &mut Graph,
    shapes: Shapes,
    donor: &Donor,
    workers: usize,
    report: &mut dyn FnMut(&str),
) -> Result<Meshes> {
    let started = std::time::Instant::now();
    let mut world = World {
        compound: "world-collision".into(),
        meshes: Vec::new(),
    };
    let mut instances = Vec::new();
    let mut triangles = 0usize;
    for (batch, groups) in shapes.faces.chunks(workers).enumerate() {
        report(&format!(
            "Preparing Collision / Mesh {}/{}",
            batch * workers + 1,
            shapes.faces.len()
        ));
        let prepared = crate::cancellation::parallel(groups, |group| {
            let (low, high) = bounds(group.iter().flatten().copied());
            let center: [f32; 3] = std::array::from_fn(|k| (low[k] + high[k]) / 2.);
            let faces = group
                .iter()
                .map(|face| face.map(|point| sub(point, center)))
                .collect::<Vec<Face>>();
            let (bytes, _) = bare_mesh(&donor.world_mesh, &faces)?;
            Ok((bytes, low, high, center, faces))
        })?;
        for (local, (bytes, low, high, center, faces)) in prepared.into_iter().enumerate() {
            let part = batch * workers + local;
            report(&format!(
                "Writing Collision / Mesh {}/{}",
                part + 1,
                shapes.faces.len()
            ));
            triangles += faces.len();
            let symbol = format!("collision{part}");
            graph.add(&symbol, donor.world_mesh_tag, &bytes, None, Vec::new())?;
            instances.push(Instance {
                translation: center,
                bounds: (low, high),
                user_data: PACKFILE_SHAPE | u64::try_from(part)?,
                triangles: faces.into(),
            });
            world.meshes.push(symbol);
        }
    }
    Ok(Meshes {
        world,
        instances,
        areas: shapes.areas,
        triangles,
        seconds: started.elapsed().as_secs_f64(),
    })
}
