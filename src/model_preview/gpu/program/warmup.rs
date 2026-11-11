//! Exercise the driver's deferred first draw in the compiler process before exporting a binary.
use super::super::{Target, Uniforms, draw, upload};
use eframe::glow::{self, HasContext};

struct Scratch<'a> {
    gl: &'a glow::Context,
    target: Option<Target>,
    vao: Option<glow::VertexArray>,
    buffers: Vec<glow::Buffer>,
    feedback: Option<glow::Buffer>,
}

impl<'a> Scratch<'a> {
    unsafe fn new(gl: &'a glow::Context) -> Result<Self, String> {
        let mut scratch = Self {
            gl,
            target: None,
            vao: None,
            buffers: Vec::new(),
            feedback: None,
        };
        // SAFETY: the helper owns this current context. Scratch releases partial allocations.
        unsafe {
            scratch.target = Some(
                Target::new(gl, [1, 1])
                    .ok_or("The shader helper could not prepare its framebuffer.")?,
            );
            scratch.vao = Some(gl.create_vertex_array()?);
            gl.bind_vertex_array(scratch.vao);
            let positions = [-0.5_f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.0, 0.5, 0.0];
            let attributes = [
                0.0_f32, 0.0, 1.0, // normal
                0.0, 0.0, 0.0, 0.0, // UVs
                1.0, 0.0, 0.0, 1.0, // tangent
                1.0, 1.0, 1.0, 1.0, // color
                1.0, 0.0, // basis
            ]
            .repeat(3);
            let indices = [0_u32, 1, 2, 1, 2, 0, 2, 0, 1];
            for (pointer, length) in [
                (
                    positions.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(&positions),
                ),
                (
                    attributes.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(attributes.as_slice()),
                ),
                (
                    indices.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(&indices),
                ),
            ] {
                let buffer = gl.create_buffer()?;
                scratch.buffers.push(buffer);
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(pointer, length),
                    glow::STATIC_DRAW,
                );
            }
            upload::layout(
                gl,
                scratch
                    .buffers
                    .as_slice()
                    .try_into()
                    .expect("three vertex buffers"),
            );
            // The first three source indices also form this dummy triangle's element list.
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(scratch.buffers[2]));
            let feedback = gl.create_buffer()?;
            scratch.feedback = Some(feedback);
            gl.bind_buffer(glow::TRANSFORM_FEEDBACK_BUFFER, Some(feedback));
            gl.buffer_data_size(
                glow::TRANSFORM_FEEDBACK_BUFFER,
                (3 * std::mem::size_of::<f32>()) as i32,
                glow::DYNAMIC_READ,
            );
        }
        Ok(scratch)
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        // SAFETY: this scope ends before the helper releases its sole current context.
        unsafe {
            self.gl.use_program(None);
            self.gl.bind_vertex_array(None);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl
                .bind_buffer_base(glow::TRANSFORM_FEEDBACK_BUFFER, 0, None);
            if let Some(buffer) = self.feedback.take() {
                self.gl.delete_buffer(buffer);
            }
            for buffer in self.buffers.drain(..) {
                self.gl.delete_buffer(buffer);
            }
            if let Some(vao) = self.vao.take() {
                self.gl.delete_vertex_array(vao);
            }
            if let Some(target) = self.target.take() {
                target.delete(self.gl);
            }
        }
    }
}

pub(super) unsafe fn draw(gl: &glow::Context, program: glow::Program) -> Result<(), String> {
    // SAFETY: binary_now calls this only on the compiler helper's current context.
    unsafe {
        let scratch = Scratch::new(gl)?;
        // Target allocation may have tried an unsupported MSAA count before its fallback.
        for _ in 0..8 {
            if gl.get_error() == glow::NO_ERROR {
                break;
            }
        }
        let target = scratch.target.as_ref().expect("prepared framebuffer");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
        gl.viewport(0, 0, 1, 1);
        draw::begin(gl);
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.clear_buffer_f32_slice(glow::COLOR, 1, &[0.0; 4]);
        gl.use_program(Some(program));
        let uniforms = Uniforms::locate(gl, program);
        uniforms.bind_samplers(gl);
        gl.uniform_matrix_3_f32_slice(
            uniforms.rotate.as_ref(),
            false,
            &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        );
        gl.uniform_2_f32(uniforms.scale.as_ref(), 1.0, 1.0);
        gl.uniform_1_f32(uniforms.depth_scale.as_ref(), 1.0);
        for native in [-1, 0] {
            gl.uniform_1_i32(uniforms.native_index.as_ref(), native);
            for quaternion in [0, 1] {
                gl.uniform_1_i32(uniforms.native_quaternion.as_ref(), quaternion);
                gl.enable(glow::RASTERIZER_DISCARD);
                draw::feedback(gl, scratch.feedback.expect("feedback buffer"), 0, 3, 0);
                gl.disable(glow::RASTERIZER_DISCARD);
                for (opaque, modulates) in [(true, false), (false, false), (false, true)] {
                    gl.uniform_1_i32(
                        uniforms.effect.as_ref(),
                        if opaque {
                            -1
                        } else {
                            crate::model_preview::effects::Kind::Native as i32
                        },
                    );
                    draw::blend(gl, opaque, modulates);
                    for indexed in [false, true] {
                        draw::triangles(gl, 0, 3, indexed);
                    }
                }
            }
        }
        gl.finish();
        let error = gl.get_error();
        if error != glow::NO_ERROR {
            return Err(format!(
                "The shader helper's preparation draw failed ({error:#06X})."
            ));
        }
        Ok(())
    }
}
