//! Expand a native controller without changing the targets of its retained common fields.
use super::{BASE, layout};
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use sundial::package_authoring::{PackageManager, declared_payload_objects, runtime};

/// Original owner addresses remain explicit so graph endpoints can follow the same move.
#[derive(Clone, Debug)]
pub struct Offsets {
    runtime: usize,
    definition: usize,
    runtime_end: usize,
    definition_end: usize,
    runtime_growth: usize,
    definition_growth: usize,
}

impl Offsets {
    /// Donor-derived root providers are replaced with the controller. Providers on retained
    /// out-of-line common helpers keep their own definitions and reciprocal runtime objects.
    pub fn provider(&self, offset: usize) -> Result<Option<usize>> {
        if (self.definition + 0x630..self.definition_end).contains(&offset) {
            ensure!(
                offset
                    .checked_add(24)
                    .is_some_and(|end| end <= self.definition_end),
                "Donor provider crosses its derived definition boundary"
            );
            return Ok(None);
        }
        self.retained(offset).map(Some)
    }

    /// References from retained native components must never enter replaced donor storage.
    pub fn retained(&self, offset: usize) -> Result<usize> {
        ensure!(
            !(self.runtime + 0xAE0..self.runtime_end).contains(&offset)
                && !(self.definition + 0x630..self.definition_end).contains(&offset),
            "Native graph refers to replaced donor-specific storage"
        );
        self.map(offset)
    }

    pub fn map(&self, offset: usize) -> Result<usize> {
        offset
            .checked_add(if offset >= self.runtime_end {
                self.runtime_growth
            } else {
                0
            })
            .and_then(|at| {
                at.checked_add(if offset >= self.definition_end {
                    self.definition_growth
                } else {
                    0
                })
            })
            .context("Native controller relocation overflow")
    }
}

/// Use declared views rather than guessing whether an adjacent resource word is an object.
pub(super) fn fields(
    manager: &PackageManager,
    tag: u32,
    payload: &Payload,
) -> Result<BTreeMap<usize, u32>> {
    let mut fields = BTreeMap::new();
    for (offset, class) in
        declared_payload_objects(manager, tag, &payload.0).map_err(anyhow::Error::msg)?
    {
        for (delta, kind) in layout(class)?.1 {
            let at = offset
                .checked_add(delta)
                .context("Native field extent overflow")?;
            ensure!(
                fields.insert(at, kind).is_none_or(|old| old == kind),
                "Conflicting controller storage declarations"
            );
        }
    }
    Ok(fields)
}

pub(super) fn grow(
    manager: &PackageManager,
    tag: u32,
    original: &Payload,
    live: usize,
    definition: usize,
    classes: (u32, u32),
) -> Result<(Payload, Offsets)> {
    ensure!(
        runtime::native_type_parent(classes.0) == Some(BASE.0)
            && runtime::native_type_parent(classes.1) == Some(BASE.1),
        "Native common controller requires direct base allocation inheritance"
    );
    let runtime_end = live
        .checked_add(layout(classes.0)?.0)
        .context("Controller runtime extent overflow")?;
    let definition_end = definition
        .checked_add(layout(classes.1)?.0)
        .context("Controller definition extent overflow")?;
    ensure!(
        runtime_end <= definition && definition_end <= original.0.len(),
        "Native controller root leaves its owner"
    );
    let offsets = Offsets {
        runtime: live,
        definition,
        runtime_end,
        definition_end,
        runtime_growth: 0xF40usize
            .saturating_sub(runtime_end - live)
            .next_multiple_of(16),
        definition_growth: 0xB30usize
            .saturating_sub(definition_end - definition)
            .next_multiple_of(16),
    };
    let mut output = Vec::with_capacity(offsets.map(original.0.len())?);
    output.extend_from_slice(&original.0[..runtime_end]);
    output.resize(output.len() + offsets.runtime_growth, 0);
    output.extend_from_slice(&original.0[runtime_end..definition_end]);
    output.resize(output.len() + offsets.definition_growth, 0);
    output.extend_from_slice(&original.0[definition_end..]);
    for (at, kind) in fields(manager, tag, original)? {
        let destination = offsets.map(at)?;
        match kind {
            1..=3 if original.u64(at)? != 0 => {
                let target = original.pointer(at)?;
                ensure!(
                    target < original.0.len(),
                    "Native relative pointer leaves its owner"
                );
                let delta = i64::try_from(offsets.map(target)?)?
                    .checked_sub(i64::try_from(destination)?)
                    .context("Native pointer relocation overflow")?;
                put(&mut output, destination, &delta.to_le_bytes())?;
            }
            9 if original.u32(at)? == tag => {
                let target = usize::try_from(original.u64(at + 8)?)?;
                ensure!(
                    target < original.0.len(),
                    "Native local object reference leaves its owner"
                );
                put(
                    &mut output,
                    destination + 8,
                    &u64::try_from(offsets.map(target)?)?.to_le_bytes(),
                )?;
            }
            _ => {}
        }
    }
    let length = u64::try_from(output.len())?;
    put(&mut output, 0, &length.to_le_bytes())?;
    Ok((Payload(output), offsets))
}
