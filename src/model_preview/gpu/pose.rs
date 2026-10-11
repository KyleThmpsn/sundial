//! Reuse sampled and uploaded deformation while a preview is paused.
use super::*;
use crate::model_preview::animation::Deformed;

pub(super) fn same(left: &Option<Arc<Deformed>>, right: &Option<Arc<Deformed>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

pub(super) unsafe fn upload(gl: &glow::Context, uploaded: &mut Uploaded, frame: &Frame) -> bool {
    if same(&uploaded.pose, &frame.pose) {
        return false;
    }
    let positions = frame
        .pose
        .as_ref()
        .map_or(frame.model.vertices.as_slice(), |pose| {
            pose.positions.as_slice()
        });
    let normals = frame
        .pose
        .as_ref()
        .map_or(frame.model.normals.as_slice(), |pose| {
            pose.normals.as_slice()
        });
    let tangents = frame
        .pose
        .as_ref()
        .map_or(frame.model.tangents.as_slice(), |pose| {
            pose.tangents.as_slice()
        });
    let expanded = expand_positions(&uploaded.order, &frame.model, positions);
    let attributes = expand_attributes(&uploaded.order, &frame.model, positions, normals, tangents);
    // SAFETY: the caller supplies the owning GL context. The model and expansion order are
    // unchanged, so both slices fit the buffers allocated during preparation.
    unsafe {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.positions));
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(expanded.as_flattened()));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(uploaded.attributes));
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes_of(&attributes));
    }
    uploaded.pose = frame.pose.clone();
    true
}
