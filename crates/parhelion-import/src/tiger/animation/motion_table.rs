//! Checked native rider and locomotion rows with private descriptor substitutions.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub enum Kind {
    Rider,
    Stance,
}

impl Kind {
    fn layout(self) -> (usize, usize, u32, &'static [usize]) {
        match self {
            Self::Rider => (8, 48, 0x8080321E, &[0, 16, 32]),
            Self::Stance => (24, 72, 0x80803378, &[0, 32, 48]),
        }
    }

    fn field(self, field: usize) -> Result<(usize, u32)> {
        match (self, field) {
            (Self::Rider, 0 | 32) => Ok((48, 0x80803221)),
            (Self::Rider, 16) => Ok((8, 0x80803222)),
            (Self::Stance, 0) => Ok((12, 0x8080338C)),
            _ => anyhow::bail!("Unsupported character motion table field"),
        }
    }
}

pub fn descriptors(
    p: &Payload,
    kind: Kind,
    choice: usize,
    field: usize,
) -> Result<BTreeSet<usize>> {
    let (at, stride, class, _) = kind.layout();
    let row = *p
        .array(at, stride, Some(class))?
        .get(choice)
        .context("Character motion choice is outside its table")?;
    let (stride, class) = kind.field(field)?;
    let result = p
        .array(row + field, stride, Some(class))?
        .into_iter()
        .map(|row| {
            let descriptor = p.u32(row)?;
            ensure!(
                descriptor != u32::MAX,
                "Character motion table has an empty descriptor"
            );
            Ok(descriptor as usize)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !result.is_empty(),
        "Character motion field has no descriptors"
    );
    Ok(result)
}

/// Append private rows and copy only the selected clip lists. Other relative arrays
/// continue to point at their existing contents, with their pointer bases relocated.
pub fn extend(
    p: &Payload,
    kind: Kind,
    routes: &BTreeMap<usize, BTreeMap<usize, BTreeMap<usize, usize>>>,
) -> Result<(Payload, BTreeMap<usize, usize>)> {
    ensure!(
        !routes.is_empty(),
        "Character motion table has no private rows"
    );
    let (field, stride, class, pointers) = kind.layout();
    let originals = p.array(field, stride, Some(class))?;
    let mut sources = originals.clone();
    let mut choices = BTreeMap::new();
    for &choice in routes.keys() {
        choices.insert(choice, sources.len());
        sources.push(
            *originals
                .get(choice)
                .context("Character motion choice is outside its table")?,
        );
    }
    let bytes = sources
        .iter()
        .flat_map(|&row| p.0[row..row + stride].iter().copied())
        .collect::<Vec<_>>();
    let mut output = p.clone();
    append(&mut output.0, field, u64::from(class), &bytes, stride)?;
    let rows = output.array(field, stride, Some(class))?;
    for (&old, &new) in sources.iter().zip(&rows) {
        for &field in pointers {
            if p.u64(old + field)? != 0 {
                let target = p.pointer(old + field + 8)?;
                put(
                    &mut output.0,
                    new + field + 8,
                    &(target as i64 - new as i64 - field as i64 - 8).to_le_bytes(),
                )?;
            }
        }
    }
    for (&choice, fields) in routes {
        let old = originals[choice];
        let new = rows[choices[&choice]];
        for (&field, mapping) in fields {
            let (stride, class) = kind.field(field)?;
            let mut bytes = Vec::new();
            for row in p.array(old + field, stride, Some(class))? {
                let descriptor = p.u32(row)? as usize;
                let replacement = *mapping
                    .get(&descriptor)
                    .context("Character motion field has an untranslated descriptor")?;
                bytes.extend(u32::try_from(replacement)?.to_le_bytes());
                bytes.extend(&p.0[row + 4..row + stride]);
            }
            append(&mut output.0, new + field, u64::from(class), &bytes, stride)?;
        }
    }
    let length = output.0.len() as u64;
    put(&mut output.0, 0, &length.to_le_bytes())?;
    Ok((output, choices))
}
