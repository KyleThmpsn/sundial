//! Checked source sequence topology and native component assembly.
mod assembly;
mod controls;
mod events;
mod expression;
mod link;
use super::links::Object;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
pub(crate) use assembly::inputs as scalar_inputs;
pub use assembly::{Bindings, Native, Resource, emit};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const NATIVE_RANGE_CLASS: u32 = 0x808093ED;

#[derive(Debug, Serialize)]
pub struct Input {
    pub object: Object,
    pub name: u32,
}

#[derive(Debug, Serialize)]
pub struct Range {
    pub offset: usize,
    pub input: u32,
    pub lower: f32,
    pub upper: f32,
}

impl Range {
    pub fn read(source: &Payload, offset: usize, input_count: usize) -> Result<Self> {
        ensure!(
            source.u32(offset.checked_sub(4).context("condition class offset")?)? == 0x808091FE,
            "unsupported source sequence condition class"
        );
        ensure!(
            source.u32(offset)? == 0,
            "unsupported sequence condition kind"
        );
        let input = source.u32(offset + 12)?;
        ensure!(
            (input as usize) < input_count,
            "sequence condition input outside table"
        );
        let lower = source.f32(offset + 4)?;
        let upper = source.f32(offset + 8)?;
        ensure!(
            lower.is_finite() && upper.is_finite(),
            "nonfinite sequence condition range"
        );
        ensure!(lower <= upper, "reversed sequence condition range");
        Ok(Self {
            offset,
            input,
            lower,
            upper,
        })
    }

    /// The caller emits this record beneath NATIVE_RANGE_CLASS and relocates its
    /// parent pointer. Input mappings address sequencer slots, not bank channels.
    pub fn native(&self, inputs: &BTreeMap<u32, u32>, count: usize) -> Result<[u8; 16]> {
        ensure!(
            self.lower.is_finite() && self.upper.is_finite() && self.lower <= self.upper,
            "invalid sequence condition range"
        );
        let input = *inputs
            .get(&self.input)
            .context("unmapped sequence condition input")?;
        ensure!(
            (input as usize) < count,
            "native condition input outside table"
        );
        let mut bytes = [0; 16];
        bytes[4..8].copy_from_slice(&self.lower.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.upper.to_le_bytes());
        bytes[12..].copy_from_slice(&input.to_le_bytes());
        Ok(bytes)
    }
}

#[derive(Debug, Serialize)]
pub struct Child {
    pub event: bool,
    pub index: usize,
}

#[derive(Debug, Serialize)]
pub struct Control {
    pub offset: usize,
    pub class: u32,
    pub name: u32,
    pub parent: Option<usize>,
    pub children: Vec<Child>,
    /// The two pointer fields have distinct scheduling roles. Preserve their
    /// slots rather than flattening them into one condition list.
    pub conditions: [Option<Range>; 2],
}

#[derive(Debug, Serialize)]
pub struct Sequence {
    pub inputs: Vec<Input>,
    pub controls: Vec<Control>,
    pub event_count: usize,
}

fn parent(source: &Payload, node: usize, controls: usize) -> Result<Option<usize>> {
    let value = source.i16(node + 6)?;
    ensure!(
        value == -1 || (value >= 0 && (value as usize) < controls),
        "sequence parent outside controls"
    );
    Ok((value >= 0).then_some(value as usize))
}

impl Sequence {
    pub fn read(source: &Payload) -> Result<Self> {
        ensure!(
            source.u64(0)? == source.0.len() as u64,
            "sequence owner size differs"
        );
        let definition = source.pointer(24)?;
        let instance = source.pointer(16)?;
        ensure!(
            source.u32(
                definition
                    .checked_sub(4)
                    .context("sequence definition class")?
            )? == 0x80808179
                && source.u32(instance.checked_sub(4).context("sequence instance class")?)?
                    == 0x80809479,
            "unsupported sequence owner"
        );
        let owner = source.u32(definition)?;
        ensure!(
            source.u32(instance)? == owner
                && source.u64(instance + 8)? == definition as u64
                && source.u64(definition + 8)? == instance as u64,
            "sequence owner pair differs"
        );
        let inputs = source
            .array(definition + 0x208, 40, Some(0x80809591))?
            .into_iter()
            .map(|at| {
                let paired = usize::try_from(source.u64(at + 8)?)?;
                ensure!(
                    source.u32(at)? == owner
                        && source.u32(at + 4)? == 0x80809590
                        && source.u32(paired)? == owner
                        && source.u32(paired + 4)? == 0x80809591
                        && source.u64(paired + 8)? == at as u64
                        && source.pointer(paired + 16)? == instance,
                    "sequence input pair differs"
                );
                ensure!(
                    source.u64(at + 16)? == 0
                        && source.u64(at + 24)? == 0x808095CE
                        && source.u32(at + 36)? == 0,
                    "sequence input contract differs"
                );
                Ok(Input {
                    object: Object {
                        owner,
                        class: 0x80809591,
                        offset: at as u64,
                    },
                    name: source.u32(at + 32)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let nodes = |field| -> Result<Vec<usize>> {
            source
                .array(definition + field, 24, Some(0x808091F1))?
                .into_iter()
                .map(|row| source.pointer(row + 16))
                .collect()
        };
        let control_nodes = nodes(0x1C8)?;
        let events = nodes(0x1D8)?;
        ensure!(!control_nodes.is_empty(), "sequence has no root control");
        let mut seen_children = BTreeSet::new();
        let mut controls = Vec::new();
        for (index, &at) in control_nodes.iter().enumerate() {
            let class = source.u32(at.checked_sub(4).context("control class offset")?)?;
            ensure!(
                matches!(class, 0x808091E3 | 0x808091D9 | 0x808091E5 | 0x808091E1),
                "unsupported source sequence control {class:08X}"
            );
            let mut children = Vec::new();
            for row in source.array(at + 0x20, 4, Some(0x808094E9))? {
                let kind = source.u16(row)?;
                let child = source.u16(row + 2)? as usize;
                ensure!(kind <= 1, "unsupported sequence child kind");
                let target = *(if kind == 0 { &control_nodes } else { &events })
                    .get(child)
                    .context("sequence child outside table")?;
                ensure!(
                    parent(source, target, control_nodes.len())? == Some(index),
                    "sequence child parent differs"
                );
                ensure!(
                    seen_children.insert((kind, child)),
                    "duplicate sequence child ownership"
                );
                children.push(Child {
                    event: kind == 1,
                    index: child,
                });
            }
            let read = |field| -> Result<Option<Range>> {
                if source.u64(at + field)? == 0 {
                    return Ok(None);
                }
                Range::read(source, source.pointer(at + field)?, inputs.len()).map(Some)
            };
            controls.push(Control {
                offset: at,
                class,
                name: source.u32(at)?,
                parent: parent(source, at, control_nodes.len())?,
                children,
                conditions: [read(0x30)?, read(0x38)?],
            });
        }
        for (index, control) in controls.iter().enumerate() {
            ensure!(
                control.parent.is_none() || seen_children.contains(&(0, index)),
                "unlinked sequence control"
            );
            let mut ancestors = BTreeSet::from([index]);
            let mut next = control.parent;
            while let Some(index) = next {
                ensure!(ancestors.insert(index), "sequence control cycle");
                next = controls[index].parent;
            }
        }
        for (index, &event) in events.iter().enumerate() {
            ensure!(
                parent(source, event, controls.len())?.is_some()
                    && seen_children.contains(&(1, index)),
                "unlinked sequence event"
            );
        }
        Ok(Self {
            inputs,
            controls,
            event_count: events.len(),
        })
    }
}
