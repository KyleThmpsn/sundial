//! The native ribbon path consumes float4 records through an SRV, independently
//! of the mesh vertex streams. Keep the source record indexing and equations.
use super::Material;
use crate::d2_mot::{native::shader::replace_once, tfx::program};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Serialize)]
pub struct Vertex {
    pub hlsl: String,
    pub material: Material,
    pub buffer_slot: u8,
    pub output_vectors: usize,
}

fn count(text: &str, slot: u8) -> Result<usize> {
    let prefix = format!("cbuffer cb{slot} : register(b{slot})\n{{\n  float4 cb{slot}[");
    let tail = text
        .split_once(&prefix)
        .context("ribbon constant buffer declaration")?
        .1;
    Ok(tail
        .split_once("];\n}")
        .context("ribbon constant buffer extent")?
        .0
        .parse()?)
}

/// Adapt the buffer-fed ribbon family to native material bindings. This does
/// not serialize a particle system or establish its buffer allocation. The
/// caller must provide the converted program's matching float4 record layout.
///
/// Shipped native ribbon materials use extern 26 for the SRV and draw inputs,
/// extern 25 for the emitter vectors, and CB11 for the four transform rows.
/// Source ribbons use externs 27/26 and CB1 respectively. Both versions retain
/// the same five draw-input offsets, float4 SRV indexing and eight-row CB12.
pub fn ribbon_vertex(source: &str, material: &Material) -> Result<Vertex> {
    let source = source.replace("\r\n", "\n");
    ensure!(
        count(&source, 1)? == 4 && count(&source, 12)? == 8,
        "unsupported ribbon transform or view buffer"
    );
    let output_vectors = count(&source, 0)?;
    ensure!(
        (1..=256).contains(&output_vectors) && material.constants.len() <= 256,
        "ribbon material constant capacity"
    );
    ensure!(
        !source.contains("register(b11)") && !source.contains("cb11["),
        "ribbon native transform slot is already occupied"
    );
    ensure!(
        source.matches("cbuffer ").count() == 3 && source.contains(": SV_VERTEXID"),
        "unsupported ribbon shader interface"
    );
    let mut slots = Vec::new();
    let instructions = program::parse(&material.code)?;
    ensure!(
        instructions.len() >= 2 && instructions.len() % 2 == 0,
        "incomplete ribbon binding pairs"
    );
    ensure!(
        instructions[0].op == 0x4D && instructions[1].op == 0x57,
        "ribbon resource binding must precede draw inputs"
    );
    for pair in instructions[2..].chunks_exact(2) {
        ensure!(
            pair[0].op == 0x4A && pair[1].op == 0x52,
            "unsupported ribbon draw-input expression"
        );
    }
    let mut bindings = program::Bindings {
        constant_count: material.constants.len(),
        output_count: output_vectors,
        ..Default::default()
    };
    for instruction in instructions {
        let i = instruction;
        match i.op {
            0x4D => {
                ensure!(i.args == [27, 0], "unsupported ribbon resource provider");
                bindings
                    .external_textures
                    .insert([0x4D, 27, 0], [0x3F, 26, 0]);
            }
            0x57 => {
                ensure!(i.args[0] >> 5 == 2, "ribbon buffer is not a vertex binding");
                let slot = i.args[0] & 31;
                slots.push(slot);
                bindings.texture_slots.insert(i.args[0], i.args[0]);
            }
            0x4A => {
                ensure!(
                    i.args[0] == 27 && matches!(i.args[1], 69 | 74..=77),
                    "unmapped ribbon draw input"
                );
                bindings
                    .external_textures
                    .insert([i.op, 27, i.args[1]], [0x3C, 26, i.args[1]]);
            }
            0x52 => {}
            _ => anyhow::bail!("unsupported ribbon binding operation {:02X}", i.op),
        }
    }
    let [buffer_slot] = slots.as_slice() else {
        anyhow::bail!("ribbon needs one particle buffer");
    };
    ensure!(
        source.contains(&format!(
            "Buffer<float4> t{buffer_slot} : register(t{buffer_slot});"
        )) && source.matches("register(t").count() == 1,
        "ribbon buffer declaration differs"
    );
    let lowered = program::lower(&material.code, &bindings)?;
    lowered.require_runtime_inputs()?;
    let hlsl = replace_once(
        &source,
        "cbuffer cb1 : register(b1)\n{\n  float4 cb1[4];\n}",
        "cbuffer cb11 : register(b11)\n{\n  float4 cb11[4];\n}",
    )?
    .replace("cb1[", "cb11[");
    // The decompiler widens this three-lane output to float4. Preserve the
    // source DXBC signature rather than introduce an uninitialized fourth lane.
    ensure!(
        source.matches("o2.").count() == 1 && source.contains("o2.xyz ="),
        "unsupported ribbon position varying"
    );
    let hlsl = replace_once(
        &hlsl,
        "out float4 o2 : TEXCOORD2",
        "out float3 o2 : TEXCOORD2",
    )?;
    Ok(Vertex {
        hlsl,
        material: Material {
            code: lowered.code,
            constants: material.constants.clone(),
        },
        buffer_slot: *buffer_slot,
        output_vectors,
    })
}
