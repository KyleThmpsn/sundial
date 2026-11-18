//! Rebuild the selected descriptor array with its retained native curves and added source
//! columns. Retained references stay inside the private owner and preserve later native edits.
use super::*;
use crate::item::RuntimeResourceAppend;
use crate::tag_payload::{append_native_array, array_at, read_u64, relative_target, write_bytes};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Curve {
    translation: usize,
    column: usize,
    metadata: [u8; 24],
    samples: Vec<u32>,
}

fn rows(
    owner: &[u8],
    descriptor: usize,
    size: usize,
    expected: u32,
) -> AuthoringResult<Vec<usize>> {
    if read_u64(owner, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, start, class) = array_at(owner, descriptor)?;
    if class != expected
        || count
            .checked_mul(size)
            .and_then(|n| start.checked_add(n))
            .is_none_or(|end| end > owner.len())
    {
        return Err(invalid(format!(
            "Imported curve array at 0x{descriptor:X} has class 0x{class:08X}, expected 0x{expected:08X}, or exceeds its owner with {count} rows of {size} bytes"
        )));
    }
    Ok((0..count).map(|index| start + index * size).collect())
}

pub(super) fn append(
    owner: &[u8],
    resource: usize,
    group: u32,
    descriptor: u32,
    curves: &[Curve],
) -> AuthoringResult<RuntimeResourceAppend> {
    let definition = relative_target(owner, 24)?;
    let tables = rows(owner, definition + 0x48, 48, 0x80803975)?;
    let keys = rows(owner, definition + 0x238, 40, 0x808038C7)?;
    if tables.is_empty() || tables.len() != keys.len() {
        return Err(invalid("Imported curve table keys differ"));
    }
    let selected = keys
        .iter()
        .enumerate()
        .filter_map(|(i, &at)| (read_u32(owner, at + 16).ok() == Some(group)).then_some(i))
        .collect::<Vec<_>>();
    if selected.len() > 1 {
        return Err(invalid("Imported curve table key is ambiguous"));
    }
    let table = tables[selected.first().copied().unwrap_or(0)];
    if resource.checked_add(descriptor as usize) != Some(table) {
        return Err(invalid(
            "Imported curves do not name the selected stat table",
        ));
    }
    let original = rows(owner, table, 56, 0x80803979)?;
    if curves.iter().any(|c| c.translation >= 15 || c.column >= 32) {
        return Err(invalid(
            "Imported curve exceeds the native operand address space",
        ));
    }
    let count = curves
        .iter()
        .map(|c| c.translation + 1)
        .max()
        .unwrap_or(0)
        .max(original.len());
    let mut bytes = vec![0; 16];
    append_native_array(&mut bytes, 0, 0x80803979, count, &vec![0; count * 56])?;
    let (_, header, start, _) = array_at(&bytes, 0)?;
    let mut storage = Storage {
        owner,
        resource,
        bytes,
        pointers: Vec::new(),
        copies: Vec::new(),
    };
    for index in 0..count {
        let row = start + index * 56;
        let mut added = curves
            .iter()
            .filter(|c| c.translation == index)
            .collect::<Vec<_>>();
        added.sort_by_key(|c| c.column);
        let from = original.get(index).copied();
        if added.is_empty() {
            storage.retain(
                from.ok_or_else(|| invalid("Imported curve table has a gap"))?,
                row,
            )?;
        } else {
            storage.extend(from, row, &added)?;
        }
    }
    Ok(RuntimeResourceAppend {
        binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        resource_index: 0,
        bytes: storage.bytes,
        arrays: vec![(descriptor, header, count as u64)],
        slots: Vec::new(),
        pointers: Vec::new(),
        references: Vec::new(),
        retained_pointers: storage.pointers,
        retained_bytes: storage.copies,
    })
}

struct Storage<'a> {
    owner: &'a [u8],
    resource: usize,
    bytes: Vec<u8>,
    pointers: Vec<(usize, u32)>,
    copies: Vec<(usize, u32, usize)>,
}

impl Curve {
    fn validate(&self, first: &Self) -> AuthoringResult<()> {
        let word =
            |at| u32::from_le_bytes(self.metadata[at..at + 4].try_into().unwrap_or_default());
        let (low, high) = (f32::from_bits(word(8)), f32::from_bits(word(12)));
        if self.metadata != first.metadata
            || self.samples.len() != first.samples.len()
            || word(0) > 18
            || word(4) != 0
            || !low.is_finite()
            || !high.is_finite()
            || high <= low
            || self.metadata[16] == 0
            || self.metadata[17..] != [0; 7]
            || self.samples.is_empty()
            || self.samples.len() > i32::MAX as usize
            || self
                .samples
                .iter()
                .any(|&bits| !f32::from_bits(bits).is_finite())
        {
            return Err(invalid(
                "Imported source curve has an invalid sampling contract",
            ));
        }
        Ok(())
    }
}

impl Storage<'_> {
    fn relative(&self, target: usize) -> AuthoringResult<u32> {
        target
            .checked_sub(self.resource)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("Retained stat data precedes its resource"))
    }

    fn retain(&mut self, from: usize, to: usize) -> AuthoringResult<()> {
        write_bytes(&mut self.bytes, to, &self.owner[from..from + 56])?;
        for (field, class) in [(0, 0x8080397F), (16, 0x80809F2A)] {
            let existing = rows(self.owner, from + field, 16, class)?;
            if existing.is_empty() {
                write_bytes(&mut self.bytes, to + field, &[0; 16])?;
            } else {
                let target = relative_target(self.owner, from + field + 8)?;
                self.pointers.push((to + field + 8, self.relative(target)?));
            }
        }
        Ok(())
    }

    fn extend(&mut self, from: Option<usize>, row: usize, added: &[&Curve]) -> AuthoringResult<()> {
        let first = added[0];
        for curve in added {
            curve.validate(first)?;
        }
        let (outputs, tiers) = if let Some(from) = from {
            if self.owner[from + 32..from + 56] != first.metadata {
                return Err(invalid("Imported curve changes an existing stat domain"));
            }
            (
                rows(self.owner, from, 16, 0x8080397F)?,
                rows(self.owner, from + 16, 16, 0x80809F2A)?,
            )
        } else {
            (Vec::new(), Vec::new())
        };
        if (from.is_some() && tiers.len() != first.samples.len())
            || added
                .iter()
                .enumerate()
                .any(|(i, c)| c.column != outputs.len() + i)
        {
            return Err(invalid("Imported curve column or tier populations differ"));
        }
        let columns = outputs.len() + added.len();
        self.recipients(row, &outputs, columns)?;
        self.samples(row + 16, &tiers, outputs.len(), columns, added)?;
        write_bytes(&mut self.bytes, row + 32, &first.metadata)
    }

    fn recipients(&mut self, row: usize, outputs: &[usize], columns: usize) -> AuthoringResult<()> {
        append_native_array(
            &mut self.bytes,
            row,
            0x8080397F,
            columns,
            &vec![0; columns * 16],
        )?;
        let (_, _, destinations, _) = array_at(&self.bytes, row)?;
        // New columns have no direct recipients. Original recipients retain their arrays.
        for (column, &output) in outputs.iter().enumerate() {
            let target = destinations + column * 16;
            if read_u64(self.owner, output)? == 0 {
                continue;
            }
            // Recipient records stay in place, including their original class and header.
            array_at(self.owner, output)?;
            write_bytes(&mut self.bytes, target, &self.owner[output..output + 16])?;
            let header = relative_target(self.owner, output + 8)?;
            self.pointers.push((target + 8, self.relative(header)?));
        }
        Ok(())
    }

    fn samples(
        &mut self,
        row: usize,
        tiers: &[usize],
        old_columns: usize,
        columns: usize,
        added: &[&Curve],
    ) -> AuthoringResult<()> {
        let count = added[0].samples.len();
        append_native_array(
            &mut self.bytes,
            row,
            0x80809F2A,
            count,
            &vec![0; count * 16],
        )?;
        let (_, _, samples, _) = array_at(&self.bytes, row)?;
        for tier in 0..count {
            let descriptor = samples + tier * 16;
            let mut cells = vec![0; columns * 4];
            for curve in added {
                cells[curve.column * 4..curve.column * 4 + 4]
                    .copy_from_slice(&curve.samples[tier].to_le_bytes());
            }
            append_native_array(&mut self.bytes, descriptor, 0x8080000F, columns, &cells)?;
            if let Some(&original) = tiers.get(tier) {
                let old = rows(self.owner, original, 4, 0x8080000F)?;
                if old.len() != old_columns {
                    return Err(invalid("Native curve column counts differ"));
                }
                if let Some(&source) = old.first() {
                    let (_, _, destination, _) = array_at(&self.bytes, descriptor)?;
                    self.copies
                        .push((destination, self.relative(source)?, old_columns * 4));
                }
            }
        }
        Ok(())
    }
}
