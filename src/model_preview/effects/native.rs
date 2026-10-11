//! Decode bounded native programs and bind their resources to the preview scene.
use super::*;
mod attributes;
mod coverage;
mod cube;
mod derivative;
mod evaluate;
mod gear;
mod glsl;
mod gpu;
mod image;
mod layered;
mod motion;
mod opaque;
mod program;
mod resource;
mod shade;
mod skinning;
mod surface;
mod vertex;

pub(in crate::model_preview) use attributes::{Attributes, load as load_attributes};
pub(in crate::model_preview) use coverage::load as load_cutoff;
pub(in crate::model_preview) use gear::map_transform;
pub(in crate::model_preview) use glsl::HELPERS as GPU_HELPERS;
pub(in crate::model_preview) use gpu::source;
pub(crate) use motion::Motion;
pub(in crate::model_preview) use motion::load as load_motion;
pub(in crate::model_preview) use opaque::intensity::ambient as ambient_visibility;
pub(in crate::model_preview) use opaque::intensity::decode as decode_intensity;
pub(in crate::model_preview) use opaque::{LegacyNormal, load_normal, load_opaque};
pub(in crate::model_preview) use shade::{Depth, Pixel, sample, sample_with_ambient};
pub(in crate::model_preview) use skinning::apply as remap_skin;
pub(in crate::model_preview) use surface::{load as load_surface, sample as sample_surface};
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
    pub(in crate::model_preview) opaque_uv: Option<[f32; 4]>,
    pub(in crate::model_preview) intensity: bool,
    pub(in crate::model_preview) deferred: bool,
    pub(in crate::model_preview) decal: bool,
    pub(in crate::model_preview) ambient_power: Option<f32>,
    base_gain: Option<opaque::Gain>,
    paint: Option<opaque::Paint>,
    pub(in crate::model_preview) bindings: Vec<Binding>,
}

struct Vertex {
    code: program::Program,
    constants: Vec<[f32; 4]>,
    expression: Option<Program>,
    quaternion: bool,
    stored_uv: Option<[f32; 4]>,
    samplers: Vec<texture::Sampler>,
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
    pub vertex: bool,
    pub role: Role,
    pub color: bool,
    pub cube: Option<cube::Cube>,
    pub layered: Option<layered::Layered>,
    /// Native sampler registers start at one. Load-only resources have no sampler.
    pub sampler: Option<usize>,
}

impl Native {
    pub(in crate::model_preview) fn opaque(&self) -> bool {
        self.opaque_uv.is_some() || self.deferred
    }
    pub(in crate::model_preview) fn base_gain(&self, frame: &Frame) -> Option<[f32; 3]> {
        self.base_gain.as_ref()?.value(frame)
    }
    pub(in crate::model_preview) fn paint(&self, frame: &Frame) -> Option<[f32; 2]> {
        self.paint.as_ref()?.value(frame)
    }
    pub(in crate::model_preview) fn base_metal(&self, frame: &Frame) -> Option<f32> {
        self.paint.as_ref()?.metal(frame)
    }
    pub(in crate::model_preview) fn normal(&self, frame: &Frame) -> Option<[[f32; 2]; 4]> {
        self.paint.as_ref()?.normal(frame)
    }
    pub(in crate::model_preview) fn grain(&self, frame: &Frame) -> Option<[f32; 3]> {
        self.paint.as_ref()?.grain(frame)
    }
    pub(in crate::model_preview) fn texture_unit(&self, binding: usize) -> usize {
        binding
            + if self.opaque() && !self.deferred {
                6
            } else {
                0
            }
    }
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
    objects: ObjectInputs<'_>,
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
        contract(&code, true, false)?;
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
        samplers: texture::stage_samplers(manager, bytes, 0x48),
    }))
}

pub(super) fn load(
    manager: &PackageManager,
    tag: u32,
    bytes: &[u8],
    objects: ObjectInputs<'_>,
    surface: u8,
    model: &mut Model,
) -> Result<Material, String> {
    let pixel = program::Program::read(
        &super::read::shader_bytes(manager, u32_at(bytes, 0x2C8)?, 0)?,
        0,
    )?;
    load_program(
        manager,
        tag,
        bytes,
        objects,
        surface,
        model,
        (pixel, false, false),
    )
}

fn load_program(
    manager: &PackageManager,
    _tag: u32,
    bytes: &[u8],
    objects: ObjectInputs<'_>,
    surface: u8,
    model: &mut Model,
    (pixel, opaque, deferred): (program::Program, bool, bool),
) -> Result<Material, String> {
    contract(&pixel, false, deferred)?;
    let globals = crate::dyes::material::global_channels(manager);
    let constants = super::read::stage_constants(manager, bytes, 0x2C8)?;
    check_constants(&pixel, &constants)?;
    let expression = Program::material(bytes, 0x2C8, &globals, objects, surface, constants.len())?;
    let vertex = if opaque {
        None
    } else {
        load_vertex(manager, bytes, &globals, objects, surface)?
    };
    let mut samplers = texture::stage_samplers(manager, bytes, 0x2C8);
    let mut plan = resource::Plan::new(manager, model);
    plan.stage(bytes, &pixel, &samplers, false, 0)?;
    if let Some(vertex) = &vertex
        && vertex.stored_uv.is_none()
    {
        plan.stage(bytes, &vertex.code, &vertex.samplers, true, samplers.len())?;
        samplers.extend_from_slice(&vertex.samplers);
    }
    let (bindings, pending) = plan.finish();
    if opaque && !deferred && bindings.len() > 3 {
        return Err("The opaque effect texture count exceeds preview limits".into());
    }
    let material = Material {
        kind: Kind::Native,
        constants,
        program: expression,
        samplers,
        native: Some(Native {
            pixel,
            vertex,
            opaque_uv: opaque.then_some([1.0, 1.0, 0.0, 0.0]),
            ambient_power: None,
            intensity: false,
            deferred,
            decal: false,
            base_gain: None,
            paint: None,
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
    texture::retain_all(model, pending)?;
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

fn contract(program: &program::Program, vertex: bool, deferred: bool) -> Result<(), String> {
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
    if vertex
        && program
            .instructions
            .iter()
            .any(|i| matches!(i.code, 69 | 108 | 122 | 124))
    {
        return Err("The vertex effect requires pixel derivatives".into());
    }
    if program.resources.iter().any(|r| r.integer && r.slot != 3) {
        return Err("The effect requires an unavailable integer texture".into());
    }
    for semantic in &program.outputs {
        let supported = if vertex {
            (semantic.name == "TEXCOORD" && semantic.index < 9) || semantic.system == 1
        } else {
            semantic.name == "SV_TARGET"
                && semantic.register == semantic.index as usize
                && (semantic.index == 0 || deferred && semantic.index < 3)
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
    for (at, i) in program.instructions.iter().enumerate() {
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
        if matches!(i.code, 122 | 124)
            && (vertex || i.operands[1].kind != 1 && program.derivatives[at].is_none())
        {
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
