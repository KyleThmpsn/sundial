//! Evaluate structurally validated Shadowkeep opaque surface programs before studio lighting.
//! Explicit image roles and material UV equations stay in the original pixel dependency graph.
use super::*;
use program::{Instruction, Operand, Program as Code};
mod decal;
mod normal;

fn literal(operand: &Operand, lane: usize, value: f32) -> bool {
    operand.kind == 4
        && operand.modifier == 0
        && operand.literal[operand.lanes[lane]] == value.to_bits()
}

fn output(code: &Code, target: u32, lane: usize) -> Option<(usize, &Instruction)> {
    code.instructions.iter().enumerate().rev().find(|(_, row)| {
        row.operands.first().is_some_and(|o| {
            o.kind == 2 && o.indices[0].base == target && o.mask & (1 << lane) != 0
        })
    })
}

fn producer<'a>(
    code: &'a Code,
    source: &Operand,
    lane: usize,
    before: usize,
) -> Option<(usize, &'a Instruction, usize)> {
    if source.kind != 0
        || source.modifier != 0
        || source.indices.len() != 1
        || source.indices[0].relative.is_some()
    {
        return None;
    }
    let lane = source.lanes[lane];
    code.instructions[..before]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, row)| {
            let d = row.operands.first()?;
            (d.kind == 0
                && d.indices[0].base == source.indices[0].base
                && d.mask & (1 << lane) != 0)
                .then_some((at, row, lane))
        })
}

fn normal_contract(code: &Code) -> bool {
    let Some((at, row)) = output(code, 1, 0) else {
        return false;
    };
    normal_vector(code, at, row)
}

fn normal_vector(code: &Code, at: usize, row: &Instruction) -> bool {
    normal::packed(code, at, row)
}

fn visibility_contract(code: &Code) -> bool {
    let Some((_, y)) = output(code, 2, 1) else {
        return false;
    };
    let Some((_, z)) = output(code, 2, 2) else {
        return false;
    };
    z.code == 54
        && literal(&z.operands[1], 2, 0.0)
        && ((y.code == 54
            && y.operands[1].kind == 4
            && y.operands[1].modifier == 0
            && (0.0..=0.5).contains(&f32::from_bits(
                y.operands[1].literal[y.operands[1].lanes[1]],
            )))
            || (y.code == 56
                && !y.saturate
                && (literal(&y.operands[1], 1, 0.5) || literal(&y.operands[2], 1, 0.5)))
            || opaque::intensity::recover_surface(code))
}

fn supported(code: &Code, decal: bool) -> bool {
    code.outputs.len() == 3
        && (0..3).all(|target| {
            code.outputs.iter().any(|s| {
                s.name == "SV_TARGET" && s.index == target && s.register == target as usize
            })
        })
        && code.buffers.iter().all(|&(slot, _)| matches!(slot, 0 | 12))
        && code.resources.len() <= 9
        && code
            .resources
            .iter()
            .all(|r| r.dimension == 3 && !r.integer)
        && !code.instructions.iter().any(|i| {
            (!decal && matches!(i.code, 22 | 48))
                || i.operands
                    .iter()
                    .any(|o| o.indices.iter().any(|i| i.relative.is_some()))
        })
        && (decal || normal_contract(code) && visibility_contract(code))
}

/// Geometry decoding already applies the model placement. Compose a material's original
/// vertex UV equations with its inverse, so the pixel stage receives the original varying.
fn vertex_uv(
    manager: &PackageManager,
    bytes: &[u8],
    model_uv: [f32; 4],
) -> Result<Option<[f32; 4]>, String> {
    let tag = u32_at(bytes, 0x48)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(Some([1.0, 1.0, 0.0, 0.0]));
    }
    if model_uv.iter().any(|v| !v.is_finite()) || model_uv[..2].iter().any(|v| v.abs() < 1e-8) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, tag, 1)?;
    let Ok(mut code) = Code::read_stored(&raw) else {
        return Ok(None);
    };
    let constants = super::super::read::stage_constants(manager, bytes, 0x48)?;
    gear::bind_model_uv(&mut code, model_uv);
    let Some(output) = code
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 3)
    else {
        return Ok(None);
    };
    let operand = Operand {
        kind: 2,
        indices: vec![program::Index {
            base: output.register as u32,
            relative: None,
        }],
        lanes: [0, 1, 2, 3],
        mask: 15,
        modifier: 0,
        literal: [0; 4],
    };
    let Some(x) = gear::value(&code, &constants, &operand, 0, code.instructions.len(), 0) else {
        return Ok(None);
    };
    let Some(y) = gear::value(&code, &constants, &operand, 1, code.instructions.len(), 0) else {
        return Ok(None);
    };
    if x[1] != 0.0 || y[0] != 0.0 {
        return Ok(None);
    }
    let scale = [x[0] / model_uv[0], y[1] / model_uv[1]];
    let uv = [
        scale[0],
        scale[1],
        x[2] - scale[0] * model_uv[2],
        y[2] - scale[1] * model_uv[3],
    ];
    Ok(uv.iter().all(|v| v.is_finite()).then_some(uv))
}

/// Rigid object programs have no bone palette beyond the three root transform rows.
/// Execute their stored varyings, including local coordinates and vertex-table loads.
fn rigid_vertex(manager: &PackageManager, bytes: &[u8]) -> Result<bool, String> {
    let tag = u32_at(bytes, 0x48)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(false);
    }
    let raw = super::super::read::shader_bytes(manager, tag, 1)?;
    let Ok(code) = Code::read_stored(&raw) else {
        return Ok(false);
    };
    Ok(code.buffers.contains(&(11, 11))
        && code.buffers.iter().all(|&(slot, count)| match slot {
            0 => count <= 128,
            11 => count == 11,
            12 => count <= 14,
            _ => false,
        }))
}

pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
    objects: ObjectInputs<'_>,
    dye: u8,
    uv: [f32; 4],
    model: &mut Model,
) -> Result<Option<Material>, String> {
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let pixel = u32_at(&bytes, 0x2C8)?;
    if matches!(pixel, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, pixel, 0)?;
    let code = match Code::read(&raw, 0) {
        Ok(code) => code,
        Err(error) => {
            if std::env::var_os("SUNDIAL_PREVIEW_TRACE").is_some() {
                model.notices.push(format!("Surface 0x{tag:08X}: {error}"));
            }
            return Ok(None);
        }
    };
    let full_decal = bytes[0x20] == 0x9a && decal::full(&code);
    let decal = full_decal || matches!(bytes[0x20], 0x96 | 0x97) && decal::supported(&code);
    if !supported(&code, decal) {
        if std::env::var_os("SUNDIAL_PREVIEW_TRACE").is_some() {
            model.notices.push(format!(
                "Surface 0x{tag:08X}: unsupported deferred contract, normal {}, visibility {}, buffers {:?}",
                normal_contract(&code), visibility_contract(&code), code.buffers
            ));
        }
        return Ok(None);
    }
    let explicit = resource::explicit(&bytes, 0x2D0)?;
    if code
        .resources
        .iter()
        .any(|r| !explicit.contains_key(&r.slot) && !full_decal)
    {
        return Ok(None);
    }
    let rigid = rigid_vertex(manager, &bytes)?;
    let uv = if rigid {
        None
    } else {
        let Some(uv) = vertex_uv(manager, &bytes, uv)? else {
            return Ok(None);
        };
        Some(uv)
    };
    let intensity = opaque::intensity::recover_surface(&code);
    let mut material = load_program(
        manager,
        tag,
        &bytes,
        objects,
        dye,
        model,
        (code, !rigid, true),
    )?;
    let native = material.native.as_mut().unwrap();
    native.opaque_uv = uv;
    native.intensity = intensity;
    native.decal = decal;
    native.full_decal = full_decal;
    if full_decal
        && (!native
            .bindings
            .iter()
            .any(|b| matches!(b.role, Role::SceneNormal))
            || native
                .bindings
                .iter()
                .any(|b| !explicit.contains_key(&b.slot) && !matches!(b.role, Role::SceneNormal)))
    {
        return Err("The deferred decal has no bound receiver normal".into());
    }
    native.ambient_power = opaque::intensity::ambient_power(manager);
    if std::env::var_os("SUNDIAL_PREVIEW_TRACE").is_some() {
        trace(tag, &material, model);
    }
    Ok(Some(material))
}

/// A notice naming a loaded surface's recovered contract and each bound image's size and
/// centre texel, for `SUNDIAL_PREVIEW_TRACE` runs of the headless icon route.
fn trace(tag: u32, material: &Material, model: &mut Model) {
    let Some(native) = material.native.as_ref() else {
        return;
    };
    let mut text = format!(
        "Surface 0x{tag:08X}: intensity {}, decal {}, uv {:?}, {} samplers",
        native.intensity,
        native.decal,
        native.opaque_uv,
        material.samplers.len()
    );
    for binding in &native.bindings {
        let image = match binding.role {
            Role::Texture(index) => model.textures.get(index).map(|texture| {
                let [width, height] = texture.size;
                let at = ((height / 2) * width + width / 2) * 4;
                format!(
                    "{width}x{height} centre {:?}",
                    texture.rgba.get(at..at + 4).unwrap_or(&[])
                )
            }),
            _ => None,
        };
        text.push_str(&format!(
            ", t{} {} sampler {:?} {}",
            binding.slot,
            if binding.color { "sRGB" } else { "linear" },
            binding.sampler,
            image.unwrap_or_else(|| "implicit".into())
        ));
    }
    // The program's three targets at the centre of the map, facing the viewer.
    let probe = material.frame(0.0).and_then(|frame| {
        let input = vertex::Input {
            vertex_id: 0,
            position: [0.0; 3],
            normal: [0.0, 0.0, -1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: [0.5, 0.5],
            detail_uv: [0.5, 0.5],
        };
        let mut varyings = input.default_varyings();
        if let Some(uv) = native.opaque_uv {
            varyings[3][0] = 0.5 * uv[0] + uv[2];
            varyings[3][1] = 0.5 * uv[1] + uv[3];
        }
        let pixel = Pixel {
            varyings,
            dx: [[0.0; 4]; 9],
            dy: [[0.0; 4]; 9],
            screen: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
            distance: 1.0,
            front: true,
            depth: Depth {
                values: &[],
                color: None,
                normal: None,
                size: [1, 1],
                top: 0,
                scale: 1.0,
            },
            exposure: 1.0,
            irradiance: [0.0, 0.0, 0.0, 1.0],
        };
        shade::outputs(model, material, &frame, &shader::Bindings::default(), pixel)
    });
    text.push_str(&format!(
        ", probe {:?}",
        probe.map(|outputs| [outputs[0], outputs[1], outputs[2]])
    ));
    model.notices.push(text);
}

pub(in crate::model_preview) struct Sample {
    pub surface: shader::Sample,
    pub normal: [f32; 3],
    pub ambient: f32,
    pub coverage: f32,
}

pub(in crate::model_preview) fn sample(
    model: &Model,
    material: &Material,
    constants: &Frame,
    gear: &shader::Bindings<'_>,
    pixel: Pixel<'_>,
) -> Option<Sample> {
    let native = material.native.as_ref().filter(|n| n.deferred)?;
    let mut targets = shade::outputs(model, material, constants, gear, pixel)?;
    if targets[..3].iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let coverage = if native.decal {
        let coverage = (1.0 - targets[0][3]).clamp(0.0, 1.0);
        if coverage <= 1e-8 {
            return Some(decal::empty());
        }
        decal::unpack(&mut targets, pixel, coverage, native.full_decal)?;
        coverage
    } else {
        1.0
    };
    let packed = std::array::from_fn::<_, 3, _>(|i| targets[1][i] - 0.5);
    let radius = packed.iter().map(|v| v * v).sum::<f32>().sqrt();
    if radius < 1e-8 {
        return None;
    }
    let ambient = native.ambient_power.map_or_else(
        || (2.0 * targets[2][1]).clamp(0.0, 1.0) * targets[2][3].clamp(0.0, 1.0),
        |power| ambient_visibility(targets[2][1], targets[2][3], power),
    );
    Some(Sample {
        surface: shader::Sample {
            albedo: std::array::from_fn(|i| targets[0][i].max(0.0)),
            roughness: 1.0 - ((radius - 0.375) * 8.0).clamp(0.0, 1.0),
            metal: targets[2][0].clamp(0.0, 1.0),
            ao: ambient,
            emission: if native.intensity {
                let intensity = opaque::intensity::decode(targets[2][1]);
                std::array::from_fn(|i| targets[0][i].max(0.0) * intensity)
            } else {
                [0.0; 3]
            },
        },
        normal: packed.map(|v| v / radius),
        ambient,
        coverage,
    })
}
