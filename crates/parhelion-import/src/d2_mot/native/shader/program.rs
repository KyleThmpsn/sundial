//! Author native GPU resources from independently validated SM5 programs.
//!
//! This constructs a fresh native resource. It does not translate a source
//! material, renderer identity, constant buffer producer or shader input layout.
use super::identity::{self, Stage};
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tiger_pkg::TagHash;

#[cfg(test)]
mod tests;

pub struct Program {
    stage: Stage,
    header: Payload,
    bytecode: Vec<u8>,
}

pub struct Template {
    stage: Stage,
    header: u32,
    bytecode: u32,
}

/// Two fresh graph nodes with reciprocal package references. The shared private
/// package allocator assigns their final tags when the complete graph is built.
pub struct Assets {
    pub header: Vec<u8>,
    pub bytecode: Vec<u8>,
    pub header_template: u32,
    pub bytecode_template: u32,
    pub nodes: Vec<Value>,
}

impl Template {
    pub fn read(reader: &mut Reader, tag: u32) -> Result<Self> {
        ensure!(
            reader.is_native(),
            "shader templates require native packages"
        );
        let entry = reader
            .manager
            .get_entry(TagHash(tag))
            .context("missing shader template")?;
        ensure!(
            entry.file_type == 33,
            "shader template must be a GPU header"
        );
        let stage = Stage::from_subtype(entry.file_subtype)?;
        let bytecode = entry.reference;
        let data = reader
            .manager
            .get_entry(TagHash(bytecode))
            .context("missing shader template data")?;
        ensure!(
            data.file_type == 41 && data.file_subtype == entry.file_subtype,
            "shader template data type or stage differs"
        );
        ensure!(
            data.reference == tag,
            "shader template references are not reciprocal"
        );
        let header = reader.tag(tag, None)?;
        let code = reader.tag(bytecode, None)?;
        identity::validate(stage, &header, &code.0)?;
        Ok(Self {
            stage,
            header: tag,
            bytecode,
        })
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn header_tag(&self) -> u32 {
        self.header
    }

    pub fn bytecode_tag(&self) -> u32 {
        self.bytecode
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub stage: Stage,
    pub header_type: u8,
    pub bytecode_type: u8,
    pub subtype: u8,
    pub header_sha256: String,
    pub bytecode_sha256: String,
}

impl Program {
    /// Keep the complete GPU program and construct its native allocation header.
    /// Callers retain source metadata and resolve material inputs separately.
    pub fn new(stage: Stage, bytecode: Vec<u8>) -> Result<Self> {
        let size = u32::try_from(bytecode.len()).context("GPU program exceeds native length")?;
        let mut header = vec![0; 40];
        header[..8].copy_from_slice(&40u64.to_le_bytes());
        header[8..12].copy_from_slice(&size.to_le_bytes());
        header[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let header = Payload(header);
        identity::validate(stage, &header, &bytecode)?;
        Ok(Self {
            stage,
            header,
            bytecode,
        })
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn header(&self) -> &Payload {
        &self.header
    }

    pub fn bytecode(&self) -> &[u8] {
        &self.bytecode
    }

    pub fn inspect(&self) -> Result<super::inputs::Inspection> {
        super::inputs::Inspection::read(self.stage, &self.bytecode)
    }

    pub fn assets(&self, symbol: &str, template: &Template) -> Result<Assets> {
        ensure!(
            self.stage == template.stage,
            "shader program and template stages differ"
        );
        ensure!(
            !symbol.is_empty()
                && symbol.len() <= 128
                && symbol
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "invalid shader graph symbol"
        );
        let code = format!("{symbol}-bytecode");
        Ok(Assets {
            header: self.header.0.clone(),
            bytecode: self.bytecode.clone(),
            header_template: template.header,
            bytecode_template: template.bytecode,
            nodes: vec![
                json!({"symbol":symbol,"file":format!("{symbol}.bin"),"template":template.header,
                    "reference":code,"patches":[]}),
                json!({"symbol":code,"file":format!("{code}.bin"),"template":template.bytecode,
                    "reference":symbol,"patches":[]}),
            ],
        })
    }

    pub fn receipt(&self) -> Receipt {
        Receipt {
            stage: self.stage,
            header_type: 33,
            bytecode_type: 41,
            subtype: self.stage.subtype(),
            header_sha256: hex::encode(Sha256::digest(&self.header.0)),
            bytecode_sha256: hex::encode(Sha256::digest(&self.bytecode)),
        }
    }
}
