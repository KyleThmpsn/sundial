//! Reference declarations from native build 86657.20.08.23.1800.d2_rc.
//! Captured image SHA-256: 26554b0c48109c9752a1a015ec0c45ff1137b675666338353ca35b9e81977484.
//! Each row retains (handle, size, native layout). Layout opcodes are not member type handles.
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use crate::package_payload::{i64_at, relative_offset, u32_at, u64_at};

#[derive(Clone)]
pub(crate) struct Record {
    pub size: usize,
    pub fields: Arc<[(usize, u32)]>,
}

type Encoded = (u32, usize, Vec<(usize, u32)>);
type Records = BTreeMap<u32, Result<Record, String>>;
static NATIVE: OnceLock<Result<Records, String>> = OnceLock::new();

pub(crate) struct Registry {
    native: &'static Records,
    generated: BTreeMap<u32, Record>,
}

impl Registry {
    pub fn new() -> Result<Self, String> {
        let native = NATIVE
            .get_or_init(|| {
                let source: Vec<Encoded> = serde_json::from_str(include_str!("schema.json"))
                    .map_err(|error| format!("Native reference schemas are invalid: {error}"))?;
                let mut records = BTreeMap::new();
                for (class, size, layout) in source {
                    if records.insert(class, decode(size, &layout)).is_some() {
                        return Err("Duplicate native reference schema".into());
                    }
                }
                Ok(records)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(Self {
            native,
            generated: BTreeMap::new(),
        })
    }

    pub fn record(
        &mut self,
        handle: u32,
        read: impl FnOnce(u32) -> Result<Vec<u8>, String>,
    ) -> Result<Record, String> {
        if let Some(record) = self.native.get(&handle) {
            return record
                .clone()
                .map_err(|error| format!("Schema 0x{handle:08X}: {error}"));
        }
        if let Some(record) = self.generated.get(&handle) {
            return Ok(record.clone());
        }
        if !crate::package_runtime::is_valid_package_tag(tiger_pkg::TagHash(handle)) {
            return Err(format!("Unknown native reference schema 0x{handle:08X}"));
        }
        let record = generated(&read(handle)?)?;
        self.generated.insert(handle, record.clone());
        Ok(record)
    }
}

fn generated(data: &[u8]) -> Result<Record, String> {
    if u64_at(data, 0)? != data.len() as u64 {
        return Err("Generated reference schema has an invalid envelope".into());
    }
    let size = u32_at(data, 0x14)? as usize;
    let relative = i64_at(data, 0x38)?;
    if relative == 0 {
        return decode(size, &[]);
    }
    let block = relative_offset(0x38, 0, relative)?;
    if block < 4 || u32_at(data, block - 4)? != 0x8080_00E1 {
        return Err("Generated reference layout has an invalid marker".into());
    }
    let count =
        usize::try_from(u64_at(data, block)?).map_err(|_| "Generated layout is too large")?;
    if count > 100_000
        || block
            .checked_add(8)
            .and_then(|start| {
                count
                    .checked_mul(8)
                    .and_then(|size| start.checked_add(size))
            })
            .is_none_or(|end| end > data.len())
    {
        return Err("Generated reference layout has invalid bounds".into());
    }
    let layout = (0..count)
        .map(|i| {
            Ok((
                u32_at(data, block + 8 + i * 8)? as usize,
                u32_at(data, block + 12 + i * 8)?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    decode(size, &layout)
}

fn decode(size: usize, layout: &[(usize, u32)]) -> Result<Record, String> {
    let mut fields = BTreeMap::new();
    expand(layout, 0, size, 0, &mut 100_000, &mut fields)?;
    Ok(Record {
        size,
        fields: fields.into_iter().collect::<Vec<_>>().into(),
    })
}

fn expand(
    layout: &[(usize, u32)],
    base: usize,
    size: usize,
    depth: usize,
    remaining: &mut usize,
    fields: &mut BTreeMap<usize, u32>,
) -> Result<(), String> {
    if depth > 32 || fields.len() > 100_000 {
        return Err("Native reference layout exceeded its expansion limit".into());
    }
    let mut i = 0;
    while i < layout.len() {
        *remaining = remaining
            .checked_sub(1)
            .ok_or("Native reference layout exceeded its operation limit")?;
        let (offset, kind) = layout[i];
        i += 1;
        let offset = base
            .checked_add(offset)
            .ok_or("Native layout offset overflow")?;
        match kind {
            3 | 4 | 9 => {
                let width = match kind {
                    3 => 8,
                    4 => 4,
                    _ => 16,
                };
                if offset.checked_add(width).is_none_or(|end| end > size) {
                    return Err("Native reference field exceeds its object".into());
                }
                if fields
                    .insert(offset, kind)
                    .is_some_and(|prior| prior != kind)
                {
                    return Err("Conflicting native reference fields".into());
                }
            }
            10 => {
                let args = layout
                    .get(i..i + 3)
                    .ok_or("Truncated native repeat declaration")?;
                if args.iter().any(|(_, code)| *code != 0) {
                    return Err("Invalid native repeat arguments".into());
                }
                let (count, stride, length) = (args[0].0, args[1].0, args[2].0);
                i += 3;
                let end = i
                    .checked_add(length)
                    .ok_or("Native repeat length overflow")?;
                let body = layout.get(i..end).ok_or("Truncated native repeat body")?;
                if count > 100_000
                    || stride == 0
                    || offset
                        .checked_add(count.saturating_mul(stride))
                        .is_none_or(|end| end > size)
                {
                    return Err("Native repeat exceeds its object".into());
                }
                for j in 0..count {
                    expand(
                        body,
                        offset + j * stride,
                        size,
                        depth + 1,
                        remaining,
                        fields,
                    )?;
                }
                i = end;
            }
            11 => {
                if layout.get(i).is_none_or(|(_, code)| *code != 0) {
                    return Err("Truncated native indexed-value declaration".into());
                }
                i += 1;
            }
            // Raw pointers and scalar identifiers contain no declared package references.
            1 | 2 | 5 | 6 | 8 | 12 => {}
            _ => return Err(format!("Unsupported native layout operation {kind}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
