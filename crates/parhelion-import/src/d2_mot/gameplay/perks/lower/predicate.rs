//! The general predicate: modern conditions 21 and 36 to Shadowkeep conditions 20 and 35.
//!
//! Shadowkeep stores every test of the predicate in fixed fields of one flat record. The
//! modern record holds a list of small typed terms instead, one for each test in use, and an
//! absent term leaves its test neutral. Each term's fields land in the Shadowkeep fields below.
//! The mapping was read from twin perks whose predicate holds one or two terms and is checked
//! on every twin predicate by comparing the whole translated record.
//!
//! | Term class | First byte | Fields | Shadowkeep fields |
//! | --- | --- | --- | --- |
//! | `80803075` | 1 | Four Float32, mask | `+08` to `+17`, mask into `+38` |
//! | `80803072` | 2 | Four Float32, mask | `+18` to `+27`, mask shifted four bits into `+38` |
//! | `80803071` | 3 | Four Float32 | `+28` to `+37` |
//! | `80803070` | 4 | Rows `80803784` | Rows `808040CA` at `+60` |
//! | `8080306B` | 7 | Two bytes | `+80`, `+81` |
//! | `80803064` | 11 | One row `80803066`: key and two Float32 | `+D4`, `+D8`, `+DC` |
//!
//! The rows at `+20` hold typed tests. One `808042D5` program comparing a keyed value with
//! zero fills the key fields with the range zero to zero. Tests of the classes the record
//! table pairs become the Shadowkeep rows at `+50`. Any other term, class or comparison is
//! refused.
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use sundial::{
    investment::discovery::labels::Registry,
    package_authoring::sandbox_perk::action::native::{Block, Graph},
};

use super::Node;
use crate::d2_mot::gameplay::perks::table;
use crate::d2_mot::payload::Payload;

const ARRAY_MARKER: u32 = 0x8080_9FB8;

fn rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
    let count = usize::try_from(p.u64(field - 8)?)?;
    if p.u64(field)? == 0 {
        ensure!(count == 0, "source predicate array has a count and no rows");
        return Ok(Vec::new());
    }
    let header = p.pointer(field)?;
    ensure!(
        header >= 4
            && p.u32(header - 4)? == ARRAY_MARKER
            && p.u64(header)? == count as u64
            && p.u32(header + 8)? == class
            && count <= 64,
        "source predicate array is not {count} rows of class {class:08X}"
    );
    ensure!(
        header + 16 + count * stride <= p.0.len(),
        "source predicate array leaves its payload"
    );
    Ok((0..count).map(|row| header + 16 + row * stride).collect())
}

/// The Float32 fields of the neutral record: every test passes.
const NEUTRAL: [(usize, f32); 16] = [
    (0x0C, 1.0),
    (0x10, -1.0),
    (0x14, -1.0),
    (0x1C, 1.0),
    (0x24, 1.0),
    (0x2C, 1.0),
    (0x34, 32.0),
    (0x88, 1.0),
    (0x90, 1.0),
    (0x98, 1.0),
    (0xA0, 1.0),
    (0xA8, 1.0),
    (0xB0, 1.0),
    (0xB8, 1.0),
    (0xD8, 1.0),
    (0xDC, 1.0),
];

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

/// One test of a named property: the key and the range its value must lie in. `inverted`
/// passes outside the range, which the record can only say for the predicate as a whole.
#[derive(Clone, Copy)]
struct Range {
    key: u32,
    least: f32,
    greatest: f32,
    inverted: bool,
}

/// The scalar rows of a predicate: each a `808042D5` program that reads one keyed value and
/// compares it with a constant. Shadowkeep holds such a test as the key and a range.
///
/// The comparison word at `+88` is 0 on the twins whose Shadowkeep range is the constant
/// itself, 2 on one that compares with 0 and holds the range zero to zero, and 4 on one that
/// compares with 0.5 and holds the same range. That reads as equal, at most and below, on
/// whole numbers from zero up, and the words between and after them as their opposites:
/// 1 not equal, 3 at least and 5 above. Only 0, 2 and 4 have a twin, so the import records
/// the reading.
fn scalars(p: &Payload, at: usize) -> Result<Vec<Range>> {
    let mut ranges = Vec::new();
    let entries = rows(p, at + 0x28, 16, 0x8080_91B7)?;
    for row in &entries {
        ensure!(p.u64(*row)? == 0, "source scalar row holds flags");
        let program = p.pointer(row + 8)?;
        ensure!(
            program >= 4 && p.u32(program - 4)? == 0x8080_42D5,
            "source scalar row has another program class"
        );
        p.bytes::<0x90>(program)?;
        for (offset, expected) in [
            (0, 0),
            (0x28, 0),
            (0x30, 0),
            (0x38, 1),
            (0x40, 1),
            (0x48, 0),
            (0x50, 0),
            (0x78, 0),
            (0x80, 1),
        ] {
            ensure!(
                p.u64(program + offset)? == expected,
                "unsupported source scalar program field at {offset:X}"
            );
        }
        let bindings = rows(p, program + 0x10, 8, 0x8080_920F)?;
        let [binding] = bindings.as_slice() else {
            bail!("source scalar program reads {} values", bindings.len());
        };
        ensure!(p.u32(*binding)? <= 1, "source scalar binding differs");
        // The binding's first word is 1 for a key that modern effects write on the item and 0
        // for one they write on the player. Shadowkeep predicates read keys that stock effects
        // write on either, so both are the key at `+D4`. Only the first has a twin.
        let key = p.u32(binding + 4)?;
        ensure!(key != 0, "source scalar key is absent");
        let code = |field: usize| -> Result<Vec<u8>> {
            rows(p, field, 1, 0x8080_0009)?
                .into_iter()
                .map(|byte| p.u8(byte))
                .collect()
        };
        ensure!(
            code(program + 0x20)? == [0x4A, 0, 0x4C, 0]
                && code(program + 0x60)? == [0x42, 0, 0x4C, 0],
            "unsupported source scalar program code"
        );
        let constants = rows(p, program + 0x70, 16, 0x8080_0090)?;
        let [vector] = constants.as_slice() else {
            bail!("source scalar program holds {} constants", constants.len());
        };
        let vector = p.bytes::<16>(*vector)?;
        let constant = f32::from_le_bytes(vector[..4].try_into()?);
        ensure!(
            constant.is_finite()
                && constant >= 0.0
                && vector[4..12] == [0; 8]
                && vector[12..] == 1.0_f32.to_le_bytes(),
            "source scalar row compares with a constant of another form"
        );
        let (least, greatest, inverted) = match p.u64(program + 0x88)? {
            0 => (constant, constant, false),
            1 => (constant, constant, true),
            2 => (0.0, constant.floor(), false),
            3 => (constant.ceil(), f32::MAX, false),
            4 => (0.0, constant.ceil() - 1.0, false),
            5 => (constant.floor() + 1.0, f32::MAX, false),
            word => bail!("source scalar row holds comparison {word}"),
        };
        ensure!(
            least <= greatest,
            "source scalar row compares below zero, which no whole number passes"
        );
        ranges.push(Range {
            key,
            least,
            greatest,
            inverted,
        });
    }
    Ok(ranges)
}

/// A Shadowkeep predicate that only tests one named property: the neutral record with the key
/// at `+D4` and its range at `+D8`. `on` passes while the property is 1 and otherwise while it
/// is 0, which is also how an unwritten property reads. With a `child` it is the nested form
/// 35, which passes when the child does while the property is in range. Stock gates of both
/// forms set byte `+06`: the perks that read `A43A8C2E`, which fourteen stock Named Property
/// effects write on their own weapon.
pub fn key_gate(key: u32, on: bool, child: Option<&Node>) -> Result<Node> {
    let value = if on { 1.0 } else { 0.0 };
    range_gate(key, value, value, child)
}

/// Shadowkeep's compound condition, kind 31, which passes when every requirement has: the
/// `held` conditions, each kept for one tick, then `last`, which keeps nothing. Requirements
/// are siblings, so no predicate holds another and none shares a condition state. Stock groups
/// of this shape hold a predicate while a later event finishes the group, and a held row must
/// come before any row that is not held. Each row's mask is the kinds of its conditions.
pub fn all_of(held: &[Node], last: Option<&Node>) -> Result<Node> {
    let rows = held.len() + usize::from(last.is_some());
    ensure!(
        (2..=16).contains(&rows),
        "a compound condition needs two to sixteen requirements"
    );
    let mut root = vec![0; 24];
    put(&mut root, 0, &1.0_f32.to_le_bytes());
    put(&mut root, 4, &[0xFF, 31, 0, 0]);
    put(&mut root, 8, &(rows as u64).to_le_bytes());
    let mut graph = Graph {
        blocks: vec![
            Block {
                class: 0x8080_3E04,
                count: None,
                bytes: root,
                links: BTreeMap::from([(16, 1)]),
            },
            Block {
                class: 0x8080_3E06,
                count: Some(rows),
                bytes: vec![0; rows * 32],
                links: BTreeMap::new(),
            },
        ],
    };
    for (index, requirement) in held.iter().chain(last).enumerate() {
        // Stock rows hold predicates that test a key, which set byte `+06`, eleven of them in
        // five groups. A nested predicate or a counter adds its children's kinds to the row
        // mask, and any other condition that sets the byte has no stock row, so those are
        // left out.
        ensure!(
            (requirement.kind == 20 || requirement.bytes.get(6) == Some(&0))
                && !matches!(requirement.kind, 26 | 31 | 35),
            "a compound requirement cannot hold a condition that takes its own state"
        );
        let array = graph.blocks.len();
        graph.blocks.push(Block {
            class: 0x8080_40BA,
            count: Some(1),
            bytes: vec![0; 8],
            links: BTreeMap::new(),
        });
        let hold: f32 = if index < held.len() && last.is_some() {
            1.0 / 60.0
        } else {
            0.0
        };
        let row = index * 32;
        put(&mut graph.blocks[1].bytes, row, &hold.to_le_bytes());
        put(&mut graph.blocks[1].bytes, row + 8, &1_u64.to_le_bytes());
        put(
            &mut graph.blocks[1].bytes,
            row + 24,
            &(1_u64 << requirement.kind).to_le_bytes(),
        );
        graph.blocks[1].links.insert(row + 16, array);
        let child =
            Graph::read(&requirement.bytes, 0, requirement.class).map_err(anyhow::Error::msg)?;
        let target = graph.append(&child).map_err(anyhow::Error::msg)?;
        graph.blocks[array].links.insert(0, target);
    }
    graph.validate().map_err(anyhow::Error::msg)?;
    Ok(Node {
        class: 0x8080_3E04,
        kind: 31,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}

/// Whether a condition may sit under a predicate that tests a key. Shadowkeep's nested
/// predicate holds one condition state. It hands that state to the child when the child's
/// byte `+06` is set and to its own key test otherwise, and the key test reads the state
/// without checking that it has one. So the child of a keyed predicate must leave the state
/// alone, and it cannot be a predicate, whose own key test or child would want the state too.
fn stateless(child: &Node) -> Result<()> {
    ensure!(
        child.bytes.get(6) == Some(&0) && !matches!(child.kind, 20 | 35),
        "a predicate that tests a key cannot hold a condition that takes its state"
    );
    Ok(())
}

/// The predicate that only tests that the named property `key` lies from `least` to
/// `greatest`, alone or around a `child` condition.
fn range_gate(key: u32, least: f32, greatest: f32, child: Option<&Node>) -> Result<Node> {
    ensure!(
        !matches!(key, 0 | u32::MAX | 0x811C_9DC5),
        "a property gate needs a key"
    );
    let nested = child.is_some();
    if let Some(child) = child {
        stateless(child)?;
    }
    let mut bytes = vec![0; if nested { 0x108 } else { 0x100 }];
    put(&mut bytes, 0, &1.0_f32.to_le_bytes());
    put(&mut bytes, 4, &[0xFF, if nested { 35 } else { 20 }, 1, 0]);
    for (offset, value) in NEUTRAL {
        put(&mut bytes, offset, &value.to_le_bytes());
    }
    put(&mut bytes, 0x40, &0x20_u32.to_le_bytes());
    put(&mut bytes, 0xC8, &u32::MAX.to_le_bytes());
    put(&mut bytes, 0xD4, &key.to_le_bytes());
    put(&mut bytes, 0xD8, &least.to_le_bytes());
    put(&mut bytes, 0xDC, &greatest.to_le_bytes());
    let mut graph = Graph {
        blocks: vec![Block {
            class: if nested { 0x8080_3DCC } else { 0x8080_3DCE },
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    if let Some(child) = child {
        let child = Graph::read(&child.bytes, 0, child.class).map_err(anyhow::Error::msg)?;
        let index = graph.append(&child).map_err(anyhow::Error::msg)?;
        graph.blocks[0].links.insert(0x100, index);
    }
    graph.validate().map_err(anyhow::Error::msg)?;
    Ok(Node {
        class: graph.blocks[0].class,
        kind: if nested { 35 } else { 20 },
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}

/// The neutral Shadowkeep predicate record, where every test passes, with the source's
/// probability, dispatch bytes, hold and inversion.
fn neutral_record(nested: bool, head: &[u8; 12], hold: f32) -> Vec<u8> {
    let size = if nested { 0x108 } else { 0x100 };
    let mut bytes = vec![0; size];
    put(&mut bytes, 0, &head[..4]);
    put(
        &mut bytes,
        4,
        &[0xFF, if nested { 35 } else { 20 }, head[6], head[7]],
    );
    for (offset, value) in [
        (0x0C, 1.0_f32),
        (0x10, -1.0),
        (0x14, -1.0),
        (0x1C, 1.0),
        (0x24, 1.0),
        (0x2C, 1.0),
        (0x34, 32.0),
        (0x88, 1.0),
        (0x90, 1.0),
        (0x98, 1.0),
        (0xA0, 1.0),
        (0xA8, 1.0),
        (0xB0, 1.0),
        (0xB8, 1.0),
        (0xD8, 1.0),
        (0xDC, 1.0),
    ] {
        put(&mut bytes, offset, &value.to_le_bytes());
    }
    put(&mut bytes, 0x40, &0x20_u32.to_le_bytes());
    put(&mut bytes, 0xC8, &u32::MAX.to_le_bytes());
    put(&mut bytes, 0xD4, &0x811C_9DC5_u32.to_le_bytes());
    put(&mut bytes, 0xE0, &hold.to_le_bytes());
    bytes[0xF8] = head[8];
    bytes
}

/// Terms 1 to 3: four Float32 at `+08` onward, 16 bytes per term, and for terms 1 and 2 a
/// four-bit mask placed in `mask`.
fn float_term(p: &Payload, term: usize, kind: u8, bytes: &mut [u8], mask: &mut u32) -> Result<()> {
    ensure!(
        p.bytes::<3>(term + 1)? == [0; 3],
        "source term {kind} flags"
    );
    let floats = p.bytes::<16>(term + 4)?;
    for lane in floats.chunks_exact(4) {
        ensure!(
            f32::from_le_bytes(lane.try_into()?).is_finite(),
            "source term {kind} holds no number"
        );
    }
    put(bytes, 8 + (usize::from(kind) - 1) * 16, &floats);
    if kind != 3 {
        let bits = p.u32(term + 20)?;
        ensure!(bits < 16, "source term {kind} mask exceeds its four bits");
        *mask |= bits << ((kind - 1) * 4);
    }
    Ok(())
}

/// Term 9: four Float32, two ranges of one meter. Twin predicates place them among the eight
/// ranges Shadowkeep holds from `+84`: meter 2 at ranges 0 and 3, meter 3 at 2 and 5, and the
/// second range of meters 1 and 4 at range 4, on 13 of 14. The first range of meters 1 and 4 is
/// the whole range on every twin.
fn meter_term(p: &Payload, term: usize, bytes: &mut [u8]) -> Result<()> {
    let meter = p.u8(term + 1)?;
    ensure!(p.bytes::<2>(term + 2)? == [0; 2], "source term 9 flags");
    let (first, second) = match meter {
        2 => (Some(0), 3),
        3 => (Some(2), 5),
        1 | 4 => (None, 4),
        _ => bail!("source term 9 names meter {meter}, which no twin places"),
    };
    for lane in 0..4 {
        ensure!(
            p.f32(term + 4 + lane * 4)?.is_finite(),
            "source term 9 holds no number"
        );
    }
    let whole = [0.0_f32.to_le_bytes(), 1.0_f32.to_le_bytes()].concat();
    match first {
        Some(slot) => put(bytes, 0x84 + slot * 8, &p.bytes::<8>(term + 4)?),
        None => ensure!(
            p.bytes::<8>(term + 4)?.as_slice() == whole,
            "source term 9 narrows a range of meter {meter} that no twin places"
        ),
    }
    put(bytes, 0x84 + second * 8, &p.bytes::<8>(term + 12)?);
    Ok(())
}

/// Writes the record's one key range. A test that passes outside its range can only invert the
/// whole predicate, so it must be the only test, which `alone` says of the other tests.
///
/// Every stock predicate sets byte `+06` exactly when it holds a key: 291 keyed and 231 unkeyed
/// records over 1,138 Shadowkeep actions. The key test reads the condition state that byte asks
/// for, so the byte follows the key and not the source record.
fn key_range(bytes: &mut [u8], ranges: &[Range], alone: bool) -> Result<()> {
    if ranges.iter().any(|range| range.inverted) {
        ensure!(
            ranges.len() == 1 && alone,
            "source predicate tests a key for inequality beside other tests"
        );
        bytes[0xF8] ^= 1;
    }
    if let Some(range) = ranges.first() {
        put(bytes, 0xD4, &range.key.to_le_bytes());
        put(bytes, 0xD8, &range.least.to_le_bytes());
        put(bytes, 0xDC, &range.greatest.to_le_bytes());
    }
    bytes[6] = u8::from(!ranges.is_empty());
    Ok(())
}

/// Translate the predicate at `at`. A nested predicate also translates its child condition.
pub fn predicate(source: &Payload, at: usize, labels: Option<&Registry>) -> Result<Node> {
    let p = source;
    ensure!(at >= 4, "source predicate has no class word");
    let nested = match p.u32(at - 4)? {
        0x8080_3061 => false,
        0x8080_305F => true,
        class => bail!("source class {class:08X} is not a general predicate"),
    };
    let head = p.bytes::<12>(at)?;
    let probability = f32::from_le_bytes(head[..4].try_into()?);
    let hold = p.f32(at + 0xC)?;
    ensure!(
        probability.is_finite()
            && (0.0..=1.0).contains(&probability)
            && head[4] == 0xFF
            && head[5] == if nested { 36 } else { 21 }
            && head[8] <= 1
            && head[9..12] == [0; 3]
            && hold.is_finite()
            && hold >= 0.0,
        "unsupported source predicate dispatch, inversion or hold"
    );
    // Every twin holds the first word, which is the Shadowkeep word at `+40`, and the constant
    // 0x100 as the second.
    // The rows at `+20` each point at one typed test. A lone `808042D5` program is the scalar
    // row. Tests of the other classes are records Shadowkeep holds in the rows at `+50`.
    let tests = rows(p, at + 0x28, 16, 0x8080_91B7)?;
    let programs = tests
        .iter()
        .map(|row| Ok(p.u32(p.pointer(row + 8)? - 4)? == 0x8080_42D5))
        .collect::<Result<Vec<_>>>()?;
    let scalar = programs.iter().any(|program| *program);
    let mut ranges = if scalar { scalars(p, at)? } else { Vec::new() };
    // Both words are constant on every twin, with or without scalar rows.
    ensure!(
        p.u64(at + 0x10)? == 0x20 && p.u64(at + 0x18)? == 0x100,
        "unsupported source predicate header words"
    );
    if !nested {
        ensure!(p.u32(at + 0x40)? == 0, "unsupported source predicate tail");
    }
    let mut bytes = neutral_record(nested, &head, hold);

    let mut graph = Graph {
        blocks: vec![Block {
            class: if nested { 0x8080_3DCC } else { 0x8080_3DCE },
            count: None,
            bytes: Vec::new(),
            links: BTreeMap::new(),
        }],
    };
    let mut links = BTreeMap::new();
    let mut mask = 0u32;
    let mut seen = 0u32;
    for row in rows(p, at + 0x38, 16, 0x8080_3079)? {
        ensure!(p.u64(row)? == 0, "source predicate term row holds flags");
        let term = p.pointer(row + 8)?;
        ensure!(term >= 4, "source predicate term has no class word");
        let class = p.u32(term - 4)?;
        let kind = p.u8(term)?;
        ensure!(
            kind < 32 && seen & (1 << kind) == 0,
            "source predicate repeats term {kind}"
        );
        seen |= 1 << kind;
        match (class, kind) {
            (0x8080_3075, 1) | (0x8080_3072, 2) | (0x8080_3071, 3) => {
                float_term(p, term, kind, &mut bytes, &mut mask)?;
            }
            (0x8080_3070, 4) => {
                ensure!(p.bytes::<7>(term + 1)? == [0; 7], "source term 4 flags");
                let entries = rows(p, term + 0x10, 12, 0x8080_3784)?;
                ensure!(!entries.is_empty(), "source term 4 holds no row");
                let mut data = Vec::with_capacity(entries.len() * 12);
                for entry in &entries {
                    data.extend(p.bytes::<12>(*entry)?);
                }
                put(&mut bytes, 0x60, &(entries.len() as u64).to_le_bytes());
                links.insert(0x68, graph.blocks.len());
                graph.blocks.push(Block {
                    class: 0x8080_40CA,
                    count: Some(entries.len()),
                    bytes: data,
                    links: BTreeMap::new(),
                });
            }
            (0x8080_306B, 7) => put(&mut bytes, 0x80, &p.bytes::<2>(term + 1)?),
            (0x8080_3064, 11) => {
                ensure!(p.bytes::<7>(term + 1)? == [0; 7], "source term 11 flags");
                let entries = rows(p, term + 0x10, 12, 0x8080_3066)?;
                ensure!(!entries.is_empty(), "source key term holds no row");
                for entry in entries {
                    let range = Range {
                        key: p.u32(entry)?,
                        least: p.f32(entry + 4)?,
                        greatest: p.f32(entry + 8)?,
                        inverted: false,
                    };
                    ensure!(range.key != 0, "source key term has no key");
                    ensure!(
                        range.least.is_finite() && range.greatest.is_finite(),
                        "source key term range holds no number"
                    );
                    ranges.push(range);
                }
            }
            (0x8080_2092, 9) => meter_term(p, term, &mut bytes)?,
            _ => bail!("source predicate term {kind} of class {class:08X} has no Shadowkeep field"),
        }
    }
    put(&mut bytes, 0x38, &mask.to_le_bytes());
    key_range(&mut bytes, &ranges, seen == 0 && !nested)?;
    if scalar {
    } else if !tests.is_empty() {
        let block = table::rows_into(&mut graph, p, at + 0x28, 0x8080_91B7, labels)?
            .context("source predicate test rows")?;
        put(&mut bytes, 0x50, &(tests.len() as u64).to_le_bytes());
        links.insert(0x58, block);
    }
    // One record tests one key, and a second predicate around it would need the same state.
    ensure!(
        ranges.len() <= 1,
        "source predicate tests more than one key"
    );
    if nested && p.u64(at + 0x40)? != 0 {
        let child = super::condition_with(p, p.pointer(at + 0x40)?, labels)
            .context("nested source condition")?;
        if !ranges.is_empty() {
            stateless(&child)?;
        }
        let child = Graph::read(&child.bytes, 0, child.class).map_err(anyhow::Error::msg)?;
        links.insert(0x100, graph.append(&child).map_err(anyhow::Error::msg)?);
    }
    graph.blocks[0].bytes = bytes;
    graph.blocks[0].links = links;
    graph.validate().map_err(anyhow::Error::msg)?;
    Ok(Node {
        class: graph.blocks[0].class,
        kind: if nested { 35 } else { 20 },
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
