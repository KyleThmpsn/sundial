//! Declared dependencies of a material response slot, including inheritance.
use super::{Payload, SourceReference, Table, Write, reference, valid_tag};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub const MODERN_CLASS: u32 = 0x8080873D;
pub const NATIVE_CLASS: u32 = 0x80808BCB;

/// Read the complete variable region before following its resources. A slot can
/// inherit itself, so the enclosing closure must reserve identities before use.
pub fn source_references(
    source: &Payload,
    hashes: &BTreeMap<u64, u32>,
) -> Result<Vec<SourceReference>> {
    ensure!(
        source.0.len() >= 120 && source.u64(0)? == source.0.len() as u64,
        "material slot size differs"
    );
    let mut claimed = vec![false; source.0.len()];
    claimed[..120].fill(true);
    let mut array = |field: usize, stride: usize, class: u32| -> Result<Vec<usize>> {
        let rows = source.array(field, stride, Some(class))?;
        if rows.is_empty() {
            ensure!(
                source.u64(field + 8)? == 0,
                "empty material slot array has a pointer"
            );
            return Ok(rows);
        }
        let header = source.pointer(field + 8)?;
        ensure!(
            header >= 128
                && header.is_multiple_of(16)
                && source.u32(header - 4)? == 0x80809FB8
                && source.u64(header + 8)? == u64::from(class),
            "material slot array envelope differs"
        );
        let end = header
            .checked_add(16)
            .and_then(|at| at.checked_add(rows.len().checked_mul(stride)?))
            .context("material slot array extent overflow")?;
        let span = claimed
            .get_mut(header - 4..end)
            .context("material slot array extent")?;
        ensure!(
            span.iter().all(|&used| !used),
            "material slot arrays overlap"
        );
        span.fill(true);
        Ok(rows)
    };
    let mut references = Vec::new();
    for offset in [12, 16, 20] {
        let tag = source.u32(offset)?;
        valid_tag(tag)?;
        if !matches!(tag, 0 | u32::MAX) {
            references.push(SourceReference {
                offset,
                source: tag,
            });
        }
    }
    for field in [48, 72, 96] {
        let groups = array(field, 24, 0x8080874D)?;
        let mut names = BTreeSet::new();
        for group in groups {
            ensure!(
                names.insert(source.u64(group)?),
                "duplicate material slot resource group"
            );
            for offset in array(group + 8, 32, 0x8080BC7B)? {
                let tag = reference(source, offset, hashes)?;
                if !matches!(tag, 0 | u32::MAX) {
                    references.push(SourceReference {
                        offset,
                        source: tag,
                    });
                }
            }
        }
    }
    ensure!(
        source
            .0
            .iter()
            .zip(claimed)
            .all(|(&byte, used)| used || byte == 0),
        "material slot contains unparsed data"
    );
    Ok(references)
}

/// Material slots keep their inheritance and all three response branches. The
/// added modern scalar is not native padding that can be copied speculatively.
pub fn emit(
    source: &Payload,
    hashes: &BTreeMap<u64, u32>,
    tags: &BTreeMap<u32, u32>,
) -> Result<Table> {
    source_references(source, hashes)?;
    ensure!(
        source.u32(44)? == 0,
        "material slot has an unqualified modern response scalar at +2C"
    );
    let mut output = Write {
        bytes: source.0[..120].to_vec(),
        tags,
        references: Vec::new(),
    };
    for offset in [12, 16, 20] {
        let source = source.u32(offset)?;
        let target = if matches!(source, 0 | u32::MAX) {
            Some(source)
        } else {
            tags.get(&source).copied()
        };
        output.bytes[offset..offset + 4].copy_from_slice(&target.unwrap_or(u32::MAX).to_le_bytes());
        if !matches!(source, 0 | u32::MAX) {
            output.references.push(super::Reference {
                offset,
                source,
                target,
            });
        }
    }
    for field in [48, 72, 96] {
        let groups = source.array(field, 24, Some(0x8080874D))?;
        let start = output.array(field, groups.len(), 24, 0x80808BDB);
        for (index, group) in groups.into_iter().enumerate() {
            let to = start + index * 24;
            output.bytes[to..to + 8].copy_from_slice(&source.bytes::<8>(group)?);
            let rows = source.array(group + 8, 32, Some(0x8080BC7B))?;
            let leaves = output.array(to + 8, rows.len(), 4, 0x80800014);
            for (index, row) in rows.into_iter().enumerate() {
                let tag = reference(source, row, hashes)?;
                let target = if matches!(tag, 0 | u32::MAX) {
                    Some(tag)
                } else {
                    tags.get(&tag).copied()
                };
                let at = leaves + index * 4;
                output.bytes[at..at + 4].copy_from_slice(&target.unwrap_or(u32::MAX).to_le_bytes());
                if !matches!(tag, 0 | u32::MAX) {
                    output.references.push(super::Reference {
                        offset: at,
                        source: tag,
                        target,
                    });
                }
            }
        }
    }
    let size = output.bytes.len() as u64;
    output.bytes[..8].copy_from_slice(&size.to_le_bytes());
    Ok(Table {
        payload: Payload(output.bytes),
        references: output.references,
    })
}
