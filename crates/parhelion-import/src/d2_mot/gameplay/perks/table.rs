//! Modern perk records whose Shadowkeep counterpart holds the same fields at other offsets.
//!
//! Each entry of `table/records.json` pairs one modern class with one Shadowkeep class. The
//! pairs and their field maps come from perks both builds ship: the same perk hash, traced to
//! its modern controller and its Shadowkeep action, with the two node lists paired by position.
//! An entry records how many of those twin records the map reproduces. A modern field with no
//! Shadowkeep counterpart is accepted only with a value a twin holds, since the twin shows that
//! value needs nothing more on the Shadowkeep side. Anything else is refused, never dropped.
//!
//! The table carries layout only. Whether Shadowkeep publishes the event a condition waits
//! for, and what an effect does in play, are not established here.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use sundial::{
    investment::discovery::labels::Registry,
    package_authoring::sandbox_perk::action::native::{Block, Graph},
};

use super::lower::{self, Node};
use crate::d2_mot::{native::effects::lower_program, payload::Payload};

const EMPTY: u32 = 0x811C_9DC5;
const ARRAY_MARKER: u32 = 0x8080_9FB8;
const CODE: u32 = 0x8080_0009;
const CONSTANTS: u32 = 0x8080_0090;
/// Modern label rows, 32 bytes with the name first.
const MODERN_LABEL_ROW: u32 = 0x8080_9787;
const NATIVE_LABEL_ROW: u32 = 0x8080_94B3;
/// Shadowkeep's label registry, which every stock label filter names.
pub const NATIVE_LABELS: u32 = 0x80C7_0CA1;
const LABEL_PATH: &[u8] = b"content/common/native/sandbox/label_globals.label_globals.tft\0";
/// The tail of a value program that reads no input: two unit words, sixteen empty bytes, the
/// absent input selector and the empty key.
const PROGRAM_TAIL: [u8; 40] = {
    let mut tail = [0; 40];
    tail[0] = 1;
    tail[8] = 1;
    tail[32] = 0xFF;
    tail[36] = 0xC5;
    tail[37] = 0x9D;
    tail[38] = 0x1C;
    tail[39] = 0x81;
    tail
};

#[derive(Deserialize)]
struct File {
    records: BTreeMap<String, Entry>,
}

/// One class pair. Offsets are bytes from the record start. A `rows` entry repeats per row.
#[derive(Deserialize)]
struct Entry {
    /// The Shadowkeep class, hexadecimal.
    native: String,
    /// `condition`, `effect`, `row` or `rec`, a plain record another entry points at.
    role: String,
    /// The Shadowkeep dispatch kind of a condition or effect.
    kind: Option<u8>,
    native_size: usize,
    /// The modern bytes the map reads. For a row, the modern stride.
    modern_size: usize,
    /// `[native, modern, length]` byte runs carried unchanged.
    copy: Vec<[usize; 3]>,
    /// `[native, value]` bytes every twin holds where no modern byte supplies them.
    fill: Vec<[usize; 2]>,
    /// `[modern, length, accepted values]` for modern fields Shadowkeep has no place for.
    require: Vec<(usize, usize, Vec<String>)>,
    /// `[native, modern, shape, modern row class]` pointer fields. Shapes: `rows`, `record`
    /// (a condition), `code`, `constants` and `null` (empty in every twin).
    pointers: Vec<(usize, usize, String, Option<String>)>,
    /// `[native lists, native predicate, modern lists, modern predicate]` label filters.
    #[serde(default)]
    labels: Vec<[usize; 4]>,
    /// `[native, modern]` Float32 literals a constant modern value program supplies.
    #[serde(default)]
    literals: Vec<[usize; 2]>,
    /// How many twin records the entry was checked on, and how many it reproduces.
    twins: usize,
    exact: usize,
}

struct Record {
    class: u32,
    entry: Entry,
}

/// The label atoms each modern registry holds that Shadowkeep's registry lacks, from
/// `table/labels.json`. Atoms only: a group the registry lacks has no established expansion.
fn modern_only_atoms() -> Result<&'static BTreeMap<u32, BTreeSet<u32>>> {
    type Atoms = std::result::Result<BTreeMap<u32, BTreeSet<u32>>, String>;
    static ATOMS: OnceLock<Atoms> = OnceLock::new();
    #[derive(Deserialize)]
    struct File {
        registries: BTreeMap<String, Vec<String>>,
    }
    ATOMS
        .get_or_init(|| {
            let parse = |text: &str| {
                u32::from_str_radix(text, 16)
                    .map_err(|_| format!("modern label atom table names {text}"))
            };
            let file: File = serde_json::from_str(include_str!("table/labels.json"))
                .map_err(|error| format!("modern label atom table is invalid: {error}"))?;
            file.registries
                .iter()
                .map(|(registry, atoms)| {
                    Ok((
                        parse(registry)?,
                        atoms
                            .iter()
                            .map(|atom| parse(atom))
                            .collect::<Result<_, _>>()?,
                    ))
                })
                .collect()
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

/// The resting value of a named value a modern controller reads from its creator, from
/// `table/publications.json`: what the modern banks that publish the name hold when nothing
/// modifies it. Only names every publisher agrees on, and that no Shadowkeep bank publishes,
/// are listed. Any other name has none.
pub(super) fn resting(name: u32) -> Result<Option<[f32; 4]>> {
    type Values = std::result::Result<BTreeMap<u32, [f32; 4]>, String>;
    static VALUES: OnceLock<Values> = OnceLock::new();
    #[derive(Deserialize)]
    struct File {
        resting: BTreeMap<String, [f32; 4]>,
    }
    let values = VALUES
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/publications.json"))
                .map_err(|error| format!("modern publication table is invalid: {error}"))?;
            file.resting
                .into_iter()
                .map(|(name, value)| {
                    let name = u32::from_str_radix(&name, 16)
                        .map_err(|_| format!("modern publication table names {name}"))?;
                    if value.iter().any(|lane| !lane.is_finite()) {
                        return Err(format!("modern publication {name:08X} is not finite"));
                    }
                    Ok((name, value))
                })
                .collect()
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(values.get(&name).copied())
}

/// The Shadowkeep status key that stands for the modern status key `key`, from
/// `table/statuses.json`. A modern status Shadowkeep has under another key is listed there with
/// the stock status that plays its part. A key both games share, or one with no counterpart,
/// has none.
pub(super) fn status(key: u32) -> Result<Option<u32>> {
    type Keys = std::result::Result<BTreeMap<u32, u32>, String>;
    static KEYS: OnceLock<Keys> = OnceLock::new();
    #[derive(Deserialize)]
    struct Row {
        native: String,
    }
    #[derive(Deserialize)]
    struct File {
        statuses: BTreeMap<String, Row>,
    }
    let keys = KEYS
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/statuses.json"))
                .map_err(|error| format!("status key table is invalid: {error}"))?;
            file.statuses
                .into_iter()
                .map(|(key, row)| {
                    Ok((
                        u32::from_str_radix(&key, 16)
                            .map_err(|_| format!("status key table names {key}"))?,
                        u32::from_str_radix(&row.native, 16)
                            .map_err(|_| format!("status key table holds {}", row.native))?,
                    ))
                })
                .collect()
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(keys.get(&key).copied())
}

/// The Shadowkeep entity of the modern feedback entity with the 64-bit name `name`, from
/// `table/feedback.json`: the entity every twin perk's record pairs it with.
pub(super) fn feedback(name: u64) -> Result<Option<u32>> {
    type Entities = std::result::Result<BTreeMap<u64, u32>, String>;
    static ENTITIES: OnceLock<Entities> = OnceLock::new();
    #[derive(Deserialize)]
    struct File {
        entities: BTreeMap<String, (String, usize)>,
    }
    let entities = ENTITIES
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/feedback.json"))
                .map_err(|error| format!("feedback entity table is invalid: {error}"))?;
            file.entities
                .into_iter()
                .map(|(name, (entity, _))| {
                    Ok((
                        u64::from_str_radix(&name, 16)
                            .map_err(|_| format!("feedback entity table names {name}"))?,
                        u32::from_str_radix(&entity, 16)
                            .map_err(|_| format!("feedback entity table holds {entity}"))?,
                    ))
                })
                .collect()
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))?;
    Ok(entities.get(&name).copied())
}

/// Whether `name` is an atom modern registry `registry` holds and Shadowkeep's registry lacks.
pub(super) fn modern_only_atom(registry: u32, name: u32) -> Result<bool> {
    Ok(modern_only_atoms()?
        .get(&registry)
        .is_some_and(|atoms| atoms.contains(&name)))
}

/// The names of one of a label filter's four lists (match any, require all, exclude any,
/// exclude all together, in Shadowkeep's order) as Shadowkeep objects can carry them, by the
/// rule the Rise of Iron translator applies. A modern atom that Shadowkeep's registry lacks is
/// on no Shadowkeep object, so match any drops it, require all can never hold, exclude any drops
/// it and exclude all together always holds. Each name is paired with the registry its own row
/// names. A name that is neither in Shadowkeep's registry nor an atom of that registry is refused.
fn restrict(
    operation: usize,
    names: Vec<(u32, u32)>,
    labels: Option<&Registry>,
) -> Result<Vec<u32>> {
    let Some(labels) = labels else {
        return Ok(names.into_iter().map(|(name, _)| name).collect());
    };
    let held = |name: &u32| labels.get(*name).is_some();
    let Some(&(missing, _)) = names.iter().find(|(name, _)| !held(name)) else {
        return Ok(names.into_iter().map(|(name, _)| name).collect());
    };
    let atoms = modern_only_atoms()?;
    for (name, registry) in names.iter().filter(|(name, _)| !held(name)) {
        ensure!(
            atoms
                .get(registry)
                .is_some_and(|atoms| atoms.contains(name)),
            "Shadowkeep has no label {name:08X}"
        );
    }
    match operation {
        1 => bail!("the source label filter requires {missing:08X}, which Shadowkeep lacks"),
        3 => Ok(Vec::new()),
        _ => {
            let kept = names
                .into_iter()
                .map(|(name, _)| name)
                .filter(held)
                .collect::<Vec<_>>();
            ensure!(
                operation != 0 || !kept.is_empty(),
                "the source label filter matches only labels Shadowkeep lacks"
            );
            Ok(kept)
        }
    }
}

fn records() -> Result<&'static BTreeMap<u32, Record>> {
    static RECORDS: OnceLock<std::result::Result<BTreeMap<u32, Record>, String>> = OnceLock::new();
    RECORDS
        .get_or_init(|| {
            let file: File = serde_json::from_str(include_str!("table/records.json"))
                .map_err(|error| format!("modern perk record table is invalid: {error}"))?;
            file.records
                .into_iter()
                .map(|(modern, entry)| {
                    let parse = |text: &str| {
                        u32::from_str_radix(text, 16)
                            .map_err(|_| format!("modern perk record table names class {text}"))
                    };
                    Ok((
                        parse(&modern)?,
                        Record {
                            class: parse(&entry.native)?,
                            entry,
                        },
                    ))
                })
                .collect()
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

/// The constants a modern value program pushes with opcode `42`, by index.
pub(super) fn pushed(code: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let wide = matches!(code[at], 0x29 | 0x42..=0x4A | 0x4C);
        if code[at] == 0x42 && at + 1 < code.len() {
            found.push(usize::from(code[at + 1]));
        }
        at += if wide { 2 } else { 1 };
    }
    found
}

/// Give a pushed constant whose source sets only lane 0 the lanes Shadowkeep stores:
/// (x, s, s, s), s the sign of x. On twin programs of equal length every such constant the
/// code pushes has that form (21), and every one no push names stays as the source has it (8).
pub(super) fn widen(lanes: &mut [u8; 16]) {
    if lanes[..4] != [0; 4] && lanes[4..] == [0; 12] {
        let sign = if lanes[3] & 0x80 == 0 { 1.0f32 } else { -1.0 };
        for lane in 1..4 {
            lanes[lane * 4..lane * 4 + 4].copy_from_slice(&sign.to_le_bytes());
        }
    }
}

/// Whether the table pairs `class`, as a condition or effect, with a Shadowkeep class.
pub fn holds(class: u32) -> bool {
    records().is_ok_and(|records| {
        records
            .get(&class)
            .is_some_and(|record| record.entry.kind.is_some())
    })
}

/// The twin evidence behind a class pair: Shadowkeep class, twin records checked and reproduced.
pub fn evidence(class: u32) -> Option<(u32, usize, usize)> {
    let record = records().ok()?.get(&class)?;
    Some((record.class, record.entry.twins, record.entry.exact))
}

/// Translate the condition or effect at `at` through its table entry. `labels` is Shadowkeep's
/// label registry when the caller has it. A label filter then refuses a name Shadowkeep lacks.
pub fn node(p: &Payload, at: usize, labels: Option<&Registry>) -> Result<Node> {
    ensure!(at >= 4, "source record has no class word");
    let class = p.u32(at - 4)?;
    let record = records()?
        .get(&class)
        .with_context(|| format!("source class {class:08X} is outside the record table"))?;
    let kind = record
        .entry
        .kind
        .with_context(|| format!("source class {class:08X} is not a condition or effect"))?;
    let mut builder = Builder {
        p,
        labels,
        graph: Graph { blocks: Vec::new() },
        path: None,
        depth: 0,
    };
    builder.record(record, at)?;
    derive_masks(&mut builder.graph)?;
    let graph = builder.graph;
    graph.validate().map_err(anyhow::Error::msg)?;
    Ok(Node {
        class: record.class,
        kind,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}

/// Translate the modern row array whose descriptor pointer is at `field` into `graph`, for a
/// record written outside the table. Returns the new block, or none for an empty array.
pub fn rows_into(
    graph: &mut Graph,
    p: &Payload,
    field: usize,
    class: u32,
    labels: Option<&Registry>,
) -> Result<Option<usize>> {
    let record = records()?
        .get(&class)
        .filter(|record| record.entry.role == "row")
        .with_context(|| format!("row class {class:08X} is outside the record table"))?;
    let mut builder = Builder {
        p,
        labels,
        graph: std::mem::replace(graph, Graph { blocks: Vec::new() }),
        path: None,
        depth: 0,
    };
    let result = builder
        .array(field, record.entry.modern_size, class)
        .and_then(|positions| {
            if positions.is_empty() {
                Ok(None)
            } else {
                builder.rows(record, &positions).map(Some)
            }
        });
    *graph = builder.graph;
    result
}

struct Builder<'a> {
    p: &'a Payload,
    labels: Option<&'a Registry>,
    graph: Graph,
    /// The shared label registry path, allocated once.
    path: Option<usize>,
    depth: usize,
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) -> Result<()> {
    bytes
        .get_mut(at..at + value.len())
        .context("translated field lies outside its Shadowkeep record")?
        .copy_from_slice(value);
    Ok(())
}

impl Builder<'_> {
    /// Allocate one record and everything it points at. Returns its block index.
    fn record(&mut self, record: &Record, at: usize) -> Result<usize> {
        let index = self.graph.blocks.len();
        self.graph.blocks.push(Block {
            class: record.class,
            count: None,
            bytes: Vec::new(),
            links: BTreeMap::new(),
        });
        let mut bytes = vec![0; record.entry.native_size];
        let mut links = BTreeMap::new();
        self.fill(&record.entry, at, &mut bytes, &mut links, 0)?;
        self.graph.blocks[index].bytes = bytes;
        self.graph.blocks[index].links = links;
        Ok(index)
    }

    /// The modern array whose descriptor pointer is at `field`: its row positions, checked
    /// against the count before the pointer, the array marker and the row class.
    fn array(&self, field: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
        let p = self.p;
        ensure!(field >= 8, "source array has no count");
        let count = usize::try_from(p.u64(field - 8)?)?;
        if p.u64(field)? == 0 {
            ensure!(
                count == 0,
                "source array at {field:X} has a count and no rows"
            );
            return Ok(Vec::new());
        }
        let header = p.pointer(field)?;
        ensure!(
            header >= 4
                && p.u32(header - 4)? == ARRAY_MARKER
                && p.u64(header)? == count as u64
                && p.u32(header + 8)? == class
                && count <= 4096,
            "source array at {field:X} is not {count} rows of class {class:08X}"
        );
        let rows = header + 16;
        ensure!(
            rows + count * stride <= p.0.len(),
            "source array at {field:X} leaves its payload"
        );
        Ok((0..count).map(|row| rows + row * stride).collect())
    }

    /// The block of the record the pointer at `field` reaches. A plain record of the table, such
    /// as an event modifier's operation, is written in place. Anything else a record pointer
    /// reaches is a condition.
    fn pointed_record(&mut self, field: usize) -> Result<usize> {
        let p = self.p;
        let target = p.pointer(field)?;
        ensure!(target >= 4, "source record pointer has no class word");
        let plain = records()?
            .get(&p.u32(target - 4)?)
            .filter(|record| record.entry.role == "rec");
        self.depth += 1;
        let Some(record) = plain else {
            let node = lower::condition_with(p, target, self.labels);
            self.depth -= 1;
            let node = node?;
            let graph = Graph::read(&node.bytes, 0, node.class).map_err(anyhow::Error::msg)?;
            return self.graph.append(&graph).map_err(anyhow::Error::msg);
        };
        let index = self.record(record, target);
        self.depth -= 1;
        index
    }

    /// The block of the constants array at `field`. The program's code array comes just before
    /// it, and the constants its code pushes are widened.
    fn constants(&mut self, field: usize) -> Result<usize> {
        let p = self.p;
        let rows = self.array(field, 16, CONSTANTS)?;
        ensure!(field >= 16, "source constants have no code array");
        let code = self
            .array(field - 16, 1, CODE)?
            .into_iter()
            .map(|byte| p.u8(byte))
            .collect::<Result<Vec<_>>>()?;
        let pushed = pushed(&code);
        let mut bytes = Vec::with_capacity(rows.len() * 16);
        for (index, row) in rows.iter().enumerate() {
            let mut lanes = p.bytes::<16>(*row)?;
            if pushed.contains(&index) {
                widen(&mut lanes);
            }
            bytes.extend(lanes);
        }
        Ok(self.block(CONSTANTS, rows.len(), bytes))
    }

    /// Write one record's bytes into `out`, which starts `base` bytes into its block. Child
    /// allocations go to the graph and their pointer fields to `links`.
    fn fill(
        &mut self,
        entry: &Entry,
        at: usize,
        out: &mut [u8],
        links: &mut BTreeMap<usize, usize>,
        base: usize,
    ) -> Result<()> {
        let p = self.p;
        ensure!(self.depth < 16, "source record nests too deeply");
        let source =
            p.0.get(at..at + entry.modern_size)
                .context("source record leaves its payload")?;
        for (offset, length, accepted) in &entry.require {
            let value = hex::encode(&source[*offset..*offset + *length]);
            ensure!(
                accepted.contains(&value),
                "source field +{offset:X} holds {value}, which no Shadowkeep twin pairs with a counterpart"
            );
        }
        for &[native, modern, length] in &entry.copy {
            put(out, native, &source[modern..modern + length])?;
        }
        for &[native, value] in &entry.fill {
            put(out, native, &[u8::try_from(value)?])?;
        }
        if let Some(kind) = entry.kind {
            put(out, if entry.role == "condition" { 5 } else { 0 }, &[kind])?;
        }
        for &[native, modern] in &entry.literals {
            put(out, native, &self.literal(at + modern)?.to_le_bytes())?;
        }
        for (native, modern, shape, row_class) in &entry.pointers {
            let field = at + modern;
            let set = p.u64(field)? != 0;
            let child = match shape.as_str() {
                "null" => {
                    ensure!(
                        !set,
                        "source pointer +{modern:X} is set where every twin leaves it empty"
                    );
                    None
                }
                _ if !set => None,
                "rows" => {
                    let class = row_class
                        .as_deref()
                        .and_then(|class| u32::from_str_radix(class, 16).ok())
                        .context("record table row class")?;
                    let rows = records()?
                        .get(&class)
                        .with_context(|| format!("row class {class:08X} is outside the table"))?;
                    let positions = self.array(field, rows.entry.modern_size, class)?;
                    Some(self.rows(rows, &positions)?)
                }
                "record" => Some(self.pointed_record(field)?),
                "code" => {
                    let code = self
                        .array(field, 1, CODE)?
                        .into_iter()
                        .map(|byte| p.u8(byte))
                        .collect::<Result<Vec<_>>>()?;
                    // A value program is its code array followed by its constants array.
                    let constants = usize::try_from(p.u64(field + 8)?)?;
                    let bytes = lower_program(&code, constants, 1)?;
                    Some(self.block(CODE, bytes.len(), bytes))
                }
                "constants" => Some(self.constants(field)?),
                shape => bail!("record table names pointer shape {shape}"),
            };
            if let Some(child) = child {
                let count = self.graph.blocks[child].count;
                if let Some(count) = count {
                    ensure!(*native >= 8, "Shadowkeep array has no count");
                    put(out, native - 8, &(count as u64).to_le_bytes())?;
                }
                links.insert(base + native, child);
            } else if matches!(shape.as_str(), "rows" | "code" | "constants") && *native >= 8 {
                put(out, native - 8, &[0; 8])?;
            }
        }
        for &[lists, predicate, modern_lists, _] in &entry.labels {
            self.label_filter(at + modern_lists, out, links, base, lists, predicate)?;
        }
        Ok(())
    }

    fn block(&mut self, class: u32, count: usize, bytes: Vec<u8>) -> usize {
        self.graph.blocks.push(Block {
            class,
            count: Some(count),
            bytes,
            links: BTreeMap::new(),
        });
        self.graph.blocks.len() - 1
    }

    fn rows(&mut self, record: &Record, positions: &[usize]) -> Result<usize> {
        let index = self.block(record.class, positions.len(), Vec::new());
        let size = record.entry.native_size;
        let mut bytes = vec![0; size * positions.len()];
        let mut links = BTreeMap::new();
        for (row, at) in positions.iter().enumerate() {
            self.fill(
                &record.entry,
                *at,
                &mut bytes[row * size..(row + 1) * size],
                &mut links,
                row * size,
            )?;
        }
        self.graph.blocks[index].bytes = bytes;
        self.graph.blocks[index].links = links;
        Ok(index)
    }

    /// The Float32 a modern value program holds when it only pushes its one constant.
    fn literal(&self, at: usize) -> Result<f32> {
        let p = self.p;
        let code = self.array(at + 8, 1, CODE)?;
        let constants = self.array(at + 24, 16, CONSTANTS)?;
        ensure!(
            code.len() == 4
                && p.bytes::<4>(code[0])? == [0x42, 0, 0x4C, 0]
                && constants.len() == 1
                && p.bytes::<40>(at + 32)? == PROGRAM_TAIL,
            "source value program at {at:X} is not one constant"
        );
        let value = p.f32(constants[0])?;
        ensure!(value.is_finite(), "source value program holds no number");
        Ok(value)
    }

    /// Rebuild a label filter's four name lists as Shadowkeep rows. The package build compiles
    /// their predicate against Shadowkeep's registry, so no modern mask is carried.
    fn label_filter(
        &mut self,
        at: usize,
        out: &mut [u8],
        links: &mut BTreeMap<usize, usize>,
        base: usize,
        lists: usize,
        predicate: usize,
    ) -> Result<()> {
        let p = self.p;
        let path = match self.path {
            Some(path) => path,
            None => {
                self.graph.blocks.push(Block {
                    class: 0,
                    count: None,
                    bytes: LABEL_PATH.to_vec(),
                    links: BTreeMap::new(),
                });
                *self.path.insert(self.graph.blocks.len() - 1)
            }
        };
        // The modern dictionary reference is a path, a tag, the word 1 and an empty word. Each
        // label row also names its own registry, at `+10` of the 32-byte row.
        let reference = at + 0x40;
        let mut any = false;
        for operation in 0..4 {
            let rows = self.array(at + operation * 16 + 8, 32, MODERN_LABEL_ROW)?;
            let mut names = Vec::with_capacity(rows.len());
            for row in &rows {
                let name = p.u32(*row)?;
                ensure!(
                    name != EMPTY && name != 0 && p.u32(row + 4)? == 0,
                    "source label row has no name or holds unknown flags"
                );
                names.push((name, p.u32(row + 16)?));
            }
            let names = restrict(operation, names, self.labels)?;
            if names.is_empty() {
                continue;
            }
            any = true;
            let mut bytes = vec![0; names.len() * 24];
            let mut row_links = BTreeMap::new();
            for (index, name) in names.iter().enumerate() {
                put(&mut bytes, index * 24, &name.to_le_bytes())?;
                put(&mut bytes, index * 24 + 16, &NATIVE_LABELS.to_le_bytes())?;
                row_links.insert(index * 24 + 8, path);
            }
            let block = self.graph.blocks.len();
            self.graph.blocks.push(Block {
                class: NATIVE_LABEL_ROW,
                count: Some(names.len()),
                bytes,
                links: row_links,
            });
            put(
                out,
                lists + operation * 16,
                &(names.len() as u64).to_le_bytes(),
            )?;
            links.insert(base + lists + operation * 16 + 8, block);
        }
        if any {
            // Modern filters name more than one dictionary. Twin rows that name another
            // one hold the same name under Shadowkeep's single registry, so with that
            // registry at hand, where every kept name was just found in it, any path passes.
            let named = p.pointer(reference)?;
            ensure!(
                (self.labels.is_some()
                    || p.0.get(named..named + LABEL_PATH.len()) == Some(LABEL_PATH))
                    && p.u32(reference + 12)? == 1
                    && p.u64(reference + 16)? == 0,
                "source label filter names another registry"
            );
        }
        links.insert(base + lists + 0x40, path);
        put(out, lists + 0x48, &NATIVE_LABELS.to_le_bytes())?;
        // An empty filter has no compiled predicate. A populated one is compiled by the build.
        put(out, predicate, &[if any { 0 } else { 0xFF }])?;
        Ok(())
    }
}

/// The event mask of the condition at `index`: its own kind bit and those of its children.
fn condition_mask(graph: &Graph, index: usize, depth: usize) -> Result<u64> {
    ensure!(depth < 32, "condition tree nests too deeply");
    let block = graph.blocks.get(index).context("condition block")?;
    let kind = *block.bytes.get(5).context("condition has no kind byte")?;
    ensure!(kind < 64, "condition kind exceeds the event mask");
    let mut mask = 1u64 << kind;
    match block.class {
        0x8080_3E30 => mask |= counter_mask(graph, index, depth)?,
        0x8080_3E04 => {
            if let Some(rows) = block.links.get(&0x10) {
                let rows = &graph.blocks[*rows];
                for row in 0..rows.count.unwrap_or(0) {
                    mask |= list_mask(graph, rows, row * 0x20 + 0x10, depth)?;
                }
            }
        }
        0x8080_3DCC => {
            if let Some(child) = block.links.get(&0x100) {
                mask |= condition_mask(graph, *child, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(mask)
}

/// The kinds a counter's rows contribute, without the counter's own bit.
fn counter_mask(graph: &Graph, index: usize, depth: usize) -> Result<u64> {
    let mut mask = 0;
    if let Some(rows) = graph.blocks[index].links.get(&0x10) {
        let rows = &graph.blocks[*rows];
        for row in 0..rows.count.unwrap_or(0) {
            if let Some(child) = rows.links.get(&(row * 0x20)) {
                mask |= condition_mask(graph, *child, depth + 1)?;
            }
        }
    }
    Ok(mask)
}

/// The kinds of a condition pointer list linked at `field` of `owner`.
fn list_mask(graph: &Graph, owner: &Block, field: usize, depth: usize) -> Result<u64> {
    let mut mask = 0;
    if let Some(list) = owner.links.get(&field) {
        let list = &graph.blocks[*list];
        for row in 0..list.count.unwrap_or(0) {
            if let Some(child) = list.links.get(&(row * 8)) {
                mask |= condition_mask(graph, *child, depth + 1)?;
            }
        }
    }
    Ok(mask)
}

/// Write the routing masks Shadowkeep stores beside a counter, beside each subgroup and in a
/// timer extension: the kinds of the conditions beneath them. A counter with no contributing
/// condition stores 1.
fn derive_masks(graph: &mut Graph) -> Result<()> {
    let mut writes = Vec::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        match (block.class, block.count) {
            (0x8080_3E30, None) => {
                let mask = counter_mask(graph, index, 0)?;
                writes.push((index, 0x18, if mask == 0 { 1 } else { mask }));
            }
            (0x8080_3E06, Some(count)) => {
                for row in 0..count {
                    let mask = list_mask(graph, block, row * 0x20 + 0x10, 0)?;
                    writes.push((index, row * 0x20 + 0x18, mask));
                }
            }
            // A timer extension stores the kinds of the conditions that extend its timers.
            (0x8080_3E3B, None) => {
                writes.push((index, 0x20, list_mask(graph, block, 0x18, 0)?));
            }
            _ => {}
        }
    }
    for (index, at, mask) in writes {
        put(&mut graph.blocks[index].bytes, at, &mask.to_le_bytes())?;
    }
    Ok(())
}
