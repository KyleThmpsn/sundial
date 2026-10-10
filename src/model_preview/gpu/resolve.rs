//! Resolve in linear float storage, then encode once into the painter's framebuffer.
use super::*;

pub(super) struct Resolve {
    fbo: glow::Framebuffer,
    color: glow::Texture,
    vao: glow::VertexArray,
    program: glow::Program,
    source: Option<glow::UniformLocation>,
    bloom: Option<super::output::Bloom>,
    bloom_sources: [Option<glow::UniformLocation>; 3],
    has_bloom: Option<glow::UniformLocation>,
    filmic: Option<glow::UniformLocation>,
}

impl Resolve {
    pub(super) unsafe fn new(gl: &glow::Context, size: [i32; 2]) -> Option<Self> {
        // SAFETY: all objects belong to the current paint context. Failed construction
        // releases every object it allocated before returning.
        unsafe {
            let fragment = format!("{FRAGMENT}\n{}", crate::model_preview::output::GLSL);
            let program = link(gl, VERTEX, &fragment)?;
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
            let mut result = Self {
                fbo,
                color,
                vao,
                program,
                source: gl.get_uniform_location(program, "uSource"),
                bloom: None,
                bloom_sources: std::array::from_fn(|i| {
                    gl.get_uniform_location(program, &format!("uBloom{i}"))
                }),
                has_bloom: gl.get_uniform_location(program, "uHasBloom"),
                filmic: gl.get_uniform_location(program, "uFilmic"),
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
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::LINEAR as i32);
            }
            for parameter in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::CLAMP_TO_EDGE as i32);
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
            result.bloom = super::output::Bloom::new(gl, size);
            if result.bloom.is_none() {
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
        scene: Scene,
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
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.bind_vertex_array(Some(self.vao));
            if scene.bloom
                && let Some(bloom) = &self.bloom
            {
                bloom.paint(
                    gl,
                    self.color,
                    target.size,
                    crate::model_preview::output::background(scene, Style::Textured),
                );
            }
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
            gl.uniform_1_i32(self.has_bloom.as_ref(), i32::from(scene.bloom));
            gl.uniform_1_i32(self.filmic.as_ref(), i32::from(scene.filmic));
            if let Some(bloom) = &self.bloom {
                for (index, texture) in bloom.textures().enumerate() {
                    gl.active_texture(glow::TEXTURE1 + index as u32);
                    gl.bind_sampler(index as u32 + 1, None);
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    gl.uniform_1_i32(self.bloom_sources[index].as_ref(), index as i32 + 1);
                }
            }
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            for unit in 0..4 {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
            gl.active_texture(glow::TEXTURE0);
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
            if let Some(bloom) = self.bloom {
                bloom.delete(gl);
            }
        }
    }
}

pub(super) const VERTEX: &str = r#"#version 330 core
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
uniform sampler2D uBloom0,uBloom1,uBloom2;
uniform int uHasBloom,uFilmic;
out vec4 fragColor;
vec3 film(vec3 value);
void main() {
    vec3 value = max(texture(uSource, vUv).rgb, vec3(0.0));
    if(uHasBloom==1)value+=(texture(uBloom0,vUv).rgb+texture(uBloom1,vUv).rgb+texture(uBloom2,vUv).rgb)/3.0;
    if(uFilmic==1)value=film(value);
    value=clamp(value,0.0,1.0);
    vec3 low = value * 12.92;
    vec3 high = 1.055 * pow(value, vec3(1.0 / 2.4)) - 0.055;
    fragColor = vec4(mix(low, high, step(vec3(0.0031308), value)), 1.0);
}
"#;
