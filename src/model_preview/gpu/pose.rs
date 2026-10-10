//! Reuse sampled and uploaded deformation while a preview is paused.
use super::*;
use crate::model_preview::animation::Deformed;

#[derive(Default)]
#[cfg(test)]
pub(super) struct Cache(Option<Sample>);

#[cfg(test)]
struct Sample {
    model: Arc<Model>,
    seconds: u32,
    pose: Option<Arc<Deformed>>,
}

#[cfg(test)]
impl Cache {
    pub fn sample(&mut self, model: &Arc<Model>, seconds: f32) -> Option<Arc<Deformed>> {
        if let Some(sample) = &self.0
            && Arc::ptr_eq(&sample.model, model)
            && sample.seconds == seconds.to_bits()
        {
            return sample.pose.clone();
        }
        let pose = model.pose(seconds).map(Arc::new);
        self.0 = Some(Sample {
            model: model.clone(),
            seconds: seconds.to_bits(),
            pose: pose.clone(),
        });
        pose
    }
}

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
