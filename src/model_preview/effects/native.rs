//! Decode bounded native programs and bind their resources to the preview scene.
use super::*;
mod cube;
mod evaluate;
mod gear;
mod glsl;
mod gpu;
mod motion;
mod program;
mod shade;
mod vertex;

pub(in crate::model_preview) use gear::map_transform;
pub(in crate::model_preview) use gpu::source;
pub(crate) use motion::Motion;
pub(in crate::model_preview) use motion::load as load_motion;
pub(in crate::model_preview) use shade::{Depth, Pixel, sample};
pub(in crate::model_preview) use vertex::{Input, Varyings};

impl Native {
    pub(in crate::model_preview) fn remap_stored_uv(
        &mut self,
        model_uv: [f32; 4],
    ) -> Result<(), String> {
        let Some(uv) = self.vertex.as_mut().and_then(|v| v.stored_uv.as_mut()) else {
            return Ok(());
        };
        if model_uv.iter().any(|v| !v.is_finite()) || model_uv[..2].iter().any(|v| v.abs() < 1e-8) {
            return Err("The stored vertex texture placement is not invertible".into());
        }
        for lane in 0..2 {
            uv[lane] /= model_uv[lane];
            uv[lane + 2] -= model_uv[lane + 2] * uv[lane];
        }
        if uv.iter().all(|v| v.is_finite()) {
            Ok(())
        } else {
            Err("The stored vertex texture placement exceeds preview limits".into())
        }
    }
}

pub(in crate::model_preview) struct Triangle {
    pub values: [Varyings; 3],
    pub dx: Varyings,
    pub dy: Varyings,
}

impl Triangle {
    pub(in crate::model_preview) fn new(values: [Varyings; 3], points: [[f32; 3]; 3]) -> Self {
        let x = [points[1][0] - points[0][0], points[2][0] - points[0][0]];
        let y = [points[1][1] - points[0][1], points[2][1] - points[0][1]];
        let det = x[0] * y[1] - x[1] * y[0];
        let gradient = |vertical: bool| {
            std::array::from_fn(|r| {
                std::array::from_fn(|lane| {
                    let a = values[1][r][lane] - values[0][r][lane];
                    let b = values[2][r][lane] - values[0][r][lane];
                    if det.abs() < 1e-8 {
                        0.0
                    } else if vertical {
                        (x[0] * b - x[1] * a) / det
                    } else {
                        (a * y[1] - b * y[0]) / det
                    }
                })
            })
        };
        Self {
            values,
            dx: gradient(false),
            dy: gradient(true),
        }
    }
    pub(in crate::model_preview) fn at(&self, b: f32, c: f32) -> Varyings {
        std::array::from_fn(|r| {
            std::array::from_fn(|i| {
                self.values[0][r][i]
                    + b * (self.values[1][r][i] - self.values[0][r][i])
                    + c * (self.values[2][r][i] - self.values[0][r][i])
            })
        })
    }
}

pub(in crate::model_preview) fn tangent(
    tangents: &[[f32; 4]],
    index: usize,
    normal: [f32; 3],
) -> [f32; 4] {
    let stored = tangents.get(index).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let n = shader::normal::normalize(normal).unwrap_or([0.0, 0.0, 1.0]);
    let amount = (0..3).map(|i| stored[i] * n[i]).sum::<f32>();
    let direction = shader::normal::normalize(std::array::from_fn(|i| stored[i] - amount * n[i]));
    let t = direction.unwrap_or_else(|| {
        shader::normal::normalize(if n[2].abs() < 0.9 {
            [-n[1], n[0], 0.0]
        } else {
            [n[2], 0.0, -n[0]]
        })
        .unwrap_or([1.0, 0.0, 0.0])
    });
    [t[0], t[1], t[2], if stored[3] < 0.0 { -1.0 } else { 1.0 }]
}

pub(crate) struct Native {
    pixel: program::Program,
    vertex: Option<Vertex>,
    pub(in crate::model_preview) bindings: Vec<Binding>,
}

struct Vertex {
    code: program::Program,
    constants: Vec<[f32; 4]>,
    expression: Option<Program>,
    quaternion: bool,
    stored_uv: Option<[f32; 4]>,
    stored_color: Option<[f32; 4]>,
}

#[derive(Clone, Copy)]
pub(in crate::model_preview) enum Role {
    Texture(usize),
    Albedo,
    Normal,
    Gear,
    Detail,
    DetailNormal,
    Depth,
    Mask,
    /// Absent atmospheric lookup, occlusion and volume inputs in the studio scene.
    Scene,
}

pub(in crate::model_preview) struct Binding {
    pub slot: usize,
    pub role: Role,
    pub color: bool,
    pub cube: Option<cube::Cube>,
    /// Native sampler registers start at one. Load-only resources have no sampler.
    pub sampler: Option<usize>,
}

impl Native {
    pub(in crate::model_preview) fn animated(&self) -> bool {
        self.vertex
            .as_ref()
            .and_then(|v| v.expression.as_ref())
            .is_some_and(Program::animated)
    }
    pub(in crate::model_preview) fn remap_textures(&mut self, indices: &[Option<usize>]) {
        for binding in &mut self.bindings {
            if let Role::Texture(index) = binding.role {
                binding.role =
                    Role::Texture(indices.get(index).copied().flatten().unwrap_or(usize::MAX));
            }
        }
    }

    pub(in crate::model_preview) fn vertex_frame(&self, seconds: f32) -> Option<Frame> {
        let mut output = [[0.0; 4]; 128];
        if let Some(vertex) = &self.vertex {
            output[..vertex.constants.len()].copy_from_slice(&vertex.constants);
            if let Some(expression) = &vertex.expression {
                expression
                    .run_shader_into(&mut output[..vertex.constants.len()], seconds)
                    .ok()?;
            }
        }
        Some(output)
    }
}

/// The material's vertex stage, when it names one: its code, constants and expression, and
/// whether it deforms stored geometry.
fn load_vertex(
    manager: &PackageManager,
    bytes: &[u8],
    globals: &[[f32; 4]],
    objects: &[[f32; 4]],
    surface: u8,
) -> Result<Option<Vertex>, String> {
    let vertex_tag = u32_at(bytes, 0x48)?;
    if matches!(vertex_tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::read::shader_bytes(manager, vertex_tag, 1)?;
    let code = program::Program::read_stored(&raw)?;
    let stored_uv = if motion::stored(&code) {
        Some(
            gear::stored_vertex_uv(&code)
                .ok_or("The stored vertex texture coordinates are unavailable")?,
        )
    } else {
        contract(&code, true)?;
        None
    };
    let stored_color = if stored_uv.is_some() {
        gear::stored_vertex_color(manager, bytes, &code)?
    } else {
        None
    };
    let constants = super::read::stage_constants(manager, bytes, 0x48)?;
    check_constants(&code, &constants)?;
    let expression = Program::material(bytes, 0x48, globals, objects, surface, constants.len())?;
    // Native matrix palettes multiply indices by three. Dual-quaternion palettes
    // shift by one and consume quaternion pairs, including the single-bone form.
    let quaternion = code.instructions.iter().any(|i| i.code == 41);
    Ok(Some(Vertex {
        code,
        constants,
        expression,
        quaternion,
        stored_uv,
        stored_color,
    }))
}

/// The texture each slot the material binds explicitly, refusing a slot bound twice.
fn explicit_textures(bytes: &[u8]) -> Result<std::collections::BTreeMap<usize, u32>, String> {
    let (count, rows) = super::vertex::table(bytes, 0x2D0, 0x8080_7211, 8, 32)?;
    let mut explicit = std::collections::BTreeMap::new();
    for row in (0..count).map(|i| rows + i * 8) {
        if explicit
            .insert(u32_at(bytes, row)? as usize, u32_at(bytes, row + 4)?)
            .is_some()
        {
            return Err("The effect binds a texture slot more than once".into());
        }
    }
    Ok(explicit)
}

pub(super) fn load(
    manager: &PackageManager,
    tag: u32,
    bytes: &[u8],
    objects: &[[f32; 4]],
    surface: u8,
    model: &mut Model,
) -> Result<Material, String> {
    let pixel = program::Program::read(
        &super::read::shader_bytes(manager, u32_at(bytes, 0x2C8)?, 0)?,
        0,
    )?;
    contract(&pixel, false)?;
    let globals = crate::dyes::material::global_channels(manager);
    let constants = super::read::stage_constants(manager, bytes, 0x2C8)?;
    check_constants(&pixel, &constants)?;
    let expression = Program::material(bytes, 0x2C8, &globals, objects, surface, constants.len())?;
    let vertex = load_vertex(manager, bytes, &globals, objects, surface)?;
    let samplers = texture::material_samplers(manager, tag);
    let explicit = explicit_textures(bytes)?;
    let dye = pixel
        .buffers
        .iter()
        .find(|(slot, _)| (5..=7).contains(slot))
        .map(|(slot, _)| *slot);
    let mut bindings = Vec::new();
    let mut pending = Vec::new();
    for resource in &pixel.resources {
        let sampling = sampling_for(&pixel, resource, samplers.len())?;
        let (role, color, cube) = if let Some(&tag) = explicit.get(&resource.slot) {
            let header = manager.read_tag(tag)?;
            let color = matches!(u32_at(&header, 4)?, 29 | 72 | 75 | 78 | 91 | 93 | 99);
            let (cube, image) = if resource.dimension == 6 {
                let (cube, image) = cube::Cube::load(manager, tag)?;
                (Some(cube), image)
            } else {
                (None, texture::load(manager, tag)?)
            };
            if resource.integer {
                return Err("Integer effect images are not supported".into());
            }
            let index = if let Some(index) = model
                .textures
                .iter()
                .chain(&pending)
                .position(|t| t.tag == tag)
            {
                index
            } else {
                if model.textures.len() + pending.len() >= MAX_TEXTURES {
                    return Err("The preview texture budget is full".into());
                }
                pending.push(image);
                model.textures.len() + pending.len() - 1
            };
            (Role::Texture(index), color, cube)
        } else {
            if matches!((resource.slot, resource.dimension), (15 | 16, 3) | (17, 5))
                && pixel.buffers.contains(&(12, 13))
                && pixel.buffers.contains(&(13, 2))
            {
                bindings.push(Binding {
                    slot: resource.slot,
                    role: Role::Scene,
                    color: false,
                    cube: None,
                    sampler: sampling,
                });
                continue;
            }
            if resource.dimension != 3 {
                return Err("The reflection texture is missing".into());
            }
            let detail = dye.map(|slot| 3 + (7 - slot) * 2);
            let role = match resource.slot {
                3 if resource.integer => Role::Mask,
                0 => Role::Albedo,
                1 => Role::Normal,
                2 => Role::Gear,
                10 => Role::Depth,
                slot if Some(slot) == detail => Role::Detail,
                slot if Some(slot) == detail.map(|v| v + 1) => Role::DetailNormal,
                _ => {
                    return Err(format!(
                        "The effect texture slot {} has no preview binding",
                        resource.slot
                    ));
                }
            };
            (role, matches!(role, Role::Albedo | Role::Detail), None)
        };
        bindings.push(Binding {
            slot: resource.slot,
            role,
            color,
            cube,
            sampler: sampling,
        });
    }
    if bindings.len() > 9 {
        return Err("The effect texture count exceeds preview limits".into());
    }
    let material = Material {
        kind: Kind::Native,
        constants,
        program: expression,
        samplers,
        native: Some(Native {
            pixel,
            vertex,
            bindings,
        }),
        ..Default::default()
    };
    material
        .frame(0.0)
        .ok_or("The effect expression cannot initialize")?;
    material
        .native
        .as_ref()
        .unwrap()
        .vertex_frame(0.0)
        .ok_or("The effect vertex expression cannot initialize")?;
    model.textures.extend(pending);
    Ok(material)
}

fn check_constants(program: &program::Program, constants: &[[f32; 4]]) -> Result<(), String> {
    if program
        .buffers
        .iter()
        .any(|&(slot, count)| slot == 0 && count > constants.len())
    {
        return Err("The effect constant buffer is incomplete".into());
    }
    Ok(())
}

fn contract(program: &program::Program, vertex: bool) -> Result<(), String> {
    if program
        .instructions
        .iter()
        .any(|i| program::arity(i.code).is_none())
    {
        return Err("The effect requires an unavailable shader instruction".into());
    }
    if program
        .instructions
        .iter()
        .filter(|i| i.code == 108)
        .count()
        > 1
    {
        return Err("The effect uses more than one dependent reflection footprint".into());
    }
    for &(slot, count) in &program.buffers {
        let limit = match (vertex, slot) {
            (_, 0) => 128,
            (true, 11) => 24,
            (true, 12) => 14,
            (false, 2) => 1,
            (false, 5..=7) => 27,
            (false, 8) => 8,
            (false, 12) => 13,
            (false, 13) => 2,
            _ => return Err(format!("The effect constant scope {slot} is unavailable")),
        };
        if count > limit {
            return Err("The effect constant scope exceeds its preview contract".into());
        }
    }
    if vertex && !program.resources.is_empty() {
        return Err("Vertex texture sampling is not available".into());
    }
    if program.resources.iter().any(|r| r.integer && r.slot != 3) {
        return Err("The effect requires an unavailable integer texture".into());
    }
    for semantic in &program.outputs {
        let supported = if vertex {
            (semantic.name == "TEXCOORD" && semantic.index < 9) || semantic.system == 1
        } else {
            semantic.name == "SV_TARGET" && semantic.index == 0 && semantic.register == 0
        };
        if !supported {
            return Err("The effect output signature is unavailable".into());
        }
    }
    for semantic in &program.inputs {
        let supported = if vertex {
            match semantic.name.as_str() {
                "POSITION" | "NORMAL" | "TANGENT" | "COLOR" | "BLENDINDICES" | "BLENDWEIGHT" => {
                    semantic.index == 0
                }
                "TEXCOORD" => matches!(semantic.index, 0 | 2),
                _ => false,
            }
        } else {
            (semantic.name == "TEXCOORD" && semantic.index < 9) || matches!(semantic.system, 1 | 9)
        };
        if !supported {
            return Err(format!(
                "The effect input {}{} is unavailable",
                semantic.name, semantic.index
            ));
        }
    }
    if vertex
        && !program
            .outputs
            .iter()
            .any(|s| s.name == "TEXCOORD" && s.index == 4)
    {
        return Err("The effect has no stored-space position output".into());
    }
    for i in &program.instructions {
        if i.code == 61 && (i.operands[1].kind != 4 || i.operands[1].literal != [0; 4]) {
            return Err("Only base-level effect texture dimensions are available".into());
        }
        if i.code == 108 {
            let slot = i.operands[2].indices[0].base as usize;
            if program
                .resources
                .iter()
                .find(|r| r.slot == slot)
                .is_none_or(|r| r.dimension != 6)
            {
                return Err("The effect requires an unavailable texture footprint".into());
            }
        }
        if i.code == 38 && i.operands[0].kind != 13 {
            return Err("The effect requires a high integer product".into());
        }
        if matches!(i.code, 122 | 124) && (vertex || i.operands[1].kind != 1) {
            return Err("The effect requires an unavailable derivative".into());
        }
        if vertex && i.code == 13 {
            return Err("Vertex discard is invalid".into());
        }
    }
    Ok(())
}

fn sampling_for(
    pixel: &program::Program,
    resource: &program::Resource,
    sampler_count: usize,
) -> Result<Option<usize>, String> {
    let mut sampling = None;
    for instruction in &pixel.instructions {
        if matches!(instruction.code, 69 | 72 | 73 | 108)
            && instruction.operands[2].indices[0].base as usize == resource.slot
        {
            let sampler = instruction.operands[3].indices[0].base as usize;
            if sampler == 0 || sampler > sampler_count {
                return Err("The effect sampler is missing".into());
            }
            if sampling
                .replace(sampler - 1)
                .is_some_and(|old| old != sampler - 1)
            {
                return Err("The effect samples one image with conflicting samplers".into());
            }
        }
    }
    Ok(sampling)
}
