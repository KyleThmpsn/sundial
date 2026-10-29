//! Rise of Iron sequence topology. Event bodies and their dependencies have separate converters.
use crate::{
    presentation::put,
    tiger::{payload::Payload, sequence as native},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub mod animation;
pub mod completion;
mod condition;
pub mod hud;
pub mod input;
mod numeric;
mod plan;
pub mod sound;
pub use plan::{ActionKey, Bindings, Plan, Prepared};

/// A checked event body still needs its containing graph to allocate this private resource.
pub struct ResourcePatch {
    pub offset: usize,
    pub symbol: String,
    pub source: u32,
    pub source_class: u32,
    pub native_class: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Domain {
    Numeric,
    External,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub offset: usize,
    pub class: u32,
    pub kind: u32,
    pub domain: Domain,
}

/// An event converter's class contract. This does not replace its body or dependency conversion.
#[derive(Clone, Copy)]
pub struct NativeEvent {
    pub source_class: u32,
    pub source_kind: u32,
    pub class: u32,
}

pub struct Written {
    /// Source event offsets mapped to newly allocated native definitions.
    pub events: BTreeMap<usize, usize>,
    /// Every control and event, including duplicate authored names.
    pub nodes: BTreeMap<usize, usize>,
}

pub struct Topology {
    digest: [u8; 32],
    tables: [Vec<usize>; 3],
    events: Vec<Event>,
    inputs: usize,
}

impl Topology {
    pub fn read(source: &Payload) -> Result<Self> {
        let runtime = source.pointer(16)?;
        let definition = source.pointer(24)?;
        ensure!(
            source.u32(
                runtime
                    .checked_sub(4)
                    .context("Source sequence runtime marker")?
            )? == 0x8080079A
                && source.u32(
                    definition
                        .checked_sub(4)
                        .context("Source sequence marker")?
                )? == 0x80800610
                && source.u64(runtime + 8)? == definition as u64
                && source.u64(definition + 8)? == runtime as u64
                && source.u32(runtime)? == source.u32(definition)?,
            "Source sequence owner pair differs"
        );
        let read = |field| -> Result<Vec<usize>> {
            let rows = source.array(definition + field, 8, Some(0x80800629))?;
            ensure!(
                rows.len() < i16::MAX as usize,
                "Source sequence table exceeds its selector"
            );
            rows.into_iter().map(|row| source.pointer(row)).collect()
        };
        let tables = [read(0x160)?, read(0x170)?, read(0x180)?];
        ensure!(!tables[0].is_empty(), "Source sequence has no root control");
        let mut events = Vec::new();
        let mut unique = BTreeSet::new();
        for (domain, rows) in tables.iter().enumerate() {
            for &offset in rows {
                ensure!(
                    unique.insert(offset),
                    "Source sequence aliases a stored node"
                );
                let class = source.u32(
                    offset
                        .checked_sub(4)
                        .context("Source sequence node marker")?,
                )?;
                source.bytes::<32>(offset)?;
                if domain != 0 {
                    events.push(Event {
                        offset,
                        class,
                        kind: source.u32(offset + 0x1C)?,
                        domain: if domain == 1 {
                            Domain::Numeric
                        } else {
                            Domain::External
                        },
                    });
                }
            }
        }
        let result = Self {
            digest: Sha256::digest(&source.0).into(),
            tables,
            events,
            inputs: source
                .array(definition + 0x1A0, 48, Some(0x80800927))?
                .len(),
        };
        // Validate every stored branch, even when its event converter is not available yet.
        result.tree(source, None)?;
        condition::read(source, definition + 0x2C8, result.inputs)?;
        Ok(result)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn control_count(&self) -> usize {
        self.tables[0].len()
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }

    fn tree(
        &self,
        source: &Payload,
        classes: Option<&BTreeMap<usize, u32>>,
    ) -> Result<native::Node> {
        let mut tree = Tree {
            source,
            topology: self,
            classes,
            visited: BTreeSet::new(),
        };
        let root = tree.node(0, 0, -1, 0)?;
        ensure!(
            tree.visited.len() == self.tables.iter().map(Vec::len).sum::<usize>(),
            "Source sequence contains unreachable nodes"
        );
        Ok(root)
    }

    /// Allocate the whole native tree atomically. All external events need explicit qualified
    /// class mappings and checked instance envelopes. Callers must finish every event body,
    /// locator, input/output and resource before publishing the containing owner.
    pub fn write(
        &self,
        source: &Payload,
        target: &mut Payload,
        allocation: &mut Payload,
        envelopes: &[Payload],
        external: &BTreeMap<usize, NativeEvent>,
    ) -> Result<Written> {
        let digest: [u8; 32] = Sha256::digest(&source.0).into();
        ensure!(
            digest == self.digest,
            "Source sequence changed after intake"
        );
        let mut classes = BTreeMap::new();
        for event in &self.events {
            let class = match event.domain {
                Domain::Numeric => match (event.class, event.kind) {
                    (0x808006CE, 0) => 0x808093CB,
                    (0x80800507, 2) => 0x808093C4,
                    _ => anyhow::bail!("Unsupported source numeric event {:08X}", event.class),
                },
                Domain::External => {
                    let mapped = external.get(&event.offset).with_context(|| {
                        format!(
                            "Source external event {:08X} at {:X} has no translation",
                            event.class, event.offset
                        )
                    })?;
                    ensure!(
                        mapped.source_class == event.class && mapped.source_kind == event.kind,
                        "Source external event mapping differs at {:X}",
                        event.offset
                    );
                    mapped.class
                }
            };
            classes.insert(event.offset, class);
        }
        ensure!(
            external.len() == self.tables[2].len(),
            "External sequence mappings include absent or duplicate events"
        );
        let tree = self.tree(source, Some(&classes))?;
        let source_inputs = source.array(source.pointer(24)? + 0x1A0, 48, Some(0x80800927))?;
        let native_inputs = target.array(target.pointer(24)? + 0x188, 40, Some(0x80809789))?;
        ensure!(
            native_inputs.len() == self.inputs,
            "Native sequence inputs do not match source ordinals"
        );
        for (from, to) in source_inputs.into_iter().zip(native_inputs) {
            ensure!(
                source.u32(from + 0x28)? == target.u32(to + 0x20)?,
                "Native sequence input names do not match source ordinals"
            );
        }
        let mut output = target.clone();
        let mut storage = allocation.clone();
        let written = native::write(&mut output, &mut storage, &tree, envelopes)?;
        let definition = output.pointer(24)?;
        let mut nodes = BTreeMap::new();
        for field in [0x158, 0x168] {
            for row in output.array(definition + field, 24, Some(0x808093E6))? {
                let to = output.pointer(row + 16)?;
                let from = output.u32(to + 0x10)? as usize;
                ensure!(
                    nodes.insert(from, to).is_none(),
                    "Native sequence aliases a source node"
                );
                put(&mut output.0, to + 0x10, &source.bytes::<4>(from)?)?;
                if field == 0x158 {
                    // Control kinds are qualified independently of external event kinds.
                    // Keep the source repeat byte without copying an external dispatch ID.
                    put(&mut output.0, to + 0x32, &source.bytes::<2>(from + 0x1E)?)?;
                    if source.u32(from - 4)? == 0x80800757 {
                        let maximum = condition::exclusive_maximum(source.f32(from + 0x44)?);
                        put(&mut output.0, to + 0x5C, &maximum.to_le_bytes())?;
                    }
                    for (source_field, native_field) in [(0x30, 0x48), (0x38, 0x50)] {
                        condition::write(
                            &mut output,
                            to + native_field,
                            condition::read(source, from + source_field, self.inputs)?,
                        )?;
                    }
                }
            }
        }
        let source_definition = source.pointer(24)?;
        ensure!(
            source.u32(source_definition + 0x2C0)? == 0,
            "Source sequence starts at a different root selector"
        );
        condition::write(
            &mut output,
            definition + 0x2D8,
            condition::read(source, source_definition + 0x2C8, self.inputs)?,
        )?;
        let length = u64::try_from(output.0.len())?;
        put(&mut output.0, 0, &length.to_le_bytes())?;
        let events = written
            .events
            .into_iter()
            .map(|(from, to)| (from as usize, to))
            .collect();
        *target = output;
        *allocation = storage;
        Ok(Written { events, nodes })
    }
}

struct Tree<'a> {
    source: &'a Payload,
    topology: &'a Topology,
    classes: Option<&'a BTreeMap<usize, u32>>,
    visited: BTreeSet<usize>,
}

impl Tree<'_> {
    fn node(
        &mut self,
        domain: usize,
        index: usize,
        parent: i16,
        depth: usize,
    ) -> Result<native::Node> {
        ensure!(
            depth < 256,
            "Source sequence nesting exceeds the translation limit"
        );
        let at = *self
            .topology
            .tables
            .get(domain)
            .and_then(|table| table.get(index))
            .context("Source sequence selector exceeds its table")?;
        ensure!(
            self.visited.insert(at) && self.source.i16(at + 6)? == parent,
            "Source sequence is cyclic, shared or has a different parent"
        );
        let class = self.source.u32(at - 4)?;
        let timing = native::Timing {
            delay: [self.source.f32(at + 12)?, self.source.f32(at + 16)?],
            duration: [self.source.f32(at + 20)?, self.source.f32(at + 24)?],
            flags: self.source.u32(at + 8)?,
        };
        ensure!(
            timing.flags & !0x1F == 0
                && timing
                    .delay
                    .iter()
                    .chain(&timing.duration)
                    .all(|value| *value >= 0.),
            "Source sequence timing flags or values differ"
        );
        let kind = if domain != 0 {
            native::Kind::Event(match self.classes {
                Some(classes) => *classes.get(&at).context("Sequence event is not mapped")?,
                None => class,
            })
        } else {
            ensure!(
                self.source.u8(at + 0x1E)? <= 1 && self.source.u8(at + 0x1F)? == 0,
                "Source sequence control has unsupported conditions"
            );
            for field in [0x30, 0x38] {
                condition::read(self.source, at + field, self.topology.inputs)?;
            }
            let rows = self.source.array(at + 0x20, 4, Some(0x8080052C))?;
            ensure!(!rows.is_empty(), "Source sequence control has no children");
            let mut children = Vec::new();
            for row in rows {
                let domain = self.source.u16(row)? as usize;
                ensure!(domain < 3, "Source sequence child domain differs");
                children.push(self.node(
                    domain,
                    self.source.u16(row + 2)? as usize,
                    i16::try_from(index)?,
                    depth + 1,
                )?);
            }
            match (class, self.source.u16(at + 0x1C)?) {
                (0x808007B0, 0) if self.source.u8(at + 0x1E)? == 1 => {
                    native::Kind::Repeat(children)
                }
                (0x808007B0, 0) => native::Kind::Serial(children),
                (0x80800691, 1) => native::Kind::Parallel(children),
                (0x80800757, 6) => {
                    let input = self.source.u32(at + 0x48)?;
                    let bounds = [self.source.f32(at + 0x40)?, self.source.f32(at + 0x44)?];
                    ensure!(
                        (input as usize) < self.topology.inputs && bounds[0] <= bounds[1],
                        "Source sequence range or input differs"
                    );
                    native::Kind::Range {
                        input,
                        bounds,
                        children,
                    }
                }
                (0x80800403, 2) => {
                    let weights = self.source.array(at + 0x40, 4, Some(0x8080000F))?;
                    ensure!(
                        weights.len() == children.len(),
                        "Source sequence weight count differs"
                    );
                    let weights = weights
                        .into_iter()
                        .zip(children)
                        .map(|(row, child)| Ok((self.source.f32(row)?, child)))
                        .collect::<Result<Vec<_>>>()?;
                    let total: f32 = weights.iter().map(|(weight, _)| *weight).sum();
                    ensure!(
                        total.is_finite()
                            && total > 0.
                            && weights.iter().all(|(weight, _)| *weight >= 0.),
                        "Source sequence choice has invalid weights"
                    );
                    native::Kind::Weighted(weights)
                }
                _ => anyhow::bail!("Unsupported source sequence control {class:08X}"),
            }
        };
        // Byte offsets remain distinct when authored names are repeated. Restore names after writing.
        Ok(native::Node {
            name: u32::try_from(at)?,
            timing,
            kind,
        })
    }
}
