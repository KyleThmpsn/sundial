//! A map's own kill volumes: the donor's volume record and packfile rewritten so each of the
//! donor's named entries holds a volume of the level, and the rest stand far below the map.
//!
//! A volume record (`80809C36`) names its keyed descriptor at `+0x18`. That descriptor keeps an
//! array of entries (`80809A6D`, 24 bytes: a self-relative name, the key, and the slot type `60`
//! and index the entry takes among the object's slots) at `+0x28`, an array of volumes
//! (`808099D0`, `0x120` bytes) at `+0x38`, and the packfile at `+0x48`. A volume row names
//! itself, its key and slot, the entity its volume is (`+0x20`), the shape record that binds it to
//! the packfile's shape of its index (`+0x98`), its box (`+0xB0` and `+0xC0`), its footprint's
//! corners as float4 points at `+0xD0`, their triangles as byte triples at `+0xE0`, its height
//! at `+0xF0`, its footprint's centre at the bottom at `+0x110` and flags at `+0x11C`. The pooled
//! shape records, points and triangles follow the rows, then two arrays (`8080919F` and
//! `808091A4`, counted and named at header `+0x20` and `+0x30`) that name the record's other
//! descriptors and its component header, a string blob (`80800065`) holding the names' length,
//! and the names. Embedded records sit 8-aligned behind their class word, and arrays 16-aligned
//! behind their marker. Rebuilding Distant Shore's record from its own content this way gives its
//! bytes back exactly.
//!
//! The packfile's root shape array holds one shape per volume: the prisms of its triangles in a
//! list shape, or a static compound beyond three. The object's slots name the volumes by the FNV-1
//! hash of their names, so the names, keys and slots stay the donor's and only the geometry
//! changes. Distant Shore's 62 entries are ten `kv_` volumes that kill, 38 `kv_soft_` volumes
//! that start a countdown and 14 `slip` volumes.
use parhelion_import::halo_reach::level::volumes::{self as level, Kind, Volume};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{
        bounded_relative_target, read_i64, read_u32, read_u64, synchronize_payload_size, write_i64,
        write_u32, write_u64,
    },
};

const ARRAY_MARKER: u32 = 0x8080_9FBD;
/// The header's pointer to the keyed descriptor, and its two tail arrays: each a count and a
/// pointer, with its class and row size.
const HEADER_DESCRIPTOR: usize = 0x18;
const TAIL: [(usize, u32, usize); 2] = [(0x20, 0x8080_919F, 8), (0x30, 0x8080_91A4, 16)];
/// The descriptor's entries, volumes and packfile.
const DESCRIPTOR_ENTRIES: usize = 0x28;
const DESCRIPTOR_VOLUMES: usize = 0x38;
const DESCRIPTOR_PACKFILE: usize = 0x48;
const ENTRY_CLASS: u32 = 0x8080_9A6D;
const ENTRY_SIZE: usize = 24;
const ROW_CLASS: u32 = 0x8080_99D0;
const ROW_SIZE: usize = 0x120;
const ROW_SHAPE: usize = 0x98;
const ROW_LOW: usize = 0xB0;
const ROW_HIGH: usize = 0xC0;
const ROW_POINTS: usize = 0xD0;
const ROW_TRIANGLES: usize = 0xE0;
const ROW_HEIGHT: usize = 0xF0;
const ROW_BASE: usize = 0x110;
/// A shape record, with its nested record when its first word points at one, else without.
const SHAPE_CLASS: u32 = 0x8080_929B;
const SHAPE_SIZE: usize = 0x28;
const SHAPE_WITH_NESTED_SIZE: usize = 0x48;
const POINT_CLASS: u32 = 0x8080_0094;
const TRIANGLE_CLASS: u32 = 0x8080_9B92;
const BLOB_CLASS: u32 = 0x8080_0065;
/// Entries that kill, and among them those that start a countdown first.
const KILL_PREFIX: &str = "kv_";
const SOFT: &str = "soft";
/// Where unused entries' volumes stand, in meters below the map's origin.
const BURIED: f32 = -5000.;

fn align(value: usize, to: usize) -> usize {
    value.div_ceil(to) * to
}

/// The C string at `at`.
fn name_at(payload: &[u8], at: usize) -> AuthoringResult<String> {
    let rest = payload
        .get(at..)
        .ok_or_else(|| invalid("A volume name leaves its record"))?;
    let end = rest
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| invalid("A volume name has no terminator"))?;
    String::from_utf8(rest[..end].to_vec()).map_err(|_| invalid("A volume name is not UTF-8"))
}

/// The rows of the array counted at `count_field` and named at the pointer after it, checked to
/// be of `class` and that count.
fn array(
    payload: &[u8],
    count_field: usize,
    class: u32,
    label: &str,
) -> AuthoringResult<(usize, usize)> {
    let count = read_u64(payload, count_field)?;
    let header = bounded_relative_target(payload, count_field + 8, label)?;
    if header < 4
        || read_u32(payload, header - 4)? != ARRAY_MARKER
        || read_u64(payload, header)? != count
        || read_u32(payload, header + 8)? != class
    {
        return Err(invalid(format!(
            "{label} is not an array of {count} rows of class {class:08X}"
        )));
    }
    let count = usize::try_from(count).map_err(|_| invalid(format!("{label} count")))?;
    Ok((count, header + 16))
}

/// A donor volume record's keyed descriptor and its arrays.
struct Donor<'a> {
    payload: &'a [u8],
    descriptor: usize,
    entries: usize,
    rows: usize,
    count: usize,
}

impl<'a> Donor<'a> {
    fn read(payload: &'a [u8]) -> AuthoringResult<Self> {
        let descriptor = bounded_relative_target(payload, HEADER_DESCRIPTOR, "Volume descriptor")?;
        let (entry_count, entries) = array(
            payload,
            descriptor + DESCRIPTOR_ENTRIES,
            ENTRY_CLASS,
            "Volume entries",
        )?;
        let (count, rows) = array(
            payload,
            descriptor + DESCRIPTOR_VOLUMES,
            ROW_CLASS,
            "Volume rows",
        )?;
        if entry_count != count
            || count == 0
            || entries + count * ENTRY_SIZE > rows - 16
            || rows + count * ROW_SIZE > payload.len()
        {
            return Err(invalid(format!(
                "A volume record has {entry_count} entries and {count} volumes"
            )));
        }
        Ok(Self {
            payload,
            descriptor,
            entries,
            rows,
            count,
        })
    }

    fn row(&self, index: usize) -> usize {
        self.rows + index * ROW_SIZE
    }

    fn entry(&self, index: usize) -> usize {
        self.entries + index * ENTRY_SIZE
    }

    fn names(&self) -> AuthoringResult<Vec<String>> {
        (0..self.count)
            .map(|index| {
                name_at(
                    self.payload,
                    bounded_relative_target(self.payload, self.entry(index), "Volume entry name")?,
                )
            })
            .collect()
    }
}

/// The packfile a volume record names, when its entries include volumes that kill, else none.
pub(super) fn packfile_of(record: &[u8]) -> AuthoringResult<Option<TagHash>> {
    let donor = Donor::read(record)?;
    if !donor
        .names()?
        .iter()
        .any(|name| name.starts_with(KILL_PREFIX))
    {
        return Ok(None);
    }
    Ok(Some(TagHash(read_u32(
        record,
        donor.descriptor + DESCRIPTOR_PACKFILE,
    )?)))
}

/// A stand-in for an entry no volume takes, far below the map.
fn stub(name: &str) -> Volume {
    Volume {
        name: name.to_owned(),
        kind: Kind::Kill,
        corners: vec![[0., 0.], [1., 0.], [0., 1.]],
        z: [BURIED - 1., BURIED],
    }
}

/// Which volume each of the donor's entries holds. Entries named `kv_` kill, and those with
/// `soft` in their name after a countdown, in the donor's order. Volumes that kill take kill
/// entries, then countdown entries once those run out, and volumes that start a countdown take
/// countdown entries, then kill entries. Every other entry holds a stand-in, and a volume with no
/// entry left is left out.
fn assign(names: &[String], volumes: &[Volume]) -> Vec<Volume> {
    let soft = |name: &str| name.starts_with(KILL_PREFIX) && name.contains(SOFT);
    let kill = |name: &str| name.starts_with(KILL_PREFIX) && !name.contains(SOFT);
    let slots = |select: &dyn Fn(&str) -> bool| {
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| select(name))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let (mut kill_slots, mut soft_slots) = (slots(&kill), slots(&soft));
    let (mut kills, mut softs): (Vec<&Volume>, Vec<&Volume>) =
        volumes.iter().partition(|volume| volume.kind == Kind::Kill);
    let mut assigned: Vec<Option<Volume>> = vec![None; names.len()];
    // Each kind takes its own entries, then whatever the other kind left.
    for _ in 0..2 {
        while !kills.is_empty() && !kill_slots.is_empty() {
            assigned[kill_slots.remove(0)] = Some(kills.remove(0).clone());
        }
        while !softs.is_empty() && !soft_slots.is_empty() {
            assigned[soft_slots.remove(0)] = Some(softs.remove(0).clone());
        }
        std::mem::swap(&mut kill_slots, &mut soft_slots);
    }
    assigned
        .into_iter()
        .enumerate()
        .map(|(index, volume)| volume.unwrap_or_else(|| stub(&names[index])))
        .collect()
}

/// The record's bytes as they are laid out: embedded records 8-aligned behind their class word,
/// arrays 16-aligned behind their marker, and self-relative pointers written last.
struct Layout {
    out: Vec<u8>,
    pointers: Vec<(usize, usize)>,
}

impl Layout {
    fn pad_to(&mut self, at: usize) {
        if self.out.len() < at {
            self.out.resize(at, 0);
        }
    }

    fn record(&mut self, class: u32, body: &[u8]) -> usize {
        let at = align(self.out.len() + 4, 8);
        self.pad_to(at - 4);
        self.out.extend_from_slice(&class.to_le_bytes());
        self.out.extend_from_slice(body);
        at
    }

    fn array(&mut self, class: u32, count: usize, rows: &[u8]) -> AuthoringResult<usize> {
        let at = align(self.out.len() + 4, 16);
        self.pad_to(at - 4);
        self.out.extend_from_slice(&ARRAY_MARKER.to_le_bytes());
        let count = u64::try_from(count).map_err(|_| invalid("Volume array count"))?;
        self.out.extend_from_slice(&count.to_le_bytes());
        self.out.extend_from_slice(&class.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]);
        self.out.extend_from_slice(rows);
        Ok(at)
    }

    fn pointer(&mut self, field: usize, target: usize) {
        self.pointers.push((field, target));
    }

    fn finish(mut self) -> AuthoringResult<Vec<u8>> {
        for (field, target) in self.pointers {
            let relative = i64::try_from(target)
                .ok()
                .zip(i64::try_from(field).ok())
                .map(|(target, field)| target - field)
                .ok_or_else(|| invalid("A volume record pointer overflowed"))?;
            write_i64(&mut self.out, field, relative)?;
        }
        synchronize_payload_size(self.out)
    }
}

fn write_f32s(out: &mut [u8], at: usize, values: &[f32]) -> AuthoringResult<()> {
    for (k, value) in values.iter().enumerate() {
        write_u32(out, at + k * 4, value.to_bits())?;
    }
    Ok(())
}

/// The donor's volume `record` and `packfile` rebuilt with `volumes` in the entries they take:
/// the map's record, still naming the donor's record and packfile tags, and its packfile.
pub(super) fn build(
    record: &[u8],
    packfile: &[u8],
    volumes: &[Volume],
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let donor = Donor::read(record)?;
    let names = donor.names()?;
    let assigned = assign(&names, volumes);
    // The donor's pooled shape records, which bind each row to the packfile shape of its index.
    let mut shapes = Vec::with_capacity(donor.count);
    for (index, name) in names.iter().enumerate() {
        let at = bounded_relative_target(record, donor.row(index) + ROW_SHAPE, "Volume shape")?;
        if at < 4 || read_u32(record, at - 4)? != SHAPE_CLASS {
            return Err(invalid(format!("Volume {name} has no shape record")));
        }
        let size = if read_i64(record, at)? != 0 {
            SHAPE_WITH_NESTED_SIZE
        } else {
            SHAPE_SIZE
        };
        shapes.push(
            record
                .get(at..at + size)
                .ok_or_else(|| invalid(format!("Volume {name}'s shape record leaves its record")))?
                .to_vec(),
        );
    }
    // The tail: two arrays naming the record's other descriptors and its component header, then
    // the string blob and the names.
    let mut tails = Vec::with_capacity(TAIL.len());
    let mut tail_end = 0;
    for (field, class, row_size) in TAIL {
        let (count, rows) = array(record, field, class, "Volume record tail")?;
        let mut targets = Vec::with_capacity(count);
        for k in 0..count {
            let row = rows + k * row_size;
            let rest = record
                .get(row + 8..row + row_size)
                .ok_or_else(|| invalid("A volume record tail row leaves its record"))?
                .to_vec();
            targets.push((
                bounded_relative_target(record, row, "Volume record tail row")?,
                rest,
            ));
        }
        tails.push(targets);
        tail_end = tail_end.max(rows + count * row_size);
    }
    let blob = align(tail_end + 4, 8);
    let strings_at = blob + 8;
    let strings = record
        .get(strings_at..)
        .ok_or_else(|| invalid("A volume record ends before its names"))?;
    if read_u32(record, blob - 4)? != BLOB_CLASS || read_u64(record, blob)? != strings.len() as u64
    {
        return Err(invalid("A volume record's names have no blob header"));
    }

    let mut out = Layout {
        out: record[..donor.rows].to_vec(),
        pointers: Vec::new(),
    };
    for index in 0..donor.count {
        out.out
            .extend_from_slice(&record[donor.row(index)..donor.row(index) + ROW_SIZE]);
    }
    for (index, volume) in assigned.iter().enumerate() {
        let row = donor.row(index);
        let (low, high) = volume.bounds();
        let [x, y] = volume.centroid();
        let z = volume.z[0];
        let triangles = volume
            .triangles()
            .map_err(|error| invalid(format!("Volume {}: {error:#}", volume.name)))?;
        write_f32s(&mut out.out, row + ROW_LOW, &[low[0], low[1], low[2], 1.])?;
        write_f32s(
            &mut out.out,
            row + ROW_HIGH,
            &[high[0], high[1], high[2], 1.],
        )?;
        write_f32s(&mut out.out, row + ROW_HEIGHT, &[volume.z[1] - z])?;
        write_f32s(&mut out.out, row + ROW_BASE, &[x, y, z])?;
        let corners =
            u64::try_from(volume.corners.len()).map_err(|_| invalid("Volume corner count"))?;
        write_u64(&mut out.out, row + ROW_POINTS, corners)?;
        let triangle_count =
            u64::try_from(triangles.len()).map_err(|_| invalid("Volume triangle count"))?;
        write_u64(&mut out.out, row + ROW_TRIANGLES, triangle_count)?;
        let shape = out.record(SHAPE_CLASS, &shapes[index]);
        out.pointer(row + ROW_SHAPE, shape);
        let points = volume
            .corners
            .iter()
            .flat_map(|&[x, y]| [x, y, z, 1.])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let points_at = out.array(POINT_CLASS, volume.corners.len(), &points)?;
        out.pointer(row + ROW_POINTS + 8, points_at);
        let bytes = triangles.iter().flatten().copied().collect::<Vec<_>>();
        let triangles_at = out.array(TRIANGLE_CLASS, triangles.len(), &bytes)?;
        out.pointer(row + ROW_TRIANGLES + 8, triangles_at);
    }
    for ((field, class, row_size), targets) in TAIL.into_iter().zip(&tails) {
        let rows = targets
            .iter()
            .flat_map(|(_, rest)| [0u8; 8].into_iter().chain(rest.iter().copied()))
            .collect::<Vec<_>>();
        // `array` returns the header: its count, then its class, then the rows 16 bytes on.
        let at = out.array(class, targets.len(), &rows)?;
        for (k, (target, _)) in targets.iter().enumerate() {
            out.pointer(at + 16 + k * row_size, *target);
        }
        out.pointer(field + 8, at);
    }
    out.record(BLOB_CLASS, &(strings.len() as u64).to_le_bytes());
    let names_at = out.out.len();
    out.out.extend_from_slice(strings);
    // Entries and rows name themselves in the moved names.
    for index in 0..donor.count {
        for field in [donor.entry(index), donor.row(index)] {
            let old = bounded_relative_target(record, field, "Volume name")?;
            let moved = old
                .checked_sub(strings_at)
                .map(|offset| names_at + offset)
                .ok_or_else(|| invalid("A volume name lies before the names"))?;
            out.pointer(field, moved);
        }
    }
    let record = out.finish()?;
    let packfile = level::packfile(packfile, &assigned)
        .map_err(|error| invalid(format!("Volume packfile: {error:#}")))?;
    Ok((record, packfile))
}
