//! The `.merge` sidecar, version 2, as Sunrise f1e7c66 reads it (`table_merge/sidecar.cpp`) and
//! Sunrise Studio 007dd7a6 writes it (`MergeSidecarFormat.cs`). Little-endian packed records,
//! with nothing after the last entry.
use super::Status;

pub const MAGIC: u32 = 0x4752_4D53;
pub const VERSION: u16 = 2;
/// The loader refuses a larger file, so damaged data can't exhaust memory.
pub const BYTE_LIMIT: usize = 64 << 20;
/// `Op::parent_op` of an op that sits directly in the entry.
pub const TOP_LEVEL_OP: u32 = u32::MAX;
/// `ShapeField::reference` of an owned array whose elements hold no pointers.
pub const NO_ELEMENT_SHAPE: u16 = 0xFFFF;
/// `Site::target` bit that names an op of the same entry instead of a target.
pub const OP_LOCAL_TARGET: u16 = 0x8000;
pub const OP_LOCAL_MASK: u16 = OP_LOCAL_TARGET - 1;
/// A typed array descriptor: a u64 count, then an i64 counted from its own field.
pub const DESCRIPTOR_SIZE: usize = 16;
/// A row or object pointer, an i64 counted from its own field.
pub const ROW_POINTER_SIZE: usize = 8;
/// `Op::flags` bit: no two merged rows may share a key.
pub const UNIQUE_KEY: u8 = 0x01;
/// `Op::flags` bit: merged keys must stay strictly ascending.
pub const ORDERED: u8 = 0x02;
/// `Site::flags` bit: the site value is signed.
pub const SIGNED_SITE: u8 = 0x01;
pub const METADATA_DIGEST_SIZE: usize = 20;
/// A sidecar binds to the package header's metadata digest at this offset.
pub const PACKAGE_DIGEST_OFFSET: usize = 0x118;
/// A sidecar sits beside its package under the same stem, with this extension.
pub const EXTENSION: &str = "merge";

const KEY_WIDTH: u8 = 4;
const HEADER_SIZE: usize = 48;
const TARGET_SIZE: usize = 16;
const SHAPE_HEADER_SIZE: usize = 8;
const SHAPE_FIELD_SIZE: usize = 8;
const ENTRY_HEADER_SIZE: usize = 64;
const DIFF_HEADER_SIZE: usize = 8;
const OP_SIZE: usize = 40;
const SITE_SIZE: usize = 8;

/// How the loader may merge one entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Opaque,
    Table,
    Manifest,
    Added,
}

impl EntryKind {
    fn from_byte(byte: u8) -> Option<Self> {
        [Self::Opaque, Self::Table, Self::Manifest, Self::Added]
            .get(usize::from(byte))
            .copied()
    }
    fn byte(self) -> u8 {
        self as u8
    }
}

/// What one pointer-bearing field of a row holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A typed array descriptor.
    OwnedArray,
    /// A self-relative pointer to a row start.
    RowPointer,
    /// A self-relative pointer to an object outside every array.
    ObjectPointer,
}

impl FieldKind {
    /// The bytes the field covers in its row.
    pub fn size(self) -> usize {
        if self == Self::OwnedArray {
            DESCRIPTOR_SIZE
        } else {
            ROW_POINTER_SIZE
        }
    }
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::OwnedArray),
            2 => Some(Self::RowPointer),
            3 => Some(Self::ObjectPointer),
            _ => None,
        }
    }
    fn byte(self) -> u8 {
        self as u8 + 1
    }
}

/// How one op's rows merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Append,
    SortedMap,
}

impl OpKind {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Append),
            2 => Some(Self::SortedMap),
            _ => None,
        }
    }
    fn byte(self) -> u8 {
        self as u8 + 1
    }
}

/// One index space: a top-level append op of an installed table entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub table: u32,
    pub descriptor: u32,
    pub stock_count: u32,
    /// The mod's final row count.
    pub mod_count: u32,
}

/// One pointer-bearing field of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeField {
    pub row_offset: u32,
    pub kind: FieldKind,
    /// Element shape of an owned array, op of a row pointer, or `NO_ELEMENT_SHAPE`.
    pub reference: u16,
}

/// One row layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub stride: u32,
    pub fields: Vec<ShapeField>,
}

/// One changed stock range and its old bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub offset: u32,
    pub old: Vec<u8>,
}

/// One typed array the merge rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub kind: OpKind,
    pub flags: u8,
    /// 0, or 4 for a u32 key.
    pub key_width: u8,
    pub key_offset: u32,
    /// `TOP_LEVEL_OP`, or the op whose row holds this descriptor.
    pub parent_op: u32,
    pub parent_row: u32,
    /// Entry offset at top level, else the offset inside the parent row.
    pub descriptor: u32,
    pub shape: u32,
    pub stock_count: u32,
    pub mod_count: u32,
    /// The largest merged count allowed.
    pub limit: u32,
    /// 0, or the id shared by tables whose rows are added in step.
    pub pair_group: u32,
}

/// One field that holds the number of a row the mod added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub offset: u32,
    /// 1, 2 or 4 bytes.
    pub width: u8,
    pub flags: u8,
    /// An index into the targets, or `OP_LOCAL_TARGET` with an op of the same entry.
    pub target: u16,
}

/// One entry's merge description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: u32,
    pub kind: EntryKind,
    pub stock_size: u32,
    /// Must equal the entry's size in the package.
    pub built_size: u32,
    pub stock_sha256: [u8; 32],
    pub diffs: Vec<Diff>,
    pub ops: Vec<Op>,
    pub sites: Vec<Site>,
}

/// One package file's merge data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sidecar {
    pub package: u16,
    pub patch: u16,
    /// Copy of the package header's metadata digest.
    pub metadata_digest: [u8; METADATA_DIGEST_SIZE],
    pub targets: Vec<Target>,
    pub shapes: Vec<Shape>,
    /// Sorted by entry index.
    pub entries: Vec<Entry>,
}

impl Sidecar {
    /// The description of entry `index`, or `None` when the file has none.
    pub fn entry(&self, index: u32) -> Option<&Entry> {
        let at = self
            .entries
            .binary_search_by_key(&index, |entry| entry.index)
            .ok()?;
        Some(&self.entries[at])
    }

    pub fn shape(&self, index: u32) -> Option<&Shape> {
        self.shapes.get(usize::try_from(index).ok()?)
    }

    /// Parses and checks one sidecar against the package it sits beside, as the loader does.
    pub fn parse(
        file: &[u8],
        package: u16,
        patch: u16,
        digest: &[u8; METADATA_DIGEST_SIZE],
    ) -> Result<Self, Status> {
        let format = || Status::Format;
        if file.len() > BYTE_LIMIT {
            return Err(Status::Format);
        }
        let mut cursor = Cursor { bytes: file, at: 0 };
        if !cursor.fits(1, HEADER_SIZE) || cursor.u32().ok_or_else(format)? != MAGIC {
            return Err(Status::Format);
        }
        // A new version may define flag bits, so the version is checked first.
        if cursor.u16().ok_or_else(format)? != VERSION {
            return Err(Status::Version);
        }
        let flags = cursor.u16().ok_or_else(format)?;
        let header_package = cursor.u16().ok_or_else(format)?;
        let header_patch = cursor.u16().ok_or_else(format)?;
        let header_digest = cursor.array::<METADATA_DIGEST_SIZE>().ok_or_else(format)?;
        let target_count = cursor.u32().ok_or_else(format)?;
        let shape_count = cursor.u32().ok_or_else(format)?;
        let entry_count = cursor.u32().ok_or_else(format)?;
        let reserved = cursor.u32().ok_or_else(format)?;
        if flags != 0 {
            return Err(Status::Format);
        }
        if header_package != package || header_patch != patch || header_digest != *digest {
            return Err(Status::Binding);
        }
        if reserved != 0 {
            return Err(Status::Format);
        }
        let targets = read_targets(&mut cursor, target_count).ok_or_else(format)?;
        if !cursor.fits(u64::from(shape_count), SHAPE_HEADER_SIZE) {
            return Err(Status::Format);
        }
        let shapes = (0..shape_count)
            .map(|index| read_shape(&mut cursor, index, shape_count))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(format)?;
        if !cursor.fits(u64::from(entry_count), ENTRY_HEADER_SIZE) {
            return Err(Status::Format);
        }
        let reach = pointer_reach(&shapes);
        let mut entries: Vec<Entry> = Vec::new();
        for _ in 0..entry_count {
            let entry =
                read_entry(&mut cursor, &shapes, &reach, target_count).ok_or_else(format)?;
            if entries.last().is_some_and(|last| entry.index <= last.index) {
                return Err(Status::Format);
            }
            entries.push(entry);
        }
        if !cursor.done() {
            return Err(Status::Format);
        }
        Ok(Self {
            package,
            patch,
            metadata_digest: header_digest,
            targets,
            shapes,
            entries,
        })
    }

    /// Encodes the sidecar, then reads it back, so an encoder fault can't ship.
    pub fn write(&self) -> Result<Vec<u8>, Status> {
        let count = |value: usize| u32::try_from(value).map_err(|_| Status::Capacity);
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.package.to_le_bytes());
        out.extend_from_slice(&self.patch.to_le_bytes());
        out.extend_from_slice(&self.metadata_digest);
        for value in [
            count(self.targets.len())?,
            count(self.shapes.len())?,
            count(self.entries.len())?,
            0,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for target in &self.targets {
            for value in [
                target.table,
                target.descriptor,
                target.stock_count,
                target.mod_count,
            ] {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for shape in &self.shapes {
            out.extend_from_slice(&shape.stride.to_le_bytes());
            out.extend_from_slice(&count(shape.fields.len())?.to_le_bytes());
            for field in &shape.fields {
                out.extend_from_slice(&field.row_offset.to_le_bytes());
                out.extend_from_slice(&[field.kind.byte(), 0]);
                out.extend_from_slice(&field.reference.to_le_bytes());
            }
        }
        for entry in &self.entries {
            out.extend_from_slice(&entry.index.to_le_bytes());
            out.extend_from_slice(&[entry.kind.byte(), 0, 0, 0]);
            out.extend_from_slice(&entry.stock_size.to_le_bytes());
            out.extend_from_slice(&entry.built_size.to_le_bytes());
            out.extend_from_slice(&entry.stock_sha256);
            for value in [
                count(entry.diffs.len())?,
                count(entry.ops.len())?,
                count(entry.sites.len())?,
                0,
            ] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            for diff in &entry.diffs {
                out.extend_from_slice(&diff.offset.to_le_bytes());
                out.extend_from_slice(&count(diff.old.len())?.to_le_bytes());
                out.extend_from_slice(&diff.old);
            }
            for op in &entry.ops {
                out.extend_from_slice(&[op.kind.byte(), op.flags, op.key_width, 0]);
                for value in [
                    op.key_offset,
                    op.parent_op,
                    op.parent_row,
                    op.descriptor,
                    op.shape,
                    op.stock_count,
                    op.mod_count,
                    op.limit,
                    op.pair_group,
                ] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
            for site in &entry.sites {
                out.extend_from_slice(&site.offset.to_le_bytes());
                out.extend_from_slice(&[site.width, site.flags]);
                out.extend_from_slice(&site.target.to_le_bytes());
            }
        }
        if out.len() > BYTE_LIMIT {
            return Err(Status::Capacity);
        }
        Self::parse(&out, self.package, self.patch, &self.metadata_digest)?;
        Ok(out)
    }
}

/// Reads packed little-endian records front to back and refuses any read past the end.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let bytes = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    /// Whether `count` records of `size` bytes can still follow.
    fn fits(&self, count: u64, size: usize) -> bool {
        self.at <= self.bytes.len() && count <= ((self.bytes.len() - self.at) / size) as u64
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Targets must not count down or repeat.
fn read_targets(cursor: &mut Cursor<'_>, count: u32) -> Option<Vec<Target>> {
    if !cursor.fits(u64::from(count), TARGET_SIZE) {
        return None;
    }
    let mut targets = Vec::new();
    for _ in 0..count {
        let target = Target {
            table: cursor.u32()?,
            descriptor: cursor.u32()?,
            stock_count: cursor.u32()?,
            mod_count: cursor.u32()?,
        };
        if target.mod_count < target.stock_count {
            return None;
        }
        targets.push(target);
    }
    let mut keys = targets
        .iter()
        .map(|target| (target.table, target.descriptor))
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.windows(2)
        .all(|pair| pair[0] != pair[1])
        .then_some(targets)
}

/// Element shapes must come later than the shape naming them, so shapes form no cycle.
fn read_shape(cursor: &mut Cursor<'_>, index: u32, shape_count: u32) -> Option<Shape> {
    let stride = cursor.u32()?;
    let field_count = cursor.u32()?;
    if stride == 0 || !cursor.fits(u64::from(field_count), SHAPE_FIELD_SIZE) {
        return None;
    }
    let mut fields = Vec::new();
    let mut end = 0u64;
    for _ in 0..field_count {
        let row_offset = cursor.u32()?;
        let kind = FieldKind::from_byte(cursor.u8()?)?;
        let reserved = cursor.u8()?;
        let reference = cursor.u16()?;
        // An object pointer names no shape and no op.
        if reserved != 0 || (kind == FieldKind::ObjectPointer && reference != NO_ELEMENT_SHAPE) {
            return None;
        }
        let field_end = u64::from(row_offset) + kind.size() as u64;
        if u64::from(row_offset) < end || field_end > u64::from(stride) {
            return None;
        }
        end = field_end;
        if kind == FieldKind::OwnedArray
            && reference != NO_ELEMENT_SHAPE
            && (u32::from(reference) <= index || u32::from(reference) >= shape_count)
        {
            return None;
        }
        fields.push(ShapeField {
            row_offset,
            kind,
            reference,
        });
    }
    Some(Shape { stride, fields })
}

/// Per shape, the largest op a row pointer in it or in its owned elements names, or -1.
/// Element shapes only point forward, so one backward pass sees every child first.
fn pointer_reach(shapes: &[Shape]) -> Vec<i64> {
    let mut reach = vec![-1i64; shapes.len()];
    for index in (0..shapes.len()).rev() {
        for field in &shapes[index].fields {
            let named = match field.kind {
                FieldKind::RowPointer => i64::from(field.reference),
                FieldKind::OwnedArray if field.reference != NO_ELEMENT_SHAPE => {
                    reach[usize::from(field.reference)]
                }
                _ => continue,
            };
            reach[index] = reach[index].max(named);
        }
    }
    reach
}

fn read_entry(
    cursor: &mut Cursor<'_>,
    shapes: &[Shape],
    reach: &[i64],
    target_count: u32,
) -> Option<Entry> {
    let index = cursor.u32()?;
    let kind = cursor.u8()?;
    let reserved = cursor.array::<3>()?;
    let stock_size = cursor.u32()?;
    let built_size = cursor.u32()?;
    let stock_sha256 = cursor.array::<32>()?;
    let diff_count = cursor.u32()?;
    let op_count = cursor.u32()?;
    let site_count = cursor.u32()?;
    let reserved2 = cursor.u32()?;
    let kind = EntryKind::from_byte(kind)?;
    let valid = match kind {
        EntryKind::Opaque => diff_count == 0 && op_count == 0,
        // Only ops move pointers into added bytes, so a table without ops cannot grow.
        EntryKind::Table => {
            stock_size > 0
                && built_size >= stock_size
                && (op_count != 0 || built_size == stock_size)
        }
        EntryKind::Manifest => diff_count == 0 && op_count == 0 && site_count == 0,
        EntryKind::Added => stock_size == 0 && diff_count == 0 && op_count == 0,
    };
    if reserved != [0; 3] || reserved2 != 0 || !valid {
        return None;
    }
    let diffs = read_diffs(cursor, diff_count, stock_size)?;
    let ops = read_ops(cursor, op_count, shapes, reach, built_size)?;
    let sites = read_sites(cursor, site_count, target_count, op_count, built_size)?;
    Some(Entry {
        index,
        kind,
        stock_size,
        built_size,
        stock_sha256,
        diffs,
        ops,
        sites,
    })
}

/// Diffs must stay in the stock region, in order and apart.
fn read_diffs(cursor: &mut Cursor<'_>, count: u32, stock_size: u32) -> Option<Vec<Diff>> {
    if !cursor.fits(u64::from(count), DIFF_HEADER_SIZE) {
        return None;
    }
    let mut diffs = Vec::new();
    let mut end = 0u64;
    for _ in 0..count {
        let offset = cursor.u32()?;
        let length = cursor.u32()?;
        if length == 0 || u64::from(offset) < end {
            return None;
        }
        end = u64::from(offset) + u64::from(length);
        if end > u64::from(stock_size) {
            return None;
        }
        let old = cursor.take(usize::try_from(length).ok()?)?.to_vec();
        diffs.push(Diff { offset, old });
    }
    Some(diffs)
}

fn read_ops(
    cursor: &mut Cursor<'_>,
    count: u32,
    shapes: &[Shape],
    reach: &[i64],
    built_size: u32,
) -> Option<Vec<Op>> {
    if !cursor.fits(u64::from(count), OP_SIZE) {
        return None;
    }
    let mut ops: Vec<Op> = Vec::new();
    for _ in 0..count {
        let kind = cursor.u8()?;
        let flags = cursor.u8()?;
        let key_width = cursor.u8()?;
        let reserved = cursor.u8()?;
        let kind = OpKind::from_byte(kind)?;
        let op = Op {
            kind,
            flags,
            key_width,
            key_offset: cursor.u32()?,
            parent_op: cursor.u32()?,
            parent_row: cursor.u32()?,
            descriptor: cursor.u32()?,
            shape: cursor.u32()?,
            stock_count: cursor.u32()?,
            mod_count: cursor.u32()?,
            limit: cursor.u32()?,
            pair_group: cursor.u32()?,
        };
        if reserved != 0
            || !op_valid(&op, &ops, shapes, built_size)
            || reach[op.shape as usize] >= i64::from(count)
        {
            return None;
        }
        ops.push(op);
    }
    Some(ops)
}

/// Whether one op obeys its layout rules. `earlier` holds the entry's checked ops before it.
fn op_valid(op: &Op, earlier: &[Op], shapes: &[Shape], built_size: u32) -> bool {
    if op.flags & !(UNIQUE_KEY | ORDERED) != 0
        || (op.key_width != 0 && op.key_width != KEY_WIDTH)
        || op.shape as usize >= shapes.len()
        || op.mod_count < op.stock_count
        || op.mod_count > op.limit
    {
        return false;
    }
    let stride = u64::from(shapes[op.shape as usize].stride);
    let keyed = op.flags != 0 || op.kind == OpKind::SortedMap;
    if (keyed && op.key_width == 0)
        || (op.key_width != 0 && u64::from(op.key_offset) + u64::from(op.key_width) > stride)
    {
        return false;
    }
    let descriptor_end = u64::from(op.descriptor) + DESCRIPTOR_SIZE as u64;
    if op.parent_op == TOP_LEVEL_OP {
        return descriptor_end <= u64::from(built_size);
    }
    // A parent comes first, so nesting forms no cycle.
    let Some(parent) = earlier.get(op.parent_op as usize) else {
        return false;
    };
    op.parent_row < parent.mod_count
        && descriptor_end <= u64::from(shapes[parent.shape as usize].stride)
}

/// Sites must not overlap, must stay in the entry and must name a target or an op.
fn read_sites(
    cursor: &mut Cursor<'_>,
    count: u32,
    target_count: u32,
    op_count: u32,
    built_size: u32,
) -> Option<Vec<Site>> {
    if !cursor.fits(u64::from(count), SITE_SIZE) {
        return None;
    }
    let mut sites = Vec::new();
    let mut end = 0u64;
    for _ in 0..count {
        let site = Site {
            offset: cursor.u32()?,
            width: cursor.u8()?,
            flags: cursor.u8()?,
            target: cursor.u16()?,
        };
        if !matches!(site.width, 1 | 2 | 4)
            || site.flags & !SIGNED_SITE != 0
            || u64::from(site.offset) < end
        {
            return None;
        }
        end = u64::from(site.offset) + u64::from(site.width);
        let local = site.target & OP_LOCAL_TARGET != 0;
        if end > u64::from(built_size)
            || (local && u32::from(site.target & OP_LOCAL_MASK) >= op_count)
            || (!local && u32::from(site.target) >= target_count)
        {
            return None;
        }
        sites.push(site);
    }
    Some(sites)
}
