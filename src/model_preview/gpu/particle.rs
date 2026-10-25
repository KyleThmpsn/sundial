//! Instanced mesh studies and additive sprite studies in the shared HDR composition.
use super::*;
use crate::model_preview::{
    particle_material,
    texture::{AddressMode, Sampler},
};
use resource::Objects;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    Material,
    Sprites,
}

pub(super) struct Renderer {
    objects: Objects,
    mesh: glow::Program,
    sprite: glow::Program,
    instances: glow::Buffer,
    sprite_vao: glow::VertexArray,
    material: Option<([glow::Texture; 3], [glow::Sampler; 3], [i32; 6])>,
    wrap: glow::Sampler,
    clamp: glow::Sampler,
}

impl Renderer {
    unsafe fn new(gl: &glow::Context, model: &Model) -> Result<Self, String> {
        let mut objects = Objects::default();
        // SAFETY: construction and cleanup share the paint thread's current context.
        let built = unsafe {
            (|| {
                let mesh = objects.program(
                    link(gl, MESH_VERTEX, MESH_FRAGMENT)
                        .ok_or("The GPU particle material could not initialize.")?,
                );
                let sprite = objects.program(
                    link(gl, SPRITE_VERTEX, SPRITE_FRAGMENT)
                        .ok_or("The GPU sprite material could not initialize.")?,
                );
                let instances = objects.buffer(gl)?;
                let sprite_vao = objects.array(gl)?;
                let wrap = objects.sampler(gl, &Sampler::default())?;
                let clamp = objects.sampler(
                    gl,
                    &Sampler {
                        u: AddressMode::Clamp,
                        v: AddressMode::Clamp,
                        ..Default::default()
                    },
                )?;
                let material =
                    if model.has_particle_material_study() && model.particle_sources.is_empty() {
                        let particle = &model.assets.particles[0];
                        let mut images = Vec::new();
                        let mut samplers = Vec::new();
                        let mut mirror = [0; 6];
                        for slot in 0..3 {
                            let source = &particle
                                .material_textures
                                .iter()
                                .find(|(index, _)| *index == slot)
                                .ok_or("A particle material texture is missing.")?
                                .1;
                            images.push(objects.image(gl, source, false)?);
                            let settings = &particle.material_samplers[slot as usize];
                            samplers.push(objects.sampler(gl, settings)?);
                            mirror[slot as usize * 2] =
                                i32::from(settings.u == AddressMode::MirrorOnce);
                            mirror[slot as usize * 2 + 1] =
                                i32::from(settings.v == AddressMode::MirrorOnce);
                        }
                        Some((
                            images.try_into().unwrap(),
                            samplers.try_into().unwrap(),
                            mirror,
                        ))
                    } else {
                        None
                    };
                Ok::<_, String>((mesh, sprite, instances, sprite_vao, material, wrap, clamp))
            })()
        };
        match built {
            Ok((mesh, sprite, instances, sprite_vao, material, wrap, clamp)) => Ok(Self {
                objects,
                mesh,
                sprite,
                instances,
                sprite_vao,
                material,
                wrap,
                clamp,
            }),
            Err(error) => {
                // SAFETY: only locally created objects are released here.
                unsafe {
                    objects.delete(gl);
                }
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn paint(
        &mut self,
        gl: &glow::Context,
        uploaded: &Uploaded,
        frame: &Frame,
        target: &Target,
        center: [f32; 3],
        scale: f32,
        depth_radius: f32,
        rotate: &[f32; 9],
        deformed: bool,
        pass: Pass,
    ) -> Result<(), String> {
        // SAFETY: State binds the HDR target. All sampled textures and geometry belong to
        // that context. Particle passes preserve HDR values and never write object depth.
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.depth_mask(false);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.enable(glow::DEPTH_TEST);
            if pass == Pass::Material
                && let Some(batch) = particle_material::prepare(&frame.model, frame.seconds)
                && let Some((textures, samplers, mirror)) = self.material.as_ref()
            {
                let instances = batch.gpu_instances();
                if !instances.is_empty() {
                    gl.use_program(Some(self.mesh));
                    gl.bind_vertex_array(Some(uploaded.vao));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instances));
                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        bytes_of(instances.as_flattened().as_flattened()),
                        glow::STREAM_DRAW,
                    );
                    for row in 0..7 {
                        gl.enable_vertex_attrib_array(8 + row);
                        gl.vertex_attrib_pointer_f32(
                            8 + row,
                            4,
                            glow::FLOAT,
                            false,
                            112,
                            row as i32 * 16,
                        );
                        gl.vertex_attrib_divisor(8 + row, 1);
                    }
                    let location = |name| gl.get_uniform_location(self.mesh, name);
                    gl.uniform_3_f32(
                        location("uCenter").as_ref(),
                        center[0],
                        center[1],
                        center[2],
                    );
                    gl.uniform_matrix_3_f32_slice(location("uRotate").as_ref(), false, rotate);
                    gl.uniform_2_f32(
                        location("uScale").as_ref(),
                        2.0 * scale / target.size[0] as f32,
                        2.0 * scale / target.size[1] as f32,
                    );
                    gl.uniform_2_f32(
                        location("uPan").as_ref(),
                        2.0 * frame.camera.pan[0],
                        -2.0 * frame.camera.pan[1],
                    );
                    gl.uniform_1_f32(
                        location("uDepthScale").as_ref(),
                        (1.0 - 1e-4) / depth_radius,
                    );
                    gl.uniform_1_f32(location("uExposure").as_ref(), frame.scene.exposure);
                    gl.uniform_1_i32(location("uDeformed").as_ref(), i32::from(deformed));
                    gl.uniform_1_i32(location("uGeometry").as_ref(), 11);
                    if deformed {
                        uploaded.deformation.as_ref().unwrap().bind(gl, self.mesh);
                    }
                    gl.uniform_2_i32_slice(location("uMirror[0]").as_ref(), mirror);
                    for (unit, name) in ["uDistortion", "uMask", "uRamp"].into_iter().enumerate() {
                        gl.active_texture(glow::TEXTURE0 + unit as u32);
                        gl.bind_texture(glow::TEXTURE_2D, Some(textures[unit]));
                        gl.bind_sampler(unit as u32, Some(samplers[unit]));
                        gl.uniform_1_i32(location(name).as_ref(), unit as i32);
                    }
                    for group in &uploaded.groups {
                        if group.key.emitter {
                            gl.draw_arrays_instanced(
                                glow::TRIANGLES,
                                group.first,
                                group.count,
                                instances.len() as i32,
                            );
                        }
                    }
                    for row in 0..7 {
                        gl.disable_vertex_attrib_array(8 + row);
                        gl.vertex_attrib_divisor(8 + row, 0);
                    }
                }
            }
            if pass == Pass::Sprites && !frame.model.particle_sources.is_empty() {
                self.sprites(gl, uploaded, frame, target.size, center, scale);
            }
            gl.disable(glow::BLEND);
            gl.depth_mask(true);
            for unit in 0..3 {
                gl.bind_sampler(unit, None);
            }
            if gl.get_error() != glow::NO_ERROR {
                return Err("The GPU particle draw failed.".into());
            }
        }
        Ok(())
    }

    unsafe fn sprites(
        &self,
        gl: &glow::Context,
        uploaded: &Uploaded,
        frame: &Frame,
        size: [i32; 2],
        center: [f32; 3],
        scale: f32,
    ) {
        // SAFETY: all texture indices came from the loaded model. The six procedural
        // vertices bound each sprite and its fragment shader performs the study's reads.
        unsafe {
            gl.disable(glow::DEPTH_TEST);
            gl.use_program(Some(self.sprite));
            gl.bind_vertex_array(Some(self.sprite_vao));
            let location = |name| gl.get_uniform_location(self.sprite, name);
            gl.uniform_2_f32(location("uSize").as_ref(), size[0] as f32, size[1] as f32);
            for (unit, name) in ["uColor", "uRamp", "uMask"].into_iter().enumerate() {
                gl.uniform_1_i32(location(name).as_ref(), unit as i32);
                gl.bind_sampler(
                    unit as u32,
                    Some(if unit == 1 { self.clamp } else { self.wrap }),
                );
            }
            let (sy, cy) = frame.camera.yaw.sin_cos();
            let (sp, cp) = frame.camera.pitch.sin_cos();
            for source in &frame.model.particle_sources {
                let Some(texture) = frame.model.textures.get(source.texture) else {
                    continue;
                };
                let age = (frame.seconds / source.period + source.phase).rem_euclid(1.0);
                let point: [f32; 3] =
                    std::array::from_fn(|i| source.position[i] + source.drift[i] * age - center[i]);
                let forward = sy * point[0] + cy * point[1];
                let x = size[0] as f32 * (0.5 + frame.camera.pan[0])
                    + (cy * point[0] - sy * point[1]) * scale;
                let y = size[1] as f32 * (0.5 + frame.camera.pan[1])
                    - (sp * forward + cp * point[2]) * scale;
                let dx = cy * source.drift[0] - sy * source.drift[1];
                let dy =
                    -(sp * (sy * source.drift[0] + cy * source.drift[1]) + cp * source.drift[2]);
                let angle = if dx.abs() + dy.abs() > 0.0001 {
                    dy.atan2(dx)
                } else {
                    -0.4
                };
                let (sin, cos) = angle.sin_cos();
                let width = (source.width * scale * (0.7 + age * 0.5))
                    .clamp(3.0, (size[0].min(size[1]) as f32 * 0.6).max(3.0));
                let height = width
                    / (texture.size[0] as f32 / texture.size[1].max(1) as f32).clamp(1.0, 16.0);
                gl.uniform_4_f32(location("uSprite").as_ref(), x, y, width, height);
                gl.uniform_2_f32(location("uAngle").as_ref(), cos, sin);
                gl.uniform_1_f32(
                    location("uStrength").as_ref(),
                    (1.0 - age).powf(1.4) * 0.7 * frame.scene.exposure,
                );
                let ramp = source
                    .gradient
                    .and_then(|i| uploaded.textures.get(i))
                    .and_then(|roles| roles[1]);
                gl.uniform_1_i32(location("uHasRamp").as_ref(), i32::from(ramp.is_some()));
                for (unit, image) in [
                    uploaded.textures[source.texture][1],
                    ramp,
                    uploaded.textures[source.texture][0],
                ]
                .into_iter()
                .enumerate()
                {
                    gl.active_texture(glow::TEXTURE0 + unit as u32);
                    gl.bind_texture(glow::TEXTURE_2D, image);
                }
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
            }
        }
    }

    pub unsafe fn delete(self, gl: &glow::Context) {
        // SAFETY: called by the uploaded model's owner on the paint context.
        unsafe {
            self.objects.delete(gl);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn paint(
    gl: &glow::Context,
    uploaded: &mut Uploaded,
    frame: &Frame,
    target: &Target,
    center: [f32; 3],
    scale: f32,
    depth_radius: f32,
    rotate: &[f32; 9],
    deformed: bool,
    pass: Pass,
) -> Result<(), String> {
    if (pass == Pass::Material && !frame.model.has_particle_material_study())
        || (pass == Pass::Sprites && frame.model.particle_sources.is_empty())
    {
        return Ok(());
    }
    // SAFETY: resources are temporarily moved out so the draw may borrow the other upload
    // fields. They are restored on success and failure for normal owner cleanup.
    unsafe {
        let mut renderer = match uploaded.particles.take() {
            Some(renderer) => renderer,
            None => Renderer::new(gl, &frame.model)?,
        };
        let result = renderer.paint(
            gl,
            uploaded,
            frame,
            target,
            center,
            scale,
            depth_radius,
            rotate,
            deformed,
            pass,
        );
        uploaded.particles = Some(renderer);
        result
    }
}

const MESH_VERTEX: &str = r#"#version 330 core
layout(location=0)in vec3 aPosition;
layout(location=2)in vec2 aUv;
layout(location=7)in uvec3 aIndices;
layout(location=8)in vec4 aSecond;
layout(location=9)in vec4 aDistortion;
layout(location=10)in vec4 aFirst;
layout(location=11)in vec4 aPlacement;
layout(location=12)in vec4 aRamp;
layout(location=13)in vec4 aIntensity;
layout(location=14)in vec4 aState;
uniform samplerBuffer uGeometry;
uniform bool uDeformed;
uniform vec3 uCenter;
uniform mat3 uRotate;
uniform vec2 uScale,uPan;
uniform float uDepthScale;
out vec2 vUv;
flat out vec4 vSecond,vDistortion,vFirst,vRamp,vIntensity,vState;
void main(){vec3 point=uDeformed?texelFetch(uGeometry,int(aIndices.x)*3).xyz:aPosition;
 if(aState.y!=0.0){float s=sin(aPlacement.w),c=cos(aPlacement.w);point=aPlacement.xyz+vec3(point.x,c*point.y-s*point.z,s*point.y+c*point.z)*0.1;}
 vec3 p=uRotate*(point-uCenter);gl_Position=vec4(p.xy*uScale+uPan,clamp(p.z*uDepthScale,-1.0,1.0),1.0);
 vUv=aUv;vSecond=aSecond;vDistortion=aDistortion;vFirst=aFirst;vRamp=aRamp;vIntensity=aIntensity;vState=aState;
}
"#;

const MESH_FRAGMENT: &str = r#"#version 330 core
in vec2 vUv;
flat in vec4 vSecond,vDistortion,vFirst,vRamp,vIntensity,vState;
uniform sampler2D uDistortion,uMask,uRamp;
uniform ivec2 uMirror[3];
uniform float uExposure;
out vec4 color;
vec2 coordinate(vec2 uv,int index){return vec2(uMirror[index].x!=0?abs(uv.x):uv.x,uMirror[index].y!=0?abs(uv.y):uv.y);}
void main(){if(vState.x==0.0)discard;
 vec2 noise=textureLod(uDistortion,coordinate(vUv*vDistortion.xy+vDistortion.zw,0),0.0).xy;
 vec2 first=vUv*vFirst.xy+vFirst.zw+(noise-vec2(0.0,0.2))*vRamp.w;
 float a=textureLod(uMask,coordinate(first,1),0.0).r;
 float b=textureLod(uMask,coordinate(vUv*vSecond.xy+vSecond.zw,1),0.0).r;
 vec3 ramp=textureLod(uRamp,coordinate(vec2(clamp(a*b*vRamp.x+vRamp.y,0.0,1.0),0.0),2),0.0).rgb;
 float edge=max(1.0-abs(vUv.x-0.5)*2.222222,0.0);float coverage=clamp(edge*edge*vIntensity.z,0.0,1.0);
 color=vec4(ramp*coverage*vIntensity.y*uExposure*0.02,0.0);
}
"#;

const SPRITE_VERTEX: &str = r#"#version 330 core
uniform vec4 uSprite;
uniform vec2 uSize,uAngle;
out vec2 vUv;
void main(){const vec2 corners[6]=vec2[6](vec2(0,0),vec2(1,0),vec2(1,1),vec2(0,0),vec2(1,1),vec2(0,1));
 vUv=corners[gl_VertexID];vec2 p=(vUv-0.5)*uSprite.zw;
 p=uSprite.xy+vec2(uAngle.x*p.x-uAngle.y*p.y,uAngle.y*p.x+uAngle.x*p.y);
 gl_Position=vec4(p.x/uSize.x*2.0-1.0,1.0-p.y/uSize.y*2.0,0.0,1.0);}
"#;

const SPRITE_FRAGMENT: &str = r#"#version 330 core
in vec2 vUv;
uniform sampler2D uColor,uRamp,uMask;
uniform bool uHasRamp;
uniform float uStrength;
out vec4 color;
void main(){vec4 source=textureLod(uColor,vUv,0.0);vec3 rgb=source.rgb;
 if(uHasRamp)rgb=textureLod(uRamp,vec2(textureLod(uMask,vUv,0.0).r,0.0),0.0).rgb;
 color=vec4(rgb*source.a*uStrength,0.0);}
"#;
