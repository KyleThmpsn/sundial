//! Pre-batching draw-order oracle, retained for independent live OpenGL image comparison.
//! This is the source behavior captured before the indexed submission change.
use super::super::*;
use crate::model_preview::effects::{self, native};
use std::borrow::Cow;

pub(in crate::model_preview::gpu) fn groups<'a>(
    uploaded: &'a Uploaded,
    frame: &Frame,
) -> Cow<'a, [Group]> {
    let model = &frame.model;
    let transparent = |group: &Group| {
        group
            .key
            .effect
            .is_some_and(|index| !model.effects[index].opaque())
    };
    if frame.style != Style::Textured || !uploaded.groups.iter().any(transparent) {
        return Cow::Borrowed(&uploaded.groups);
    }
    let positions = frame
        .pose
        .as_ref()
        .map_or(model.vertices.as_slice(), |p| p.positions.as_slice());
    let normals = frame
        .pose
        .as_ref()
        .map_or(model.normals.as_slice(), |p| p.normals.as_slice());
    let tangents = frame
        .pose
        .as_ref()
        .map_or(model.tangents.as_slice(), |p| p.tangents.as_slice());
    let center = frame.pose.as_ref().map_or(uploaded.framing[0].0, |p| {
        p.framing_center(uploaded.framing[0].0)
    });
    let (sy, cy) = frame.camera.yaw.sin_cos();
    let (sp, cp) = frame.camera.pitch.sin_cos();
    let depth = |point: [f32; 3]| {
        let [x, y, z] = std::array::from_fn::<_, 3, _>(|i| point[i] - center[i]);
        cp * (sy * x + cy * y) - sp * z
    };
    let mut groups: Vec<Group> = Vec::with_capacity(uploaded.groups.len());
    let mut triangles = Vec::new();
    for group in &uploaded.groups {
        if uploaded.hide_emitter && group.key.emitter {
            continue;
        }
        if !transparent(group) {
            groups.push(group.clone());
            continue;
        }
        let native = group
            .key
            .effect
            .and_then(|i| model.effects[i].native.as_ref());
        let constants = native.and_then(|n| n.vertex_frame(frame.seconds));
        if native.is_some() && constants.is_none() {
            continue;
        }
        for first in (group.first..group.first + group.count).step_by(3) {
            let index = uploaded.order[first as usize / 3] as usize;
            let corners = model.triangles[index];
            let mut points =
                corners.map(|v| positions.get(v as usize).copied().unwrap_or_default());
            if let Some((native, constants)) = native.zip(constants.as_ref()) {
                let Some(transformed) =
                    displaced(native, constants, model, corners, points, normals, tangents)
                else {
                    continue;
                };
                points = transformed;
            }
            let distance = points.map(depth).into_iter().sum::<f32>();
            if distance.is_finite() {
                triangles.push((
                    distance,
                    index,
                    Group {
                        first,
                        count: 3,
                        indexed: false,
                        key: group.key,
                    },
                ));
            }
        }
    }
    // Material batching changed the stored order. Restore it explicitly for equal depths.
    triangles.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, _, triangle) in triangles {
        match groups.last_mut() {
            Some(previous)
                if previous.key == triangle.key
                    && previous.first + previous.count == triangle.first =>
            {
                previous.count += triangle.count;
            }
            _ => groups.push(triangle),
        }
    }
    Cow::Owned(groups)
}

fn displaced(
    native: &native::Native,
    constants: &effects::Frame,
    model: &Model,
    corners: [u32; 3],
    mut points: [[f32; 3]; 3],
    normals: &[[f32; 3]],
    tangents: &[[f32; 4]],
) -> Option<[[f32; 3]; 3]> {
    let ab: [f32; 3] = std::array::from_fn(|i| points[1][i] - points[0][i]);
    let ac: [f32; 3] = std::array::from_fn(|i| points[2][i] - points[0][i]);
    let flat = shader::normal::normalize([
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ])
    .unwrap_or([0.0, 0.0, 1.0]);
    for (point, vertex) in points.iter_mut().zip(corners) {
        let vertex = vertex as usize;
        let normal = normals
            .get(vertex)
            .copied()
            .and_then(shader::normal::normalize)
            .unwrap_or(flat);
        let uv = model.uvs.get(vertex).copied().unwrap_or_default();
        let input = native::Input {
            position: *point,
            normal,
            tangent: native::tangent(tangents, vertex, normal),
            color: model.colors.get(vertex).copied().unwrap_or([1.0; 4]),
            uv,
            detail_uv: model.detail_uvs.get(vertex).copied().unwrap_or(uv),
        };
        let value = native.vertex(model, &input, constants)?;
        *point = [value[4][0], value[4][1], value[4][2]];
    }
    Some(points)
}
