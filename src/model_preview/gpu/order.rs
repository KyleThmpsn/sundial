//! GPU-derived transparent depths, stable CPU ranking and persistent indexed submission.
use super::*;

pub(super) struct Cache {
    opaque: Vec<Group>,
    depth_groups: Vec<Group>,
    sources: Vec<Source>,
    vertex_animated: bool,
    initialized: bool,
    pose: Option<Arc<crate::model_preview::animation::Deformed>>,
    seconds: Option<u32>,
    animate: bool,
    particle_study: bool,
    view: Option<[u32; 2]>,
    ranked: Vec<(f32, usize)>,
    indices: Vec<u32>,
    groups: Vec<Group>,
    buffer: Option<glow::Buffer>,
    depths: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Source {
    key: Key,
    index: usize,
    first: u32,
}

impl Cache {
    pub fn new(
        model: &Model,
        groups: &[Group],
        order: &[u32],
        _bind_center: [f32; 3],
        hide_emitter: bool,
    ) -> Self {
        let mut opaque = Vec::new();
        let mut depth_groups = Vec::new();
        let mut sources = Vec::new();
        let mut vertex_animated = false;
        for group in groups {
            if hide_emitter && group.key.emitter {
                continue;
            }
            let Some(effect) = group.key.effect.filter(|&i| !model.effects[i].opaque()) else {
                opaque.push(group.clone());
                continue;
            };
            vertex_animated |= model.effects[effect]
                .native
                .as_ref()
                .is_some_and(crate::model_preview::effects::native::Native::animated);
            depth_groups.push(group.clone());
            for first in (group.first..group.first + group.count).step_by(3) {
                sources.push(Source {
                    key: group.key,
                    index: order[first as usize / 3] as usize,
                    first: first as u32,
                });
            }
        }
        let depths = vec![0xFF; sources.len() * 12];
        Self {
            opaque,
            depth_groups,
            sources,
            vertex_animated,
            initialized: false,
            pose: None,
            seconds: None,
            animate: false,
            particle_study: false,
            view: None,
            ranked: Vec::new(),
            indices: Vec::new(),
            groups: Vec::new(),
            buffer: None,
            depths,
        }
    }

    fn clock(&self, frame: &Frame) -> Option<u32> {
        let model = &frame.model;
        (self.vertex_animated
            || frame.animate
                && frame.pose.is_none()
                && (model.has_animation()
                    || model.has_cloth()
                    || model
                        .motions
                        .iter()
                        .any(crate::model_preview::effects::Motion::animated)))
        .then_some(frame.seconds.to_bits())
    }

    fn needs_update(&self, frame: &Frame) -> bool {
        frame.style == Style::Textured
            && !self.depth_groups.is_empty()
            && (!self.initialized
                || !pose::same(&self.pose, &frame.pose)
                || self.seconds != self.clock(frame)
                || self.animate != frame.animate
                || self.particle_study != frame.scene.particle_study
                || self.view != Some([frame.camera.yaw.to_bits(), frame.camera.pitch.to_bits()]))
    }

    pub fn groups<'a>(&'a self, style: Style, original: &'a [Group]) -> &'a [Group] {
        if style == Style::Textured && !self.depth_groups.is_empty() {
            &self.groups
        } else {
            original
        }
    }

    fn sort(&mut self, frame: &Frame) {
        self.ranked.clear();
        for (index, bytes) in self.depths.chunks_exact(12).enumerate() {
            let depth = bytes
                .chunks_exact(4)
                .map(|v| f32::from_ne_bytes(v.try_into().unwrap()))
                .sum::<f32>();
            if depth.is_finite() {
                self.ranked.push((depth, index));
            }
        }
        self.ranked.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| self.sources[a.1].index.cmp(&self.sources[b.1].index))
        });
        self.groups.clone_from(&self.opaque);
        self.indices.clear();
        for &(_, index) in &self.ranked {
            let source = self.sources[index];
            let first = self.indices.len() as i32;
            self.indices.extend(source.first..source.first + 3);
            match self.groups.last_mut() {
                Some(previous) if previous.indexed && previous.key == source.key => {
                    previous.count += 3
                }
                _ => self.groups.push(Group {
                    key: source.key,
                    indexed: true,
                    first,
                    count: 3,
                }),
            }
        }
        self.pose = frame.pose.clone();
        self.seconds = self.clock(frame);
        self.animate = frame.animate;
        self.particle_study = frame.scene.particle_study;
        self.view = Some([frame.camera.yaw.to_bits(), frame.camera.pitch.to_bits()]);
        self.initialized = true;
    }

    pub unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: the upload owner releases this buffer on its original context.
        unsafe {
            if let Some(buffer) = self.buffer {
                gl.delete_buffer(buffer);
            }
        }
    }
}

pub(super) unsafe fn update(
    gl: &glow::Context,
    uniforms: &Uniforms,
    uploaded: &mut Uploaded,
    frame: &Frame,
    target: &Target,
) -> Result<bool, String> {
    if !uploaded.ordering.needs_update(frame) {
        return Ok(false);
    }
    // SAFETY: State binds the main program, model VAO, deformation, camera and target before
    // this pass. Only one float per corner returns to the CPU. Geometry stays on the GPU.
    unsafe {
        let buffer = match uploaded.ordering.buffer {
            Some(buffer) => buffer,
            None => {
                let buffer = gl.create_buffer()?;
                uploaded.ordering.buffer = Some(buffer);
                buffer
            }
        };
        uploaded.ordering.depths.fill(0xFF);
        gl.bind_buffer(glow::TRANSFORM_FEEDBACK_BUFFER, Some(buffer));
        gl.buffer_data_u8_slice(
            glow::TRANSFORM_FEEDBACK_BUFFER,
            &uploaded.ordering.depths,
            glow::DYNAMIC_READ,
        );
        gl.enable(glow::RASTERIZER_DISCARD);
        draw::groups(
            gl,
            uniforms,
            uploaded,
            frame,
            target,
            &uploaded.ordering.depth_groups,
            draw::Pass::Depth(buffer),
        );
        gl.disable(glow::RASTERIZER_DISCARD);
        gl.bind_buffer_base(glow::TRANSFORM_FEEDBACK_BUFFER, 0, None);
        gl.bind_buffer(glow::COPY_READ_BUFFER, Some(buffer));
        gl.get_buffer_sub_data(glow::COPY_READ_BUFFER, 0, &mut uploaded.ordering.depths);
        gl.bind_buffer(glow::COPY_READ_BUFFER, None);
        if gl.get_error() != glow::NO_ERROR {
            return Err("The GPU could not read transparent depths.".into());
        }
        uploaded.ordering.sort(frame);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(uploaded.indices));
        let indices = &uploaded.ordering.indices;
        let bytes = std::slice::from_raw_parts(
            indices.as_ptr().cast::<u8>(),
            std::mem::size_of_val(indices.as_slice()),
        );
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
    }
    Ok(true)
}
