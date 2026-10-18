//! Entry merges and the passes over them (Sunrise f1e7c66 `table_merge/entry_merge.cpp`).
use super::manifest::union_manifest;
use super::plan::{Shifts, check_sites, plan_shifts, shift_sites};
use super::rows::{Frame, ROW_ALIGNMENT, Span, WORD_SIZE, has_size_word, merge_ops};
use super::sidecar::{Entry, EntryKind, Op, OpKind, TOP_LEVEL_OP};
use super::{
    Conflict, Dropped, EntryInputs, Fallback, Ignored, Input, Outcome, Replacement, Shifted,
    Status, entry_of, tag_of,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Marks a stock byte no input has written yet.
const NO_OWNER: i32 = -1;

/// Whether the input declares an append op in this entry.
fn appends(spec: Option<&Entry>) -> bool {
    spec.is_some_and(|spec| spec.ops.iter().any(|op| op.kind == OpKind::Append))
}

/// Whether the input added rows to any append op of this entry.
fn adds_rows(spec: Option<&Entry>) -> bool {
    spec.is_some_and(|spec| {
        spec.ops
            .iter()
            .any(|op| op.kind == OpKind::Append && op.mod_count > op.stock_count)
    })
}

/// Whether any input of the entry declares an append op.
fn row_table(entry: &EntryInputs<'_>) -> bool {
    entry
        .inputs
        .iter()
        .any(|input| appends(entry_of(input, entry.entry)))
}

/// Whether the op's array and every parent row exist in the stock bytes.
fn stock_anchored(spec: &Entry, op: &Op) -> bool {
    // A parent op comes before its children, so the walk ends.
    let mut at = op;
    while at.parent_op != TOP_LEVEL_OP {
        let parent = &spec.ops[at.parent_op as usize];
        if at.parent_row >= parent.stock_count {
            return false;
        }
        at = parent;
    }
    true
}

/// Whether two stock-anchored ops of two inputs name the same array. Op indices differ between
/// inputs, so the descriptor path is compared.
fn same_array(first: &Entry, a: &Op, second: &Entry, b: &Op) -> bool {
    let (mut x, mut y) = (a, b);
    while x.descriptor == y.descriptor {
        let (top_x, top_y) = (x.parent_op == TOP_LEVEL_OP, y.parent_op == TOP_LEVEL_OP);
        if top_x || top_y {
            return top_x && top_y;
        }
        if x.parent_row != y.parent_row {
            return false;
        }
        x = &first.ops[x.parent_op as usize];
        y = &second.ops[y.parent_op as usize];
    }
    false
}

/// Rows earlier inputs added to each stock-anchored op of input `index`. 0 for an op inside an
/// added row, whose number names a different row in each mod.
fn op_local_shifts(entry: &EntryInputs<'_>, index: usize) -> Vec<u32> {
    let Some(spec) = entry_of(&entry.inputs[index], entry.entry) else {
        return Vec::new();
    };
    let mut shifts = vec![0u32; spec.ops.len()];
    for (op, shift) in spec.ops.iter().zip(&mut shifts) {
        if !stock_anchored(spec, op) {
            continue;
        }
        for earlier in &entry.inputs[..index] {
            let Some(before) = entry_of(earlier, entry.entry) else {
                continue;
            };
            for other in &before.ops {
                if other.kind == OpKind::Append
                    && stock_anchored(before, other)
                    && same_array(before, other, spec, op)
                {
                    *shift += other.mod_count - other.stock_count;
                }
            }
        }
    }
    shifts
}

/// A copy of one input with its sites shifted. With the row layer off the copy stays raw.
fn shifted_copy(
    entry: &EntryInputs<'_>,
    index: usize,
    shifts: &Shifts,
    rows_on: bool,
) -> Result<Vec<u8>, Status> {
    let input = &entry.inputs[index];
    let mut copy = input.bytes.to_vec();
    let has_sites = entry_of(input, entry.entry).is_some_and(|spec| !spec.sites.is_empty());
    if rows_on && has_sites {
        let local = op_local_shifts(entry, index);
        shift_sites(input, entry.entry, shifts, &local, &mut copy)?;
    }
    Ok(copy)
}

/// Writes the stock bytes back over every diff and checks them against the stock digest. `Stock`
/// when the copy wasn't built from the stock bytes the other inputs name.
fn restore_stock(input: &Input<'_>, spec: &Entry, first: &Entry) -> Result<Vec<u8>, Status> {
    if spec.stock_size != first.stock_size
        || spec.stock_sha256 != first.stock_sha256
        || spec.built_size as usize != input.bytes.len()
        || spec.built_size < spec.stock_size
    {
        return Err(Status::Stock);
    }
    let mut stock = input.bytes[..spec.stock_size as usize].to_vec();
    for diff in &spec.diffs {
        let start = diff.offset as usize;
        stock[start..start + diff.old.len()].copy_from_slice(&diff.old);
    }
    if Sha256::digest(&stock)[..] == spec.stock_sha256[..] {
        Ok(stock)
    } else {
        Err(Status::Stock)
    }
}

/// How one edit range sits against the spans the row engine owns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement {
    Outside,
    Inside,
    Crossing,
}

fn place(spans: &[Span], begin: u32, end: u32) -> Placement {
    let mut overlaps = false;
    for span in spans {
        if begin >= span.begin && end <= span.end {
            return Placement::Inside;
        }
        overlaps = overlaps || (begin < span.end && end > span.begin);
    }
    if overlaps {
        Placement::Crossing
    } else {
        Placement::Outside
    }
}

/// The stock-region ranges one later input changes: its diffs, and its sites there.
fn edit_ranges(spec: &Entry) -> Vec<(u32, u32)> {
    let mut ranges = spec
        .diffs
        .iter()
        .map(|diff| (diff.offset, diff.offset + diff.old.len() as u32))
        .chain(
            spec.sites
                .iter()
                .filter(|site| site.offset < spec.stock_size)
                .map(|site| (site.offset, site.offset + u32::from(site.width))),
        )
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges
}

/// The free stock range between spans that holds `[begin, end)`, which lies outside them.
fn gap_of(spans: &[Span], begin: u32, end: u32, stock_size: u32) -> (u32, u32) {
    let (mut low, mut high) = (0, stock_size);
    for span in spans {
        if span.end <= begin {
            low = low.max(span.end);
        } else if span.begin >= end {
            high = high.min(span.begin);
        }
    }
    (low, high)
}

/// Shared state of one entry's byte merge.
struct ByteMerge<'s> {
    stock: &'s [u8],
    /// The input that last wrote each stock byte, or `NO_OWNER`.
    owner: Vec<i32>,
    /// Conflicting bytes per earlier input, for the current later input.
    clashes: Vec<u32>,
}

impl ByteMerge<'_> {
    /// Three-way merges one range, one aligned word at a time, so no field mixes two mods. A word
    /// where the later copy undoes an earlier mod's byte is taken whole from the later copy.
    fn merge_range(
        &mut self,
        output: &mut [u8],
        copy: &[u8],
        later: usize,
        range: (u32, u32),
        gap: (u32, u32),
    ) {
        let owner = later as i32;
        let mut word = range.0 - range.0 % WORD_SIZE as u32;
        while word < range.1 {
            let first = word.max(gap.0) as usize;
            let last = (word + WORD_SIZE as u32).min(gap.1) as usize;
            let lost = (first..last).any(|at| {
                self.owner[at] != NO_OWNER && self.owner[at] < owner && output[at] != copy[at]
            });
            for at in first..last {
                let next = copy[at];
                let in_range = at >= range.0 as usize && at < range.1 as usize;
                if next == output[at] || (!lost && (!in_range || next == self.stock[at])) {
                    continue;
                }
                if self.owner[at] != NO_OWNER && self.owner[at] < owner {
                    self.clashes[self.owner[at] as usize] += 1;
                }
                output[at] = next;
                self.owner[at] = owner;
            }
            word += WORD_SIZE as u32;
        }
    }

    /// Applies every later input's stock-region edits that lie outside the row engine's spans.
    /// `Format` when an edit crosses a span edge.
    fn merge_bytes(
        &mut self,
        entry: &EntryInputs<'_>,
        specs: &[&Entry],
        copies: &[Vec<u8>],
        spans: &[Span],
        output: &mut [u8],
        conflicts: &mut Vec<Conflict>,
    ) -> Result<(), Status> {
        // Input 0's bytes are already in the output, so it owns every stock byte it changed.
        for ((owner, merged), stock) in self.owner.iter_mut().zip(&*output).zip(self.stock) {
            if merged != stock {
                *owner = 0;
            }
        }
        let stock_size = self.stock.len() as u32;
        for later in 1..specs.len() {
            self.clashes.fill(0);
            for (begin, end) in edit_ranges(specs[later]) {
                let placement = place(spans, begin, end);
                // A site that runs past the stock end has no stock bytes to merge against.
                if placement == Placement::Crossing || end > stock_size {
                    return Err(Status::Format);
                }
                if placement == Placement::Outside {
                    let gap = gap_of(spans, begin, end, stock_size);
                    self.merge_range(output, &copies[later], later, (begin, end), gap);
                }
            }
            for (earlier, &bytes) in self.clashes[..later].iter().enumerate() {
                if bytes != 0 {
                    conflicts.push(Conflict {
                        package: entry.package,
                        entry: entry.entry,
                        earlier: entry.inputs[earlier].owner,
                        later: entry.inputs[later].owner,
                        bytes,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Appends each later input's added region at the output end, keeping its 16-byte phase. Per
/// input, the new start of its added region minus the stock size, and 0 for input 0.
fn place_blobs(copies: &[Vec<u8>], stock_size: u32, output: &mut Vec<u8>) -> Vec<i64> {
    let mut delta = vec![0i64; copies.len()];
    for (index, copy) in copies.iter().enumerate().skip(1) {
        let size = output.len() as u64;
        // Added regions keep their offset modulo the row alignment, so their alignment holds.
        let phase = u64::from(stock_size) % ROW_ALIGNMENT;
        let mut start = size - size % ROW_ALIGNMENT + phase;
        if start < size {
            start += ROW_ALIGNMENT;
        }
        output.resize(start as usize, 0);
        output.extend_from_slice(&copy[stock_size as usize..]);
        delta[index] = start as i64 - i64::from(stock_size);
    }
    delta
}

/// Merges one table entry: proof, added regions, byte edits, rows, size word.
fn merge_table(
    entry: &EntryInputs<'_>,
    specs: &[&Entry],
    copies: &[Vec<u8>],
    conflicts: &mut Vec<Conflict>,
) -> Result<Vec<u8>, Status> {
    let mut stock = Vec::new();
    for (index, (input, spec)) in entry.inputs.iter().zip(specs).enumerate() {
        let restored = restore_stock(input, spec, specs[0])?;
        if index == 0 {
            stock = restored;
        }
    }
    let stock_size = specs[0].stock_size;
    let mut output = copies[0].clone();
    let blob_delta = place_blobs(copies, stock_size, &mut output);
    let mut frame = Frame {
        stock: &stock,
        output,
        blob_delta,
    };
    let mut bytes = ByteMerge {
        stock: &stock,
        owner: vec![NO_OWNER; stock_size as usize],
        clashes: vec![0; specs.len()],
    };
    // The row engine unifies the inputs' ops by array, and the byte merge works around its spans.
    let mut byte_merge = |spans: &[Span], output: &mut Vec<u8>, found: &mut Vec<Conflict>| {
        bytes.merge_bytes(entry, specs, copies, spans, output, found)
    };
    let first = conflicts.len();
    let status = merge_ops(
        &mut frame,
        copies,
        &entry.inputs,
        entry.entry,
        &mut byte_merge,
        conflicts,
    );
    for conflict in &mut conflicts[first..] {
        conflict.package = entry.package;
        conflict.entry = entry.entry;
    }
    status?;
    let mut output = frame.output;
    if has_size_word(&stock) {
        let size = output.len() as u64;
        output[..8].copy_from_slice(&size.to_le_bytes());
    }
    Ok(output)
}

/// Merges one entry that two or more inputs change, or says why it falls back.
fn merge_entry(
    entry: &EntryInputs<'_>,
    copies: &[Vec<u8>],
    conflicts: &mut Vec<Conflict>,
) -> Result<Vec<u8>, Status> {
    let mut specs = Vec::new();
    for input in &entry.inputs {
        let spec = entry_of(input, entry.entry).ok_or(Status::Unsupported)?;
        if specs
            .first()
            .is_some_and(|first: &&Entry| first.kind != spec.kind)
        {
            return Err(Status::Mismatch);
        }
        specs.push(spec);
    }
    match specs[0].kind {
        EntryKind::Table => merge_table(entry, &specs, copies, conflicts),
        EntryKind::Manifest => {
            let mut output = copies[0].clone();
            for copy in &copies[1..] {
                union_manifest(copy, &mut output)?;
            }
            Ok(output)
        }
        EntryKind::Opaque | EntryKind::Added => Err(Status::Unsupported),
    }
}

/// One merge pass over every entry, with fixed shifts.
struct Pass<'o> {
    shifts: &'o Shifts,
    rows_on: bool,
    outcome: &'o mut Outcome,
}

impl Pass<'_> {
    /// `Some` with why the row layer must turn off and the pass rerun, and the table at fault.
    fn run(&mut self, entries: &[EntryInputs<'_>]) -> Option<(Status, u32)> {
        for entry in entries {
            let off = match entry.inputs.len() {
                0 => None,
                1 => self.single(entry),
                _ => self.shared(entry),
            };
            if off.is_some() {
                return off;
            }
        }
        None
    }

    /// An entry one mod changes is replaced only when its sites moved.
    fn single(&mut self, entry: &EntryInputs<'_>) -> Option<(Status, u32)> {
        match shifted_copy(entry, 0, self.shifts, self.rows_on) {
            Ok(copy) => {
                self.replace_if_changed(entry, copy, entry.inputs[0].bytes);
                None
            }
            Err(status) => Some((status, tag_of(entry.package, entry.entry))),
        }
    }

    fn shared(&mut self, entry: &EntryInputs<'_>) -> Option<(Status, u32)> {
        let rows = row_table(entry);
        let tag = tag_of(entry.package, entry.entry);
        if rows && !self.rows_on {
            self.drop_rows(entry, tag);
            return None;
        }
        let mut copies = Vec::with_capacity(entry.inputs.len());
        for index in 0..entry.inputs.len() {
            match shifted_copy(entry, index, self.shifts, self.rows_on) {
                Ok(copy) => copies.push(copy),
                Err(status) => return Some((status, tag)),
            }
        }
        let served = entry
            .inputs
            .last()
            .expect("a shared entry has inputs")
            .bytes;
        let mut conflicts = Vec::new();
        match merge_entry(entry, &copies, &mut conflicts) {
            Ok(output) => {
                self.outcome.conflicts.extend(conflicts);
                self.replace_if_changed(entry, output, served);
                None
            }
            Err(status) if rows => Some((status, tag)),
            Err(status) => {
                self.outcome.fallbacks.push(Fallback {
                    package: entry.package,
                    entry: entry.entry,
                    reason: status,
                });
                // The last input wins, with its sites still pointing at its shifted rows.
                let last = copies.pop().expect("a shared entry has copies");
                self.replace_if_changed(entry, last, served);
                None
            }
        }
    }

    /// A row table with the row layer off keeps the last mod's rows only.
    fn drop_rows(&mut self, entry: &EntryInputs<'_>, tag: u32) {
        let reason = self
            .outcome
            .rows_off
            .map_or(Status::Unsupported, |(reason, _)| reason);
        self.outcome.fallbacks.push(Fallback {
            package: entry.package,
            entry: entry.entry,
            reason,
        });
        for input in &entry.inputs[..entry.inputs.len() - 1] {
            if adds_rows(entry_of(input, entry.entry)) {
                self.outcome.dropped.push(Dropped {
                    owner: input.owner,
                    table: tag,
                });
            }
        }
    }

    fn replace_if_changed(&mut self, entry: &EntryInputs<'_>, bytes: Vec<u8>, served: &[u8]) {
        if bytes != served {
            self.outcome.replacements.push(Replacement {
                package: entry.package,
                entry: entry.entry,
                bytes,
            });
        }
    }
}

/// A sidecar whose entry size or sites don't hold is ignored, and so is all merge data of its
/// mod. Its rows may have to move, and an ignored entry couldn't follow them.
fn check_inputs<'a>(entries: &[EntryInputs<'a>], outcome: &mut Outcome) -> Vec<EntryInputs<'a>> {
    let mut work = entries.to_vec();
    let mut seen = BTreeSet::new();
    let mut ignored = BTreeSet::new();
    for entry in &work {
        for input in &entry.inputs {
            let Some(spec) = entry_of(input, entry.entry) else {
                continue;
            };
            // Site offsets were checked against the built size, so the size must hold first.
            let status = if spec.built_size as usize != input.bytes.len() {
                Err(Status::Binding)
            } else {
                check_sites(input, entry.entry)
            };
            let Err(reason) = status else {
                continue;
            };
            ignored.insert(input.owner);
            // One warning per mod and package.
            if seen.insert((input.owner, entry.package)) {
                outcome.ignored.push(Ignored {
                    owner: input.owner,
                    package: entry.package,
                    reason,
                });
            }
        }
    }
    for entry in &mut work {
        for input in &mut entry.inputs {
            if ignored.contains(&input.owner) {
                input.sidecar = None;
            }
        }
    }
    work
}

/// Runs the plan and the passes.
pub(super) fn merge_all(entries: &[EntryInputs<'_>]) -> Outcome {
    let mut outcome = Outcome::default();
    let work = check_inputs(entries, &mut outcome);
    let mut shifts = Shifts::default();
    if let Err(off) = plan_shifts(&work, &mut shifts) {
        outcome.rows_off = Some(off);
        shifts.clear();
    }
    let rows_on = outcome.rows_off.is_none();
    let off = Pass {
        shifts: &shifts,
        rows_on,
        outcome: &mut outcome,
    }
    .run(&work);
    if let Some(off) = off {
        // Row tables can't merge, so every shift is dropped and the pass reruns. With the row
        // layer off no entry can turn it off again.
        outcome.rows_off = Some(off);
        outcome.replacements.clear();
        outcome.fallbacks.clear();
        outcome.conflicts.clear();
        outcome.dropped.clear();
        shifts.clear();
        let _ = Pass {
            shifts: &shifts,
            rows_on: false,
            outcome: &mut outcome,
        }
        .run(&work);
    }
    if outcome.rows_off.is_none() {
        outcome.shifted = shifts
            .iter()
            .map(|(owner, table, descriptor, rows)| Shifted {
                owner,
                table,
                descriptor,
                rows,
            })
            .collect();
    }
    outcome
        .replacements
        .sort_by_key(|replacement| (replacement.package, replacement.entry));
    outcome
}
