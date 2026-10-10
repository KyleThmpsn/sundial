//! Plan and emit a particle program together with every material consumer of its workspace.
use super::{Context, Node, material, program};
use crate::d2_mot::{
    particles::{renderer, system::System},
    payload::Payload,
};
use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) struct Converted {
    pub program: String,
    pub material: String,
    pub material_evidence: Value,
    pub workspace: Value,
}

pub(super) fn convert(c: &mut Context, system: &System) -> Result<Converted> {
    // A single CPU system has one render material. GPU and additional shader consumers
    // must be rejected before the paired rewrite can claim a complete consumer closure.
    ensure!(
        system.compute.iter().all(Option::is_none) && system.gpu_binding.is_none(),
        "particle workspace conversion requires a complete CPU material closure"
    );
    let source = c.source.tag(system.program, Some(0x80806927))?.0.clone();
    let mut program = program::prepare(&source, c.request.inputs)
        .with_context(|| format!("particle program {:08X}", system.program))?;
    let mut material = material::prepare(c, system.material)
        .with_context(|| format!("particle material {:08X}", system.material))?;
    let before = program
        .workspace_bytes
        .context("particle workspace is missing")?;
    let compacted = before > 160;
    let workspace = if compacted {
        let rewritten = renderer::compact_source(&program, &material.stages)
            .with_context(|| format!("particle workspace {:08X}", system.program))?;
        let evidence = json!({
            "source_bytes": before,
            "native_bytes": rewritten.program.native_workspace_bytes()?,
            "registers": rewritten.registers,
            "moved_vectors": rewritten.expressions.keys().copied().collect::<Vec<_>>(),
        });
        program = rewritten.program;
        material.stages = rewritten
            .materials
            .try_into()
            .map_err(|_| anyhow::anyhow!("particle material stage closure changed"))?;
        evidence
    } else {
        json!({"source_bytes": before, "native_bytes": program.native_workspace_bytes()?,
            "registers": (0..before / 16).collect::<Vec<_>>(), "moved_vectors": []})
    };
    // A shared source material can consume several source programs. Their rewritten
    // layouts need distinct nodes, even when their shaders and textures are shared.
    let program_symbol = if compacted {
        format!(
            "particle-program-{:08X}-material-{:08X}",
            system.program, system.material
        )
    } else {
        format!("particle-program-{:08X}", system.program)
    };
    let material_symbol = if compacted {
        format!(
            "particle-material-{:08X}-program-{:08X}",
            system.material, system.program
        )
    } else {
        format!("particle-material-{:08X}", system.material)
    };
    if !c.nodes.contains(&program_symbol) {
        // Several source programs intentionally share a seed identity. Each private
        // program needs a stable identity for its own emitted layout.
        let digest = Sha256::digest(program_symbol.as_bytes());
        let identity = u32::from_le_bytes(digest[..4].try_into()?);
        // Private compiled identities sit above the runtime package range and outside
        // the stock identity range. Refuse a collision within the emitted graph.
        let identity = 0x8280_0000 | (identity & 0x007F_FFFF);
        for node in &c.nodes.list {
            if node.template == c.templates.program {
                ensure!(
                    Payload(node.payload.clone()).u32(0xF8)? != identity,
                    "converted particle programs share a compiled identity"
                );
            }
        }
        let payload = program::native(&source, &program, identity)?;
        c.nodes.add(Node {
            symbol: program_symbol.clone(),
            template: c.templates.program,
            payload,
            reference: None,
            patches: Vec::new(),
        })?;
    }
    let material_evidence = if c.nodes.contains(&material_symbol) {
        json!({"source": format!("{:08X}", system.material), "symbol": material_symbol, "shared": true})
    } else {
        material::convert(c, system.material, &material, &material_symbol)
            .with_context(|| format!("particle material {:08X}", system.material))?
    };
    Ok(Converted {
        program: program_symbol,
        material: material_symbol,
        material_evidence,
        workspace,
    })
}
