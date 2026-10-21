//! Convert cooked convex leaves without approximating their surface or radius.
use super::*;

fn shapes(
    source: &Value,
    shape: &Value,
    path: &mut Vec<(i64, usize)>,
    out: &mut Vec<(i64, usize)>,
) -> Result<()> {
    let kind = shape[0].as_i64().context("Source collision shape kind")?;
    let index = usize::try_from(shape[1].as_u64().context("Source collision shape index")?)?;
    match kind {
        1 | 2 | 4 => out.push((kind, index)),
        14 | 15 => {
            ensure!(
                !path.contains(&(kind, index)) && path.len() < 16,
                "Cyclic source collision compound"
            );
            path.push((kind, index));
            if kind == 14 {
                let children = source["lists"]
                    .get(index)
                    .and_then(Value::as_array)
                    .context("Source collision list")?;
                for child in children {
                    shapes(source, child, path, out)?;
                }
            } else {
                let mopp = source["mopps"]
                    .get(index)
                    .context("Source collision MOPP")?;
                let info = vector::<4>(&mopp["code_info"])?;
                ensure!(
                    mopp["type"] == 10
                        && mopp["shape"][0] == 14
                        && mopp["code_size"].as_i64().is_some_and(|size| size > 0)
                        && info[3] > 0.
                        && mopp["tail"] == json!([0, 0, 0, 0]),
                    "Source collision MOPP is not a supported unscaled list wrapper"
                );
                // The MOPP stores query acceleration over this collection, not
                // another surface. Its code-info quantization is not a geometry
                // transform. The native dynamic body builds its own acceleration
                // from the same leaves and keeps their source mass and frames.
                shapes(source, &mopp["shape"], path, out)?;
            }
            path.pop();
        }
        _ => bail!("Source collision shape {kind} needs an explicit native translation"),
    }
    Ok(())
}

pub(in super::super) fn build(scene: &Scene, bones: &Value, index: usize) -> Result<Vec<Shape>> {
    let source = &scene.object.gameplay["rigid_body"];
    let row = &source["bodies"][index];
    let node = usize::try_from(row["node"].as_u64().context("Physics node index")?)?;
    let name = source["nodes"][node]
        .as_str()
        .context("Physics node name")?;
    let model = &scene.models[0].model;
    let matches = model
        .bones
        .iter()
        .enumerate()
        .filter(|(_, bone)| bone.name == name)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Physics node is absent or ambiguous in the render rig"
    );
    let worlds = rig::world(&model.bones)?;
    let mut selected = Vec::new();
    shapes(
        &source["collision"],
        &row["shape"],
        &mut Vec::new(),
        &mut selected,
    )?;
    let mut out = Vec::new();
    for (kind, shape) in selected {
        if kind == 1 {
            let capsule = &source["collision"]["capsules"][shape];
            let position = |key: &str| -> Result<[f32; 3]> {
                let point = vector::<3>(&capsule[key])?.map(|v| v * scene.metres());
                let object = rig::point(&worlds[matches[0].0], point, true);
                Ok(super::super::super::seat::frame(bones, 0, (object, [0., 0., 0., 1.]))?.0)
            };
            let radius = capsule["radius_source"]
                .as_f64()
                .context("Source capsule radius")? as f32;
            out.push(Shape::Capsule {
                a: position("a_source")?,
                b: position("b_source")?,
                radius: radius * scene.metres(),
            });
            continue;
        }
        let (points, equations, radius, world) = if kind == 4 {
            let source = &source["collision"]["polyhedra"][shape];
            let points = source["vertices_source"]
                .as_array()
                .context("Source convex vertices")?
                .iter()
                .map(vector::<3>)
                .collect::<Result<Vec<_>>>()?;
            let equations = source["planes_source"]
                .as_array()
                .context("Source convex planes")?
                .iter()
                .map(vector::<4>)
                .collect::<Result<Vec<_>>>()?;
            let radius = source["radius_source"]
                .as_f64()
                .context("Source convex radius")? as f32;
            (points, equations, radius, worlds[matches[0].0])
        } else {
            let source = &source["collision"]["boxes"][shape];
            let half = vector::<3>(&source["half_extents_source"])?;
            let radius = source["radius_source"]
                .as_f64()
                .context("Source box radius")? as f32;
            let mut transform = vector::<16>(&source["transform_source"])?;
            for a in 0..3 {
                for b in 0..3 {
                    let dot = (0..3)
                        .map(|k| transform[a * 4 + k] * transform[b * 4 + k])
                        .sum::<f32>();
                    ensure!(
                        (dot - if a == b { 1. } else { 0. }).abs() < 0.0001,
                        "Source box transform is not orthonormal"
                    );
                }
                transform[12 + a] *= scene.metres();
            }
            transform[15] = 1.;
            let points = (0..8)
                .map(|corner| {
                    std::array::from_fn(|axis| {
                        if corner & (1 << axis) != 0 {
                            half[axis]
                        } else {
                            -half[axis]
                        }
                    })
                })
                .collect();
            let mut equations = Vec::new();
            for axis in 0..3 {
                for sign in [-1., 1.] {
                    let mut plane = [0.; 4];
                    plane[axis] = sign;
                    plane[3] = -half[axis];
                    equations.push(plane);
                }
            }
            (
                points,
                equations,
                radius,
                rig::multiply(&worlds[matches[0].0], &transform),
            )
        };
        let position = |p: [f32; 3]| -> Result<[f32; 3]> {
            let object = rig::point(&world, p.map(|v| v * scene.metres()), true);
            Ok(super::super::super::seat::frame(bones, 0, (object, [0., 0., 0., 1.]))?.0)
        };
        let origin = position([0.; 3])?;
        let root_origin =
            super::super::super::seat::frame(bones, 0, ([0.; 3], [0., 0., 0., 1.]))?.0;
        let vertices = points
            .into_iter()
            .map(position)
            .collect::<Result<Vec<_>>>()?;
        let planes = equations
            .into_iter()
            .map(|plane| {
                let direction = rig::point(&world, plane[..3].try_into().unwrap(), false);
                let mut normal =
                    super::super::super::seat::frame(bones, 0, (direction, [0., 0., 0., 1.]))?.0;
                for axis in 0..3 {
                    normal[axis] -= root_origin[axis];
                }
                let distance = plane[3] * scene.metres()
                    - normal.iter().zip(origin).map(|(a, b)| a * b).sum::<f32>();
                Ok([normal[0], normal[1], normal[2], distance])
            })
            .collect::<Result<Vec<_>>>()?;
        out.push(Shape::Hull(Hull {
            vertices,
            planes,
            radius: radius * scene.metres(),
        }));
    }
    ensure!(
        !out.is_empty(),
        "Selected source body has no collision shapes"
    );
    Ok(out)
}
