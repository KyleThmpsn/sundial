//! Native particle materials (class 808071E8) from source materials (80806DAA).
//!
//! The layout follows 1,331 twin materials compiled for both games. Bind mode, render state
//! and stage programs carry over. A native stage is the source stage with sixteen bytes added
//! before its object count, eight-byte texture rows naming tags, sampler rows naming native
//! resources naming native samplers or textures, and the renderer's extern inputs renumbered:
//! the particle draw inputs move from
//! extern 27 to 26 and the emitter vectors from extern 26 to 25.
use super::{Context, Node, array, shader, texture};
use crate::d2_mot::{particles::renderer::Material, payload::Payload, tfx};
use anyhow::{Context as _, Result, ensure};
use std::collections::BTreeMap;

mod resources;

const SOURCE_STAGES: [usize; 6] = [0x70, 0x100, 0x190, 0x220, 0x2B0, 0x340];
const NATIVE_STAGES: [usize; 6] = [0x48, 0xE8, 0x188, 0x228, 0x2C8, 0x368];
const NATIVE_SIZE: usize = 0x408;
const FNV64_BASIS: u64 = 0xCBF29CE484222325;

pub(super) struct Prepared {
    source: Payload,
    /// Complete numeric consumer closure after the other shader stages have been refused.
    pub stages: [Material; 2],
}

struct Stage {
    textures: Vec<(u32, String)>,
    code: Vec<u8>,
    tfx_constants: Vec<u8>,
    resources: resources::Resources,
    values: Vec<u8>,
    writes: bool,
    matrix_evidence: Vec<serde_json::Value>,
}

fn vectors(p: &Payload, field: usize) -> Result<Vec<u8>> {
    Ok(p.array(field, 16, Some(0x80800090))?
        .into_iter()
        .flat_map(|at| p.0[at..at + 16].iter().copied())
        .collect())
}

/// Renderer externs the source programs read, renumbered for the native renderer.
fn externs(
    code: &[u8],
    bindings: &mut tfx::program::Bindings,
    render_inputs: &std::path::Path,
) -> Result<Vec<serde_json::Value>> {
    let mut evidence = Vec::new();
    for instruction in tfx::program::parse(code)? {
        match (instruction.op, instruction.args) {
            (0x4C, [8, 0]) => {
                evidence.push(crate::d2_mot::native::shader::packed::rigid_matrix(
                    render_inputs,
                )?);
                bindings
                    .external_textures
                    .insert([0x4C, 8, 0], [0x3E, 8, 0]);
            }
            (0x4A, [27, offset]) => {
                bindings
                    .external_textures
                    .insert([0x4A, 27, *offset], [0x3C, 26, *offset]);
            }
            (0x4B, [26, vector]) => {
                ensure!(
                    *vector < 10,
                    "particle material exceeds the native emitter packet"
                );
                bindings
                    .external_textures
                    .insert([0x4B, 26, *vector], [0x3D, 25, *vector]);
            }
            (0x4A, [26, scalar]) => {
                ensure!(
                    *scalar < 40,
                    "particle material exceeds the native emitter packet"
                );
                bindings
                    .external_textures
                    .insert([0x4A, 26, *scalar], [0x3C, 25, *scalar]);
            }
            (0x4D, [27, 0]) => {
                bindings
                    .external_textures
                    .insert([0x4D, 27, 0], [0x3F, 26, 0]);
            }
            // A vertex-stage buffer binding keeps the slot the shader declares.
            (0x57, [slot]) => {
                ensure!(
                    *slot >> 5 == 2,
                    "particle buffer binding is not a vertex resource"
                );
                bindings.texture_slots.insert(*slot, *slot);
            }
            _ => {}
        }
    }
    Ok(evidence)
}

fn stage(
    c: &mut Context,
    source: &Payload,
    base: usize,
    expression: &Material,
    vertex: bool,
    extra: &[u8],
) -> Result<Stage> {
    let mut textures = Vec::new();
    for row in source.array(base + 8, 24, None)? {
        let slot = source.u32(row)?;
        let tag = c.source.ref64(source, row + 8)?;
        textures.push((slot, texture::convert(c, tag)?));
    }
    let resources = resources::Resources::read(c, source, base + 0x40)?;
    let code = &expression.code;
    let mut tfx_constants = expression.constants.concat();
    let external = source.u32(base + 0x74)?;
    let values = if [0, u32::MAX, 0x811C9DC5].contains(&external) {
        vectors(source, base + 0x50)?
    } else {
        // An immutable constant buffer holds the stage's values in place of the inline table.
        let header = c.source.tag(external, None)?;
        ensure!(
            header.0.len() == 16 && header.u64(8)? == 0,
            "particle material constant buffer is not immutable"
        );
        let data = c.source.tag(c.source.reference(external)?, None)?;
        ensure!(
            data.0.len().is_multiple_of(16) && header.u32(0)? as usize == data.0.len(),
            "particle material constant buffer size differs"
        );
        data.0.clone()
    };
    ensure!(
        source.u32(base + 0x60)? == 0,
        "particle material reads object channels"
    );
    let mut bindings = tfx::program::Bindings {
        constant_count: tfx_constants.len() / 16,
        output_count: values.len() / 16,
        sampler_count: resources.tags.len(),
        sampler_stage: Some(if vertex { 2 } else { 1 }),
        globals: c.globals.clone(),
        ..Default::default()
    };
    resources.bind(code, &mut bindings, &mut tfx_constants)?;
    let matrix_evidence = externs(code, &mut bindings, c.request.render_inputs)?;
    let lowered = tfx::program::lower(code, &bindings)
        .with_context(|| format!("particle material stage {base:X} program"))?;
    lowered.require_runtime_inputs()?;
    resources.validate_samplers(&lowered)?;
    let writes = lowered.evidence.iter().any(|row| row["translated"] == true);
    let mut code = lowered.code;
    code.extend_from_slice(extra);
    Ok(Stage {
        textures,
        code,
        tfx_constants,
        resources,
        values,
        writes,
        matrix_evidence,
    })
}

fn unused(source: &Payload, base: usize) -> Result<()> {
    ensure!(
        source.u32(base)? == u32::MAX,
        "particle material uses another shader stage"
    );
    for field in [8, 0x20, 0x30, 0x40, 0x50] {
        ensure!(
            source.u64(base + field)? == 0,
            "unused particle material stage carries data"
        );
    }
    Ok(())
}

fn write_stage(
    out: &mut Vec<u8>,
    base: usize,
    stage: &Stage,
    identity: u64,
    patches: &mut Vec<(usize, String)>,
) -> Result<()> {
    let rows = stage
        .textures
        .iter()
        .flat_map(|(slot, _)| slot.to_le_bytes().into_iter().chain(u32::MAX.to_le_bytes()))
        .collect::<Vec<_>>();
    let start = array(out, base + 8, 0x80807211, &rows, 8)?;
    for (index, (_, symbol)) in stage.textures.iter().enumerate() {
        patches.push((start + index * 8 + 4, symbol.clone()));
    }
    let hash = if stage.textures.is_empty() {
        FNV64_BASIS
    } else {
        identity
    };
    out[base + 0x18..base + 0x20].copy_from_slice(&hash.to_le_bytes());
    array(out, base + 0x20, 0x80800009, &stage.code, 1)?;
    array(out, base + 0x30, 0x80800090, &stage.tfx_constants, 16)?;
    let resources = stage
        .resources
        .tags
        .iter()
        .flat_map(|tag| tag.to_le_bytes().into_iter().chain([0; 12]))
        .collect::<Vec<_>>();
    let start = array(out, base + 0x40, 0x808073F3, &resources, 16)?;
    for (index, symbol) in &stage.resources.patches {
        patches.push((start + index * 16, symbol.clone()));
    }
    array(out, base + 0x50, 0x80800090, &stage.values, 16)?;
    // Native flag 0x10 requests writable constant storage before the program runs.
    out[base + 0x74] = if stage.values.is_empty() {
        0
    } else if stage.writes {
        0x10
    } else {
        0
    };
    let present = if stage.values.is_empty() { u32::MAX } else { 0 };
    out[base + 0x80..base + 0x84].copy_from_slice(&present.to_le_bytes());
    out[base + 0x84..base + 0x88].copy_from_slice(&u32::MAX.to_le_bytes());
    Ok(())
}

pub(super) fn prepare(c: &mut Context, tag: u32) -> Result<Prepared> {
    let source = c.source.tag(tag, Some(0x80806DAA))?;
    let source = Payload(source.0.clone());
    ensure!(
        source.u64(0)? == source.0.len() as u64,
        "particle material size differs"
    );
    ensure!(
        source.0[0x10..0x20].iter().all(|v| *v == 0)
            && source.0[0x34..0x48].iter().all(|v| *v == 0)
            && source.u32(0x48)? == u32::MAX
            && source.u32(0x4C)? == 0x017F7FFF
            && source.u32(0x50)? == u32::MAX
            && source.0[0x54..0x70].iter().all(|v| *v == 0),
        "particle material header differs from the inspected form"
    );
    for (index, base) in SOURCE_STAGES.iter().enumerate() {
        if index != 0 && index != 4 {
            unused(&source, *base)?;
        }
    }
    let scopes = source.u64(0x20)?;
    ensure!(
        source.u64(0x28)? == scopes
            && ((scopes >> 32) & !0x40) == 0
            && (scopes as u32 & !(0x7 | (1 << 13) | (1 << 14))) == 0,
        "particle material scopes {scopes:016X} are uninspected"
    );
    let word = source.u32(0x0C)?;
    ensure!(
        matches!(word >> 24, 8 | 12),
        "particle material stage word differs"
    );
    let expression = |base: usize| -> Result<Material> {
        Ok(Material {
            code: source
                .array(base + 0x20, 1, Some(0x80800009))?
                .into_iter()
                .map(|at| source.u8(at))
                .collect::<Result<_>>()?,
            constants: source
                .array(base + 0x30, 16, Some(0x80800090))?
                .into_iter()
                .map(|at| source.bytes(at))
                .collect::<Result<_>>()?,
        })
    };
    let stages = [expression(0x70)?, expression(0x2B0)?];
    Ok(Prepared { source, stages })
}

pub(super) fn convert(
    c: &mut Context,
    tag: u32,
    prepared: &Prepared,
    symbol: &str,
) -> Result<serde_json::Value> {
    let source = &prepared.source;
    let scopes = source.u64(0x20)?;
    let word = source.u32(0x0C)?;
    let vertex = shader::vertex(c, source.u32(0x70)?)?;
    let pixel = shader::pixel(c, source.u32(0x2B0)?)?;
    shader::link(&vertex.bytecode, &pixel.bytecode)?;
    let vertex_stage = stage(c, source, 0x70, &prepared.stages[0], true, &vertex.bindings)?;
    let pixel_stage = stage(
        c,
        source,
        0x2B0,
        &prepared.stages[1],
        false,
        &pixel.bindings,
    )?;
    let mut out = vec![0; NATIVE_SIZE];
    out[8..12].copy_from_slice(&source.u32(8)?.to_le_bytes());
    // The stage word's top byte is twice the native value in every twin.
    out[0x0C..0x10]
        .copy_from_slice(&((word & 0x00FF_FFFF) | (((word >> 24) / 2) << 24)).to_le_bytes());
    let mut native_scopes = scopes as u32 & !(1 << 14);
    if pixel.atmosphere {
        native_scopes |= 1 << 14;
    }
    native_scopes |= pixel.scopes;
    out[0x18..0x1C].copy_from_slice(&native_scopes.to_le_bytes());
    out[0x1C..0x20].copy_from_slice(&(native_scopes | 0x8000_0000).to_le_bytes());
    out[0x20..0x24].copy_from_slice(&source.u32(0x30)?.to_le_bytes());
    out[0x24..0x28].copy_from_slice(&0x007F_7F00u32.to_le_bytes());
    out[0x28..0x2C].copy_from_slice(&u32::MAX.to_le_bytes());
    for (index, base) in NATIVE_STAGES.iter().enumerate() {
        out[*base..*base + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        if index != 0 && index != 4 {
            out[base + 0x80..base + 0x84].copy_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    let mut patches = vec![(0x48, vertex.symbol.clone()), (0x2C8, pixel.symbol.clone())];
    // Variants of one source material keep its texture set even when their numeric programs differ.
    let identity = 0x5A5A_0000_0000_0000 | u64::from(tag);
    write_stage(&mut out, 0x48, &vertex_stage, identity, &mut patches)?;
    write_stage(&mut out, 0x2C8, &pixel_stage, identity ^ 0x4, &mut patches)?;
    let size = out.len() as u64;
    out[0..8].copy_from_slice(&size.to_le_bytes());
    let evidence = serde_json::json!({
        "source": format!("{tag:08X}"),
        "symbol": symbol,
        "vertex": vertex.evidence,
        "pixel": pixel.evidence,
        "scopes": format!("{native_scopes:08X}"),
        "matrix_providers": [vertex_stage.matrix_evidence, pixel_stage.matrix_evidence],
        "textures": pixel_stage.textures.iter().map(|(slot, s)| serde_json::json!({"slot":slot,"symbol":s})).collect::<Vec<_>>(),
        "resources": pixel_stage.resources.tags.iter().enumerate().map(|(index, tag)| serde_json::json!({
            "index": index,
            "tag": format!("{tag:08X}"),
            "symbol": pixel_stage.resources.patches.iter().find(|(i, _)| *i == index).map(|(_, symbol)| symbol),
        })).collect::<Vec<_>>(),
    });
    c.nodes.add(Node {
        symbol: symbol.to_owned(),
        template: c.templates.material,
        payload: out,
        reference: None,
        patches,
    })?;
    Ok(evidence)
}

/// Native global channel indices by source index, matched by channel name.
pub(super) fn globals(render_inputs: &std::path::Path) -> Result<BTreeMap<u8, u8>> {
    let read = |era: &str| -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&std::fs::read(
            render_inputs.join(format!("tfx-{era}/context.json")),
        )?)?)
    };
    let (modern, native) = (read("modern")?, read("native")?);
    let native_by_hash = native["channels"]
        .as_array()
        .context("native render channels")?
        .iter()
        .map(|row| {
            Ok((
                row["hash"]
                    .as_str()
                    .context("native channel hash")?
                    .to_owned(),
                u8::try_from(row["index"].as_u64().context("native channel index")?)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut result = BTreeMap::new();
    for row in modern["channels"]
        .as_array()
        .context("source render channels")?
    {
        let hash = row["hash"].as_str().context("source channel hash")?;
        if let Some(native) = native_by_hash.get(hash) {
            result.insert(
                u8::try_from(row["index"].as_u64().context("source channel index")?)?,
                *native,
            );
        }
    }
    Ok(result)
}
