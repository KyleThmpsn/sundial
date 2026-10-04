//! Immutable constant buffers with checked package templates and exact data.
use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tiger_pkg::TagHash;

#[cfg(test)]
mod tests;

pub struct Buffer {
    header: Vec<u8>,
    data: Vec<u8>,
}

pub struct Template {
    header: u32,
    data: u32,
}

pub struct Assets {
    pub header: Vec<u8>,
    pub data: Vec<u8>,
    pub nodes: Vec<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub header_type: u8,
    pub data_type: u8,
    pub subtype: u8,
    pub vectors: usize,
    pub header_sha256: String,
    pub data_sha256: String,
}

impl Template {
    pub fn read(reader: &mut Reader, tag: u32) -> Result<Self> {
        ensure!(
            reader.is_native(),
            "constant buffer templates require native packages"
        );
        let (data_tag, _) = read_package(reader, tag)?;
        Ok(Self {
            header: tag,
            data: data_tag,
        })
    }

    pub fn header_tag(&self) -> u32 {
        self.header
    }

    pub fn data_tag(&self) -> u32 {
        self.data
    }
}

fn read_package(reader: &mut Reader, tag: u32) -> Result<(u32, Buffer)> {
    let header = reader
        .manager
        .get_entry(TagHash(tag))
        .context("missing constant buffer header")?;
    ensure!(
        header.file_type == 32 && header.file_subtype == 7,
        "constant buffer header type differs"
    );
    let data_tag = header.reference;
    let data = reader
        .manager
        .get_entry(TagHash(data_tag))
        .context("missing constant buffer data")?;
    ensure!(
        data.file_type == 40 && data.file_subtype == 7,
        "constant buffer data type differs"
    );
    ensure!(
        data.reference == tag,
        "constant buffer references are not reciprocal"
    );
    let header = reader.tag(tag, None)?;
    let data = reader.tag(data_tag, None)?;
    Ok((data_tag, Buffer::read(&header.0, data.0.clone())?))
}

impl Buffer {
    pub fn from_package(reader: &mut Reader, tag: u32) -> Result<Self> {
        read_package(reader, tag).map(|(_, buffer)| buffer)
    }

    pub fn new(data: Vec<u8>) -> Result<Self> {
        ensure!(
            !data.is_empty() && data.len() <= 65536 && data.len().is_multiple_of(16),
            "constant buffer must contain 1 to 4096 complete vectors"
        );
        let mut header = vec![0; 16];
        header[..4].copy_from_slice(&u32::try_from(data.len())?.to_le_bytes());
        Ok(Self { header, data })
    }

    /// The nonzero dynamic flag belongs to CPU-updated frame or other scopes.
    /// This adapter preserves only the validated immutable material form.
    pub fn read(header: &[u8], data: Vec<u8>) -> Result<Self> {
        ensure!(
            header.len() == 16 && header[4..].iter().all(|&byte| byte == 0),
            "constant buffer has unsupported flags or header layout"
        );
        let buffer = Self::new(data)?;
        ensure!(
            buffer.header == header,
            "constant buffer byte length differs"
        );
        Ok(buffer)
    }

    pub fn header(&self) -> &[u8] {
        &self.header
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn vectors(&self) -> usize {
        self.data.len() / 16
    }

    pub fn assets(&self, symbol: &str, template: &Template) -> Result<Assets> {
        ensure!(
            !symbol.is_empty()
                && symbol.len() <= 128
                && symbol
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "invalid constant buffer graph symbol"
        );
        let data = format!("{symbol}-data");
        Ok(Assets {
            header: self.header.clone(),
            data: self.data.clone(),
            nodes: vec![
                json!({"symbol":symbol,"file":format!("{symbol}.bin"),"template":template.header,
                    "reference":data,"patches":[]}),
                json!({"symbol":data,"file":format!("{data}.bin"),"template":template.data,
                    "reference":symbol,"patches":[]}),
            ],
        })
    }

    pub fn receipt(&self) -> Receipt {
        Receipt {
            header_type: 32,
            data_type: 40,
            subtype: 7,
            vectors: self.vectors(),
            header_sha256: format!("{:x}", Sha256::digest(&self.header)),
            data_sha256: format!("{:x}", Sha256::digest(&self.data)),
        }
    }
}
