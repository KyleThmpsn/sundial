//! Float bloom targets and verified Shadowkeep brightness and blur arithmetic.
use super::*;

struct Image {
    fbo: glow::Framebuffer,
    texture: glow::Texture,
    size: [i32; 2],
}

impl Image {
    unsafe fn new(gl: &glow::Context, size: [i32; 2]) -> Option<Self> {
        // SAFETY: construction and cleanup use the current paint context.
        unsafe {
            let texture = gl.create_texture().ok()?;
            let fbo = match gl.create_framebuffer() {
                Ok(value) => value,
                Err(_) => {
                    gl.delete_texture(texture);
                    return None;
                }
            };
            let image = Self { fbo, texture, size };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                image.delete(gl);
                return None;
            }
            Some(image)
        }
    }

    unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: called on the context that owns these targets.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}

pub(super) struct Bloom {
    program: glow::Program,
    source: Option<glow::UniformLocation>,
    step: Option<glow::UniformLocation>,
    mode: Option<glow::UniformLocation>,
    background: Option<glow::UniformLocation>,
    levels: Vec<(Image, Image)>,
}

impl Bloom {
    pub(super) unsafe fn new(gl: &glow::Context, mut size: [i32; 2]) -> Option<Self> {
        // SAFETY: link and all allocations belong to the live paint context. Each partial
        // construction is released on failure, including its already completed levels.
        unsafe {
            let program = link(gl, super::resolve::VERTEX, FRAGMENT)?;
            let mut result = Self {
                program,
                source: gl.get_uniform_location(program, "uSource"),
                step: gl.get_uniform_location(program, "uStep"),
                mode: gl.get_uniform_location(program, "uMode"),
                background: gl.get_uniform_location(program, "uBackground"),
                levels: Vec::new(),
            };
            for _ in 0..3 {
                size = size.map(|v| (v + 1) / 2);
                let Some(image) = Image::new(gl, size) else {
                    result.delete(gl);
                    return None;
                };
                let Some(temp) = Image::new(gl, size) else {
                    image.delete(gl);
                    result.delete(gl);
                    return None;
                };
                result.levels.push((image, temp));
            }
            Some(result)
        }
    }

    pub(super) unsafe fn paint(
        &self,
        gl: &glow::Context,
        source: glow::Texture,
        size: [i32; 2],
        background: [f32; 3],
    ) {
        // SAFETY: every pass reads a separate completed image and writes an owned float
        // target. The caller binds its fullscreen VAO and disables depth, scissor and blend.
        unsafe {
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_sampler(0, None);
            gl.uniform_1_i32(self.source.as_ref(), 0);
            gl.uniform_3_f32(
                self.background.as_ref(),
                background[0],
                background[1],
                background[2],
            );
            let mut input = (source, size);
            for (index, (image, _)) in self.levels.iter().enumerate() {
                self.pass(gl, input, image, i32::from(index != 0));
                input = (image.texture, image.size);
            }
            for (image, temp) in &self.levels {
                self.pass(gl, (image.texture, image.size), temp, 2);
                self.pass(gl, (temp.texture, temp.size), image, 3);
            }
        }
    }

    unsafe fn pass(
        &self,
        gl: &glow::Context,
        source: (glow::Texture, [i32; 2]),
        output: &Image,
        mode: i32,
    ) {
        // SAFETY: input and output are distinct textures owned by this paint context.
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(output.fbo));
            gl.viewport(0, 0, output.size[0], output.size[1]);
            gl.bind_texture(glow::TEXTURE_2D, Some(source.0));
            gl.uniform_2_f32(
                self.step.as_ref(),
                1.0 / source.1[0] as f32,
                1.0 / source.1[1] as f32,
            );
            gl.uniform_1_i32(self.mode.as_ref(), mode);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }

    pub(super) fn textures(&self) -> impl Iterator<Item = glow::Texture> + '_ {
        self.levels.iter().map(|(image, _)| image.texture)
    }

    pub(super) unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: all objects are deleted on their owning paint context.
        unsafe {
            for (image, temp) in self.levels {
                image.delete(gl);
                temp.delete(gl);
            }
            gl.delete_program(self.program);
        }
    }
}

const FRAGMENT: &str = r#"#version 330 core
in vec2 vUv;
uniform sampler2D uSource;
uniform vec2 uStep;
uniform int uMode;
uniform vec3 uBackground;
out vec4 fragColor;
vec3 bright(vec3 rgb) { return rgb * (0.016 + 0.0005 * dot(rgb, vec3(0.3, 0.59, 0.11))); }
void main() {
    vec3 value;
    if(uMode < 2) {
        value = (clamp(texture(uSource,vUv+uStep*vec2(-0.5,-0.5)).rgb,0.0,65504.0)
               + clamp(texture(uSource,vUv+uStep*vec2( 0.5,-0.5)).rgb,0.0,65504.0)
               + clamp(texture(uSource,vUv+uStep*vec2(-0.5, 0.5)).rgb,0.0,65504.0)
               + clamp(texture(uSource,vUv+uStep*vec2( 0.5, 0.5)).rgb,0.0,65504.0)) * 0.25;
        if(uMode==0)value=max(bright(value)-bright(uBackground),vec3(0.0));
    } else {
        vec2 axis = uMode==2 ? vec2(uStep.x,0.0) : vec2(0.0,uStep.y);
        value = texture(uSource,vUv-axis*4.5).rgb*0.05882
              + texture(uSource,vUv-axis*(7.0/3.0)).rgb*0.17647
              + texture(uSource,vUv).rgb*0.52941
              + texture(uSource,vUv+axis*(7.0/3.0)).rgb*0.17647
              + texture(uSource,vUv+axis*4.5).rgb*0.05882;
    }
    fragColor=vec4(value,1.0);
}
"#;
