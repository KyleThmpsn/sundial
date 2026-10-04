//! Native rigid material carriers and source plans with explicit unresolved fields.
//! There is deliberately no executable material emitter before root flags resolve.
use super::{Rigid, bytes, digest};
use crate::d2_mot::native::shader::{
    buffer,
    identity::Stage,
    program::{Program, Template as ShaderTemplate},
};
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

fn inactive(payload: &Payload, base: usize, modern: bool) -> Result<()> {
    let stride = if modern { 144 } else { 160 };
    let mut expected = vec![0; stride];
    expected[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let external = if modern { 0x74 } else { 0x84 };
    expected[external..external + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    ensure!(
        payload.0.get(base..base + stride) == Some(expected.as_slice()),
        "rigid material has an additional shader stage or stage metadata"
    );
    Ok(())
}

fn pixel_contract(program: &Program) -> Result<()> {
    let inputs = program.inspect()?.inputs;
    ensure!(
        program.stage() == Stage::Pixel
            && inputs.constant_buffers == BTreeMap::from([(0, 1)])
            && inputs.resources.is_empty()
            && inputs.samplers.is_empty()
            && inputs.unordered_access.is_empty()
            && inputs.inputs == ["dcl_input_ps linear v0.xyzw"]
            && inputs.outputs
                == [
                    "dcl_output o0.xyzw",
                    "dcl_output o1.xyzw",
                    "dcl_output o2.xyzw"
                ],
        "rigid pixel executable has a different constant or three-target contract"
    );
    ensure!(
        inputs
            .constant_reads
            .get(&0)
            .is_some_and(|rows| rows.iter().all(|row| row.index == Some(0))),
        "rigid pixel executable reads an unavailable color vector"
    );
    Ok(())
}

pub struct Template {
    tag: u32,
    vertex: ShaderTemplate,
    pixel: ShaderTemplate,
    color: buffer::Template,
    evidence: Value,
}

impl Template {
    /// Validate a real native three-target rigid material and all GPU templates.
    /// This does not authorize mapping source packed root flags to this carrier.
    pub fn read(reader: &mut Reader, tag: u32) -> Result<Self> {
        ensure!(
            reader.is_native(),
            "rigid material template requires native packages"
        );
        let payload = reader.tag(tag, Some(0x808071E8))?;
        ensure!(
            payload.0.len() >= 1032
                && payload.u64(0)? as usize == payload.0.len()
                && payload.u32(8)? == 1
                && payload.u32(12)? == 0
                && payload.u64(16)? == 0
                && payload.u32(24)? == 7
                && payload.u32(28)? == 0x80000007
                && payload.u32(32)? == 0
                && payload.u32(36)? == 0x007F7F00
                && payload.u32(40)? == u32::MAX
                && payload.u32(44)? == 0
                && payload.0[48..72].iter().all(|v| *v == 0),
            "native rigid material root envelope differs"
        );
        for base in [0xE8, 0x188, 0x228, 0x368] {
            inactive(&payload, base, false)?;
        }
        for base in [0x48, 0x2C8] {
            for offset in [8, 0x20, 0x30, 0x40] {
                ensure!(
                    payload.u64(base + offset)? == 0,
                    "native rigid material has additional resources or expressions"
                );
            }
            ensure!(
                payload.u32(base + 0x70)? == 0 && payload.u32(base + 0x74)? == 0,
                "native rigid material requires dynamic material writes"
            );
        }
        ensure!(
            payload.u64(0x98)? == 0
                && payload.u32(0xC8)? == u32::MAX
                && payload.u32(0xCC)? == u32::MAX
                && payload.u64(0x318)? == 1
                && payload.u32(0x348)? == 0,
            "native rigid material constant selection differs"
        );
        let vertex = ShaderTemplate::read(reader, payload.u32(0x48)?)?;
        let pixel = ShaderTemplate::read(reader, payload.u32(0x2C8)?)?;
        ensure!(
            vertex.stage() == Stage::Vertex && pixel.stage() == Stage::Pixel,
            "native rigid material shader stages differ"
        );
        let vertex_program = Program::new(
            Stage::Vertex,
            reader.tag(vertex.bytecode_tag(), None)?.0.clone(),
        )?;
        let inputs = vertex_program.inspect()?.inputs;
        ensure!(
            inputs.constant_buffers == BTreeMap::from([(11, 8), (12, 14)])
                && inputs.resources.is_empty()
                && inputs.samplers.is_empty()
                && inputs.unordered_access.is_empty(),
            "native rigid material vertex producer contract differs"
        );
        let pixel_program = Program::new(
            Stage::Pixel,
            reader.tag(pixel.bytecode_tag(), None)?.0.clone(),
        )?;
        pixel_contract(&pixel_program)?;
        let color_tag = payload.u32(0x34C)?;
        let color = buffer::Template::read(reader, color_tag)?;
        let values = buffer::Buffer::from_package(reader, color_tag)?;
        ensure!(
            values.vectors() == 1,
            "native rigid material color extent differs"
        );
        let evidence = json!({"tag":format!("{tag:08X}"),"native_class":"808071E8",
            "source_template_sha256":digest(&payload.0),"scope_masks":["00000007","80000007"],
            "packed_root":"007F7F00","state":"00000000","inline_vector":hex::encode(bytes(&payload, 0x318, 16)?),
            "vertex":vertex_program.receipt(),"pixel":pixel_program.receipt(),"color":values.receipt(),
            "color_header":format!("{color_tag:08X}"),"color_bits":hex::encode(values.data())});
        Ok(Self {
            tag,
            vertex,
            pixel,
            color,
            evidence,
        })
    }
    pub fn receipt(&self) -> &Value {
        &self.evidence
    }
    pub fn vertex_template(&self) -> &ShaderTemplate {
        &self.vertex
    }
    pub fn pixel_template(&self) -> &ShaderTemplate {
        &self.pixel
    }
    pub fn color_template(&self) -> &buffer::Template {
        &self.color
    }
    pub fn tag(&self) -> u32 {
        self.tag
    }
}

pub struct Plan {
    pixel: Program,
    color: buffer::Buffer,
    evidence: Value,
}

impl Plan {
    pub fn read(
        reader: &mut Reader,
        material: u32,
        rigid: &Rigid,
        template: &Template,
    ) -> Result<Self> {
        ensure!(
            !reader.is_native(),
            "rigid material plan requires source packages"
        );
        ensure!(
            rigid.receipt()["material"].as_str() == Some(format!("{material:08X}").as_str()),
            "rigid vertex receipt belongs to another material"
        );
        let payload = reader.tag(material, Some(0x80806DAA))?;
        ensure!(
            payload.0.len() >= 976
                && payload.u64(0)? as usize == payload.0.len()
                && payload.u32(8)? == 1
                && payload.u32(32)? == 7
                && payload.u32(40)? == 7
                && payload.u32(48)? == 0
                && payload.u64(0x38)? == 0,
            "source rigid material root or render state differs"
        );
        for base in [0x100, 0x190, 0x220, 0x340] {
            inactive(&payload, base, true)?;
        }
        for offset in [8, 0x20, 0x30, 0x40] {
            ensure!(
                payload.u64(0x2B0 + offset)? == 0,
                "source rigid pixel material has additional bindings or expressions"
            );
        }
        ensure!(
            payload.u64(0x300)? == 1
                && payload.u32(0x310)? == 0
                && payload.u32(0x314)? == 0
                && payload.u32(0x320)? == 0,
            "source rigid pixel constant selection differs"
        );
        let shader = payload.u32(0x2B0)?;
        let entry = reader
            .manager
            .get_entry(tiger_pkg::TagHash(shader))
            .context("rigid source pixel header")?;
        ensure!(
            entry.file_type == 33 && entry.file_subtype == 0,
            "rigid source pixel stage differs"
        );
        let reference = entry.reference;
        let other = reader
            .manager
            .get_entry(tiger_pkg::TagHash(reference))
            .context("rigid source pixel data")?;
        ensure!(
            other.file_type == 41 && other.file_subtype == 0 && other.reference == shader,
            "rigid source pixel package pair differs"
        );
        let header = reader.tag(shader, None)?;
        let code = reader.tag(reference, None)?.0.clone();
        ensure!(
            header.0.len() == 40
                && header.u64(0)? == 40
                && header.u32(8)? as usize == code.len()
                && header.0[16..].iter().all(|v| *v == 0),
            "rigid source pixel header differs"
        );
        let pixel = Program::new(Stage::Pixel, code)?;
        pixel_contract(&pixel)?;
        let color = buffer::Buffer::from_package(reader, payload.u32(0x324)?)?;
        ensure!(color.vectors() == 1, "rigid source color extent differs");
        let evidence = json!({"source":format!("{material:08X}"),"source_class":"80806DAA",
            "source_payload_sha256":digest(&payload.0),"source_payload":hex::encode(&payload.0),
            "source_pixel_header":hex::encode(&header.0),"source_pixel_header_sha256":digest(&header.0),
            "source_pixel_identity":format!("{:08X}",header.u32(12)?),"source_identity_resolved":false,
            "pixel":pixel.receipt(),"color":color.receipt(),"color_bits":hex::encode(color.data()),
            "inline_vector":hex::encode(bytes(&payload, 0x300, 16)?),"rigid":rigid.receipt(),
            "native_template":template.receipt(),"material_conversion_complete":false,
            "unresolved":[{"field":"packed_root","source_offset":76,
                "source":format!("{:08X}",payload.u32(76)?),"native_offset":36,"native":"007F7F00"},
                {"field":"pixel_scope_high_bit","source_mask":"00000007","native_mask":"80000007"}],
            "package_enrolled":false,"gameplay_verified":false});
        Ok(Self {
            pixel,
            color,
            evidence,
        })
    }
    pub fn pixel(&self) -> &Program {
        &self.pixel
    }
    pub fn color(&self) -> &buffer::Buffer {
        &self.color
    }
    pub fn receipt(&self) -> &Value {
        &self.evidence
    }
}
