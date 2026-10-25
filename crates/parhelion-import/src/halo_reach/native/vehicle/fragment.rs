//! Detached source objects in a private rigid native assembly.
use super::{body, collision, damage, secondary::Bundle};
use crate::{
    halo_reach::{
        self, ExportRequest, Scene, View,
        cache::{Cache, Tag},
        resource::Pages,
        rig,
    },
    presentation::{Graph, put},
    tiger::{draws, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ENTITY: u32 = 0x80c0d9bf;

fn owner(bundle: &Bundle, class: u32) -> Result<u32> {
    let found = bundle
        .owners
        .iter()
        .filter_map(|(&tag, payload)| {
            let instance = payload.pointer(16).ok()?;
            (payload.u32(instance - 4).ok() == Some(class)).then_some(tag)
        })
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Fragment component {class:08X} is absent or ambiguous"
    );
    Ok(found[0])
}

fn hit_geometry(scene: &Scene) -> Result<Vec<[[f32; 3]; 3]>> {
    let source = &scene.object.gameplay["rigid_body"]["shot_collision"];
    let regions = source["regions"]
        .as_array()
        .context("Fragment hit regions")?;
    let model = &scene.models[0].model;
    let worlds = rig::world(&model.bones)?;
    let mut triangles = Vec::new();
    for region in regions {
        let permutations = region["permutations"]
            .as_array()
            .context("Fragment hit permutations")?;
        ensure!(
            permutations.len() == 1,
            "Fragment hit geometry needs a permutation selector"
        );
        for bsp in permutations[0]["bsps"]
            .as_array()
            .context("Fragment hit BSPs")?
        {
            let index = usize::try_from(bsp["node"].as_u64().context("Fragment hit node")?)?;
            let name = source["nodes"][index]
                .as_str()
                .context("Fragment hit node name")?;
            let matches = model
                .bones
                .iter()
                .enumerate()
                .filter(|(_, bone)| bone.name == name)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Fragment hit node is absent or ambiguous in its model"
            );
            for surface in bsp["surfaces"]
                .as_array()
                .context("Fragment hit surfaces")?
            {
                let mut points = Vec::new();
                for point in surface["points_source"]
                    .as_array()
                    .context("Fragment hit points")?
                {
                    let values = point
                        .as_array()
                        .filter(|values| values.len() == 3)
                        .context("Fragment point length")?;
                    let mut position = [0.; 3];
                    for (target, value) in position.iter_mut().zip(values) {
                        *target = value.as_f64().context("Fragment hit coordinate")? as f32
                            * scene.metres();
                        ensure!(target.is_finite(), "Nonfinite fragment hit coordinate");
                    }
                    points.push(rig::point(&worlds[matches[0].0], position, true));
                }
                ensure!(points.len() >= 3, "Fragment hit polygon is empty");
                for index in 1..points.len() - 1 {
                    triangles.push([points[0], points[index], points[index + 1]]);
                }
            }
        }
    }
    ensure!(!triangles.is_empty(), "Fragment has no hit geometry");
    Ok(triangles)
}

fn presentation(bundle: &mut Bundle, graph: &Graph, owner: u32) -> Result<()> {
    let model = graph.read("model")?;
    let capacity = usize::try_from(model.u32(draws::DRAW_INDEX_CAPACITY)?)?;
    ensure!(
        capacity > 0 && capacity.is_multiple_of(32),
        "Fragment draw capacity differs"
    );
    let words = capacity / 32;
    let payload = bundle
        .owners
        .get_mut(&owner)
        .context("Fragment render owner")?;
    let (instance, definition) = damage::definition(payload, 0x808072b8, 0x808072bd)?;
    damage::region::parts(payload, definition, &[])?;
    damage::array(payload, instance + 0x170, 0x8080000b, 4, words)?;
    damage::allocation(
        &mut bundle
            .allocations
            .get_mut(&owner)
            .context("Fragment render allocation")?
            .1,
        &[0x7da384e0],
        0x8080000b,
        words,
    )?;
    bundle
        .resources
        .entry(owner)
        .or_default()
        .insert(definition + 0x1dc, "model".into());
    bundle.seal(owner)
}

fn physics(
    native: &mut Reader,
    bundle: &mut Bundle,
    graph: &mut Graph,
    scene: &Scene,
    frame: &[f32; 16],
) -> Result<Value> {
    let source = &scene.object.gameplay["rigid_body"];
    let bodies = source["bodies"]
        .as_array()
        .context("Fragment source bodies")?;
    ensure!(
        bodies.len() == 1,
        "Articulated fragments need a native constraint translation"
    );
    let bones = json!({"bones":[{"inverse_object_transform":[0.,0.,0.,1.,0.,0.,0.,1.]}]});
    let mut hulls = collision::hulls(scene, &bones, 0)?;
    for hull in &mut hulls {
        match hull {
            crate::tiger::physics::Shape::Hull(hull) => {
                for vertex in &mut hull.vertices {
                    *vertex = rig::point(frame, *vertex, false);
                }
                for plane in &mut hull.planes {
                    let normal = rig::point(frame, plane[..3].try_into()?, false);
                    plane[..3].copy_from_slice(&normal);
                }
            }
            crate::tiger::physics::Shape::Capsule { a, b, .. } => {
                *a = rig::point(frame, *a, false);
                *b = rig::point(frame, *b, false);
            }
        }
    }
    let mut triangles = hit_geometry(scene)?;
    for point in triangles.iter_mut().flatten() {
        *point = rig::point(frame, *point, false);
    }
    let mut properties = body::properties(scene, 0)?;
    properties.rotate(frame);
    let owner = owner(bundle, 0x808092d8)?;
    let payload = bundle.owners.get_mut(&owner).unwrap();
    let (_, definition) = damage::definition(payload, 0x808092d8, 0x80808a0c)?;
    let rows = payload.array(definition + 0x388, 0x90, Some(0x80808c07))?;
    ensure!(rows.len() == 2, "Fragment physical envelope differs");
    for (index, &row) in rows.iter().enumerate() {
        let solid = index == 0;
        ensure!(
            (payload.u32(row + 0x18)? >> 8) & 0xff == (if solid { 1 } else { 2 })
                && payload.u32(row + 0x80)? == (if solid { 1 } else { 4 })
                && payload.u32(row + 8)? == 0,
            "Fragment body motion or filter differs"
        );
        put(&mut payload.0, row + 4, &0x811c9dc5u32.to_le_bytes())?;
        put(&mut payload.0, row + 12, &0u32.to_le_bytes())?;
        put(
            &mut payload.0,
            row + 0x88,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        if solid {
            body::write(payload, row, &bones, &properties)?;
        }
    }
    damage::region::parts(payload, definition, &[])?;
    put(&mut payload.0, definition + 0x3b8, &[0; 16])?;
    let previous = payload.u32(definition + 0x380)?;
    let template = native.tag(0x80b822ba, None)?;
    let bytes = crate::tiger::physics::vehicle_shapes(
        &template.0,
        &hulls,
        &[(0..hulls.len()).collect()],
        std::slice::from_ref(&triangles),
    )?;
    graph.add("collision", previous, &bytes, None, Vec::new())?;
    bundle
        .resources
        .entry(owner)
        .or_default()
        .insert(definition + 0x380, "collision".into());
    bundle.seal(owner)?;
    Ok(
        json!({"source":source["tag"],"mass":properties.mass,"center":properties.center,
        "inertia":properties.inertia,"hulls":hulls.len(),"hit_triangles":triangles.len(),
        "limits":["Native rigid-body integration, friction and damping remain. Live impacts and debris retirement require gameplay acceptance."]}),
    )
}

fn remove_visuals(bundle: &mut Bundle) -> Result<Vec<u32>> {
    let mut removed = Vec::new();
    for (&owner, payload) in &bundle.owners {
        let definition = payload.pointer(24)?;
        if payload.u32(definition - 4)? != 0x808084e9 {
            continue;
        }
        let events = payload.array(definition + 0x168, 24, Some(0x808093e6))?;
        let classes = events
            .iter()
            .map(|&row| payload.u32(payload.pointer(row + 16)? - 4))
            .collect::<Result<Vec<_>>>()?;
        if !classes.is_empty() && classes.iter().all(|&class| class == 0x80806cc6) {
            removed.push(owner);
        } else {
            // Preserve the stock lifecycle sequence independently of visuals.
            // The exact live retirement behavior still needs acceptance.
            ensure!(
                classes == [0x808093cb, 0x80806b98, 0x80804ab0],
                "Fragment flow contains an unqualified retained event"
            );
        }
    }
    ensure!(!removed.is_empty(), "Fragment visual flow is absent");
    for &owner in &removed {
        bundle.remove(owner)?;
    }
    Ok(removed)
}

pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    source: &Tag,
    scale: f32,
    orientation: [f32; 4],
) -> Result<(String, Value)> {
    ensure!(
        source.group == "bloc",
        "Detached source object is not a rigid block"
    );
    let orientation = rig::quaternion(orientation)?;
    let frame = rig::transform([0.; 3], orientation);
    let prefix = format!(
        "fragment-{:08X}-{}",
        source.datum,
        orientation
            .into_iter()
            .map(|value| format!("{:08X}", value.to_bits()))
            .collect::<String>()
    );
    let mut local = Graph {
        root: graph.root.join(&prefix),
        nodes: Vec::new(),
    };
    std::fs::create_dir_all(&local.root)?;
    let request = ExportRequest {
        cache: String::new(),
        group: source.group.clone(),
        path: source.path.clone(),
        view: View::World,
        variant: None,
        permutations: BTreeMap::new(),
        audio: None,
    };
    let scene = super::super::scale::scene(halo_reach::scene(cache, pages, &request)?, scale)?;
    let mut bundle = Bundle::read(native, ENTITY)?;
    ensure!(
        bundle.entity().u16(0x96)? == 6,
        "Fragment template object kind differs"
    );
    let removed = remove_visuals(&mut bundle)?;
    let (render, visual) =
        super::super::fragment::build(cache, pages, native, &scene, &mut local, ENTITY, &frame)?;
    presentation(&mut bundle, &local, render)?;
    let physical = physics(native, &mut bundle, &mut local, &scene, &frame)?;
    let entity = bundle.emit(&mut local, "entity")?;
    graph.extend(local, &prefix)?;
    Ok((
        format!("{prefix}-{entity}"),
        json!({"source":source,"frame_rotation":orientation,"presentation":visual,"physics":physical,
        "removed_visual_flows":removed,"attachments":scene.object.attachments,"gameplay_verified":false,
        "cleanup":"Native rigid fragment lifecycle"}),
    ))
}
