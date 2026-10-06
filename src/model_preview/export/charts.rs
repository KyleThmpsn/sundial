//! Preserve posed native geometry and normal bases while assigning independent UV charts.
use super::*;

pub(super) fn primary(model: &Model, layer: &bake::Layer) -> Vec<[[f32; 2]; 3]> {
    layer
        .triangles
        .iter()
        .map(|&t| {
            model.triangles[t].map(|v| model.uvs.get(v as usize).copied().unwrap_or_default())
        })
        .collect()
}

pub(super) fn frames(
    model: &Model,
    pose: Option<&crate::model_preview::animation::Deformed>,
    triangle: usize,
) -> Result<[shader::normal::Basis; 3], String> {
    use shader::normal::{Basis, normalize};
    let indices = model.triangles[triangle];
    let normals = pose.map_or(model.normals.as_slice(), |p| p.normals.as_slice());
    let tangents = pose.map_or(model.tangents.as_slice(), |p| p.tangents.as_slice());
    let stored =
        indices.map(|v| Basis::stored(*normals.get(v as usize)?, *tangents.get(v as usize)?));
    if let [Some(a), Some(b), Some(c)] = stored {
        return Ok([a, b, c]);
    }
    let positions = pose.map_or(model.vertices.as_slice(), |p| p.positions.as_slice());
    let points = indices.map(|v| positions[v as usize]);
    let uv = indices.map(|v| model.uvs.get(v as usize).copied().unwrap_or_default());
    let basis = Basis::triangle(points, uv).ok_or_else(|| {
        format!("Triangle {triangle} has no usable tangent frame for normal-map export")
    })?;
    Ok(indices.map(|v| {
        basis.with_normal(
            normals
                .get(v as usize)
                .copied()
                .and_then(normalize)
                .unwrap_or(basis.normal),
        )
    }))
}

pub(super) fn vertices(
    buffer: &mut Buffer,
    model: &Model,
    pose: Option<&crate::model_preview::animation::Deformed>,
    layer: &bake::Layer,
    coordinates: &[[[f32; 2]; 3]],
) -> Result<(Value, Vec<u32>, bool), String> {
    if coordinates.len() != layer.triangles.len() {
        return Err("The repacked UV chart does not match its geometry".into());
    }
    let indices: Vec<_> = layer
        .triangles
        .iter()
        .flat_map(|&t| model.triangles[t])
        .collect();
    let native_positions = pose.map_or(model.vertices.as_slice(), |p| p.positions.as_slice());
    let native_normals = pose.map_or(model.normals.as_slice(), |p| p.normals.as_slice());
    let native_tangents = pose.map_or(model.tangents.as_slice(), |p| p.tangents.as_slice());
    let points: Vec<_> = indices
        .iter()
        .map(|&i| upright(native_positions[i as usize]))
        .collect();
    let mut normals: Vec<_> = indices
        .iter()
        .map(|&i| {
            native_normals
                .get(i as usize)
                .copied()
                .unwrap_or([0.0, 0.0, 1.0])
        })
        .collect();
    let mapped_frames = layer
        .normal
        .as_ref()
        .map(|_| {
            layer
                .triangles
                .iter()
                .map(|&triangle| frames(model, pose, triangle))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    if let Some(frames) = &mapped_frames {
        normals = frames.iter().flatten().map(|basis| basis.normal).collect();
    }
    let position = positions(buffer, &points);
    let normal = super::normals(buffer, points.len(), &normals);
    let uv: Vec<_> = coordinates
        .iter()
        .flatten()
        .flatten()
        .copied()
        .flat_map(f32::to_le_bytes)
        .collect();
    let view = buffer.view(&uv, Some(ARRAY_BUFFER));
    let texcoord = buffer.accessor(view, FLOAT, "VEC2", points.len());
    let tangent_values = mapped_frames.map_or_else(
        || {
            indices
                .iter()
                .zip(&normals)
                .flat_map(|(&i, &n)| {
                    let tangent = crate::model_preview::effects::native::tangent(
                        native_tangents,
                        i as usize,
                        n,
                    );
                    let direction = upright([tangent[0], tangent[1], tangent[2]]);
                    // The baked normal map mirrors green. Mirror the bitangent as well so its
                    // contribution to the native surface normal remains unchanged.
                    [direction[0], direction[1], direction[2], -tangent[3]]
                })
                .collect::<Vec<_>>()
        },
        |frames| {
            frames
                .into_iter()
                .flatten()
                .flat_map(|basis| {
                    let tangent = basis.tangent();
                    let direction = upright([tangent[0], tangent[1], tangent[2]]);
                    [direction[0], direction[1], direction[2], -tangent[3]]
                })
                .collect()
        },
    );
    let tangents: Vec<_> = tangent_values
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let view = buffer.view(&tangents, Some(ARRAY_BUFFER));
    let tangent = buffer.accessor(view, FLOAT, "VEC4", points.len());
    let mut attributes = super::attributes(position, normal, Some(texcoord));
    attributes["TANGENT"] = json!(tangent);
    let count = u32::try_from(points.len())
        .map_err(|_| "Repacked geometry exceeds the export index range")?;
    Ok((
        attributes,
        (0..count).collect(),
        points.len() <= usize::from(u16::MAX),
    ))
}
