//! Native particle shaders.
//!
//! Sprite, mesh and ribbon vertex shaders read the emitter transform from CB1 in the source
//! game and from CB11 natively. Unlit transparent pixel shaders read the atmosphere at t20/t21
//! where the native transparent scope binds it at t16/t17, blend a second ambient lookup at t13
//! that native particles do not have, and scale emissive color by the frame exposure in
//! `cb13[1].x` where native emissive shaders read `.z`. Those are register changes, applied to
//! the compiled source shaders. Lit transparent shaders need their lighting rewritten and go
//! through the forward-lighting adapter instead.
use super::{Context, Node, dxbc};
use anyhow::{Context as _, Result, ensure};
use std::fs;

pub(super) struct Shader {
    pub symbol: String,
    pub bytecode: Vec<u8>,
    /// Native transparent-advanced scope (CB8) is read.
    pub atmosphere: bool,
    /// TFX appended to the pixel program to bind renderer resources the shader reads.
    pub bindings: Vec<u8>,
    /// Scope bits the rewritten shader requires beyond the material's own.
    pub scopes: u32,
    pub evidence: serde_json::Value,
}

fn source_code(c: &mut Context, header: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    let entry = c
        .source
        .manager
        .get_entry(tiger_pkg::TagHash(header))
        .with_context(|| format!("missing particle shader {header:08X}"))?;
    ensure!(
        entry.file_type == 33,
        "particle shader {header:08X} is not a shader header"
    );
    let header_bytes = c.source.tag(header, None)?.0.clone();
    let code = c.source.tag(c.source.reference(header)?, None)?.0.clone();
    ensure!(
        header_bytes.len() == 40 && code.starts_with(b"DXBC"),
        "particle shader {header:08X} envelope differs"
    );
    Ok((header_bytes, code))
}

fn add(c: &mut Context, source: u32, vertex: bool, bytecode: &[u8]) -> Result<String> {
    let symbol = format!("particle-shader-{source:08X}");
    let template = if vertex {
        &c.templates.vertex
    } else {
        &c.templates.pixel
    };
    let mut header = template.header.clone();
    header[8..12].copy_from_slice(&u32::try_from(bytecode.len())?.to_le_bytes());
    let (header_tag, data_tag) = (template.header_tag, template.data_tag);
    let data_symbol = format!("{symbol}-bytecode");
    c.nodes.add(Node {
        symbol: symbol.clone(),
        template: header_tag,
        payload: header,
        reference: Some(data_symbol.clone()),
        patches: Vec::new(),
    })?;
    c.nodes.add(Node {
        symbol: data_symbol,
        template: data_tag,
        payload: bytecode.to_vec(),
        reference: Some(symbol.clone()),
        patches: Vec::new(),
    })?;
    Ok(symbol)
}

pub(super) fn vertex(c: &mut Context, header: u32) -> Result<Shader> {
    let (_, code) = source_code(c, header)?;
    let (buffers, resources) = dxbc::declarations(&code)?;
    ensure!(
        buffers.contains(&1) && buffers.iter().all(|b| matches!(b, 0 | 1 | 12)),
        "particle vertex shader {header:08X} constant buffers {buffers:?} are unsupported"
    );
    // Ribbons read their particle records through t2, bound by the material program.
    ensure!(
        resources.iter().all(|r| *r == 2),
        "particle vertex shader {header:08X} resources {resources:?} are unsupported"
    );
    let patched = dxbc::patch(
        &code,
        &dxbc::Remap {
            constant_buffers: vec![(1, 11)],
            ..Default::default()
        },
    )?;
    ensure!(
        patched.changes > 1,
        "particle vertex shader transform was not remapped"
    );
    let symbol = if c.nodes.contains(&format!("particle-shader-{header:08X}")) {
        format!("particle-shader-{header:08X}")
    } else {
        add(c, header, true, &patched.bytecode)?
    };
    Ok(Shader {
        symbol,
        bytecode: patched.bytecode,
        atmosphere: false,
        bindings: Vec::new(),
        scopes: 0,
        evidence: serde_json::json!({"source":format!("{header:08X}"),"family":if resources.is_empty() {"sprite"} else {"ribbon"},"remapped_operands":patched.changes}),
    })
}

pub(super) fn pixel(c: &mut Context, header: u32) -> Result<Shader> {
    let (_, code) = source_code(c, header)?;
    let (buffers, resources) = dxbc::declarations(&code)?;
    if buffers.contains(&3) {
        return lit(c, header, &code);
    }
    ensure!(
        buffers.iter().all(|b| matches!(b, 0 | 2 | 8 | 12 | 13)),
        "particle pixel shader {header:08X} constant buffers {buffers:?} are unsupported"
    );
    ensure!(
        resources
            .iter()
            .all(|r| matches!(r, 0..=9 | 10 | 11 | 13 | 20 | 21)),
        "particle pixel shader {header:08X} resources {resources:?} are unsupported"
    );
    ensure!(
        !resources.contains(&13) || resources.contains(&11),
        "particle pixel shader {header:08X} reads t13 without t11"
    );
    let patched = dxbc::patch(
        &code,
        &dxbc::Remap {
            resources: vec![(20, 16), (21, 17)],
            reads: vec![(13, 11)],
            exposure: buffers.contains(&13).then_some((13, 1)),
            ..Default::default()
        },
    )?;
    let symbol = if c.nodes.contains(&format!("particle-shader-{header:08X}")) {
        format!("particle-shader-{header:08X}")
    } else {
        add(c, header, false, &patched.bytecode)?
    };
    Ok(Shader {
        symbol,
        bytecode: patched.bytecode,
        atmosphere: buffers.contains(&8),
        bindings: Vec::new(),
        scopes: 0,
        evidence: serde_json::json!({"source":format!("{header:08X}"),"family":"unlit transparent","remapped_operands":patched.changes}),
    })
}

/// Forward-lit transparent shaders: decompile, adapt the lighting and recompile.
fn lit(c: &mut Context, header: u32, code: &[u8]) -> Result<Shader> {
    let folder = c.work.join("shaders");
    fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{header:08X}.dxbc"));
    fs::write(&path, code)?;
    if !path.with_extension("hlsl").exists() {
        crate::d2_mot::support::decompile(c.request.decompiler, &path)?;
    }
    let source = fs::read_to_string(path.with_extension("hlsl"))?;
    let adapted =
        crate::d2_mot::native::effects::particle::pixel(&source, code, c.request.render_inputs)?;
    fs::write(
        folder.join(format!("{header:08X}-native.hlsl")),
        &adapted.hlsl,
    )?;
    let (bytecode, warnings) = crate::d2_mot::native::shader::compile(&adapted.hlsl, false)?;
    fs::write(folder.join(format!("{header:08X}-native.log")), &warnings)?;
    let symbol = if c.nodes.contains(&format!("particle-shader-{header:08X}")) {
        format!("particle-shader-{header:08X}")
    } else {
        add(c, header, false, &bytecode)?
    };
    Ok(Shader {
        symbol,
        bytecode,
        atmosphere: true,
        bindings: adapted.bindings,
        scopes: u32::try_from(adapted.required_scopes)?,
        evidence: serde_json::json!({"source":format!("{header:08X}"),"family":"lit transparent","recompiled":true}),
    })
}

/// Every pixel interpolant must come from the vertex output with the same semantic, in the same
/// register, within its written components.
pub(super) fn link(vertex: &[u8], pixel: &[u8]) -> Result<()> {
    let outputs = dxbc::signature(vertex, b"OSGN")?;
    for (semantic, index, register, mask) in dxbc::signature(pixel, b"ISGN")? {
        if semantic.starts_with("SV_") {
            continue;
        }
        let output = outputs
            .iter()
            .find(|(s, i, _, _)| *s == semantic && *i == index)
            .with_context(|| format!("pixel input {semantic}{index} has no vertex output"))?;
        ensure!(
            output.2 == register && mask & !output.3 == 0,
            "pixel input {semantic}{index} is packed differently from its vertex output"
        );
    }
    Ok(())
}
