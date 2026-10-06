//! Resolve in linear float storage, then encode once into the painter's framebuffer.
use super::*;

pub(super) struct Resolve {
    fbo: glow::Framebuffer,
    color: glow::Texture,
    vao: glow::VertexArray,
    program: glow::Program,
    source: Option<glow::UniformLocation>,
}

impl Resolve {
    pub(super) unsafe fn new(gl: &glow::Context, size: [i32; 2]) -> Option<Self> {
        // SAFETY: all objects belong to the current paint context. Failed construction
        // releases every object it allocated before returning.
        unsafe {
            let program = link(gl, VERTEX, FRAGMENT)?;
            let fbo = match gl.create_framebuffer() {
                Ok(value) => value,
                Err(_) => {
                    gl.delete_program(program);
                    return None;
                }
            };
            let color = match gl.create_texture() {
                Ok(value) => value,
                Err(_) => {
                    gl.delete_framebuffer(fbo);
                    gl.delete_program(program);
                    return None;
                }
            };
            let vao = match gl.create_vertex_array() {
                Ok(value) => value,
                Err(_) => {
                    gl.delete_texture(color);
                    gl.delete_framebuffer(fbo);
                    gl.delete_program(program);
                    return None;
                }
            };
            let result = Self {
                fbo,
                color,
                vao,
                program,
                source: gl.get_uniform_location(program, "uSource"),
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(color));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA32F as i32,
                size[0],
                size[1],
                0,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(None),
            );
            for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::NEAREST as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color),
                0,
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                result.delete(gl);
                return None;
            }
            Some(result)
        }
    }

    pub(super) unsafe fn paint(
        &self,
        gl: &glow::Context,
        target: &Target,
        previous: Option<glow::Framebuffer>,
        origin: [i32; 2],
    ) {
        // SAFETY: the multisample source and single-sample destination have identical
        // dimensions and float formats. The final triangle stays inside the painter's viewport.
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.fbo));
            gl.blit_framebuffer(
                0,
                0,
                target.size[0],
                target.size[1],
                0,
                0,
                target.size[0],
                target.size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
            gl.viewport(origin[0], origin[1], target.size[0], target.size[1]);
            gl.enable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_sampler(0, None);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.color));
            gl.uniform_1_i32(self.source.as_ref(), 0);
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);
        }
    }

    pub(super) unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: called with the context that constructed these objects.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.color);
            gl.delete_vertex_array(self.vao);
            gl.delete_program(self.program);
        }
    }
}

const VERTEX: &str = r#"#version 330 core
out vec2 vUv;
void main() {
    vec2 point = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
    vUv = point;
    gl_Position = vec4(point * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FRAGMENT: &str = r#"#version 330 core
in vec2 vUv;
uniform sampler2D uSource;
out vec4 fragColor;
void main() {
    vec3 value = clamp(texture(uSource, vUv).rgb, 0.0, 1.0);
    vec3 low = value * 12.92;
    vec3 high = 1.055 * pow(value, vec3(1.0 / 2.4)) - 0.055;
    fragColor = vec4(mix(low, high, step(vec3(0.0031308), value)), 1.0);
}
"#;
