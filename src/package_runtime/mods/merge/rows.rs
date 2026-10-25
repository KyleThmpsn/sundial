//! The row engine (Sunrise f1e7c66 `table_merge/rows.cpp`). It unifies every input's ops by the
//! array they rebuild, merges stock rows three ways, appends each input's added rows after the
//! rows earlier inputs added, and rewrites every pointer that moved.
use super::layout::{
    HEADER_SIZE, MARKER_BACK, align, fits, i64_at, optional_array_at, put, u32_at, u64_at,
};
use super::sidecar::{
    DESCRIPTOR_SIZE, FieldKind, NO_ELEMENT_SHAPE, ORDERED, Op as OpRecord, OpKind, Shape,
    ShapeField, TOP_LEVEL_OP, UNIQUE_KEY,
};
use super::{Conflict, Input, Status, entry_of};
use std::collections::{BTreeMap, BTreeSet};

/// Edits clash per aligned 8-byte word, so no field of 8 bytes or less mixes two mods.
pub(super) const WORD_SIZE: usize = 8;
/// Authored array headers and added entry regions start on 16-byte boundaries.
pub(super) const ROW_ALIGNMENT: u64 = 16;
/// Source id of the stock bytes, beside the input indices.
const STOCK: usize = usize::MAX;
/// Merged id of no op: the parent of a top-level op, or an op an input doesn't declare.
const NO_OP: u32 = u32::MAX;
/// Sidecar offsets are u32, so a merged entry must stay below 4 GiB.
const ENTRY_LIMIT: u64 = u32::MAX as u64;
/// Row skip marks: the byte merge leaves both alone, and a nested descriptor is rebuilt.
const POINTER_BYTE: u8 = 1;
const NESTED_BYTE: u8 = 2;

/// Bytes two inputs both changed, keyed by (earlier input, later input).
type Tally = BTreeMap<(usize, usize), u32>;

/// Whether the stock u64 at +0 holds the stock size, so a merged entry holds its own.
pub(super) fn has_size_word(stock: &[u8]) -> bool {
    u64_at(stock, 0).is_some_and(|size| size == stock.len() as u64)
}

/// A stock range the byte merge leaves to the row engine: an array, a descriptor, a header count
/// or the size word.
#[derive(Clone, Copy, Debug)]
pub(super) struct Span {
    pub begin: u32,
    pub end: u32,
}

/// The merged entry being built.
pub(super) struct Frame<'s> {
    /// Proven stock bytes, `[0, stock size)`.
    pub stock: &'s [u8],
    pub output: Vec<u8>,
    /// Per input: where its added region now starts, minus the stock size.
    pub blob_delta: Vec<i64>,
}

/// Merges the stock-region bytes the row engine doesn't own, given its array and structure spans.
pub(super) type SpanMerge<'m> =
    dyn FnMut(&[Span], &mut Vec<u8>, &mut Vec<Conflict>) -> Result<(), Status> + 'm;

/// Three-way merges one input's row into `merged`, one row-relative word at a time. A word the
/// input changed is taken whole, minus skipped bytes, and a clash goes to the input.
fn three_way(
    merged: &mut [u8],
    writers: &mut [usize],
    stock: &[u8],
    view: &[u8],
    skip: &[u8],
    input: usize,
    tally: &mut Tally,
) {
    let open = |at: usize| skip.is_empty() || skip[at] == 0;
    for word in (0..merged.len()).step_by(WORD_SIZE) {
        let end = (word + WORD_SIZE).min(merged.len());
        if !(word..end).any(|at| open(at) && view[at] != stock[at]) {
            continue;
        }
        for at in word..end {
            if !open(at) || merged[at] == view[at] {
                continue;
            }
            if merged[at] != stock[at] {
                *tally.entry((writers[at], input)).or_default() += 1;
            }
            merged[at] = view[at];
            writers[at] = input;
        }
    }
}

/// One array as one source holds it. `header` is 0 when the source has none.
#[derive(Clone, Copy, Debug, Default)]
struct View {
    descriptor: usize,
    header: usize,
    data: usize,
    count: u64,
}

/// One op merged across every input that declares it.
struct Op {
    parent: u32,
    record: OpRecord,
    /// The first input that declares the op. Its shape is the reference.
    owner: usize,
    stride: usize,
    /// Per input: its op index, or `NO_OP`.
    local: Vec<u32>,
    stock: View,
    views: Vec<View>,
    /// Per input: rows that earlier inputs added.
    before: Vec<u64>,
    count: u64,
    /// The merged header in the output, or 0 when the merged op has no array.
    header: usize,
    /// 1 for each row byte a pointer field covers.
    pointer_bytes: Vec<u8>,
    /// Sorted map only: the merged rows in key order.
    map_rows: Vec<u8>,
}

/// One pointer field: where it was read, and its target mapped into the output.
#[derive(Clone, Copy, Debug)]
struct Pointer {
    null: bool,
    target: usize,
    raw: usize,
    count: u64,
    source: usize,
    field: usize,
}

impl Pointer {
    fn empty(source: usize, field: usize) -> Self {
        Self {
            null: true,
            target: 0,
            raw: 0,
            count: 0,
            source,
            field,
        }
    }
    /// Whether two pointer fields name the same output target.
    fn same(&self, other: &Self) -> bool {
        self.null == other.null
            && self.count == other.count
            && (self.null || self.target == other.target)
    }
}

/// One array span of one source, for the location map.
#[derive(Clone, Copy, Debug)]
struct Region {
    begin: usize,
    end: usize,
    op: u32,
    source: usize,
}

/// Where one address landed in the merged ops. `op` is `NO_OP` outside every array.
#[derive(Clone, Copy, Debug)]
struct Location {
    op: u32,
    in_row: bool,
    offset: usize,
}

impl Default for Location {
    fn default() -> Self {
        Self {
            op: NO_OP,
            in_row: false,
            offset: 0,
        }
    }
}

/// One sorted map row while the union is built.
struct MapSlot<'s> {
    row: Vec<u8>,
    /// The stock row, or `None` for a key some input added.
    stock: Option<&'s [u8]>,
    /// Per byte: the input that last wrote it. Empty until a stock row changes.
    writers: Vec<usize>,
    adder: usize,
}

/// The region holding `at`. Regions are sorted and don't overlap.
fn find_region(regions: &[Region], at: usize) -> Option<&Region> {
    let index = regions.partition_point(|region| region.begin <= at);
    let region = regions.get(index.checked_sub(1)?)?;
    (at < region.end).then_some(region)
}

/// Sorts one source's regions. `false` when two of them overlap.
fn sort_regions(regions: &mut [Region]) -> bool {
    regions.sort_by_key(|region| region.begin);
    regions.windows(2).all(|pair| pair[0].end <= pair[1].begin)
}

/// Merges the ops of one entry: unify, resolve, count, place, write.
struct Engine<'a, 's> {
    stock: &'s [u8],
    inputs: &'s [Input<'a>],
    entry: u32,
    copies: &'s [Vec<u8>],
    ops: Vec<Op>,
    /// Per input: the ops it declares.
    records: Vec<&'a [OpRecord]>,
    /// Per input: its op index to merged op id.
    merged: Vec<Vec<u32>>,
    ids: BTreeMap<(u32, u32, u32), u32>,
    /// Nested ops by (parent op, parent row).
    children: BTreeMap<(u32, u64), Vec<u32>>,
    output: Vec<u8>,
    blob_delta: Vec<i64>,
    stock_regions: Vec<Region>,
    input_regions: Vec<Vec<Region>>,
    tally: Tally,
    visited: BTreeSet<(usize, usize)>,
    row: Vec<u8>,
    writers: Vec<usize>,
    row_skip: Vec<u8>,
    changed: Vec<bool>,
}

impl<'a, 's> Engine<'a, 's> {
    fn source_bytes(&self, source: usize) -> &[u8] {
        if source == STOCK {
            self.stock
        } else {
            &self.copies[source]
        }
    }

    fn view(op: &Op, source: usize) -> View {
        if source == STOCK {
            op.stock
        } else {
            op.views[source]
        }
    }

    /// The input whose shapes describe `source`'s rows of `op`.
    fn holder_of(op: &Op, source: usize) -> usize {
        if source != STOCK && op.local[source] != NO_OP {
            source
        } else {
            op.owner
        }
    }

    /// The shape `input` declares for `op`. The input declares it.
    fn shape_of(&self, op: &Op, input: usize) -> &'a Shape {
        let record = &self.records[input][op.local[input] as usize];
        self.inputs[input]
            .sidecar
            .and_then(|sidecar| sidecar.shape(record.shape))
            .expect("a declared op names a shape the sidecar holds")
    }

    /// The merged op a row pointer field of `input`'s shape names.
    fn row_op(&self, input: usize, field: &ShapeField) -> u32 {
        if field.kind != FieldKind::RowPointer {
            return NO_OP;
        }
        self.merged[input]
            .get(usize::from(field.reference))
            .copied()
            .unwrap_or(NO_OP)
    }

    /// Unifies every input's ops by (parent, parent row, descriptor) and reads their arrays.
    fn build(&mut self) -> Result<(), Status> {
        for input in self.inputs {
            self.records
                .push(entry_of(input, self.entry).map_or(&[][..], |entry| &entry.ops[..]));
        }
        for input in 0..self.inputs.len() {
            self.merged.push(vec![NO_OP; self.records[input].len()]);
            // A parent op comes before its children, so its merged id is already known.
            for local in 0..self.records[input].len() {
                self.unify(input, local as u32)?;
            }
        }
        for id in 0..self.ops.len() {
            let stock = self.resolve(id, STOCK)?;
            if stock.count != u64::from(self.ops[id].record.stock_count) {
                return Err(Status::Stock);
            }
            self.ops[id].stock = stock;
            // An input that doesn't declare the op must keep the stock row count.
            for input in 0..self.copies.len() {
                let view = self.resolve(id, input)?;
                let op = &self.ops[id];
                let expected = if op.local[input] == NO_OP {
                    op.record.stock_count
                } else {
                    self.records[input][op.local[input] as usize].mod_count
                };
                if view.count != u64::from(expected) {
                    return Err(Status::Mismatch);
                }
                self.ops[id].views[input] = view;
            }
        }
        Ok(())
    }

    /// Maps op `local` of one input onto a merged op, creating it on first sight.
    fn unify(&mut self, input: usize, local: u32) -> Result<(), Status> {
        let record = self.records[input][local as usize];
        let shape = self.inputs[input]
            .sidecar
            .and_then(|sidecar| sidecar.shape(record.shape))
            .ok_or(Status::Format)?;
        let nested = record.parent_op != TOP_LEVEL_OP;
        let parent = if nested {
            self.merged[input][record.parent_op as usize]
        } else {
            NO_OP
        };
        // An array inside an added row belongs to that row, and the row's own pointers move it.
        if nested
            && (parent == NO_OP
                || record.parent_row >= self.ops[parent as usize].record.stock_count)
        {
            return Ok(());
        }
        let key = (
            parent,
            if parent == NO_OP {
                0
            } else {
                record.parent_row
            },
            record.descriptor,
        );
        let next = self.ops.len() as u32;
        let (id, created) = match self.ids.get(&key) {
            Some(&id) => (id, false),
            None => {
                self.ids.insert(key, next);
                (next, true)
            }
        };
        if created {
            self.admit(&record, shape, parent)?;
        } else {
            self.agree(&self.ops[id as usize], &record, shape)?;
        }
        if created {
            let mut pointer_bytes = vec![0u8; shape.stride as usize];
            for field in &shape.fields {
                let start = field.row_offset as usize;
                pointer_bytes[start..start + field.kind.size()].fill(POINTER_BYTE);
            }
            let count = self.inputs.len();
            self.ops.push(Op {
                parent,
                record,
                owner: input,
                stride: shape.stride as usize,
                local: vec![NO_OP; count],
                stock: View::default(),
                views: vec![View::default(); count],
                before: vec![0; count],
                count: 0,
                header: 0,
                pointer_bytes,
                map_rows: Vec::new(),
            });
        }
        self.ops[id as usize].local[input] = local;
        self.merged[input][local as usize] = id;
        Ok(())
    }

    /// Whether the engine can rebuild a new op of this form.
    fn admit(&mut self, record: &OpRecord, shape: &Shape, parent: u32) -> Result<(), Status> {
        // Sorted map rows move by key, so they may hold no pointers.
        if record.kind == OpKind::SortedMap && !shape.fields.is_empty() {
            return Err(Status::Unsupported);
        }
        if parent == NO_OP {
            return Ok(());
        }
        // A nested op lives in a row of an append op.
        if self.ops[parent as usize].record.kind != OpKind::Append {
            return Err(Status::Unsupported);
        }
        let id = self.ops.len() as u32;
        self.children
            .entry((parent, u64::from(record.parent_row)))
            .or_default()
            .push(id);
        Ok(())
    }

    /// Whether a later input declares the op the same way.
    fn agree(&self, op: &Op, record: &OpRecord, shape: &Shape) -> Result<(), Status> {
        let first = &op.record;
        let reference = self.shape_of(op, op.owner);
        let same_fields = shape.fields.len() == reference.fields.len()
            && shape
                .fields
                .iter()
                .zip(&reference.fields)
                .all(|(one, other)| one.row_offset == other.row_offset && one.kind == other.kind);
        if record.kind != first.kind
            || record.flags != first.flags
            || record.key_width != first.key_width
            || record.key_offset != first.key_offset
            || record.stock_count != first.stock_count
            || record.limit != first.limit
            || shape.stride != reference.stride
            || !same_fields
        {
            return Err(Status::Mismatch);
        }
        Ok(())
    }

    /// Reads op `id`'s array as `source` holds it.
    fn resolve(&self, id: usize, source: usize) -> Result<View, Status> {
        let op = &self.ops[id];
        let bytes = self.source_bytes(source);
        let mut at = u64::from(op.record.descriptor);
        if op.parent != NO_OP {
            // The parent row is a stock row, which every source holds.
            let parent = &self.ops[op.parent as usize];
            at += Self::view(parent, source).data as u64
                + u64::from(op.record.parent_row) * parent.stride as u64;
        }
        if !fits(bytes, at, DESCRIPTOR_SIZE as u64) {
            return Err(Status::Format);
        }
        let array = optional_array_at(bytes, at as usize).ok_or(Status::Format)?;
        let view = View {
            descriptor: at as usize,
            data: array.data,
            header: if array.data == 0 {
                0
            } else {
                array.data - HEADER_SIZE
            },
            count: array.count,
        };
        if !fits(bytes, view.data as u64, array.count * op.stride as u64) {
            return Err(Status::Format);
        }
        Ok(view)
    }

    /// Counts every merged op, builds each sorted map's union, and checks the limit.
    fn count_rows(&mut self) -> Result<(), Status> {
        for id in 0..self.ops.len() {
            if self.ops[id].record.kind == OpKind::SortedMap {
                let mut slots = BTreeMap::new();
                self.map_rows(id, STOCK, &mut slots)?;
                for input in 0..self.inputs.len() {
                    self.map_rows(id, input, &mut slots)?;
                }
                let op = &mut self.ops[id];
                op.count = slots.len() as u64;
                for slot in slots.into_values() {
                    op.map_rows.extend_from_slice(&slot.row);
                }
            } else {
                let op = &mut self.ops[id];
                let mut added = 0u64;
                for input in 0..self.inputs.len() {
                    op.before[input] = added;
                    added += op.views[input].count - u64::from(op.record.stock_count);
                }
                op.count = u64::from(op.record.stock_count) + added;
            }
            let op = &self.ops[id];
            if op.count > u64::from(op.record.limit) {
                return Err(Status::Limit);
            }
        }
        Ok(())
    }

    /// Adds one source's sorted map rows to the union. `Order` when the source's keys don't
    /// ascend, `Mismatch` when it dropped a stock key.
    fn map_rows(
        &mut self,
        id: usize,
        source: usize,
        slots: &mut BTreeMap<u32, MapSlot<'s>>,
    ) -> Result<(), Status> {
        let op = &self.ops[id];
        let from = Self::view(op, source);
        let stride = op.stride;
        let key_offset = op.record.key_offset as usize;
        let stock_count = u64::from(op.record.stock_count);
        let (stock, copies) = (self.stock, self.copies);
        let bytes: &'s [u8] = if source == STOCK {
            stock
        } else {
            &copies[source]
        };
        let mut present = 0u64;
        for row in 0..from.count as usize {
            let at = from.data + row * stride;
            let key = u32_at(bytes, at + key_offset).ok_or(Status::Format)?;
            if row != 0 && key <= u32_at(bytes, at - stride + key_offset).ok_or(Status::Format)? {
                return Err(Status::Order);
            }
            let row_bytes = &bytes[at..at + stride];
            match slots.get_mut(&key) {
                None => {
                    slots.insert(
                        key,
                        MapSlot {
                            row: row_bytes.to_vec(),
                            stock: (source == STOCK).then_some(row_bytes),
                            writers: Vec::new(),
                            adder: source,
                        },
                    );
                }
                Some(slot) => {
                    if let Some(stock) = slot.stock {
                        present += 1;
                        slot.writers.resize(stride, STOCK);
                        three_way(
                            &mut slot.row,
                            &mut slot.writers,
                            stock,
                            row_bytes,
                            &[],
                            source,
                            &mut self.tally,
                        );
                    } else if slot.row != row_bytes {
                        // Two inputs added one key with different rows: the later row wins.
                        let differing = slot
                            .row
                            .iter()
                            .zip(row_bytes)
                            .filter(|(old, new)| old != new)
                            .count() as u32;
                        *self.tally.entry((slot.adder, source)).or_default() += differing;
                        slot.row = row_bytes.to_vec();
                        slot.adder = source;
                    }
                }
            }
        }
        if source == STOCK || present == stock_count {
            Ok(())
        } else {
            Err(Status::Mismatch)
        }
    }

    /// Places every merged array at the output end and writes its header.
    fn place(&mut self) -> Result<(), Status> {
        for id in 0..self.ops.len() {
            let op = &self.ops[id];
            let source = if op.stock.header != 0 {
                STOCK
            } else {
                match op.views.iter().position(|view| view.header != 0) {
                    Some(input) => input,
                    // No source holds a header, so no source holds a row either.
                    None => continue,
                }
            };
            let from = Self::view(op, source);
            let header = align(self.output.len() as u64 + MARKER_BACK as u64, ROW_ALIGNMENT);
            let end = header + HEADER_SIZE as u64 + op.count * op.stride as u64;
            if end > ENTRY_LIMIT {
                return Err(Status::Capacity);
            }
            let header = header as usize;
            let count = op.count;
            let prefix = self.source_bytes(source)
                [from.header - MARKER_BACK..from.header + HEADER_SIZE]
                .to_vec();
            self.output.resize(end as usize, 0);
            put(&mut self.output, header - MARKER_BACK, &prefix);
            put(&mut self.output, header, &count.to_le_bytes());
            self.ops[id].header = header;
        }
        Ok(())
    }

    /// Indexes every source's array spans for the location map.
    fn index_regions(&mut self) -> Result<(), Status> {
        self.input_regions = vec![Vec::new(); self.inputs.len()];
        for (id, op) in self.ops.iter().enumerate() {
            for source in 0..=self.inputs.len() {
                let stock = source == self.inputs.len();
                let from = if stock { op.stock } else { op.views[source] };
                // An array an input left in place is found through the stock span.
                if from.header == 0
                    || (!stock && from.header == op.stock.header && from.count == op.stock.count)
                {
                    continue;
                }
                let region = Region {
                    begin: from.header - MARKER_BACK,
                    end: from.data + from.count as usize * op.stride,
                    op: id as u32,
                    source: if stock { STOCK } else { source },
                };
                if stock {
                    self.stock_regions.push(region);
                } else {
                    self.input_regions[source].push(region);
                }
            }
        }
        let mut valid = sort_regions(&mut self.stock_regions);
        for regions in &mut self.input_regions {
            valid = sort_regions(regions) && valid;
        }
        if valid { Ok(()) } else { Err(Status::Format) }
    }

    /// Maps one source address into the output. `Pointer` when it has no place there.
    fn map(&self, source: usize, at: usize) -> Result<(usize, Location), Status> {
        let mut location = Location::default();
        let hit = (source != STOCK)
            .then(|| find_region(&self.input_regions[source], at))
            .flatten()
            .or_else(|| find_region(&self.stock_regions, at));
        let Some(hit) = hit else {
            if at < self.stock.len() {
                return Ok((at, location));
            }
            let added = source != STOCK && at < self.copies[source].len();
            return if added {
                Ok(((at as i64 + self.blob_delta[source]) as usize, location))
            } else {
                Err(Status::Pointer)
            };
        };
        let op = &self.ops[hit.op as usize];
        let from = Self::view(op, hit.source);
        if op.header == 0 {
            return Err(Status::Pointer);
        }
        location.op = hit.op;
        if at < from.data {
            return Ok((op.header + at - from.header, location));
        }
        // Sorted map rows move by key, so nothing may point into them.
        if op.record.kind != OpKind::Append {
            return Err(Status::Pointer);
        }
        let index = ((at - from.data) / op.stride) as u64;
        location.in_row = true;
        location.offset = (at - from.data) % op.stride;
        // Added rows follow the stock rows and every earlier input's added rows.
        let row = if index < u64::from(op.record.stock_count) {
            index
        } else {
            index + op.before[hit.source]
        };
        let mapped = op.header + HEADER_SIZE + row as usize * op.stride + location.offset;
        Ok((mapped, location))
    }

    /// Reads and maps one pointer field. `Pointer` when it has no place in the output.
    fn read_pointer(
        &self,
        source: usize,
        field: usize,
        kind: FieldKind,
        row_op: u32,
    ) -> Result<Pointer, Status> {
        let bytes = self.source_bytes(source);
        let mut output = Pointer::empty(source, field);
        if !fits(bytes, field as u64, kind.size() as u64) {
            return Err(Status::Format);
        }
        let mut base = field;
        if kind == FieldKind::OwnedArray {
            output.count = u64_at(bytes, field).ok_or(Status::Format)?;
            base = field + 8;
        }
        let value = i64_at(bytes, base).ok_or(Status::Format)?;
        if value == 0 {
            return Ok(output);
        }
        if value < -(base as i64) || value >= (bytes.len() - base) as i64 {
            return Err(Status::Pointer);
        }
        output.null = false;
        output.raw = (base as i64 + value) as usize;
        let (target, location) = self.map(source, output.raw)?;
        output.target = target;
        // A row pointer must land on a row start of the op its shape names.
        if kind == FieldKind::RowPointer
            && (row_op == NO_OP
                || !location.in_row
                || location.op != row_op
                || location.offset != 0)
        {
            return Err(Status::Pointer);
        }
        Ok(output)
    }

    /// Writes one pointer field at an output address, counted from that address.
    fn write_pointer(&mut self, at: usize, kind: FieldKind, pointer: &Pointer) {
        let mut base = at;
        if kind == FieldKind::OwnedArray {
            put(&mut self.output, at, &pointer.count.to_le_bytes());
            base = at + 8;
        }
        let offset = if pointer.null {
            0
        } else {
            pointer.target as i64 - base as i64
        };
        put(&mut self.output, base, &offset.to_le_bytes());
    }

    /// Writes one op's descriptor at an output address.
    fn write_descriptor(&mut self, at: usize, id: usize) {
        let op = &self.ops[id];
        let pointer = Pointer {
            null: op.header == 0,
            target: op.header,
            raw: 0,
            count: op.count,
            source: STOCK,
            field: 0,
        };
        self.write_pointer(at, FieldKind::OwnedArray, &pointer);
    }

    /// Writes one pointer at `at`, then relocates the array it owns. `holder` owns the shapes.
    fn emit(
        &mut self,
        at: usize,
        field: &ShapeField,
        holder: usize,
        pointer: &Pointer,
        write: bool,
    ) -> Result<(), Status> {
        if write {
            self.write_pointer(at, field.kind, pointer);
        }
        if field.kind != FieldKind::OwnedArray || pointer.null {
            return Ok(());
        }
        self.walk(pointer.source, holder, pointer.field, field.reference)
    }

    /// Writes one merged op: its descriptor, its rows and its key checks.
    fn write_op(&mut self, id: usize) -> Result<(), Status> {
        if self.ops[id].parent == NO_OP {
            let descriptor = self.ops[id].record.descriptor as usize;
            self.write_descriptor(descriptor, id);
        }
        if self.ops[id].record.kind == OpKind::SortedMap {
            let start = self.ops[id].header + HEADER_SIZE;
            let rows = std::mem::take(&mut self.ops[id].map_rows);
            self.output[start..start + rows.len()].copy_from_slice(&rows);
            self.ops[id].map_rows = rows;
            return Ok(());
        }
        for row in 0..u64::from(self.ops[id].record.stock_count) {
            self.write_stock_row(id, row)?;
        }
        for input in 0..self.inputs.len() {
            self.write_added_rows(id, input)?;
        }
        self.check_keys(id)
    }

    /// Merges stock row `row` of op `id` across the inputs and writes it.
    fn write_stock_row(&mut self, id: usize, row: u64) -> Result<(), Status> {
        let op = &self.ops[id];
        let stride = op.stride;
        let stock_at = op.stock.data + row as usize * stride;
        let stock = self.stock;
        let stock_row: &'s [u8] = &stock[stock_at..stock_at + stride];
        self.row = stock_row.to_vec();
        self.writers = vec![STOCK; stride];
        self.changed = vec![false; self.inputs.len()];
        self.row_skip = op.pointer_bytes.clone();
        let nested = self
            .children
            .get(&(id as u32, row))
            .cloned()
            .unwrap_or_default();
        for &child in &nested {
            let start = self.ops[child as usize].record.descriptor as usize;
            self.row_skip[start..start + DESCRIPTOR_SIZE].fill(NESTED_BYTE);
        }
        // Each input has one copy, in input order.
        for (input, copy) in self.copies.iter().enumerate() {
            let from = self.ops[id].views[input];
            let at = from.data + row as usize * stride;
            let view_row = &copy[at..at + stride];
            if from.data == self.ops[id].stock.data && view_row == stock_row {
                continue;
            }
            self.changed[input] = true;
            three_way(
                &mut self.row,
                &mut self.writers,
                stock_row,
                view_row,
                &self.row_skip,
                input,
                &mut self.tally,
            );
        }
        let at = self.ops[id].header + HEADER_SIZE + row as usize * stride;
        let merged = std::mem::take(&mut self.row);
        put(&mut self.output, at, &merged);
        self.row = merged;
        let status = self.merge_pointers(id, row, at);
        for &child in &nested {
            let descriptor = self.ops[child as usize].record.descriptor as usize;
            self.write_descriptor(at + descriptor, child as usize);
        }
        status
    }

    /// Three-way merges field `index` of one stock row by mapped target. `Pointer` when two
    /// inputs moved it to different targets.
    fn pick_pointer(&self, id: usize, row: u64, index: usize) -> Result<Pointer, Status> {
        let op = &self.ops[id];
        let field = self.shape_of(op, op.owner).fields[index];
        let offset = row as usize * op.stride + field.row_offset as usize;
        let mut pick = self.read_pointer(
            STOCK,
            op.stock.data + offset,
            field.kind,
            self.row_op(op.owner, &field),
        )?;
        let stock = pick;
        for input in 0..self.inputs.len() {
            if !self.changed[input] {
                continue;
            }
            let holder = Self::holder_of(op, input);
            let row_op = self.row_op(holder, &self.shape_of(op, holder).fields[index]);
            let candidate =
                self.read_pointer(input, op.views[input].data + offset, field.kind, row_op)?;
            if candidate.same(&stock) || candidate.same(&pick) {
                continue;
            }
            if !pick.same(&stock) {
                return Err(Status::Pointer);
            }
            pick = candidate;
        }
        Ok(pick)
    }

    /// Writes every pointer field of one merged stock row and relocates what they own.
    fn merge_pointers(&mut self, id: usize, row: u64, at: usize) -> Result<(), Status> {
        let reference = self.shape_of(&self.ops[id], self.ops[id].owner);
        for (index, field) in reference.fields.iter().enumerate() {
            // A nested op's descriptor is rebuilt, not merged.
            if self.row_skip[field.row_offset as usize] == NESTED_BYTE {
                continue;
            }
            let pick = self.pick_pointer(id, row, index)?;
            let holder = Self::holder_of(&self.ops[id], pick.source);
            let own = self.shape_of(&self.ops[id], holder).fields[index];
            self.emit(at + field.row_offset as usize, &own, holder, &pick, true)?;
        }
        Ok(())
    }

    /// Appends one input's added rows after the rows of every earlier input.
    fn write_added_rows(&mut self, id: usize, input: usize) -> Result<(), Status> {
        let op = &self.ops[id];
        let from = op.views[input];
        let stride = op.stride;
        let stock_count = u64::from(op.record.stock_count);
        let header = op.header;
        let before = op.before[input];
        // Added rows mean the input declares the op, so it has a shape.
        let shape = if from.count > stock_count {
            Some(self.shape_of(op, input))
        } else {
            None
        };
        for row in stock_count..from.count {
            let source = from.data + row as usize * stride;
            let at = header + HEADER_SIZE + (row + before) as usize * stride;
            let copied = self.copies[input][source..source + stride].to_vec();
            put(&mut self.output, at, &copied);
            for field in &shape
                .expect("an input that adds rows declares the op")
                .fields
            {
                let offset = field.row_offset as usize;
                let row_op = self.row_op(input, field);
                let pointer = self.read_pointer(input, source + offset, field.kind, row_op)?;
                self.emit(at + offset, field, input, &pointer, true)?;
            }
        }
        Ok(())
    }

    /// Relocates the pointers in one owned array once. `holder` owns the shapes.
    fn walk(
        &mut self,
        source: usize,
        holder: usize,
        field: usize,
        shape: u16,
    ) -> Result<(), Status> {
        if shape == NO_ELEMENT_SHAPE {
            return Ok(());
        }
        let array = optional_array_at(self.source_bytes(source), field).ok_or(Status::Pointer)?;
        if array.count == 0 || !self.visited.insert((source, array.data)) {
            return Ok(());
        }
        let element = self.inputs[holder]
            .sidecar
            .and_then(|sidecar| sidecar.shape(u32::from(shape)))
            .ok_or(Status::Format)?;
        if !fits(
            self.source_bytes(source),
            array.data as u64,
            array.count * u64::from(element.stride),
        ) {
            return Err(Status::Format);
        }
        for index in 0..array.count as usize {
            for item in &element.fields {
                let at = array.data + index * element.stride as usize + item.row_offset as usize;
                self.relocate(source, holder, at, item)?;
            }
        }
        Ok(())
    }

    /// Rewrites one pointer field of an owned element at its output address.
    fn relocate(
        &mut self,
        source: usize,
        holder: usize,
        at: usize,
        field: &ShapeField,
    ) -> Result<(), Status> {
        let pointer = self.read_pointer(source, at, field.kind, self.row_op(holder, field))?;
        let (out, location) = self.map(source, at)?;
        // Owned elements never sit inside a rebuilt array.
        if location.op != NO_OP {
            return Err(Status::Pointer);
        }
        // A stock pointer another input edited in place can't be relocated, so the merge falls
        // back.
        let size = field.kind.size();
        if at < self.stock.len() {
            let held = &self.source_bytes(source)[at..at + size];
            if !fits(&self.output, out as u64, size as u64) || self.output[out..out + size] != *held
            {
                return Err(Status::Pointer);
            }
        }
        // A stock element is rewritten only when its target moved.
        let moved = at >= self.stock.len() || (!pointer.null && pointer.target != pointer.raw);
        self.emit(out, field, holder, &pointer, moved)
    }

    /// `Order` when ordered keys don't ascend, `Duplicate` when a new key repeats.
    fn check_keys(&self, id: usize) -> Result<(), Status> {
        let op = &self.ops[id];
        let unique = op.record.flags & UNIQUE_KEY != 0;
        let ordered = op.record.flags & ORDERED != 0;
        if !unique && !ordered {
            return Ok(());
        }
        let mut keys: Vec<(u32, bool)> = Vec::new();
        for row in 0..op.count as usize {
            let at = row * op.stride + op.record.key_offset as usize;
            let key = u32_at(&self.output, op.header + HEADER_SIZE + at).ok_or(Status::Format)?;
            if ordered && keys.last().is_some_and(|&(last, _)| key <= last) {
                return Err(Status::Order);
            }
            let added = row as u64 >= u64::from(op.record.stock_count);
            let changed = added || u32_at(self.stock, op.stock.data + at) != Some(key);
            keys.push((key, changed));
        }
        // Stock may repeat a key. A repeat of an added or changed key is refused.
        keys.sort_unstable();
        let repeat = keys
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 && (pair[0].1 || pair[1].1));
        if unique && repeat {
            Err(Status::Duplicate)
        } else {
            Ok(())
        }
    }

    /// The stock array spans and structural spans of every op.
    fn spans(&self) -> Vec<Span> {
        let mut output = Vec::new();
        let mut add = |begin: usize, end: usize| {
            output.push(Span {
                begin: begin as u32,
                end: end as u32,
            });
        };
        if has_size_word(self.stock) {
            add(0, 8);
        }
        for op in &self.ops {
            let from = op.stock;
            add(from.descriptor, from.descriptor + DESCRIPTOR_SIZE);
            if from.header != 0 {
                add(
                    from.header - MARKER_BACK,
                    from.data + from.count as usize * op.stride,
                );
                add(from.header, from.header + 8);
            }
        }
        output.sort_by_key(|span| (span.begin, span.end));
        output
    }
}

/// Reads every op once, runs the byte merge, then rebuilds the ops at the end of the output.
/// Conflicts it adds carry the entry and the mods, and the caller fills the package.
pub(super) fn merge_ops(
    frame: &mut Frame<'_>,
    copies: &[Vec<u8>],
    inputs: &[Input<'_>],
    entry: u32,
    byte_merge: &mut SpanMerge<'_>,
    conflicts: &mut Vec<Conflict>,
) -> Result<(), Status> {
    if copies.len() != inputs.len() || frame.blob_delta.len() != inputs.len() {
        return Err(Status::Format);
    }
    let mut engine = Engine {
        stock: frame.stock,
        inputs,
        entry,
        copies,
        ops: Vec::new(),
        records: Vec::new(),
        merged: Vec::new(),
        ids: BTreeMap::new(),
        children: BTreeMap::new(),
        output: std::mem::take(&mut frame.output),
        blob_delta: frame.blob_delta.clone(),
        stock_regions: Vec::new(),
        input_regions: Vec::new(),
        tally: Tally::new(),
        visited: BTreeSet::new(),
        row: Vec::new(),
        writers: Vec::new(),
        row_skip: Vec::new(),
        changed: Vec::new(),
    };
    let result = engine.run(byte_merge, conflicts);
    frame.output = std::mem::take(&mut engine.output);
    result
}

impl Engine<'_, '_> {
    fn run(
        &mut self,
        byte_merge: &mut SpanMerge<'_>,
        conflicts: &mut Vec<Conflict>,
    ) -> Result<(), Status> {
        self.build()?;
        let spans = self.spans();
        byte_merge(&spans, &mut self.output, conflicts)?;
        self.count_rows()?;
        self.place()?;
        self.index_regions()?;
        for id in 0..self.ops.len() {
            self.write_op(id)?;
        }
        for (&(earlier, later), &bytes) in &self.tally {
            conflicts.push(Conflict {
                package: 0,
                entry: self.entry,
                earlier: self.inputs[earlier].owner,
                later: self.inputs[later].owner,
                bytes,
            });
        }
        Ok(())
    }
}
