//! Float-stream rigid meshes with independently checked renderer scope producers.
//! Package enrollment and in-game producer execution remain separate acceptance.
use super::{identity::Stage, program::Program};
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use tiger_pkg::TagHash;

pub mod material;

#[cfg(test)]
mod tests;

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn bytes(payload: &Payload, at: usize, stride: usize) -> Result<Vec<u8>> {
    let rows = payload.array(at, stride, None)?;
    Ok(rows
        .into_iter()
        .flat_map(|at| payload.0[at..at + stride].iter().copied())
        .collect())
}

fn dynamic(reader: &mut Reader, tag: u32, vectors: usize) -> Result<Value> {
    let entry = reader
        .manager
        .get_entry(TagHash(tag))
        .context("rigid dynamic buffer header")?;
    ensure!(
        entry.file_type == 32 && entry.file_subtype == 7,
        "rigid producer buffer header type differs"
    );
    let reference = entry.reference;
    let other = reader
        .manager
        .get_entry(TagHash(reference))
        .context("rigid dynamic buffer data")?;
    ensure!(
        other.file_type == 40 && other.file_subtype == 7 && other.reference == tag,
        "rigid producer buffer package pair differs"
    );
    let header = reader.tag(tag, None)?;
    let data = reader.tag(reference, None)?;
    ensure!(
        header.0.len() == 16
            && header.u32(0)? as usize == vectors * 16
            && header.u32(4)? == 0
            && header.u32(8)? == 1
            && header.u32(12)? == 0
            && data.0.len() == vectors * 16,
        "rigid producer dynamic buffer layout differs"
    );
    Ok(
        json!({"tag":format!("{tag:08X}"),"data":format!("{reference:08X}"),
        "header_sha256":digest(&header.0),"data_sha256":digest(&data.0),"vectors":vectors}),
    )
}

/// Structural evidence from both actual source and native renderer scope assets.
/// Construction is private so constant-buffer shape alone cannot authorize use.
pub struct Scopes {
    evidence: Value,
}

impl Scopes {
    pub fn read(
        source: &mut Reader,
        native: &mut Reader,
        source_rigid: u32,
        native_rigid: u32,
        source_view: u32,
        native_view: u32,
    ) -> Result<Self> {
        ensure!(
            !source.is_native() && native.is_native(),
            "rigid scope package eras differ"
        );
        let rigid_source = hex::decode("4c080053004b080452044b080552054b080652064b08075207")?;
        let rigid_native = hex::decode("3e080044003d080443043d080543053d080643063d08074307")?;
        let view_source = hex::decode(
            "4c021453004c020c53044a02004a02010c42004a02004a02010c040d52084b020152094b0202520a4c0208530b4b0203520f",
        )?;
        let view_native = hex::decode(
            "3e021244003e020a44043c02003c02010c34003c02003c02010c040d43083d020143093e0206440a",
        )?;
        let mut rows = Vec::new();
        for (tag, modern, index, vectors, slot, expected) in [
            (source_rigid, true, 2, 8, 1, rigid_source),
            (native_rigid, false, 2, 8, 11, rigid_native),
            (source_view, true, 1, 16, 12, view_source),
            (native_view, false, 1, 14, 12, view_native),
        ] {
            let reader = if modern { &mut *source } else { &mut *native };
            let payload = reader.tag(tag, Some(if modern { 0x80806DBA } else { 0x808071F3 }))?;
            ensure!(
                payload.u64(0)? as usize == payload.0.len() && payload.u32(16)? == index,
                "rigid renderer scope envelope or index differs"
            );
            let base = if modern { 0xD0 } else { 0xD8 };
            let flag = base + if modern { 0x5C } else { 0x6C };
            let bind = base + if modern { 0x68 } else { 0x78 };
            let external = base + if modern { 0x6C } else { 0x7C };
            ensure!(
                bytes(&payload, base + 0x18, 1)? == expected
                    && payload.u64(base + 0x48)? as usize == vectors
                    && payload.u32(flag)? == 0x10
                    && payload.u32(bind)? == slot,
                "rigid renderer producer program, writable flag or buffer slot differs"
            );
            ensure!(
                payload.u64(base)? == 0 && payload.u64(base + 0x38)? == 0,
                "rigid renderer producer has unmodeled resources"
            );
            let buffer = dynamic(reader, payload.u32(external)?, vectors)?;
            rows.push(
                json!({"tag":format!("{tag:08X}"),"modern":modern,"index":index,
                "vectors":vectors,"slot":slot,"scope_sha256":digest(&payload.0),
                "program":hex::encode(expected),"buffer":buffer}),
            );
        }
        Ok(Self {
            evidence: json!({"producers":rows,"source_model_slot":1,
            "native_model_slot":11,"source_view_vectors":16,"native_view_vectors":14,
            "cpu_producer_execution_verified":false}),
        })
    }

    pub fn receipt(&self) -> &Value {
        &self.evidence
    }
}

/// The checked 32-byte position/UV/normal stream, separate from packed gear data.
pub struct Stream {
    data: Vec<u8>,
}

impl Stream {
    pub fn read(reader: &mut Reader, tag: u32) -> Result<Self> {
        ensure!(!reader.is_native(), "rigid stream requires source packages");
        let entry = reader
            .manager
            .get_entry(TagHash(tag))
            .context("rigid vertex header")?;
        ensure!(
            entry.file_type == 32 && entry.file_subtype == 4,
            "rigid vertex header type differs"
        );
        let reference = entry.reference;
        let other = reader
            .manager
            .get_entry(TagHash(reference))
            .context("rigid vertex data")?;
        ensure!(
            other.file_type == 40 && other.file_subtype == 4 && other.reference == tag,
            "rigid vertex package pair differs"
        );
        let header = reader.tag(tag, None)?;
        let data = reader.tag(reference, None)?;
        ensure!(
            header.0.len() == 12
                && header.u16(4)? == 32
                && header.u16(6)? == 0
                && header.u32(8)? == 0xDEADBEEF
                && header.u32(0)? as usize == data.0.len()
                && !data.0.is_empty()
                && data.0.len().is_multiple_of(32),
            "rigid float stream layout differs"
        );
        for at in (0..data.0.len()).step_by(4) {
            data.f32(at)?;
        }
        for at in (0..data.0.len()).step_by(32) {
            ensure!(
                (0..3)
                    .map(|lane| data.f32(at + 20 + lane * 4).map(|v| v * v))
                    .collect::<Result<Vec<_>>>()?
                    .iter()
                    .sum::<f32>()
                    > 0.,
                "rigid normal is degenerate"
            );
        }
        Ok(Self {
            data: data.0.clone(),
        })
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn native_layout(&self) -> i16 {
        13
    }
}

pub struct Rigid {
    program: Program,
    source: Vec<u8>,
    evidence: Value,
}

impl Rigid {
    /// Compile a dedicated rigid shader. Callers must run the configured GPU
    /// oracle against the original executable before complete graph enrollment.
    pub fn read(
        reader: &mut Reader,
        material: u32,
        hlsl: &str,
        scopes: &Scopes,
        stream: &Stream,
    ) -> Result<Self> {
        ensure!(
            !reader.is_native(),
            "rigid material requires source packages"
        );
        let payload = reader.tag(material, Some(0x80806DAA))?;
        ensure!(
            payload.0.len() >= 0x3D0
                && payload.u64(0)? as usize == payload.0.len()
                && payload.u32(8)? == 1
                && payload.u32(32)? == 7
                && payload.u32(40)? == 7
                && payload.u64(0x38)? == 0,
            "rigid material scope selection differs"
        );
        for at in [0x78, 0x90, 0xA0, 0xB0, 0xC0] {
            ensure!(
                payload.u64(at)? == 0,
                "rigid vertex material has additional bindings"
            );
        }
        ensure!(
            matches!(payload.u32(0xE4)?, 0 | u32::MAX | 0x811C9DC5),
            "rigid vertex material has external constants"
        );
        let shader = payload.u32(0x70)?;
        let entry = reader
            .manager
            .get_entry(TagHash(shader))
            .context("rigid source shader")?;
        ensure!(
            entry.file_type == 33 && entry.file_subtype == 1,
            "rigid shader package stage differs"
        );
        let reference = entry.reference;
        let other = reader
            .manager
            .get_entry(TagHash(reference))
            .context("rigid source bytecode")?;
        ensure!(
            other.file_type == 41 && other.file_subtype == 1 && other.reference == shader,
            "rigid shader package pair differs"
        );
        let header = reader.tag(shader, None)?;
        let source = reader.tag(reference, None)?.0.clone();
        ensure!(
            header.0.len() == 40
                && header.u64(0)? == 40
                && header.u32(8)? as usize == source.len()
                && header.0[16..].iter().all(|v| *v == 0),
            "rigid source shader header differs"
        );
        let original = Program::new(Stage::Vertex, source.clone())?;
        let inspected = original.inspect()?.inputs;
        ensure!(
            inspected.constant_buffers == BTreeMap::from([(1, 8), (12, 15)])
                && inspected.resources.is_empty()
                && inspected.samplers.is_empty()
                && inspected.unordered_access.is_empty(),
            "rigid vertex executable bindings differ"
        );
        for (&slot, reads) in &inspected.constant_reads {
            let allowed: BTreeSet<u32> = if slot == 1 {
                (0..8).collect()
            } else {
                BTreeSet::from([0, 1, 2, 7, 14])
            };
            ensure!(
                reads
                    .iter()
                    .all(|r| r.index.is_some_and(|i| allowed.contains(&i))),
                "rigid executable has an unmodeled producer field"
            );
        }
        ensure!(
            inspected.inputs == ["dcl_input v0.xyz", "dcl_input v1.xyz", "dcl_input v3.xy"]
                && inspected.outputs
                    == [
                        "dcl_output o0.xyzw",
                        "dcl_output o1.xyzw",
                        "dcl_output o2.xyz",
                        "dcl_output_siv o3.xyzw, position"
                    ],
            "rigid executable float stream or output contract differs"
        );
        let hlsl = hlsl.replace("\r\n", "\n");
        ensure!(
            hlsl.matches("cbuffer cb1 : register(b1)").count() == 1
                && hlsl.matches("float4 cb1[8];").count() == 1
                && hlsl.matches("float4 cb12[15];").count() == 1
                && hlsl.contains("cb12[14].xyzw")
                && !hlsl.contains("needs manual fix"),
            "rigid decompiled declaration form differs"
        );
        let text = hlsl
            .replace("cbuffer cb1 : register(b1)", "cbuffer cb11 : register(b11)")
            .replace("cb1[", "cb11[")
            .replace("float4 cb12[15];", "float4 cb12[14];")
            .replace("cb12[14].xyzw", "cb12[13].xyzw");
        let (code, diagnostics) = super::compile(&text, true)?;
        ensure!(
            diagnostics.trim().is_empty(),
            "rigid shader compiler diagnostics: {diagnostics}"
        );
        let program = Program::new(Stage::Vertex, code)?;
        let target = program.inspect()?.inputs;
        ensure!(
            target.constant_buffers == BTreeMap::from([(11, 8), (12, 14)])
                && target.resources.is_empty()
                && target.samplers.is_empty()
                && target.unordered_access.is_empty(),
            "compiled rigid shader target bindings differ"
        );
        for (&slot, reads) in &target.constant_reads {
            let allowed: BTreeSet<u32> = if slot == 11 {
                (0..8).collect()
            } else {
                BTreeSet::from([0, 1, 2, 7, 13])
            };
            ensure!(
                reads
                    .iter()
                    .all(|r| r.index.is_some_and(|i| allowed.contains(&i))),
                "compiled rigid shader has an unmodeled native producer field"
            );
        }
        let evidence = json!({"material":format!("{material:08X}"),"source_shader":format!("{shader:08X}"),
            "source_header_sha256":digest(&header.0),"source_bytecode_sha256":digest(&source),
            "source_identity":format!("{:08X}",header.u32(12)?),"source_identity_resolved":false,
            "source_hlsl_sha256":digest(hlsl.as_bytes()),"adapted_hlsl_sha256":digest(text.as_bytes()),
            "scope_producers":scopes.receipt(),"native_input_layout":stream.native_layout(),"vertex_stride":32,
            "vertex_data_sha256":digest(stream.data()),
            "program":program.receipt(),"gpu_equivalence_required":true,
            "package_enrolled":false,"gameplay_verified":false});
        Ok(Self {
            program,
            source,
            evidence,
        })
    }
    pub fn program(&self) -> &Program {
        &self.program
    }
    pub fn source_bytecode(&self) -> &[u8] {
        &self.source
    }
    pub fn receipt(&self) -> &Value {
        &self.evidence
    }
}
