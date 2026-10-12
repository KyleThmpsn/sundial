//! Selected source convex hulls in the native root-body frame.
use super::{METRES, assembly::Assembly, body};
use crate::{
    halo_reach::{Scene, rig},
    presentation::{Graph, put},
    tiger::{physics::Hull, reader::Reader},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

fn polyhedra(
    source: &Value,
    shape: &Value,
    path: &mut Vec<usize>,
    out: &mut Vec<usize>,
) -> Result<()> {
    let kind = shape[0].as_i64().context("Source collision shape kind")?;
    let index = usize::try_from(shape[1].as_u64().context("Source collision shape index")?)?;
    match kind {
        4 => {
            out.push(index);
        }
        14 => {
            ensure!(
                !path.contains(&index) && path.len() < 16,
                "Cyclic source collision compound"
            );
            let children = source["lists"]
                .get(index)
                .and_then(Value::as_array)
                .context("Source collision list")?;
            path.push(index);
            for child in children {
                polyhedra(source, child, path, out)?;
            }
            path.pop();
        }
        _ => bail!("Source collision shape {kind} needs an explicit native translation"),
    }
    Ok(())
}

fn vector<const N: usize>(value: &Value) -> Result<[f32; N]> {
    let values = value
        .as_array()
        .filter(|v| v.len() == N)
        .context("Collision vector length")?;
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = value.as_f64().context("Collision vector number")? as f32;
        ensure!(out.is_finite(), "Nonfinite collision vector");
    }
    Ok(result)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
) -> Result<Value> {
    let model = &scene.models[0].model;
    let source = &scene.object.gameplay["rigid_body"];
    let (selected, _) = body::selection(model, source)?;
    let worlds = rig::world(&model.bones)?;
    let mut hulls = Vec::new();
    let mut report = Vec::new();
    for index in selected {
        let body = &source["bodies"][index];
        let node = usize::try_from(body["node"].as_u64().context("Physics node index")?)?;
        let name = source["nodes"][node]
            .as_str()
            .context("Physics node name")?;
        let matches = model
            .bones
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name == name)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Physics node is absent or ambiguous in the render rig"
        );
        let world = &worlds[matches[0].0];
        let mut selected_hulls = Vec::new();
        polyhedra(
            &source["collision"],
            &body["shape"],
            &mut Vec::new(),
            &mut selected_hulls,
        )?;
        for poly in selected_hulls {
            let source_hull = &source["collision"]["polyhedra"][poly];
            let vertices = source_hull["vertices_source"]
                .as_array()
                .context("Source collision vertices")?
                .iter()
                .map(|value| {
                    let source_point = vector::<3>(value)?.map(|v| v * METRES);
                    let object = rig::point(world, source_point, true);
                    Ok(super::super::seat::frame(bones, 0, (object, [0., 0., 0., 1.]))?.0)
                })
                .collect::<Result<Vec<_>>>()?;
            let origin = rig::point(world, [0.; 3], true);
            let local_origin = super::super::seat::frame(bones, 0, (origin, [0., 0., 0., 1.]))?.0;
            let root_origin = super::super::seat::frame(bones, 0, ([0.; 3], [0., 0., 0., 1.]))?.0;
            let planes = source_hull["planes_source"]
                .as_array()
                .context("Source collision planes")?
                .iter()
                .map(|value| {
                    let plane = vector::<4>(value)?;
                    let normal = rig::point(world, plane[..3].try_into().unwrap(), false);
                    let mut normal =
                        super::super::seat::frame(bones, 0, (normal, [0., 0., 0., 1.]))?.0;
                    for axis in 0..3 {
                        normal[axis] -= root_origin[axis];
                    }
                    let distance = plane[3] * METRES
                        - normal
                            .iter()
                            .zip(local_origin)
                            .map(|(a, b)| a * b)
                            .sum::<f32>();
                    Ok([normal[0], normal[1], normal[2], distance])
                })
                .collect::<Result<Vec<_>>>()?;
            let radius = source_hull["radius_source"]
                .as_f64()
                .context("Source convex radius")? as f32
                * METRES;
            report.push(json!({"body":index,"polyhedron":poly,"vertices":vertices.len(),"planes":planes.len(),"radius_metres":radius}));
            hulls.push(Hull {
                vertices,
                planes,
                radius,
            });
        }
    }
    let owner = assembly.owner(0x808092d8)?;
    let payload = assembly.payloads.get_mut(&owner).unwrap();
    let definition = payload.pointer(24)?;
    ensure!(
        payload.u32(definition - 4)? == 0x80808a0c,
        "Collision body definition differs"
    );
    let rows = payload.array(definition + 0x388, 0x90, Some(0x80808c07))?;
    ensure!(!rows.is_empty(), "Native collision body is empty");
    let filters = rows
        .iter()
        .map(|&row| payload.u32(row + 0x18))
        .collect::<Result<Vec<_>>>()?;
    let template = native.tag(0x80b822ba, None)?;
    let bytes = crate::tiger::physics::compound(&template.0, &hulls, &filters)?;
    let previous = payload.u32(definition + 0x380)?;
    native.tag(previous, None)?;
    graph.add("vehicle-collision", previous, &bytes, None, Vec::new())?;
    // Body categories, callbacks and filter policies remain native. Every shape
    // uses the root frame, so donor child bones cannot move the source collision.
    for row in &rows {
        put(&mut payload.0, row + 8, &0u32.to_le_bytes())?;
    }
    // The old cloth hull belongs to the donor silhouette, not the source model.
    put(&mut payload.0, definition + 0x3b8, &[0; 16])?;
    assembly.resource(owner, definition + 0x380, "vehicle-collision".into())?;
    Ok(
        json!({"owner":format!("{owner:08X}"),"source":source["tag"],"hulls":report,
        "native_bodies":rows.len(),"root_bone":0,"gameplay_verified":false,
        "limits":["Selected source convex hulls use a rigid compound. Source damage deformation and articulated collision are not translated. Live impacts and vehicle clearance require gameplay acceptance."]}),
    )
}
