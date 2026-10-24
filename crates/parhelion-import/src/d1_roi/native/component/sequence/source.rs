//! Read a bounded scalar-only source sequence without dropping control branches.
use super::*;

fn timing(source: &Payload, at: usize) -> Result<sequence::Timing> {
    ensure!(
        source.u32(at + 8)? & !0x1f == 0,
        "Source sequence timing flags differ"
    );
    Ok(sequence::Timing {
        delay: [source.f32(at + 12)?, source.f32(at + 16)?],
        duration: [source.f32(at + 20)?, source.f32(at + 24)?],
        flags: source.u32(at + 8)?,
    })
}

struct Tree<'a> {
    source: &'a Payload,
    controls: Vec<usize>,
    events: Vec<usize>,
    visited: BTreeSet<usize>,
    inputs: usize,
}

impl Tree<'_> {
    fn node(&mut self, event: bool, index: usize, parent: i16) -> Result<sequence::Node> {
        let at = *(if event { &self.events } else { &self.controls })
            .get(index)
            .context("Source sequence selector exceeds its table")?;
        ensure!(
            self.visited.insert(at) && self.source.i16(at + 6)? == parent,
            "Source sequence is cyclic, shared or has a different parent"
        );
        let class = self.source.u32(at - 4)?;
        let kind = if event {
            ensure!(
                class == 0x80800507 && self.source.u32(at + 28)? == 2,
                "Source sequence has an unsupported event"
            );
            sequence::Kind::Event(0x808093C4)
        } else {
            ensure!(
                self.source.u64(at + 0x30)? == 0
                    && self.source.u64(at + 0x38)? == 0
                    && self.source.u8(at + 0x1e)? <= 1
                    && self.source.u8(at + 0x1f)? == 0,
                "Source sequence control has unsupported conditions"
            );
            let mut children = Vec::new();
            for row in self.source.array(at + 0x20, 4, Some(0x8080052C))? {
                let event = self.source.u16(row)?;
                ensure!(event <= 1, "Source sequence child kind differs");
                children.push(self.node(
                    event == 1,
                    self.source.u16(row + 2)? as usize,
                    i16::try_from(index)?,
                )?);
            }
            match (class, self.source.u16(at + 28)?) {
                // Kind 0 runs its children in order. The source header lies 0x10 before the
                // native one, so byte +1E is native +32, which activates the control again.
                (0x808007B0, 0) if self.source.u8(at + 0x1e)? == 1 => {
                    sequence::Kind::Repeat(children)
                }
                (0x808007B0, 0) => sequence::Kind::Serial(children),
                (0x80800691, 1) => sequence::Kind::Parallel(children),
                (0x80800757, 6) => {
                    let input = self.source.u32(at + 0x48)?;
                    ensure!(
                        (input as usize) < self.inputs,
                        "Source range input exceeds its table"
                    );
                    sequence::Kind::Range {
                        input,
                        bounds: [self.source.f32(at + 0x40)?, self.source.f32(at + 0x44)?],
                        children,
                    }
                }
                (0x80800403, 2) => {
                    let weights = self.source.array(at + 0x40, 4, Some(0x8080000F))?;
                    ensure!(
                        weights.len() == children.len(),
                        "Source sequence weight count differs"
                    );
                    sequence::Kind::Weighted(
                        weights
                            .into_iter()
                            .zip(children)
                            .map(|(row, child)| Ok((self.source.f32(row)?, child)))
                            .collect::<Result<_>>()?,
                    )
                }
                _ => anyhow::bail!("Unsupported source scalar sequence control {class:08X}"),
            }
        };
        // Offset keys identify every node while assembling. Authored names are restored afterward.
        Ok(sequence::Node {
            name: u32::try_from(at)?,
            timing: timing(self.source, at)?,
            kind,
        })
    }
}

pub(super) fn tree(source: &Payload, inputs: usize) -> Result<sequence::Node> {
    let r = source.pointer(24)?;
    ensure!(
        source.u32(r - 4)? == 0x80800610 && source.u64(r + 0x180)? == 0,
        "Source scalar sequence resource differs or contains external events"
    );
    let read = |field| {
        source
            .array(r + field, 8, Some(0x80800629))?
            .into_iter()
            .map(|at| source.pointer(at))
            .collect::<Result<Vec<_>>>()
    };
    let mut tree = Tree {
        source,
        controls: read(0x160)?,
        events: read(0x170)?,
        visited: BTreeSet::new(),
        inputs,
    };
    ensure!(
        !tree.controls.is_empty() && !tree.events.is_empty(),
        "Source scalar sequence is empty"
    );
    let root = tree.node(false, 0, -1)?;
    ensure!(
        tree.visited.len() == tree.controls.len() + tree.events.len(),
        "Source sequence contains unreachable nodes"
    );
    Ok(root)
}

pub(super) fn events(
    source: &Payload,
    target: &mut Payload,
    written: &sequence::Written,
    outputs: usize,
) -> Result<()> {
    for (&from, &to) in &written.events {
        let from = from as usize;
        let ordinal = source.u32(from + 0x24)?;
        ensure!(
            (ordinal as usize) < outputs,
            "Source numeric event output exceeds its table"
        );
        put(&mut target.0, to + 0x38, &source.bytes::<8>(from + 0x20)?)?;
        let code = source.array_range(from + 0x28, 1, Some(0x80800009))?;
        let constants = source.array(from + 0x38, 16, Some(0x80800090))?;
        let lanes = constants
            .iter()
            .map(|&at| {
                Ok([
                    source.u32(at)?,
                    source.u32(at + 4)?,
                    source.u32(at + 8)?,
                    source.u32(at + 12)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            lanes
                .iter()
                .flatten()
                .all(|&v| f32::from_bits(v).is_finite()),
            "Nonfinite sequence constant"
        );
        let inputs = source.array(from + 0x58, 4, Some(0x80800007))?;
        ensure!(
            inputs.len() == 1
                && source.u32(inputs[0])? == u32::MAX
                && source.u64(from + 0x48)? == 1
                && source.u32(from + 0x50)? == 1
                && source.u32(from + 0x54)? <= 1,
            "Source numeric event requires additional input translation"
        );
        channel::program::validate(&source.0[code.clone()], &lanes, 1)?;
        array(&mut target.0, to + 0x40, 0x80800009, &source.0[code], 1)?;
        let constants = constants
            .into_iter()
            .map(|at| source.bytes::<16>(at))
            .collect::<Result<Vec<_>>>()?
            .concat();
        array(&mut target.0, to + 0x50, 0x80800090, &constants, 16)?;
        // Native dimensions are two u64 values. D1's second word is a u32 output
        // count followed by a compiler flag, not a native high count word.
        put(&mut target.0, to + 0x60, &1u64.to_le_bytes())?;
        put(&mut target.0, to + 0x68, &1u64.to_le_bytes())?;
        // The source explicitly lists its builtin time input as -1. Native events supply it implicitly.
        array(&mut target.0, to + 0x70, 0x80800007, &[], 4)?;
    }
    let d = target.pointer(24)?;
    for field in [0x158, 0x168] {
        for row in target.array(d + field, 24, Some(0x808093E6))? {
            let to = target.pointer(row + 16)?;
            let from = target.u32(to + 16)? as usize;
            put(&mut target.0, to + 16, &source.bytes::<4>(from)?)?;
            put(&mut target.0, to + 0x30, &source.bytes::<4>(from + 0x1c)?)?;
        }
    }
    Ok(())
}
