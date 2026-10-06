//! Bind and draw the opaque and supported transparent material passes.
use super::*;

pub(super) unsafe fn groups(
    gl: &glow::Context,
    uniforms: &Uniforms,
    uploaded: &Uploaded,
    frame: &Frame,
    target: &Target,
) {
    // SAFETY: called by State::draw with the current painter context and uploaded resources.
    unsafe {
        let dyes = shader::dyes(&frame.model, frame.seconds);
        let effect_frames = crate::model_preview::effects::frames(&frame.model, frame.seconds);
        gl.uniform_1_i32(uniforms.scene_depth.as_ref(), 9);
        for (unit, location) in uniforms.effect_textures.iter().enumerate() {
            gl.uniform_1_i32(location.as_ref(), 6 + unit as i32);
        }
        let mut copied_depth = false;
        let hide_emitter = frame.style == Style::Textured && uploaded.hide_emitter;
        for group in &uploaded.groups {
            if hide_emitter && group.key.emitter {
                continue;
            }
            for unit in 0..10 {
                gl.bind_sampler(unit, None);
            }
            gl.uniform_1_i32(uniforms.native_index.as_ref(), -1);
            bind_dye_map(gl, uniforms, uploaded, group.key.dye_map, group.key.slot);
            gl.uniform_1_i32(
                uniforms.native_detail.as_ref(),
                i32::from(group.key.native_detail),
            );
            gl.uniform_1_f32(
                uniforms.cutoff.as_ref(),
                group.key.cutoff.map(f32::from_bits).unwrap_or(0.5),
            );
            let effect = group.key.effect.filter(|_| frame.style == Style::Textured);
            if let Some(index) = effect {
                let material = &frame.model.effects[index];
                let Some(constants) = effect_frames[index].as_ref() else {
                    continue;
                };
                effect_state(gl, uniforms, target, material, &mut copied_depth);
                gl.uniform_4_f32_slice(
                    uniforms.effect_constants.as_ref(),
                    constants.as_flattened(),
                );
                for (slot, texture) in material.textures.iter().enumerate() {
                    gl.active_texture(glow::TEXTURE6 + slot as u32);
                    gl.bind_texture(
                        glow::TEXTURE_2D,
                        texture
                            .and_then(|i| uploaded.textures.get(i))
                            .and_then(|roles| roles[usize::from(material.color[slot])]),
                    );
                }
                gl.active_texture(glow::TEXTURE9);
                gl.bind_texture(glow::TEXTURE_2D, Some(target.scene_depth));
                let bindings: Vec<_> = material.sampling().map_or_else(
                    || {
                        if material.normal.is_some() {
                            Vec::new()
                        } else {
                            vec![(0, 1), (1, 2), (3, 0), (6, 0), (7, 0), (8, 3)]
                        }
                    },
                    |units| {
                        units
                            .into_iter()
                            .enumerate()
                            .map(|(unit, sampler)| (unit as u32, sampler))
                            .collect()
                    },
                );
                for (unit, sampler) in bindings {
                    gl.bind_sampler(
                        unit,
                        uploaded
                            .samplers
                            .get(index)
                            .and_then(|s| s.get(sampler))
                            .copied(),
                    );
                }
            } else {
                gl.disable(glow::BLEND);
                gl.disable(glow::FRAMEBUFFER_SRGB);
                gl.depth_mask(true);
                gl.uniform_1_i32(uniforms.effect.as_ref(), -1);
            }
            let dye = dyes
                .get(usize::from(group.key.slot))
                .and_then(Option::as_ref);
            let textures = [
                group.key.albedo,
                group.key.gearstack,
                group.key.normal,
                dye.and_then(|d| d.detail),
                dye.and_then(|d| d.normal),
            ];
            for (unit, texture) in textures.iter().enumerate() {
                gl.active_texture(glow::TEXTURE0 + unit as u32);
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    texture
                        .and_then(|i| uploaded.textures.get(i))
                        .and_then(|roles| roles[usize::from(matches!(unit, 0 | 3))]),
                );
            }
            let has = [
                textures[0].is_some(),
                textures[1].is_some(),
                textures[2].is_some(),
                textures[3].is_some(),
                textures[4].is_some(),
                dye.is_some(),
                group.key.clip,
                uploaded.lookup.is_some(),
                group.key.constant.is_some(),
            ];
            if let Some(constant) = group.key.constant.map(|c| c.map(f32::from_bits)) {
                gl.uniform_3_f32(
                    uniforms.constant.as_ref(),
                    constant[0],
                    constant[1],
                    constant[2],
                );
            }
            gl.active_texture(glow::TEXTURE5);
            gl.bind_texture(glow::TEXTURE_2D, uploaded.lookup);
            for (location, value) in uniforms.has.iter().zip(has) {
                gl.uniform_1_i32(location.as_ref(), i32::from(value));
            }
            bind_dye(gl, uniforms, dye);
            bind_legacy(
                gl,
                uniforms,
                effect.and_then(|i| {
                    frame.model.effects[i]
                        .normal
                        .as_ref()?
                        .frame(effect_frames[i].as_ref()?)
                }),
                dye,
            );
            if let Some(index) = effect {
                if let Some(native) = &frame.model.effects[index].native {
                    let Some(constants) = native.vertex_frame(frame.seconds) else {
                        continue;
                    };
                    gl.uniform_1_i32(uniforms.native_index.as_ref(), index as i32);
                    gl.uniform_1_i32(
                        uniforms.native_quaternion.as_ref(),
                        i32::from(native.quaternion()),
                    );
                    gl.uniform_4_f32_slice(
                        uniforms.native_vertex_constants.as_ref(),
                        constants.as_flattened(),
                    );
                    let vectors = dye.map_or([[0.0; 4]; 27], |d| d.vectors);
                    gl.uniform_4_f32_slice(uniforms.native_dye.as_ref(), vectors.as_flattened());
                    let mut present = [0; 9];
                    for (unit, binding) in native.bindings.iter().enumerate() {
                        let texture_unit = native.texture_unit(unit);
                        use crate::model_preview::effects::native::Role;
                        let texture = match binding.role {
                            Role::Texture(index) => Some(index),
                            Role::Albedo => group.key.albedo,
                            Role::Normal => group.key.normal,
                            Role::Gear => group.key.gearstack,
                            Role::Detail => dye.and_then(|d| d.detail),
                            Role::DetailNormal => dye.and_then(|d| d.normal),
                            Role::Depth | Role::Mask | Role::Scene => None,
                        };
                        let texture = texture
                            .and_then(|i| uploaded.textures.get(i))
                            .and_then(|roles| roles[usize::from(binding.color)]);
                        present[unit] = i32::from(texture.is_some());
                        gl.active_texture(glow::TEXTURE0 + texture_unit as u32);
                        gl.bind_texture(glow::TEXTURE_2D, texture);
                        gl.bind_sampler(
                            texture_unit as u32,
                            binding
                                .sampler
                                .and_then(|s| {
                                    uploaded
                                        .samplers
                                        .get(index)
                                        .and_then(|samplers| samplers.get(s))
                                })
                                .copied(),
                        );
                    }
                    gl.uniform_1_i32_slice(uniforms.native_present.as_ref(), &present);
                }
            }
            gl.draw_arrays(glow::TRIANGLES, group.first, group.count);
        }
    }
}

unsafe fn bind_legacy(
    gl: &glow::Context,
    uniforms: &Uniforms,
    decode: Option<[f32; 3]>,
    dye: Option<&shader::Dye>,
) {
    // SAFETY: the caller supplies the current draw context and this shader's locations.
    unsafe {
        gl.uniform_1_i32(
            uniforms.has_legacy_normal.as_ref(),
            i32::from(decode.is_some()),
        );
        if let Some([x, y, z]) = decode {
            gl.uniform_3_f32(uniforms.legacy_normal.as_ref(), x, y, z);
            let detail = dye.map_or([2.0, -1.0, 0.0, 0.0], |d| d.vectors[2]);
            gl.uniform_3_f32(
                uniforms.legacy_detail.as_ref(),
                detail[0],
                detail[1],
                detail[2],
            );
        }
    }
}

unsafe fn effect_state(
    gl: &glow::Context,
    uniforms: &Uniforms,
    target: &Target,
    material: &crate::model_preview::effects::Material,
    copied_depth: &mut bool,
) {
    // SAFETY: called by groups with the current renderer context and target.
    unsafe {
        if material.opaque() {
            gl.disable(glow::BLEND);
        } else {
            copy_effect_depth(gl, target, copied_depth);
            gl.enable(glow::BLEND);
        }
        gl.disable(glow::FRAMEBUFFER_SRGB);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        gl.depth_mask(material.opaque());
        gl.uniform_1_i32(
            uniforms.effect.as_ref(),
            if material.opaque() {
                -1
            } else {
                material.kind as i32
            },
        );
    }
}

unsafe fn bind_dye(gl: &glow::Context, uniforms: &Uniforms, dye: Option<&shader::Dye>) {
    let Some(dye) = dye else { return };
    // SAFETY: called by groups with the current painter context and program.
    unsafe {
        let s = &dye.surface;
        gl.uniform_3_f32(
            uniforms.dye_albedo.as_ref(),
            s.albedo[0],
            s.albedo[1],
            s.albedo[2],
        );
        gl.uniform_3_f32(
            uniforms.dye_worn.as_ref(),
            s.worn_albedo[0],
            s.worn_albedo[1],
            s.worn_albedo[2],
        );
        gl.uniform_3_f32(
            uniforms.emissive.as_ref(),
            s.emissive[0],
            s.emissive[1],
            s.emissive[2],
        );
        gl.uniform_4_f32_slice(uniforms.params.as_ref(), &s.params);
        gl.uniform_4_f32_slice(uniforms.worn_params.as_ref(), &s.worn_params);
        gl.uniform_4_f32_slice(uniforms.rough.as_ref(), &s.roughness);
        gl.uniform_4_f32_slice(uniforms.worn_rough.as_ref(), &s.worn_roughness);
        gl.uniform_4_f32_slice(uniforms.wear.as_ref(), &s.wear);
        gl.uniform_4_f32_slice(uniforms.detail_transform.as_ref(), &dye.transform);
        gl.uniform_4_f32_slice(uniforms.normal_transform.as_ref(), &dye.normal_transform);
        gl.uniform_1_f32(uniforms.iridescence_id.as_ref(), s.iridescence);
    }
}

unsafe fn bind_dye_map(
    gl: &glow::Context,
    uniforms: &Uniforms,
    uploaded: &Uploaded,
    map: Option<crate::model_preview::texture::DyeMap>,
    slot: u8,
) {
    // SAFETY: called by groups with the current painter context and uploaded textures.
    unsafe {
        gl.uniform_1_i32(uniforms.dye_map.as_ref(), 10);
        gl.uniform_1_i32(uniforms.has_dye_map.as_ref(), i32::from(map.is_some()));
        gl.uniform_1_i32(uniforms.map_slot.as_ref(), i32::from(slot));
        if let Some(map) = map {
            gl.active_texture(glow::TEXTURE10);
            gl.bind_sampler(10, None);
            gl.bind_texture(glow::TEXTURE_2D, uploaded.textures[map.texture][0]);
            gl.uniform_4_f32_slice(
                uniforms.map_transform.as_ref(),
                &map.transform.map(f32::from_bits),
            );
        }
    }
}

unsafe fn copy_effect_depth(gl: &glow::Context, target: &Target, copied_depth: &mut bool) {
    let size = target.size;
    // SAFETY: called with the current draw context and its complete target.
    unsafe {
        if !*copied_depth {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(target.scene_fbo));
            gl.blit_framebuffer(
                0,
                0,
                size[0],
                size[1],
                0,
                0,
                size[0],
                size[1],
                glow::DEPTH_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            *copied_depth = true;
        }
    }
}
