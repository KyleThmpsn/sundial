//! Source particle shader arithmetic with explicit native scene and resource bindings.
use crate::{
    d1_roi::{Source, expression, shader},
    presentation::{append, put},
    tiger::{
        payload::Payload,
        reader::Reader,
        shader::{dxbc, identity::Stage, program::Program},
    },
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
pub mod assets;
mod ribbon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    VertexShader,
    PixelShader,
    Texture,
    Sampler,
}

pub struct Reference {
    pub offset: usize,
    pub source: u32,
    pub kind: Kind,
}

pub struct Shader {
    pub source: u32,
    pub stage: Stage,
    pub bytecode: Vec<u8>,
    pub constant_source: Option<u32>,
}

/// A material fragment whose resource words are still symbolic obligations. The containing
/// system must match its vertex signature to the definition and translate every resource.
pub struct Material {
    pub payload: Payload,
    pub references: Vec<Reference>,
    pub shaders: Vec<Shader>,
    /// Minimum bytes that the containing definition must publish to the emitter packet.
    pub emitter_bytes: usize,
    /// Native appearance rows after pairing with a source definition. An unpaired fragment
    /// cannot establish how its input signature relates to the particle record stride.
    pub appearance_rows: Option<u8>,
    pub ribbon: bool,
    pub(super) definition: Option<u32>,
}

fn emitter_bytes(code: &[u8]) -> Result<usize> {
    let mut needed = 0;
    let mut at = 0;
    while let Some(&op) = code.get(at) {
        let count = expression::operand_bytes(op)?;
        let args = code
            .get(at + 1..at + 1 + count)
            .context("Truncated particle material expression")?;
        if let [24, row] = args {
            let end = match op {
                0x3C => (usize::from(*row) + 1) * 4,
                0x3D => (usize::from(*row) + 1) * 16,
                _ => 0,
            };
            needed = needed.max(end);
        }
        at += 1 + count;
    }
    Ok(needed)
}

fn vectors(source: &Payload, at: usize) -> Result<Vec<u8>> {
    Ok(source
        .array(at, 16, Some(0x80800090))?
        .into_iter()
        .map(|row| source.bytes::<16>(row))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Qualify actual reads separately from declarations, whose unused rows may be larger.
/// The material's own constants are supplied by its translated expression and initial bank.
/// `advanced` is the advanced transparent buffer slot when the material requests that scope.
/// Source pixel programs read its rows 4, 6 and 7, which the two builds' producers fill alike
/// for corresponding scene inputs. Native row 5 holds a scene vector where the source writes
/// zero, so a read of it is refused.
fn scene_inputs(
    code: &[u8],
    stage: Stage,
    values: usize,
    transparent: bool,
    advanced: Option<u32>,
) -> Result<()> {
    let range = dxbc::chunk(code, b"SHEX")?
        .or(dxbc::chunk(code, b"SHDR")?)
        .context("Particle shader has no instruction program")?;
    let words = code[range]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte word")))
        .collect::<Vec<_>>();
    for instruction in dxbc::walk(&words)? {
        for operand in instruction.operands {
            if operand.kind != 8 {
                continue;
            }
            ensure!(
                operand.indices.len() == 2,
                "Particle constant-buffer dimensions differ"
            );
            let slot =
                words[operand.indices[0].context("Relative particle constant-buffer slot")?];
            let row = words[operand.indices[1].context("Relative particle scene-vector row")?];
            let declared = instruction.opcode == 0x59;
            let admitted = match (stage, slot) {
                (_, 0) => {
                    usize::try_from(row)? < values || declared && usize::try_from(row)? <= values
                }
                (Stage::Vertex, 11) => row < 7 || declared && row <= 8,
                (Stage::Vertex, 12) => row < 8 || declared && row <= 10,
                (Stage::Pixel, 12) => matches!(row, 7 | 12) || declared && row <= 14,
                (Stage::Pixel, 13) => row == 1 || declared && row <= 4,
                (Stage::Pixel, 9) if transparent => {
                    matches!(row, 0 | 2..=5) || declared && row <= 6
                }
                (Stage::Pixel, slot) if advanced == Some(slot) => {
                    matches!(row, 4 | 6 | 7) || declared && row <= 8
                }
                _ => false,
            };
            ensure!(
                admitted,
                "Particle shader reads an unqualified scene buffer {slot} row {row}"
            );
        }
    }
    Ok(())
}

impl Source {
    /// Pair source simulation storage with its render inputs. Ribbon materials fetch one
    /// appearance record for each pair of vertices through the native draw buffer.
    pub fn particle_material_for(
        &self,
        tag: u32,
        definition: u32,
        native: &mut Reader,
    ) -> Result<Material> {
        ensure!(
            self.reference(definition)? == 0x80801A6A,
            "Particle material definition class differs"
        );
        let source = self.read(definition)?;
        ensure!(
            source.0.len() >= 0x138 && source.u64(0)? == source.0.len() as u64,
            "Particle material definition extent differs"
        );
        let mut material = self.particle_material(tag, native)?;
        ribbon::bind(&mut material, &source)?;
        material.definition = Some(definition);
        self.unchanged()?;
        Ok(material)
    }

    /// Preserve a CPU particle material's SM5 operations and stage interfaces. Its scene inputs
    /// come from the native `frame`, `view`, `rigid_model`, `transparent` and
    /// `transparent_advanced` producers. Object inputs and additional shader stages require their
    /// own producer contracts. This does not allocate package entries or certify a complete
    /// particle draw.
    pub fn particle_material(&self, tag: u32, native: &mut Reader) -> Result<Material> {
        ensure!(
            self.reference(tag)? == 0x80801C32,
            "Source particle material class differs"
        );
        let source = self.read(tag)?;
        ensure!(
            source.0.len() >= 0x348 && source.u64(0)? == source.0.len() as u64,
            "Source particle material extent differs"
        );
        let globals = expression::Globals::read(self, native)?;
        for from in [0xC8, 0x168, 0x208] {
            ensure!(
                matches!(source.u32(from)?, 0 | u32::MAX),
                "Particle material has an unsupported shader stage"
            );
        }
        let mut output = Payload(vec![0; 0x408]);
        put(&mut output.0, 8, &source.bytes::<32>(8)?)?;
        put(
            &mut output.0,
            12,
            &(source.u32(12)? & 0x00FF_FFFF).to_le_bytes(),
        )?;
        output.0[0x27] = 0;
        // `frame`, `view` and `rigid_model` are ordinals 0 to 2 in both builds.
        let qualified = 0b111 | 1 << globals.depth.index | 1 << globals.advanced.index;
        for at in [0x18, 0x1C] {
            let mask = source.u32(at)?;
            ensure!(
                mask & !qualified == 0,
                "Particle material needs an unqualified render scope ({mask:08X})"
            );
            let mapped = globals.scope_mask(mask)?;
            put(
                &mut output.0,
                at,
                &(mapped | if at == 0x1C { 0x80000000 } else { 0 }).to_le_bytes(),
            )?;
        }
        for at in [0x28, 0x368, 0x3E8, 0x3EC] {
            put(&mut output.0, at, &u32::MAX.to_le_bytes())?;
        }
        for to in [0x48, 0xE8, 0x188, 0x228, 0x2C8] {
            for at in [to, to + 0x80, to + 0x84] {
                put(&mut output.0, at, &u32::MAX.to_le_bytes())?;
            }
        }
        let transparent = source.u32(0x1C)? & (1 << globals.depth.index) != 0;
        let advanced = (source.u32(0x1C)? & (1 << globals.advanced.index) != 0)
            .then_some(globals.advanced.slot);
        let mut references = Vec::new();
        let mut shaders = Vec::new();
        let mut packet_bytes = 0;
        for (from, to, stage, packed, kind) in [
            (0x28, 0x48, Stage::Vertex, 2, Kind::VertexShader),
            (0x2A8, 0x2C8, Stage::Pixel, 1, Kind::PixelShader),
        ] {
            let shader_tag = source.u32(from)?;
            ensure!(
                !matches!(shader_tag, 0 | u32::MAX),
                "Particle material is missing a shader"
            );
            ensure!(
                source.u32(from + 0x78)? == 0,
                "Particle material object inputs need a bound producer"
            );
            let textures = stage_textures(&source, from)?;
            let samplers = source
                .array(from + 0x48, 16, Some(0x80801ACC))?
                .into_iter()
                .map(|at| source.u32(at))
                .collect::<Result<Vec<_>>>()?;
            let constant_source = match source.u32(from + 0x84)? {
                0 | u32::MAX => None,
                value => Some(value),
            };
            let values = if let Some(external) = constant_source {
                self.constant_buffer(external)?.1
            } else {
                vectors(&source, from + 0x58)?
            };
            let range = source.array_range(from + 0x28, 1, Some(0x80800009))?;
            packet_bytes = packet_bytes.max(emitter_bytes(&source.0[range.clone()])?);
            let plates = std::array::from_fn(|_| String::new());
            let detail = std::array::from_fn(|_| String::new());
            let mut expression = expression::particle(
                &source.0[range],
                vectors(&source, from + 0x38)?,
                &expression::Inputs {
                    globals: &globals,
                    dye: expression::Dye::Unresolved,
                    plates: &plates,
                    detail: &detail,
                    objects: 0,
                    object_channels: &[],
                    outputs: values.len() / 16,
                    samplers: samplers.len(),
                    stage: packed,
                    reticle: false,
                    light: false,
                },
            )?;
            ensure!(
                expression.textures.is_empty() && expression.reticle_mask.is_none(),
                "Particle material requests an unqualified private texture producer"
            );
            expression.initialize(&values)?;
            let shader_source = self.read(shader_tag)?;
            let shader_range = shader_source.array_range(8, 1, Some(0x80800009))?;
            let code = &shader_source.0[shader_range];
            scene_inputs(code, stage, values.len() / 16, transparent, advanced)?;
            let mut bytecode = shader::compact(code)?;
            if stage == Stage::Pixel {
                bytecode = shader::light_samples(&bytecode, &globals.light_samples)?;
                if transparent {
                    bytecode = shader::depth(&bytecode, &globals.depth)?;
                }
                bytecode = shader::bloom_limit(&bytecode)?;
            }
            Program::new(stage, bytecode.clone())?;
            shaders.push(Shader {
                source: shader_tag,
                stage,
                bytecode,
                constant_source,
            });
            references.push(Reference {
                offset: to,
                source: shader_tag,
                kind,
            });
            write_stage_resources(
                &mut output,
                to,
                (textures, samplers),
                (&expression.code, &expression.constants, &values),
                &mut references,
            )?;
        }
        self.unchanged()?;
        Ok(Material {
            payload: output,
            references,
            shaders,
            emitter_bytes: packet_bytes,
            appearance_rows: None,
            ribbon: false,
            definition: None,
        })
    }
}

type ShaderResources = (BTreeMap<u32, u32>, Vec<u32>);

fn write_stage_resources(
    output: &mut Payload,
    to: usize,
    (textures, samplers): ShaderResources,
    (code, constants, values): (&[u8], &[u8], &[u8]),
    references: &mut Vec<Reference>,
) -> Result<()> {
    let rows = textures
        .keys()
        .flat_map(|slot| slot.to_le_bytes().into_iter().chain(u32::MAX.to_le_bytes()))
        .collect::<Vec<_>>();
    append(&mut output.0, to + 8, 0x80807211, &rows, 8)?;
    for (row, resource) in output
        .array(to + 8, 8, Some(0x80807211))?
        .into_iter()
        .zip(textures.values())
    {
        references.push(Reference {
            offset: row + 4,
            source: *resource,
            kind: Kind::Texture,
        });
    }
    let mut hash = 0xCBF29CE484222325u64;
    for (slot, resource) in textures {
        for byte in slot.to_le_bytes().into_iter().chain(resource.to_le_bytes()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001B3);
        }
    }
    put(&mut output.0, to + 0x18, &hash.to_le_bytes())?;
    append(&mut output.0, to + 0x20, 0x80800009, code, 1)?;
    append(&mut output.0, to + 0x30, 0x80800090, constants, 16)?;
    let rows = samplers
        .iter()
        .flat_map(|_| u32::MAX.to_le_bytes().into_iter().chain([0; 12]))
        .collect::<Vec<_>>();
    append(&mut output.0, to + 0x40, 0x808073F3, &rows, 16)?;
    for (row, resource) in output
        .array(to + 0x40, 16, Some(0x808073F3))?
        .into_iter()
        .zip(samplers)
    {
        references.push(Reference {
            offset: row,
            source: resource,
            kind: Kind::Sampler,
        });
    }
    append(&mut output.0, to + 0x50, 0x80800090, values, 16)?;
    output.0[to + 0x74] = if values.is_empty() { 0 } else { 0x10 };
    put(
        &mut output.0,
        to + 0x80,
        &(if values.is_empty() { u32::MAX } else { 0 }).to_le_bytes(),
    )?;
    Ok(())
}

fn stage_textures(source: &Payload, from: usize) -> Result<BTreeMap<u32, u32>> {
    let mut textures = BTreeMap::new();
    for row in source.array(from + 0x10, 8, Some(0x80801B28))? {
        let value = source.u32(row + 4)?;
        if !matches!(value, 0 | u32::MAX) {
            ensure!(
                textures.insert(source.u32(row)?, value).is_none(),
                "Particle texture slot is duplicated"
            );
        }
    }
    Ok(textures)
}
