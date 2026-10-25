//! Program resources whose fixed blocks and array positions match native twins.
use super::Reference;
use crate::d2_mot::{
    native::effects::{controller::constants, procedural, put},
    payload::Payload,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub struct Resource {
    pub payload: Payload,
    pub class: u32,
    pub programs: usize,
    pub references: Vec<Reference>,
}

struct Layout {
    class: u32,
    fixed: usize,
    programs: &'static [usize],
    inputs: usize,
}

fn layout(class: u32) -> Result<Layout> {
    let (class, fixed, programs, inputs): (u32, usize, &[usize], usize) = match class {
        0x808031DE => (0x80803EB0, 136, &[0x28, 0x58], 1),
        0x808031D8 => (
            0x80803EAA,
            992,
            &[
                0x18, 0x48, 0x80, 0xB8, 0xE8, 0x120, 0x158, 0x188, 0x1C0, 0x200, 0x230, 0x268,
                0x2A0, 0x2D0, 0x308, 0x340, 0x370, 0x3A8,
            ],
            2,
        ),
        0x808031C2 => (0x80803E94, 152, &[0x18, 0x68], 1),
        0x808031D3 => (0x80803EA5, 232, &[0x10, 0x48, 0x80, 0xB8], 2),
        0x808031C7 => (0x80803E99, 88, &[0x18], 1),
        _ => bail!("unsupported program resource class {class:08X}"),
    };
    Ok(Layout {
        class,
        fixed,
        programs,
        inputs,
    })
}

struct Read<'a> {
    source: &'a Payload,
    fixed: usize,
    spans: BTreeMap<usize, usize>,
    markers: Vec<usize>,
}

impl Read<'_> {
    fn array(&mut self, field: usize, class: u32, stride: usize) -> Result<Vec<usize>> {
        let rows = self.source.array(field, stride, Some(class))?;
        ensure!(
            rows.len() <= 65536,
            "program resource array capacity differs"
        );
        if rows.is_empty() {
            ensure!(
                self.source.u64(field + 8)? == 0,
                "empty program resource array has a pointer"
            );
            return Ok(rows);
        }
        let header = self.source.pointer(field + 8)?;
        ensure!(
            header >= self.fixed + 4
                && header % 16 == 0
                && self.source.u32(header - 4)? == 0x80809FB8
                && self.source.u64(header + 8)? == u64::from(class),
            "program resource array envelope differs"
        );
        let start = header - 4;
        let end = header + 16 + rows.len() * stride;
        ensure!(
            self.spans
                .range(..=start)
                .next_back()
                .is_none_or(|(_, end)| *end <= start)
                && self
                    .spans
                    .range(start..)
                    .next()
                    .is_none_or(|(at, _)| *at >= end),
            "program resource arrays overlap"
        );
        self.spans.insert(start, end);
        self.markers.push(start);
        Ok(rows)
    }

    fn finish(&self) -> Result<()> {
        let mut end = self.fixed;
        for (&start, &following) in &self.spans {
            ensure!(
                self.source.0[end..start].iter().all(|v| *v == 0),
                "unparsed program resource data at {end:X}"
            );
            end = following;
        }
        ensure!(
            self.source.0[end..].iter().all(|v| *v == 0),
            "unparsed program resource tail at {end:X}"
        );
        Ok(())
    }
}

fn bind_declarations(
    source: &Payload,
    read: &mut Read<'_>,
    tags: &BTreeMap<u32, u32>,
    changes: &mut Vec<(usize, Vec<u8>)>,
    references: &mut Vec<Reference>,
) -> Result<()> {
    ensure!(
        source.bytes::<16>(24)? == [0; 16],
        "program resource root extension differs"
    );
    let rows = read.array(8, 0x80803005, 36)?;
    if !rows.is_empty() {
        changes.push((
            source.pointer(16)? + 8,
            0x80803D95u64.to_le_bytes().to_vec(),
        ));
    }
    for at in rows {
        ensure!(
            matches!(source.u32(at)?, 0x200..=0x202)
                && source.u32(at + 24)? == 1
                && source.u32(at + 32)? == 0,
            "unsupported program resource declaration kind or extension"
        );
        for offset in [4, 8, 12, 16, 20, 28] {
            let offset = at + offset;
            let tag = source.u32(offset)?;
            if tag == 0 || tag == u32::MAX {
                continue;
            }
            let target = tags.get(&tag).copied();
            if let Some(target) = target {
                ensure!(
                    (0x80800001..=0x81FFFFFF).contains(&target),
                    "invalid program resource dependency tag"
                );
            }
            changes.push((offset, target.unwrap_or(u32::MAX).to_le_bytes().to_vec()));
            references.push(Reference {
                offset,
                source: tag,
                target,
            });
        }
    }
    Ok(())
}

/// Convert every embedded equation and preserve source scalars, flags and row
/// order. Missing dependency bindings become unset slots and explicit records.
pub fn emit(source: &Payload, class: u32, tags: &BTreeMap<u32, u32>) -> Result<Resource> {
    let layout = layout(class)?;
    ensure!(
        source.u64(0)? == source.0.len() as u64 && source.0.len() >= layout.fixed,
        "program resource size differs"
    );
    if class == 0x808031D8 {
        ensure!(
            source.u32(12)? == 0xBF800000,
            "active unsupported program resource scalar"
        );
    }
    let mut read = Read {
        source,
        fixed: layout.fixed,
        spans: BTreeMap::new(),
        markers: Vec::new(),
    };
    let mut changes = Vec::new();
    let mut references = Vec::new();
    if class == 0x808031DE {
        bind_declarations(source, &mut read, tags, &mut changes, &mut references)?;
    }
    for &at in layout.programs {
        ensure!(
            source.u64(at + 32)? == layout.inputs as u64 && source.u64(at + 40)? == 1,
            "program resource input or output dimensions differ"
        );
        let code = read.array(at, 0x80800009, 1)?;
        let first = *code.first().context("program resource has no equation")?;
        let rows = read.array(at + 16, 0x80800090, 16)?;
        let code = &source.0[first..first + code.len()];
        let native = procedural::lower_program(code, rows.len(), layout.inputs)?;
        ensure!(
            native.len() == code.len(),
            "program resource code changes placement"
        );
        changes.push((first, native));
        if let Some(&first) = rows.first() {
            for &row in &rows {
                for lane in 0..4 {
                    source.f32(row + lane * 4)?;
                }
            }
            let mut vectors = source.0[first..first + rows.len() * 16].to_vec();
            constants::broadcast(code, &mut vectors);
            changes.push((first, vectors));
        }
    }
    read.finish()?;
    let mut payload = source.clone();
    if class == 0x808031D8 {
        put(&mut payload.0, 12, &source.bytes::<8>(16)?)?;
        put(&mut payload.0, 20, &[0; 4])?;
    }
    for at in read.markers {
        put(&mut payload.0, at, &0x80809FBDu32.to_le_bytes())?;
    }
    for (at, bytes) in changes {
        put(&mut payload.0, at, &bytes)?;
    }
    Ok(Resource {
        payload,
        class: layout.class,
        programs: layout.programs.len(),
        references,
    })
}
