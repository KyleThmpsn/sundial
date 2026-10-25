//! Single-stream float geometry used by particle emission meshes.
//!
//! Modern input layout 12 and native layout 13 both carry position float3,
//! texture coordinates float2 and normal float3 in a 32-byte stream. This
//! converter authors geometry without borrowing a weapon model or material.
use super::*;
use crate::d2_mot::{mapping, reader::Reader};

pub mod assets;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    VertexBuffer,
    IndexBuffer,
    Material,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub offset: usize,
    pub source: u32,
    pub kind: Kind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Buffer {
    pub source: u32,
    #[serde(default)]
    pub data_source: u32,
    pub kind: Kind,
    pub header: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Geometry {
    /// Native class 808073A5. References remain null until explicitly linked.
    pub bytes: Vec<u8>,
    pub references: Vec<Reference>,
    pub buffers: Vec<Buffer>,
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

fn array(bytes: &mut Vec<u8>, at: usize, count: usize, stride: usize, class: u32) -> usize {
    let header = (bytes.len() + 4).next_multiple_of(16);
    bytes.resize(header + 16 + count * stride, 0);
    put(bytes, at, &(count as u64).to_le_bytes());
    put(bytes, at + 8, &((header - at - 8) as i64).to_le_bytes());
    put(bytes, header - 4, &0x80809FBDu32.to_le_bytes());
    put(bytes, header, &(count as u64).to_le_bytes());
    put(bytes, header + 8, &class.to_le_bytes());
    header + 16
}

fn read_buffer(reader: &mut Reader, tag: u32, kind: Kind) -> Result<Buffer> {
    let subtype = match kind {
        Kind::VertexBuffer => 4,
        Kind::IndexBuffer => 6,
        Kind::Material => bail!("material is not a geometry buffer"),
    };
    let entry = reader
        .manager
        .get_entry(tiger_pkg::TagHash(tag))
        .context("particle mesh buffer header is missing")?;
    ensure!(
        entry.file_type == 32 && entry.file_subtype == subtype,
        "particle mesh buffer header type differs"
    );
    let data_tag = entry.reference;
    let entry = reader
        .manager
        .get_entry(tiger_pkg::TagHash(data_tag))
        .context("particle mesh buffer payload is missing")?;
    ensure!(
        entry.file_type == 40
            && entry.file_subtype == subtype
            && entry.file_size <= 32 * 1024 * 1024,
        "particle mesh buffer payload type or size differs"
    );
    ensure!(
        entry.reference == tag,
        "particle mesh buffer references are not reciprocal"
    );
    let header = reader.tag(tag, None)?;
    let data = reader.tag(data_tag, None)?;
    let buffer = Buffer {
        source: tag,
        data_source: data_tag,
        kind,
        header: header.0.clone(),
        data: data.0.clone(),
    };
    buffer.validate()?;
    Ok(buffer)
}

impl Buffer {
    pub fn validate(&self) -> Result<()> {
        let header = Payload(self.header.clone());
        let data = Payload(self.data.clone());
        ensure!(
            data.0.len() <= 32 * 1024 * 1024,
            "particle buffer exceeds capacity"
        );
        match self.kind {
            Kind::VertexBuffer => {
                ensure!(
                    header.0.len() == 12
                        && header.u16(4)? == 32
                        && header.u16(6)? == 0
                        && header.u32(8)? == 0xDEADBEEF,
                    "particle vertex format differs"
                );
                ensure!(
                    header.u32(0)? as usize == data.0.len()
                        && !data.0.is_empty()
                        && data.0.len().is_multiple_of(32),
                    "particle vertex count differs"
                );
                for at in (0..data.0.len()).step_by(4) {
                    data.f32(at)?;
                }
            }
            Kind::IndexBuffer => {
                ensure!(
                    header.0.len() == 24
                        && header.u64(0)? == 1
                        && header.u64(8)? == data.0.len() as u64
                        && header.u64(16)? == 0xDEADBEEF
                        && !data.0.is_empty()
                        && data.0.len().is_multiple_of(2),
                    "particle index format differs"
                );
            }
            Kind::Material => bail!("material is not a geometry buffer"),
        }
        Ok(())
    }
}

/// Convert the validated unskinned float-stream family. Other vertex layouts,
/// auxiliary buffers and source-only draw fields require their own converter.
pub fn convert(reader: &mut Reader, tag: u32) -> Result<Geometry> {
    let source = reader.tag(tag, Some(0x80806F07))?;
    ensure!(
        source.0.len() >= 160 && source.u64(0)? == source.0.len() as u64,
        "particle mesh model size differs"
    );
    ensure!(
        source.u64(8)? == 0
            && source.u64(0x30)? == 32
            && source.u64(0x38)? == 0x0300000000000101
            && source.u64(0x40)? == 0
            && source.u64(0x48)? == 0
            && source.u64(0x90)? == u64::MAX
            && source.u64(0x98)? == 0,
        "particle mesh model header requires additional translation"
    );
    for at in (0x20..0x30).chain(0x50..0x90).step_by(4) {
        source.f32(at)?;
    }
    let meshes = source.array(16, 128, Some(0x80806EC5))?;
    ensure!(
        !meshes.is_empty() && meshes.len() <= 256,
        "particle mesh count differs"
    );
    let mut out = Geometry {
        bytes: source.0[..160].to_vec(),
        references: vec![],
        buffers: vec![],
    };
    // These unskinned model header fields have the same contract in all 469
    // native float-stream examples. Modern inserts a byte and an optional tag.
    put(&mut out.bytes, 0x38, &0x0000030000000001u64.to_le_bytes());
    put(&mut out.bytes, 0x94, &0u32.to_le_bytes());
    let target_meshes = array(&mut out.bytes, 16, meshes.len(), 136, 0x80807378);
    for (index, mesh) in meshes.into_iter().enumerate() {
        let target = target_meshes + index * 136;
        ensure!(
            [4, 8, 12, 20, 24]
                .into_iter()
                .all(|at| source.u32(mesh + at).ok() == Some(u32::MAX))
                && source.u32(mesh + 28)? == 0
                && source.bytes::<6>(mesh + 122)? == [0; 6],
            "particle mesh has auxiliary streams or unsupported fields"
        );
        ensure!(
            source.bytes::<24>(mesh + 98)? == [12; 24],
            "particle mesh input layout differs"
        );
        let parts = source.array(mesh + 32, 36, Some(0x80806ECB))?;
        ensure!(
            !parts.is_empty() && parts.len() <= u16::MAX as usize,
            "particle draw count differs"
        );
        let ranges = (0..25)
            .map(|i| source.u16(mesh + 48 + i * 2))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            ranges[0] == 0
                && ranges[24] as usize == parts.len()
                && ranges[23] == ranges[24]
                && ranges.windows(2).all(|w| w[0] <= w[1]),
            "particle mesh stage ranges require additional translation"
        );
        for offset in [0, 4, 8, 12, 16] {
            put(&mut out.bytes, target + offset, &u32::MAX.to_le_bytes());
        }
        for (i, range) in ranges.iter().take(24).enumerate() {
            put(&mut out.bytes, target + 40 + i * 2, &range.to_le_bytes());
        }
        for i in 0..23 {
            put(&mut out.bytes, target + 88 + i * 2, &13u16.to_le_bytes());
        }
        let mut selected = [0; 2];
        for (slot, (offset, kind)) in [(0, Kind::VertexBuffer), (16, Kind::IndexBuffer)]
            .into_iter()
            .enumerate()
        {
            let source_tag = source.u32(mesh + offset)?;
            let found = out.buffers.iter().position(|b| b.source == source_tag);
            let i = if let Some(i) = found {
                ensure!(
                    out.buffers[i].kind == kind,
                    "particle buffer used with conflicting types"
                );
                i
            } else {
                out.buffers.push(read_buffer(reader, source_tag, kind)?);
                out.buffers.len() - 1
            };
            selected[slot] = i;
            out.references.push(Reference {
                offset: target + offset,
                source: source_tag,
                kind,
            });
        }
        let vertices = out.buffers[selected[0]].data.len() / 32;
        let indices = &out.buffers[selected[1]].data;
        let target_parts = array(&mut out.bytes, target + 24, parts.len(), 32, 0x8080737E);
        for (index, part) in parts.into_iter().enumerate() {
            ensure!(
                source.u16(part + 4)? == u16::MAX
                    && source.u16(part + 6)? == 3
                    && source.u32(part + 24)? == 0
                    && source.u32(part + 32)? == u32::MAX,
                "particle draw record requires additional translation"
            );
            let first = source.u32(part + 8)? as usize;
            let count = source.u32(part + 12)? as usize;
            ensure!(
                count > 0 && count.is_multiple_of(3),
                "particle triangle list count differs"
            );
            let slice = indices
                .get(first * 2..(first + count) * 2)
                .context("particle draw exceeds index buffer")?;
            ensure!(
                slice
                    .chunks_exact(2)
                    .all(|v| usize::from(u16::from_le_bytes([v[0], v[1]])) < vertices),
                "particle draw addresses a missing vertex"
            );
            let material = source.u32(part)?;
            reader.tag(material, Some(0x80806DAA))?;
            let record = mapping::draw_record(&source.bytes::<36>(part)?)?;
            let at = target_parts + index * 32;
            put(&mut out.bytes, at, &record);
            out.references.push(Reference {
                offset: at,
                source: material,
                kind: Kind::Material,
            });
        }
    }
    let length = out.bytes.len() as u64;
    put(&mut out.bytes, 0, &length.to_le_bytes());
    Ok(out)
}

impl Geometry {
    /// Link only complete native dependencies, with their expected asset kinds.
    pub fn link(&self, dependencies: &BTreeMap<u32, (u32, Kind)>) -> Result<Vec<u8>> {
        let mut bytes = self.bytes.clone();
        for reference in &self.references {
            let &(tag, kind) = dependencies.get(&reference.source).with_context(|| {
                format!(
                    "particle mesh dependency {:08X} is not translated",
                    reference.source
                )
            })?;
            ensure!(
                kind == reference.kind && (0x80800001..=0x81FFFFFF).contains(&tag),
                "particle mesh dependency has an invalid native type or tag"
            );
            let slot = bytes
                .get_mut(reference.offset..reference.offset + 4)
                .context("particle mesh relocation exceeds payload")?;
            ensure!(
                slot == u32::MAX.to_le_bytes(),
                "particle mesh relocation is not unlinked"
            );
            slot.copy_from_slice(&tag.to_le_bytes());
        }
        Ok(bytes)
    }
}
