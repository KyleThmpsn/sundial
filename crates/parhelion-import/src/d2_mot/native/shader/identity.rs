//! Reuse native GPU assets only when their complete typed content matches.
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Pixel,
    Vertex,
    Compute,
}

impl Stage {
    pub fn subtype(self) -> u8 {
        match self {
            Self::Pixel => 0,
            Self::Vertex => 1,
            Self::Compute => 6,
        }
    }

    pub fn from_subtype(subtype: u8) -> Result<Self> {
        match subtype {
            0 => Ok(Self::Pixel),
            1 => Ok(Self::Vertex),
            6 => Ok(Self::Compute),
            _ => anyhow::bail!("shader package subtype {subtype} needs a format contract"),
        }
    }

    fn token(self) -> u32 {
        match self {
            Self::Pixel => 0x00000050,
            Self::Vertex => 0x00010050,
            Self::Compute => 0x00050050,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reuse {
    pub tag: u32,
    pub stage: Stage,
    pub header_sha256: String,
    pub bytecode_sha256: String,
}

struct Entry {
    tag: u32,
    header: Vec<u8>,
    code: Vec<u8>,
}

#[derive(Default)]
pub struct Catalog {
    entries: BTreeMap<(Stage, [u8; 32]), Entry>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn key(header: &[u8], code: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(header);
    hash.update(code);
    hash.finalize().into()
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    let end = at.checked_add(4).context("shader offset overflow")?;
    Ok(u32::from_le_bytes(
        bytes
            .get(at..end)
            .context("shader word outside payload")?
            .try_into()?,
    ))
}

fn signature(bytes: &[u8]) -> Result<()> {
    let count = usize::try_from(word(bytes, 0)?)?;
    ensure!(
        count <= 32 && word(bytes, 4)? == 8,
        "shader signature layout differs"
    );
    let end = 8 + count * 24;
    ensure!(end <= bytes.len(), "shader signature rows exceed chunk");
    for at in (8..end).step_by(24) {
        let name = usize::try_from(word(bytes, at)?)?;
        ensure!(
            name >= end && name < bytes.len(),
            "shader semantic name outside string table"
        );
        let text = &bytes[name..];
        let length = text
            .iter()
            .position(|byte| *byte == 0)
            .context("unterminated shader semantic")?;
        ensure!(
            length > 0 && length <= 128 && text[..length].is_ascii(),
            "shader semantic name differs"
        );
    }
    Ok(())
}

/// Validate the observed SM5 package format before comparing native content.
/// This does not establish a material's constant buffers or renderer bindings.
pub fn validate(stage: Stage, header: &Payload, code: &[u8]) -> Result<()> {
    ensure!(
        header.0.len() == 40 && header.u64(0)? == 40,
        "shader header size differs"
    );
    ensure!(
        usize::try_from(header.u32(8)?)? == code.len(),
        "shader bytecode size differs"
    );
    ensure!(
        header.u32(12)? == u32::MAX && header.0[16..].iter().all(|byte| *byte == 0),
        "shader header contains an unsupported binding"
    );
    ensure!(
        code.starts_with(b"DXBC") && code.len() >= 44 && word(code, 20)? == 1,
        "shader DXBC envelope differs"
    );
    ensure!(
        usize::try_from(word(code, 24)?)? == code.len() && word(code, 28)? == 3,
        "shader DXBC size or chunk count differs"
    );
    let mut spans = Vec::new();
    let mut kinds = BTreeSet::new();
    for index in 0..3 {
        let at = usize::try_from(word(code, 32 + index * 4)?)?;
        ensure!(
            at >= 44 && at.is_multiple_of(4),
            "shader chunk target differs"
        );
        let length = usize::try_from(word(
            code,
            at.checked_add(4).context("shader chunk overflow")?,
        )?)?;
        let start = at.checked_add(8).context("shader chunk overflow")?;
        let end = start.checked_add(length).context("shader chunk overflow")?;
        let bytes = code
            .get(start..end)
            .context("shader chunk exceeds bytecode")?;
        ensure!(
            spans
                .iter()
                .all(|&(left, right)| end <= left || at >= right),
            "shader chunks overlap"
        );
        spans.push((at, end));
        let kind: [u8; 4] = code
            .get(at..start - 4)
            .context("shader chunk kind")?
            .try_into()?;
        ensure!(kinds.insert(kind), "duplicate shader chunk");
        match &kind {
            b"ISGN" | b"OSGN" => signature(bytes)?,
            b"SHEX" => {
                ensure!(
                    length >= 8
                        && length.is_multiple_of(4)
                        && usize::try_from(word(bytes, 4)?)? == length / 4,
                    "shader executable token count differs"
                );
                ensure!(
                    word(bytes, 0)? == stage.token(),
                    "shader model or executable stage differs from package subtype"
                );
            }
            _ => anyhow::bail!("shader chunk needs an independent format contract"),
        }
    }
    ensure!(
        kinds == BTreeSet::from([*b"ISGN", *b"OSGN", *b"SHEX"]),
        "shader stage chunks differ"
    );
    Ok(())
}

impl Catalog {
    /// Callers supply a native package entry and its resolved data payload.
    /// Several identical native resources select the lowest tag deterministically.
    pub fn insert(&mut self, tag: u32, stage: Stage, header: &Payload, code: &[u8]) -> Result<()> {
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&tag),
            "native shader tag is invalid"
        );
        validate(stage, header, code)?;
        let identity = (stage, key(&header.0, code));
        if let Some(entry) = self.entries.get_mut(&identity) {
            ensure!(
                entry.header == header.0 && entry.code == code,
                "shader digest collision"
            );
            entry.tag = entry.tag.min(tag);
        } else {
            self.entries.insert(
                identity,
                Entry {
                    tag,
                    header: header.0.clone(),
                    code: code.to_vec(),
                },
            );
        }
        Ok(())
    }

    pub fn find(&self, stage: Stage, header: &Payload, code: &[u8]) -> Result<Option<Reuse>> {
        validate(stage, header, code)?;
        let Some(entry) = self.entries.get(&(stage, key(&header.0, code))) else {
            return Ok(None);
        };
        ensure!(
            entry.header == header.0 && entry.code == code,
            "shader digest collision"
        );
        Ok(Some(Reuse {
            tag: entry.tag,
            stage,
            header_sha256: digest(&header.0),
            bytecode_sha256: digest(code),
        }))
    }
}
