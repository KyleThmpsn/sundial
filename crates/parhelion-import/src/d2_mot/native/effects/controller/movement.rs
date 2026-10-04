//! Modern projectile movement owners rewritten into the native layout.
//!
//! A movement owner (modern instance `80802AA1`, definition `80802DCC`; native `8080388F` and
//! `80803B73`) is a stream of reciprocal records and arrays: the root pair, two settings pairs,
//! block expressions, slots holding curves, scalar expressions and the simulation state. The two
//! game versions serialize the same object tree in the same order. What differs is each record's
//! layout and the class ids. The rewrite therefore walks the modern tree, emits every object in
//! the native order with native record sizes, copies each field through a per-class offset map,
//! lowers expression bytecode, substitutes class ids, and recomputes every twin, parent and array
//! pointer.
//!
//! Evidence: 107 projectiles exist in both game versions with identical curve, input and slot
//! identities (the clean Shadowkeep packages against the modern packages, September 29 2026).
//! Their movement owners align record by record, and the offset maps below were voted over those
//! pairs. The reference tags a movement owner carries (slot selectors, the curve dictionary, the
//! settings' sibling components) are not part of this layout knowledge: the caller supplies the
//! native tag for each modern one, and an unmapped tag is an error, never a guess.
use super::{Relocation, procedural, put};
use crate::d2_mot::{entity::links::Object, payload::Payload};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

mod allocation;
pub mod compatibility;
mod embedded;
mod emission;
mod interfaces;
pub use interfaces::Interfaces;
#[cfg(test)]
mod tests;

use embedded::Rewriter;

/// Class ids of the modern and native movement records, in pairs.
const CLASSES: &[(u32, u32)] = &[
    (0x80802DCC, 0x80803B73), // root definition
    (0x80802AA1, 0x8080388F), // root instance
    (0x808081B0, 0x80808518), // settings1 definition
    (0x808081B1, 0x80808519), // settings1 instance
    (0x80809A9E, 0x80809BD8), // settings2 definition
    (0x80809A9F, 0x80809BD9), // settings2 instance
    (0x808081B3, 0x8080851B), // block holder definition
    (0x808081B4, 0x8080851C), // block holder instance
    (0x8080862F, 0x808089F5), // expression definition
    (0x80808630, 0x808089F6), // expression instance
    (0x80809590, 0x80809788), // input definition
    (0x80809591, 0x80809789), // input instance
    (0x808029CB, 0x808037CC), // slot definition
    (0x808029CC, 0x808037CD), // slot instance
    (0x808029CE, 0x808037CF), // curve header definition
    (0x808029CF, 0x808037D0), // curve header instance
    (0x80802A01, 0x808037FC), // curve data definition
    (0x80802A02, 0x808037FD), // curve data instance
    (0x80808631, 0x808089F7), // scalar expression definition
    (0x80808632, 0x808089F8), // scalar expression instance
    (0x808029B5, 0x808037BA), // state definition
    (0x808029B6, 0x808037BB), // state instance
    (0x808029B8, 0x808037BD), // state child
    (0x8080907C, 0x808091A4), // state provider
    (0x808095CE, 0x808097C1), // input and settings provider
    (0x808029D5, 0x808037D5), // expression valued input block
    (0x80802A08, 0x80803803), // settings provider values
    (0x808029D7, 0x808037D7), // settings referenced provider
    (0x80809FC3, 0x80809FC7), // slot selector rows
    (0x808029FC, 0x808037F7), // expression sub table
    (0x808029F8, 0x808037F3), // expression sub record
    (0x808029F6, 0x808037F1),
    (0x808029F9, 0x808037F4),
    (0x808029FB, 0x808037F6),
    (0x80809787, 0x808094B3), // curve keyframes
    (0x808091B7, 0x80809316),
    (0x808091B1, 0x80809310),
    (0x80809853, 0x80809A68),
    (0x808029D8, 0x808037D8), // category selection
    (0x808029E4, 0x808037E1), // sub records by kind: 0
    (0x808029E5, 0x808037E2), // 1
    (0x808029EF, 0x808037EA), // 2
    (0x808029E0, 0x808037DE), // 5
    (0x808029DF, 0x808037DD), // 6
    (0x808029F7, 0x808037F2), // 8
    (0x808029E3, 0x808037E0), // 12
    (0x808029D3, 0x808037D3), // 14
    (0x808029DB, 0x808037DB), // 16
    (0x808029EE, 0x808037E9), // 17
    (0x808029E2, 0x80802CE6), // 18
    (0x808029E8, 0x808037E5), // nested sequencer holders
    (0x808029E7, 0x808037E4),
    (0x808029F3, 0x808037EE),
    (0x808029F2, 0x808037ED),
    (0x808029DE, 0x808030A9), // parameter
    (0x808029EA, 0x808037E7), // hashed parameter
    (0x8080976E, 0x808093F6), // category mask trailers
    (0x8080976D, 0x808093F5),
    (0x80809FB8, 0x80809FBD), // array marker
];

const MODERN_MARKER: u32 = 0x80809FB8;
const NATIVE_MARKER: u32 = 0x80809FBD;
const CODE: u32 = 0x80800009;
const VECTORS: u32 = 0x80800090;

fn native_class(class: u32) -> u32 {
    CLASSES
        .iter()
        .find(|(modern, _)| *modern == class)
        .map_or(class, |(_, native)| *native)
}

// Offset maps, native offset then modern offset, voted over the 107 same-projectile pairs. A
// field absent from a map is left zero natively, and a modern field with no native home is
// dropped: in the corpus those hold constants the native runtime never had.

/// Root definition. The modern root holds an inline expression at +88..+F0, then its fields
/// continue, so three block shifts (+8, +68 and +A8 to +C0) separate the layouts.
const ROOT_DEFINITION: &[(usize, usize)] = &[
    (0x18, 0x18),
    (0x1C, 0x1C),
    (0x28, 0x28),
    (0x2C, 0x2C),
    (0x48, 0x48),
    (0x4C, 0x50),
    (0x50, 0x58),
    (0x54, 0x5C),
    (0x58, 0x60),
    (0x5C, 0x64),
    (0x60, 0x68),
    (0x64, 0x6C),
    (0x68, 0x70),
    (0x6C, 0x74),
    (0x70, 0x78),
    (0x74, 0x7C),
    (0x78, 0x80),
    (0x84, 0x84),
    (0x88, 0xF0),
    (0x8C, 0xF4),
    (0x90, 0xF8),
    (0xA0, 0x100),
    (0xA4, 0x104),
    (0xA8, 0x108),
    (0xB0, 0x110),
    (0xB4, 0x114),
    (0xC0, 0x120),
    (0xC8, 0x170),
    (0xD0, 0x178),
    (0xD8, 0x180),
    (0xDC, 0x184),
    (0xE0, 0x188),
    (0xF0, 0x198),
    (0x128, 0x1E8),
    (0x138, 0x1F8),
    (0x148, 0x208),
    (0x158, 0x218),
    (0x15C, 0x21C),
    (0x160, 0x220),
    (0x164, 0x224),
    (0x168, 0x228),
    (0x16C, 0x22C),
    (0x170, 0x230),
];
/// Slot definition: the modern layout inserts 8 bytes at +34.
const SLOT_DEFINITION: &[(usize, usize)] = &[
    (0x10, 0x10),
    (0x14, 0x14),
    (0x18, 0x18),
    (0x1C, 0x1C),
    (0x20, 0x20),
    (0x24, 0x24),
    (0x28, 0x28),
    (0x2C, 0x2C),
    (0x30, 0x30),
    (0x38, 0x40),
    (0x3C, 0x44),
    (0x40, 0x48),
    (0x44, 0x4C),
    (0x48, 0x50),
    (0x4C, 0x54),
    (0x50, 0x58),
    (0x54, 0x5C),
];
/// Curve data: the modern record repeats its dictionary reference block and grows by 0xA0.
const CURVE_DATA: &[(usize, usize)] = &[
    (0x10, 0x10),
    (0x14, 0x14),
    (0x18, 0x18),
    (0x1C, 0x1C),
    (0x20, 0x20),
    (0x24, 0x24),
    (0x28, 0x28),
    (0x2C, 0x2C),
    (0x30, 0x30),
    (0x34, 0x34),
    (0x38, 0x38),
    (0x3C, 0x3C),
    (0x40, 0x40),
    (0x44, 0x44),
    (0x48, 0x48),
    (0x4C, 0x4C),
    (0x50, 0x50),
    (0x54, 0x54),
    (0x58, 0x58),
    (0x5C, 0x5C),
    (0x60, 0x60),
    (0x68, 0x70),
    (0x6C, 0x74),
    (0x94, 0x134),
    (0x98, 0x138),
    (0xA8, 0x148),
    (0xAC, 0x14C),
    (0xB0, 0x150),
    (0xB4, 0x154),
    (0xB8, 0x158),
    (0xBC, 0x15C),
    (0xC0, 0x160),
    (0xC4, 0x164),
    (0xC8, 0x168),
    (0xCC, 0x16C),
    (0xD0, 0x170),
    (0xD4, 0x174),
];
/// State definition: a flag word and the child class, or -1 without a child.
const STATE_DEFINITION: &[(usize, usize)] = &[(0x10, 0x10), (0x14, 0x14)];
/// State instance: the modern layout inserts words at +110, +140, +170 and +178, and a 0x130
/// sentinel block after +21C.
const STATE_INSTANCE: &[(usize, usize)] = &[
    (0x10, 0x10),
    (0x14, 0x14),
    (0x2C, 0x2C),
    (0x50, 0x50),
    (0x58, 0x58),
    (0x60, 0x60),
    (0x7C, 0x7C),
    (0x80, 0x80),
    (0xBC, 0xBC),
    (0xF0, 0xF0),
    (0x10C, 0x114),
    (0x110, 0x118),
    (0x114, 0x11C),
    (0x138, 0x148),
    (0x13C, 0x14C),
    (0x144, 0x154),
    (0x148, 0x158),
    (0x168, 0x178),
    (0x170, 0x180),
    (0x174, 0x188),
    (0x178, 0x198),
    (0x17C, 0x19C),
    (0x19C, 0x1BC),
    (0x1AC, 0x1CC),
    (0x1B0, 0x1D0),
    (0x1C0, 0x1E0),
    (0x1FC, 0x21C),
    (0x20C, 0x22C),
    (0x23C, 0x38C),
    (0x250, 0x3A0),
    (0x254, 0x3A4),
    (0x25C, 0x3AC),
    (0x260, 0x3B0),
    (0x268, 0x3B8),
    (0x27C, 0x3CC),
    (0x290, 0x3E0),
    (0x294, 0x3E4),
    (0x29C, 0x3EC),
    (0x2A0, 0x3F0),
    (0x2A8, 0x3F8),
    (0x2BC, 0x40C),
    (0x2D0, 0x420),
    (0x2D4, 0x424),
    (0x2DC, 0x42C),
    (0x2E0, 0x430),
    (0x2E8, 0x438),
    (0x2F8, 0x450),
    (0x2FC, 0x460),
    (0x300, 0x464),
    (0x304, 0x468),
    (0x308, 0x478),
    (0x30C, 0x47C),
    (0x310, 0x480),
    (0x314, 0x490),
    (0x318, 0x494),
    (0x330, 0x4B0),
    (0x340, 0x4C0),
    (0x344, 0x4C4),
    (0x348, 0x4C8),
    (0x34C, 0x4CC),
    (0x350, 0x4D0),
    (0x354, 0x4D4),
    (0x358, 0x4D8),
    (0x35C, 0x4DC),
];
/// Settings2 definition head. Its four sibling references sit at +28, +38, +48 and +58 natively
/// and at +28, +40, +58 and +70 modernly, each modern one followed by a flag word.
const SETTINGS2_DEFINITION: &[(usize, usize)] = &[(0x10, 0x10), (0x60, 0x80), (0x64, 0x84)];
/// Settings2 instance: launch values copied from the root sit at +C4 natively and +D0 modernly.
const SETTINGS2_INSTANCE: &[(usize, usize)] = &[
    (0x14, 0x14),
    (0x20, 0x20),
    (0x24, 0x24),
    (0x30, 0x30),
    (0x38, 0x38),
    (0x3C, 0x3C),
    (0x50, 0x50),
    (0x54, 0x54),
    (0x58, 0x58),
    (0x5C, 0x5C),
    (0x70, 0x78),
    (0x74, 0x7C),
    (0x8C, 0x9C),
    (0x98, 0xA8),
    (0x9C, 0xAC),
    (0xA0, 0xB0),
    (0xA4, 0xB4),
    (0xAC, 0xB8),
    (0xC0, 0xCC),
    (0xC4, 0xD0),
    (0xC8, 0xD4),
    (0xCC, 0xD8),
    (0xD0, 0xDC),
    (0xD4, 0xE0),
    (0xD8, 0xE4),
    (0xDC, 0xE8),
    (0xE8, 0xF4),
    (0xEC, 0xF8),
    (0xF0, 0xFC),
    (0xF4, 0x100),
    (0x10C, 0x11C),
    (0x138, 0x148),
    (0x13C, 0x15C),
    (0x140, 0x160),
];

const ROOT_DEFINITION_SIZE: usize = 0x188;
const ROOT_INSTANCE_SIZE: usize = 0x50;
const SETTINGS1_DEFINITION_SIZE: usize = 0x20;
const SETTINGS1_INSTANCE_SIZE: usize = 0x30;
/// The settings2 definition ends with its state descriptor at +410 and 16 zero bytes; a
/// trailer record, when present, follows the record cut at +428 under the embedded placement.
const SETTINGS2_DEFINITION_SIZE: usize = 0x430;
const SETTINGS2_PROVIDER: usize = 0x428;
const SETTINGS2_INSTANCE_SIZE: usize = 0x168;
/// Where a modern settings2 definition's optional provider trailer would start.
const MODERN_SETTINGS2_TRAILER: usize = 0x580;
const HOLDER_SIZE: usize = 0x10;
const HOLDER_INSTANCE_SIZE: usize = 0x20;
const EXPRESSION_INSTANCE_SIZE: usize = 0x30;
const INPUT_INSTANCE_SIZE: usize = 0x60;
const MODERN_INPUT_INSTANCE_SIZE: usize = 0x30;
const SLOT_DEFINITION_SIZE: usize = 0x58;
const SLOT_INSTANCE_SIZE: usize = 0x30;
const CURVE_HEADER_SIZE: usize = 0x18;
const CURVE_INSTANCE_SIZE: usize = 0x20;
const CURVE_DATA_SIZE: usize = 0xD8;
const MODERN_CURVE_DATA_SIZE: usize = 0x178;
const STATE_DEFINITION_SIZE: usize = 0x18;
/// The modern state provider array: one row naming the root definition, placed after the
/// state definition rows in both layouts.
const MODERN_STATE_PROVIDER: u32 = 0x8080907C;
const SELECTOR_ROW: usize = 16;
const MODERN_SELECTOR_ROW: usize = 24;
/// The settings2 definition's provider rows: 18 of them, each a relative pointer to the root
/// definition, a provider metadata tag and the binding hash. Native rows are 0x20 bytes from +90
/// with the hash at +18; modern rows are 0x28 bytes from +C0 with the hash at +20. The packed
/// provider structure that follows the rows, up to the state array descriptor at +410, is the
/// native dispatch of this component class and is constant across every native movement owner.
const SETTINGS2_ROWS: usize = 18;
const SETTINGS2_NATIVE_ROWS: (usize, usize, usize) = (0x90, 0x20, 0x18);
const SETTINGS2_MODERN_ROWS: (usize, usize, usize) = (0xC0, 0x28, 0x20);

/// The cross-version lookups a rewrite needs.
pub struct Resources {
    /// Modern resource tag to its native counterpart.
    pub tags: BTreeMap<u32, u32>,
    /// 64-bit reference hash to the modern tag it names.
    pub hashes: BTreeMap<u64, u32>,
    /// Modern category dictionary index to the native index of the same name, when the
    /// native dictionary has it.
    pub categories: Vec<Option<u16>>,
}

impl Resources {
    fn tag(&self, modern: u32) -> Result<u32> {
        if modern == u32::MAX || modern == 0 {
            return Ok(modern);
        }
        self.tags.get(&modern).copied().with_context(|| {
            format!("movement owner names resource {modern:08X} without a native counterpart")
        })
    }

    fn hash(&self, hash: u64) -> Result<u32> {
        self.hashes.get(&hash).copied().with_context(|| {
            format!("movement owner names an unresolved 64-bit reference {hash:016X}")
        })
    }
}

/// The result of rewriting one movement owner.
pub struct Movement {
    pub owner: Payload,
    pub allocation: Payload,
    /// Every modern record beside the native record that replaced it.
    pub objects: Vec<Relocation>,
    /// Head references the resource map did not resolve, kept as their modern tag for the
    /// entity assembler to settle: sibling components of the same entity, or resources it
    /// still has to import. Each is the offset in the owner beside the modern tag.
    pub siblings: Vec<(usize, u32)>,
}

/// One reciprocal record: the owner tag, a class and the absolute offset of its twin.
#[derive(Clone, Copy)]
struct Record {
    at: usize,
    class: u32,
    twin: usize,
}

struct Source<'a> {
    p: &'a Payload,
    tag: u32,
    instance: usize,
    definition: usize,
    records: Vec<Record>,
    by_offset: BTreeMap<usize, usize>,
}

impl<'a> Source<'a> {
    fn read(p: &'a Payload) -> Result<Self> {
        ensure!(p.u64(0)? == p.0.len() as u64, "movement owner size differs");
        let instance = p.pointer(16)?;
        let definition = p.pointer(24)?;
        let tag = p.u32(instance)?;
        ensure!(
            p.u32(instance + 4)? == 0x80802AA1 && p.u32(definition + 4)? == 0x80802DCC,
            "unsupported movement owner classes"
        );
        let mut records = Vec::new();
        let mut at = 0;
        while at + 24 <= p.0.len() {
            if p.u32(at)? == tag {
                let class = p.u32(at + 4)?;
                if (0x8080_0000..0x8081_0000).contains(&class) {
                    let twin = usize::try_from(p.u64(at + 8)?)?;
                    if twin % 8 == 0
                        && twin + 16 <= p.0.len()
                        && twin != at
                        && p.u32(twin)? == tag
                        && usize::try_from(p.u64(twin + 8)?)? == at
                    {
                        records.push(Record { at, class, twin });
                    }
                }
            }
            at += 8;
        }
        let by_offset = records
            .iter()
            .enumerate()
            .map(|(index, record)| (record.at, index))
            .collect();
        Ok(Self {
            p,
            tag,
            instance,
            definition,
            records,
            by_offset,
        })
    }

    fn record(&self, at: usize) -> Result<Record> {
        Ok(self.records[*self
            .by_offset
            .get(&at)
            .with_context(|| format!("no movement record at {at:X}"))?])
    }

    /// The array a descriptor (count then relative pointer) names: its element records, taken
    /// as the next `count` records of the element class after the header in file order.
    fn record_array(&self, descriptor: usize, element: u32) -> Result<(usize, Vec<Record>)> {
        let count = usize::try_from(self.p.u64(descriptor)?)?;
        if count == 0 {
            return Ok((0, Vec::new()));
        }
        let header = self.p.pointer(descriptor + 8)?;
        ensure!(
            self.p.u32(header + 8)? == twin_class(element),
            "movement array header names another class"
        );
        let mut elements = Vec::with_capacity(count);
        let mut index = *self
            .by_offset
            .get(&(header + 16))
            .context("movement array elements are not records")?;
        while elements.len() < count {
            let record = *self
                .records
                .get(index)
                .context("movement array runs past the owner")?;
            if record.class == element {
                elements.push(record);
            }
            index += 1;
        }
        Ok((header, elements))
    }

    /// Both halves of a record array: the definition elements named by the definition descriptor,
    /// and the instance header the instance descriptor names, whose count must agree.
    fn array_pair(
        &self,
        def_descriptor: usize,
        inst_descriptor: usize,
        element: u32,
    ) -> Result<(Headers, Vec<Record>)> {
        let (def, elements) = self.record_array(def_descriptor, element)?;
        let count = usize::try_from(self.p.u64(inst_descriptor)?)?;
        ensure!(
            count == elements.len(),
            "movement array halves disagree on their count"
        );
        let inst = if count == 0 {
            0
        } else {
            self.p.pointer(inst_descriptor + 8)?
        };
        Ok((Headers { def, inst }, elements))
    }

    /// A raw array (bytes or vectors): the header offset and its rows.
    fn raw_array(
        &self,
        descriptor: usize,
        class: u32,
        stride: usize,
    ) -> Result<Option<(usize, Vec<u8>, usize)>> {
        let count = usize::try_from(self.p.u64(descriptor)?)?;
        if count == 0 {
            ensure!(
                self.p.u64(descriptor + 8)? == 0,
                "empty movement array with a pointer"
            );
            return Ok(None);
        }
        let header = self.p.pointer(descriptor + 8)?;
        ensure!(
            self.p.u32(header + 8)? == class,
            "movement raw array class differs"
        );
        let rows = self
            .p
            .0
            .get(header + 16..header + 16 + count * stride)
            .context("movement raw array extent")?
            .to_vec();
        Ok(Some((header, rows, count)))
    }
}

/// Array headers name the twin class of their element records: a definition array is headed
/// by the instance class and the other way round. Both classes are modern here.
fn twin_class(element: u32) -> u32 {
    let index = CLASSES
        .iter()
        .position(|(modern, _)| *modern == element)
        .expect("element class is a movement class");
    // Pairs are listed definition then instance.
    if index % 2 == 0 {
        CLASSES[index + 1].0
    } else {
        CLASSES[index - 1].0
    }
}

/// The object tree of a modern movement owner. Every record array exists twice, a definition
/// array named by a definition record and an instance array named by an instance record, so each
/// keeps both modern header offsets.
struct Tree {
    root_d: Record,
    root_i: Record,
    s1_d: Record,
    s2_d: Record,
    blocks: Vec<Block>,
    blocks_headers: Headers,
    slots: Vec<Slot>,
    slots_headers: Headers,
    scalars: Vec<Expression>,
    scalars_headers: Headers,
    states: Vec<Record>,
    states_headers: Headers,
}

/// The modern header offsets of an array's definition and instance halves.
#[derive(Clone, Copy, Default)]
struct Headers {
    def: usize,
    inst: usize,
}

struct Block {
    holder: Record,
    expression: Expression,
}

struct Slot {
    def: Record,
    curves: Vec<Curve>,
    curves_headers: Headers,
}

struct Curve {
    header: Record,
    data: Record,
    expression: Expression,
}

struct Expression {
    def: Record,
    /// The input definitions the expression declares at +40.
    inputs: Vec<Record>,
    inputs_headers: Headers,
    /// The fixed size of the definition: 0x50 for a bare scalar expression, 0x68 with the
    /// namespace and reference words, 0x98 with the sub table descriptor.
    fixed: usize,
}

impl Tree {
    fn read(s: &Source<'_>) -> Result<Self> {
        let root_i = s.record(s.instance)?;
        let root_d = s.record(s.definition)?;
        let of_class = |class: u32| -> Result<Record> {
            s.records
                .iter()
                .copied()
                .find(|record| record.class == class)
                .with_context(|| format!("movement owner lacks class {class:08X}"))
        };
        let s1_d = of_class(0x808081B0)?;
        let s2_d = of_class(0x80809A9E)?;
        let s1_i = s.record(s1_d.twin)?;
        let s2_i = s.record(s2_d.twin)?;
        let inline = s
            .records
            .iter()
            .copied()
            .find(|record| {
                record.class == 0x8080862F && record.at > root_d.at && record.at < s1_d.at
            })
            .context("modern movement root lacks its inline expression")?;
        let (blocks_headers, holders) = s.array_pair(s1_d.at + 0x10, s1_i.at + 0x20, 0x808081B3)?;
        let mut blocks = Vec::new();
        for holder in holders {
            let expression = Expression::read(s, s.record(holder.at + 0x10)?)?;
            blocks.push(Block { holder, expression });
        }
        let (slots_headers, slot_defs) =
            s.array_pair(s2_d.at + 0x90, root_i.at + 0x30, 0x808029CB)?;
        let mut slots = Vec::new();
        for def in slot_defs {
            let inst = s.record(def.twin)?;
            let (curves_headers, curve_headers) =
                s.array_pair(def.at + 0x50, inst.at + 0x20, 0x808029CE)?;
            let mut curves = Vec::new();
            for header in curve_headers {
                let data = s.record(header.at + CURVE_HEADER_SIZE)?;
                ensure!(
                    data.class == 0x80802A01,
                    "curve header is not followed by its data"
                );
                let expression = Expression::read(s, s.record(data.at + MODERN_CURVE_DATA_SIZE)?)?;
                curves.push(Curve {
                    header,
                    data,
                    expression,
                });
            }
            slots.push(Slot {
                def,
                curves,
                curves_headers,
            });
        }
        let (scalars_headers, scalar_defs) =
            s.array_pair(s2_d.at + 0xB0, inline.twin + 0x30, 0x80808631)?;
        let mut scalars = Vec::new();
        for def in scalar_defs {
            scalars.push(Expression::read(s, def)?);
        }
        let (states_headers, states) =
            s.array_pair(s2_d.at + 0x570, s2_i.at + 0x138, 0x808029B5)?;
        ensure!(!states.is_empty(), "movement owner has no state");
        Ok(Self {
            root_d,
            root_i,
            s1_d,
            s2_d,
            blocks,
            blocks_headers,
            slots,
            slots_headers,
            scalars,
            scalars_headers,
            states,
            states_headers,
        })
    }
}

impl Expression {
    fn read(s: &Source<'_>, def: Record) -> Result<Self> {
        ensure!(
            matches!(def.class, 0x8080862F | 0x80808631),
            "movement expression class differs"
        );
        let inst = s.record(def.twin)?;
        let (inputs_headers, inputs) = s.array_pair(def.at + 0x40, inst.at + 0x20, 0x80809590)?;
        let fixed = expression_fixed(s, def)?;
        Ok(Self {
            def,
            inputs,
            inputs_headers,
            fixed,
        })
    }
}

/// The fixed size of an expression definition: 0x50, 0x68 with the input namespace, or 0x98
/// with the sub table descriptor. Its first array header sits at 16-byte alignment past the
/// record, so the header's position tells the forms apart; a record without arrays runs to
/// the next record.
fn expression_fixed(s: &Source<'_>, def: Record) -> Result<usize> {
    let p = s.p;
    let next = s
        .records
        .iter()
        .map(|record| record.at)
        .find(|at| *at > def.at)
        .unwrap_or(p.0.len());
    let room = next - def.at;
    let mut marker = def.at + 0x4C;
    while marker % 16 != 12 {
        marker += 4;
    }
    while marker + 4 <= next {
        if p.u32(marker)? == MODERN_MARKER {
            let header = marker + 4;
            return [0x98, 0x68, 0x50]
                .into_iter()
                .find(|size| def.at + size + 4 <= header && def.at + size + 20 > header)
                .with_context(|| format!("movement expression at {:X} has no known form", def.at));
        }
        marker += 16;
    }
    ensure!(
        matches!(room, 0x50 | 0x68 | 0x98),
        "movement expression at {:X} has no known form",
        def.at
    );
    Ok(room)
}

struct Writer {
    out: Vec<u8>,
    /// Modern offset of a record or array header, and the native offset it was written at.
    placed: BTreeMap<usize, usize>,
    fixups: Vec<Fixup>,
}

enum Fixup {
    /// An absolute twin offset at `at`, naming the modern twin.
    Twin { at: usize, twin: usize },
    /// A relative pointer at `at` to the modern target.
    Relative { at: usize, target: usize },
    /// An array descriptor at `at` with its count and the modern header.
    Array {
        at: usize,
        count: usize,
        header: usize,
    },
    /// A relative pointer at `at` into the body of a modern record whose layout past that point is
    /// the same natively, so the target keeps its offset within the rewritten record.
    Into { at: usize, target: usize },
}

impl Writer {
    fn align(&mut self, marker: u32) -> usize {
        let at = (self.out.len() + 19) & !15;
        self.out.resize(at, 0);
        self.out[at - 4..at].copy_from_slice(&marker.to_le_bytes());
        at
    }

    /// An empty array has no header: its descriptor holds a zero count and a zero pointer.
    fn array_header(&mut self, modern_header: usize, class: u32, count: usize) {
        if count == 0 {
            return;
        }
        let at = self.align(NATIVE_MARKER);
        self.out.extend((count as u64).to_le_bytes());
        self.out.extend(u64::from(class).to_le_bytes());
        self.placed.insert(modern_header, at);
    }

    fn raw_array(
        &mut self,
        descriptor_native: usize,
        modern: Option<(usize, Vec<u8>, usize)>,
        class: u32,
    ) {
        let Some((header, rows, count)) = modern else {
            return;
        };
        self.array_header(header, class, count);
        self.out.extend(rows);
        self.fixups.push(Fixup::Array {
            at: descriptor_native,
            count,
            header,
        });
    }

    fn record(&mut self, modern: usize, bytes: Vec<u8>) -> usize {
        let at = self.out.len();
        self.placed.insert(modern, at);
        self.out.extend(bytes);
        at
    }

    /// Place an embedded record's class word and return where its body starts: at the record's
    /// alignment, behind whatever was written last.
    fn embedded(&mut self, class: u32) -> Result<usize> {
        let native = native_class(class);
        ensure!(
            native != class,
            "movement embedded record class {class:08X} has no native counterpart"
        );
        let align = embedded::alignment(class);
        let body = (self.out.len() + 4 + align - 1) & !(align - 1);
        self.out.resize(body, 0);
        self.out[body - 4..body].copy_from_slice(&native.to_le_bytes());
        Ok(body)
    }
}

/// A head reference of the root or settings, resolved: the native tag when the resource map
/// names it, else the modern tag with a flag saying the entity assembler must settle it.
fn head_reference(p: &Payload, at: usize, resources: &Resources) -> Result<(u32, bool)> {
    let (tag, flag, hash) = (p.u32(at)?, p.u32(at + 4)?, p.u64(at + 8)?);
    let modern = if tag == u32::MAX && flag == 0 && hash != 0 {
        resources.hash(hash)?
    } else {
        ensure!(
            flag <= 2 && hash == 0,
            "movement head reference at {at:X} has an unknown form"
        );
        tag
    };
    if modern == u32::MAX || modern == 0 {
        return Ok((modern, false));
    }
    Ok(match resources.tags.get(&modern) {
        Some(native) => (*native, false),
        None => (modern, true),
    })
}

/// A native record body: the modern bytes copied through an offset map into a record of the
/// native size, with the owner tag and the native class in front. Fields the map does not name
/// keep the template's value when a template record is given, else zero.
fn mapped(
    p: &Payload,
    record: Record,
    size: usize,
    map: &[(usize, usize)],
    owner: u32,
    base: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut bytes = vec![0; size];
    if let Some(base) = base {
        let n = base.len().min(size);
        bytes[..n].copy_from_slice(&base[..n]);
    }
    put(&mut bytes, 0, &owner.to_le_bytes())?;
    put(&mut bytes, 4, &native_class(record.class).to_le_bytes())?;
    for &(native, modern) in map {
        if native + 4 <= size {
            put(&mut bytes, native, &p.bytes::<4>(record.at + modern)?)?;
        }
    }
    Ok(bytes)
}

/// A native record body copied verbatim, class words substituted.
fn verbatim(p: &Payload, record: Record, size: usize, owner: u32) -> Result<Vec<u8>> {
    let mut bytes =
        p.0.get(record.at..record.at + size)
            .context("movement record extent")?
            .to_vec();
    put(&mut bytes, 0, &owner.to_le_bytes())?;
    substitute_classes(&mut bytes);
    Ok(bytes)
}

fn substitute_classes(bytes: &mut [u8]) {
    for word in bytes.chunks_exact_mut(4) {
        let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        let native = native_class(value);
        if native != value {
            word.copy_from_slice(&native.to_le_bytes());
        }
    }
}

/// Copy the parent pointer of an instance record (`+10`) as a fixup.
fn parent(w: &mut Writer, p: &Payload, record: Record, at: usize) -> Result<()> {
    let target = p.pointer(record.at + 0x10)?;
    w.fixups.push(Fixup::Relative {
        at: at + 0x10,
        target,
    });
    Ok(())
}

/// Rewrite one modern movement owner into the native layout.
///
/// `template` is a native movement owner supplying the constant prologue, header and the
/// settings binding tables. `resources` resolves every resource reference and category the
/// owner names. `owner_tag` and `allocation_tag` are the caller's package entries.
pub fn emit(
    source: &Payload,
    template: &Payload,
    template_allocation: &Payload,
    resources: &Resources,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Movement> {
    ensure!(
        template.u64(0)? == template.0.len() as u64
            && template_allocation.u64(0)? == template_allocation.0.len() as u64,
        "native movement template size differs"
    );
    let ti = template.pointer(16)?;
    let td = template.pointer(24)?;
    ensure!(
        template.u32(ti + 4)? == 0x8080388F && template.u32(td + 4)? == 0x80803B73 && ti == 0x130,
        "unsupported native movement template"
    );
    let s = Source::read(source)?;
    let tree = Tree::read(&s)?;
    let p = s.p;
    let template_records = native_records(template)?;
    let template_state_instance = {
        let record = template_records
            .iter()
            .find(|record| record.class == 0x808037BB)
            .context("native movement template lacks a state instance")?;
        ensure!(
            template.u32(record.twin + 0x14)? == 0x808037BD,
            "native movement template's state has no child object to take defaults from"
        );
        &template.0[record.at..record.at + 0x320]
    };
    let mut w = Writer {
        out: template.0[..ti - 16].to_vec(),
        placed: BTreeMap::new(),
        fixups: Vec::new(),
    };
    let mut siblings = Vec::new();

    let emitter = emission::Emitter {
        s: &s,
        tree: &tree,
        template,
        resources,
        owner_tag,
        template_records: &template_records,
    };
    let root_i_at = emitter.instance_roots(&mut w)?;
    emitter.instance_slots(&mut w)?;
    emitter.instance_expressions(&mut w)?;
    emitter.instance_states(&mut w, template_state_instance)?;
    let root_d_at = emitter.definition_root(&mut w, &mut siblings)?;
    let settings_at = emitter.definition_settings(&mut w, &mut siblings)?;
    emitter.settings_pointers(&mut w, settings_at)?;
    emitter.definition_blocks(&mut w)?;
    emitter.slot_definitions(&mut w)?;
    for slot in &tree.slots {
        emitter.slot_arrays(&mut w, slot)?;
    }
    emitter.definition_states(&mut w)?;
    emission::fixups(&mut w)?;
    // ---- header ----
    let len = w.out.len();
    put(&mut w.out, 0, &(len as u64).to_le_bytes())?;
    put(&mut w.out, 0x10, &(root_i_at as i64 - 0x10).to_le_bytes())?;
    put(&mut w.out, 0x18, &(root_d_at as i64 - 0x18).to_le_bytes())?;
    put(&mut w.out, 0x38, &((len - 0x58) as u64).to_le_bytes())?;
    put(&mut w.out, 0x44, &allocation_tag.to_le_bytes())?;
    put(
        &mut w.out,
        0x48,
        &((root_d_at - root_i_at) as u64).to_le_bytes(),
    )?;

    let objects = s
        .records
        .iter()
        .filter(|record| w.placed.contains_key(&record.at))
        .map(|record| {
            let target = w.placed[&record.at];
            let class = p.u32(record.twin + 4)?;
            Ok(Relocation {
                source: Object {
                    owner: s.tag,
                    class,
                    offset: record.at as u64,
                },
                target: Object {
                    owner: owner_tag,
                    class: native_class(class),
                    offset: target as u64,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let allocation = allocation::build(p, &tree, template_allocation)?;
    Ok(Movement {
        owner: Payload(w.out),
        allocation,
        objects,
        siblings,
    })
}

/// The reciprocal records of a native movement owner.
fn native_records(p: &Payload) -> Result<Vec<Record>> {
    let instance = p.pointer(16)?;
    let tag = p.u32(instance)?;
    let mut records = Vec::new();
    let mut at = 0;
    while at + 24 <= p.0.len() {
        if p.u32(at)? == tag {
            let class = p.u32(at + 4)?;
            if (0x8080_0000..0x8081_0000).contains(&class) {
                let twin = usize::try_from(p.u64(at + 8)?)?;
                if twin % 8 == 0
                    && twin + 16 <= p.0.len()
                    && twin != at
                    && p.u32(twin)? == tag
                    && usize::try_from(p.u64(twin + 8)?)? == at
                {
                    records.push(Record { at, class, twin });
                }
            }
        }
        at += 8;
    }
    Ok(records)
}

fn s2_i_template(template: &Payload) -> Result<usize> {
    Ok(template.pointer(16)? + ROOT_INSTANCE_SIZE + SETTINGS1_INSTANCE_SIZE)
}

fn inst_extent(s: &Source<'_>, record: Record) -> usize {
    s.records
        .iter()
        .map(|other| other.at)
        .find(|at| *at > record.at)
        .unwrap_or(s.p.0.len())
        - record.at
}

fn expression_instance(
    w: &mut Writer,
    s: &Source<'_>,
    expression: &Expression,
    owner: u32,
) -> Result<()> {
    let inst = s.record(expression.def.twin)?;
    let at = w.record(
        inst.at,
        verbatim(s.p, inst, EXPRESSION_INSTANCE_SIZE, owner)?,
    );
    w.fixups.push(Fixup::Twin {
        at,
        twin: expression.def.at,
    });
    parent(w, s.p, inst, at)?;
    if !expression.inputs.is_empty() {
        w.fixups.push(Fixup::Array {
            at: at + 0x20,
            count: expression.inputs.len(),
            header: expression.inputs_headers.inst,
        });
    }
    Ok(())
}

fn input_instances(
    w: &mut Writer,
    s: &Source<'_>,
    expression: &Expression,
    owner: u32,
) -> Result<()> {
    if expression.inputs.is_empty() {
        return Ok(());
    }
    w.array_header(
        expression.inputs_headers.inst,
        0x80809788,
        expression.inputs.len(),
    );
    for input in &expression.inputs {
        let ii = s.record(input.twin)?;
        let mut bytes = vec![0; INPUT_INSTANCE_SIZE];
        bytes[..MODERN_INPUT_INSTANCE_SIZE]
            .copy_from_slice(&s.p.0[ii.at..ii.at + MODERN_INPUT_INSTANCE_SIZE]);
        put(&mut bytes, 0, &owner.to_le_bytes())?;
        put(&mut bytes, 4, &0x80809789u32.to_le_bytes())?;
        // The native instance carries more unset sentinels than the modern one.
        for at in [0x24, 0x30, 0x38, 0x3C] {
            put(&mut bytes, at, &u32::MAX.to_le_bytes())?;
        }
        let at = w.record(ii.at, bytes);
        w.fixups.push(Fixup::Twin { at, twin: input.at });
        parent(w, s.p, ii, at)?;
    }
    Ok(())
}

fn expression_definition(
    w: &mut Writer,
    s: &Source<'_>,
    expression: &Expression,
    owner: u32,
) -> Result<()> {
    let def = expression.def;
    let mut bytes = verbatim(s.p, def, expression.fixed, owner)?;
    // Descriptors are rebuilt by fixups.
    for at in [0x10, 0x20, 0x40] {
        put(&mut bytes, at, &[0; 16])?;
    }
    if expression.fixed == 0x98 {
        put(&mut bytes, 0x88, &[0; 16])?;
    }
    let at = w.record(def.at, bytes);
    w.fixups.push(Fixup::Twin { at, twin: def.twin });
    Ok(())
}

/// An expression definition's arrays in field order: code, constants, inputs and the sub table.
fn expression_arrays(
    w: &mut Writer,
    s: &Source<'_>,
    resources: &Resources,
    expression: &Expression,
    owner: u32,
) -> Result<()> {
    let def = expression.def;
    let at = w.placed[&def.at];
    let p = s.p;
    let inputs = usize::try_from(p.u64(def.at + 0x30)?)?;
    if let Some((header, code, count)) = s.raw_array(def.at + 0x10, CODE, 1)? {
        let constants = usize::try_from(p.u64(def.at + 0x20)?)?;
        let native = procedural::lower_program(&code, constants, inputs)
            .with_context(|| format!("movement expression at {:X}", def.at))?;
        ensure!(
            native.len() == count,
            "movement expression lowering changed its length"
        );
        w.raw_array(at + 0x10, Some((header, native, count)), CODE);
    }
    if let Some((header, mut rows, count)) = s.raw_array(def.at + 0x20, VECTORS, 16)? {
        if let Some((_, code, _)) = s.raw_array(def.at + 0x10, CODE, 1)? {
            super::constants::broadcast(&code, &mut rows);
        }
        w.raw_array(at + 0x20, Some((header, rows, count)), VECTORS);
    }
    if !expression.inputs.is_empty() {
        w.array_header(
            expression.inputs_headers.def,
            0x80809789,
            expression.inputs.len(),
        );
        w.fixups.push(Fixup::Array {
            at: at + 0x40,
            count: expression.inputs.len(),
            header: expression.inputs_headers.def,
        });
        for input in &expression.inputs {
            input_definition(w, s, *input, owner)?;
        }
        for input in &expression.inputs {
            input_arrays(w, s, *input)?;
        }
    }
    if expression.fixed == 0x98 {
        sub_table(w, s, resources, inputs, def.at + 0x88, at + 0x88)?;
    }
    Ok(())
}

/// An input definition: 0x28 bytes, or 0x68 with an expression valued block.
fn input_definition(w: &mut Writer, s: &Source<'_>, input: Record, owner: u32) -> Result<()> {
    let size = if s.p.u32(input.at + 0x2C)? == 0x808029D5 {
        0x68
    } else {
        0x28
    };
    let at = w.record(input.at, verbatim(s.p, input, size, owner)?);
    w.fixups.push(Fixup::Twin {
        at,
        twin: input.twin,
    });
    Ok(())
}

fn input_arrays(w: &mut Writer, s: &Source<'_>, input: Record) -> Result<()> {
    if s.p.u32(input.at + 0x2C)? != 0x808029D5 {
        return Ok(());
    }
    // The block holds a code array at +30 and a vector array at +40 through its own descriptors.
    let at = w.placed[&input.at];
    if let Some((header, code, count)) = s.raw_array(input.at + 0x38, CODE, 1)? {
        let constants = usize::try_from(s.p.u64(input.at + 0x48)?)?;
        let native = procedural::lower_program(&code, constants, 1)
            .with_context(|| format!("movement input expression at {:X}", input.at))?;
        w.raw_array(at + 0x38, Some((header, native, count)), CODE);
    }
    if let Some((header, mut rows, count)) = s.raw_array(input.at + 0x48, VECTORS, 16)? {
        if let Some((_, code, _)) = s.raw_array(input.at + 0x38, CODE, 1)? {
            super::constants::broadcast(&code, &mut rows);
        }
        w.raw_array(at + 0x48, Some((header, rows, count)), VECTORS);
    }
    Ok(())
}

/// The sub table of a long expression: rows of an offset (relative to the row) and a kind,
/// each naming an embedded record, rewritten through the embedded record layouts.
fn sub_table(
    w: &mut Writer,
    s: &Source<'_>,
    resources: &Resources,
    inputs: usize,
    descriptor: usize,
    native_descriptor: usize,
) -> Result<()> {
    if s.p.u64(descriptor)? == 0 {
        return Ok(());
    }
    let (header, rows, count) = Rewriter::new(s.p, resources, inputs).table(descriptor)?;
    w.raw_array(
        native_descriptor,
        Some((header, rows, count)),
        native_class(embedded::SUB_TABLE),
    );
    Ok(())
}
