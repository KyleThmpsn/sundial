//! A native crosshair scene built from a modern crosshair.
//!
//! The root borrows a twin's native flow graph and object list. A twin qualifies when its modern
//! root has the source root's flow graph template: the same words before the object list, apart
//! from tags and the size fields that move with the payload length. The flow graph names each
//! object's clip, so the twin's names are replaced by the source's. The object list names, for
//! each object, its anchor piece, its animated piece, its clip and a flag.
//!
//! A source piece with a twin is that twin. Any other piece clones the native piece of a twin
//! with the same object type and component classes. Its own rig controls, skeleton, markers and
//! animation (lookup, lookup table, bank, clips and bank consumer) convert onto the clone's
//! components. A component with no converter keeps the native piece's own, recorded as a carrier.
//! Modern components with no native class are left out, recorded as dropped. Kept native pieces
//! and owners load only through the index of the native pair whose scene holds them, so the
//! converted pair inherits every such pair's index.
use super::twins::{self, Piece, Twins, native_class};
use super::*;
use crate::d2_mot::{
    markers,
    rig_convert::{self, animation::clips, animation::first_person::controls},
};
use std::collections::BTreeSet;

pub(super) const MODERN_OBJECTS: usize = 0x270;
pub(super) const MODERN_OBJECT_STRIDE: usize = 0xE0;
pub(super) const MODERN_OBJECT: u32 = 0x8080_5FB1;
const NATIVE_OBJECTS: usize = 0x218;
const NATIVE_OBJECT_STRIDE: usize = 0x40;
const NATIVE_OBJECT: u32 = 0x8080_664F;

/// The native shared-tag companion a private type-16 owner's companion is declared from.
const SHARED_COMPANION: u32 = 0x81A6_62DE;
const MODERN_LOOKUP: u32 = 0x8080_25F8;
const MODERN_CONSUMER: u32 = 0x8080_289B;
const NATIVE_LOOKUP: u32 = 0x8080_344B;
const NATIVE_CONSUMER: u32 = 0x8080_36CF;
/// Modern lookup table classes and their native counterparts, with the arrays' marker.
const TABLE_CLASSES: [(u32, u32); 5] = [
    (0x8080_9FB8, 0x8080_9FBD),
    (0x8080_25DB, 0x8080_342E),
    (0x8080_25DC, 0x8080_342F),
    (0x8080_25E1, 0x8080_3434),
    (0x8080_25E6, 0x8080_3439),
];

/// A node's symbol, the tag it was cloned from, its bytes and the offsets it names other nodes at.
type Node = (String, u32, Vec<u8>, Vec<(usize, String)>);

/// A converted crosshair's nodes, patched by the tags they were cloned from.
struct Unit {
    map: BTreeMap<u32, String>,
    nodes: Vec<Node>,
}

impl Unit {
    fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            nodes: Vec::new(),
        }
    }

    /// Add a node cloned from `template`. Every word naming the template names the node.
    fn add(&mut self, symbol: &str, template: u32, bytes: Vec<u8>, patches: Vec<(usize, String)>) {
        self.map.insert(template, symbol.to_owned());
        self.nodes
            .push((symbol.to_owned(), template, bytes, patches));
    }

    fn finish(self, graph: &Path, nodes: &mut Vec<Value>) -> Result<()> {
        fs::create_dir_all(graph.join("crosshair"))?;
        for (symbol, template, bytes, explicit) in self.nodes {
            let mut p = Payload(bytes);
            let mut patches = BTreeMap::new();
            for (at, word) in words(&p) {
                if let Some(target) = self.map.get(&word) {
                    patches.insert(at, target.clone());
                }
            }
            for (at, target) in explicit {
                patches.insert(at, target);
            }
            // The linker writes each private tag over an unresolved placeholder.
            for &at in patches.keys() {
                write_u32(&mut p.0, at, u32::MAX);
            }
            let file = format!("crosshair/{symbol}.bin");
            fs::write(graph.join(&file), &p.0)?;
            nodes.push(json!({"symbol":symbol,"template":template,"file":file,"reference":Value::Null,
                "patches":patches.into_iter().map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol})).collect::<Vec<_>>()}));
        }
        Ok(())
    }
}

/// Words before the object list, with tags and the length-dependent fields cleared.
fn template_words(r: &mut Reader, root: u32) -> Result<(usize, Vec<u32>)> {
    let p = r.tag(root, None)?;
    let resource = p.pointer(24)?;
    let mut result = Vec::new();
    for (at, word) in words(&p).take_while(|(at, _)| *at < resource) {
        let tag = word >> 24 == 0x80 && (word >> 16) & 0xFF != 0x80;
        result.push(if tag || [0, 0x48, 0x68].contains(&at) {
            0
        } else {
            word
        });
    }
    Ok((resource, result))
}

fn replace_word(bytes: &mut [u8], from: u32, to: u32) -> usize {
    let mut count = 0;
    for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
        if bytes[at..at + 4] == from.to_le_bytes() {
            bytes[at..at + 4].copy_from_slice(&to.to_le_bytes());
            count += 1;
        }
    }
    count
}

fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// The native family whose template the source's root shares. A family must name each of its
/// object clips in its native flow graph, so the source's names can take their places.
fn matching_family<'a>(
    m: &mut Reader,
    n: &mut Reader,
    twins: &'a Twins,
    names: &[u32],
    shape: &(usize, Vec<u32>),
) -> Result<&'a twins::Family> {
    for candidate in &twins.families {
        if candidate.modern_names.len() != names.len()
            || template_words(m, candidate.modern_root)? != *shape
        {
            continue;
        }
        let flow = n.tag(candidate.native_flow, None)?;
        if candidate
            .modern_names
            .iter()
            .all(|name| words(&flow).any(|(_, word)| word == *name))
        {
            return Ok(candidate);
        }
    }
    anyhow::bail!("no native crosshair shares the source flow graph template")
}

/// A piece of the source scene, as Shadowkeep will see it.
#[derive(Clone)]
enum Target {
    Native(u32),
    New(String),
}

pub(super) fn convert(
    m: &mut Reader,
    n: &mut Reader,
    twins: &Twins,
    modern_pair: u32,
    prefix: &str,
    graph: &Path,
    nodes: &mut Vec<Value>,
) -> Result<(String, Value)> {
    let first_node = nodes.len();
    let pair = m.tag(modern_pair, None)?;
    let entity = m.ref64(&pair, 0)?;
    let source = twins::piece(m, entity, true)?;
    let roots = source
        .owners
        .iter()
        .filter(|(_, class)| *class == twins::MODERN_ROOT)
        .collect::<Vec<_>>();
    let [(root, _)] = roots.as_slice() else {
        anyhow::bail!("source crosshair {entity:08X} has no unique root");
    };
    let shape = template_words(m, *root)?;
    let names = twins::modern_names(m, *root)?;
    let family = matching_family(m, n, twins, &names, &shape)?;
    ensure!(
        names.len() == family.modern_names.len(),
        "source crosshair object count differs from its native family"
    );
    let rp = m.tag(*root, None)?;
    let resource = rp.pointer(24)?;
    let mut objects = Vec::new();
    for row in rp.array(
        resource + MODERN_OBJECTS,
        MODERN_OBJECT_STRIDE,
        Some(MODERN_OBJECT),
    )? {
        let anchor = m.ref64(&rp, row + 0xA0)?;
        let piece = m.ref64(&rp, row + 0xC0).ok();
        objects.push((rp.u32(row + 0x40)?, anchor, piece, rp.u32(row + 0xD0)?));
    }
    let mut targets: BTreeMap<u32, Target> = BTreeMap::new();
    let mut reports = Vec::new();
    for &(_, anchor, piece, _) in &objects {
        for tag in std::iter::once(anchor).chain(piece) {
            if targets.contains_key(&tag) {
                continue;
            }
            let symbol = format!("{prefix}-piece-{}", targets.len());
            let (target, report) = convert_piece(m, n, twins, tag, &symbol, graph, nodes)?;
            targets.insert(tag, target);
            reports.push(report);
        }
    }
    let mut unit = Unit::new();
    // The flow graph names each object's clip. Replace the family's names with the source's.
    let mut flow = n.tag(family.native_flow, Some(0x8080_9C36))?.0.clone();
    for (from, to) in family.modern_names.iter().zip(&names) {
        if from == to {
            continue;
        }
        ensure!(
            words(&Payload(flow.clone())).all(|(_, word)| word != *to),
            "source crosshair clip {to:08X} already occurs in the native flow graph"
        );
        ensure!(
            replace_word(&mut flow, *from, *to) > 0,
            "native flow graph does not name its object clip {from:08X}"
        );
    }
    let mut list = n
        .tag(family.native_objects, Some(0x8080_9C36))?
        .as_ref()
        .clone();
    let base = list.pointer(24)?;
    let rows = list.array(
        base + NATIVE_OBJECTS,
        NATIVE_OBJECT_STRIDE,
        Some(NATIVE_OBJECT),
    )?;
    ensure!(
        rows.len() == objects.len(),
        "native crosshair object list count differs"
    );
    let mut replaced = Vec::new();
    let mut explicit = Vec::new();
    for (&row, &(name, anchor, piece, flag)) in rows.iter().zip(&objects) {
        replaced.extend([list.u32(row + 0x30)?, list.u32(row + 0x38)?]);
        write_u32(&mut list.0, row + 0x28, name);
        write_u32(&mut list.0, row + 0x3C, flag);
        for (at, tag) in [(row + 0x30, Some(anchor)), (row + 0x38, piece)] {
            match tag.map(|tag| &targets[&tag]) {
                None => write_u32(&mut list.0, at, u32::MAX),
                Some(Target::Native(native)) => write_u32(&mut list.0, at, *native),
                Some(Target::New(symbol)) => {
                    write_u32(&mut list.0, at, 0);
                    explicit.push((at, symbol.clone()));
                }
            }
        }
    }
    let written = rows
        .iter()
        .flat_map(|row| [row + 0x30, row + 0x38])
        .collect::<Vec<_>>();
    for old in replaced.into_iter().filter(|tag| *tag != u32::MAX) {
        ensure!(
            words(&list).all(|(at, word)| word != old || written.contains(&at)),
            "native crosshair object list names its piece {old:08X} outside the object rows"
        );
    }
    let root_symbol = format!("{prefix}-root");
    let flow_symbol = format!("{prefix}-flow");
    let objects_symbol = format!("{prefix}-objects");
    let pair_symbol = format!("{prefix}-pair");
    unit.add(&flow_symbol, family.native_flow, flow, Vec::new());
    unit.add(&objects_symbol, family.native_objects, list.0, explicit);
    let native_root = n
        .tag(family.native_entity, Some(twins::NATIVE_ENTITY))?
        .0
        .clone();
    unit.add(&root_symbol, family.native_entity, native_root, Vec::new());
    let native_pair = n.tag(family.native_pair, None)?;
    ensure!(
        native_pair.0.len() == 8 && native_pair.u32(0)? == family.native_entity,
        "native crosshair pair layout differs"
    );
    let mut pair_bytes = native_pair.0.clone();
    write_u32(&mut pair_bytes, 0, 0);
    unit.add(
        &pair_symbol,
        family.native_pair,
        pair_bytes,
        vec![(0, root_symbol.clone())],
    );
    unit.finish(graph, nodes)?;
    // Pieces and owners kept from other native scenes load only through their own pair's index.
    let mut inherited = BTreeSet::new();
    for node in &nodes[first_node..] {
        let file = node["file"].as_str().context("crosshair node file")?;
        for (_, word) in words(&Payload(fs::read(graph.join(file))?)) {
            if let Some(&owner) = twins.pairs.get(&word) {
                inherited.insert(owner);
            }
        }
    }
    inherited.remove(&family.native_pair);
    // The pair is a shared-tag owner, so it loads the scene through a companion of its own,
    // cloned at link time from the native pair's and extended with the inherited pairs'.
    let companion = n.tag(SHARED_COMPANION, None)?;
    let file = format!("crosshair/{pair_symbol}-companion.bin");
    fs::write(graph.join(&file), &companion.0)?;
    nodes.push(
        json!({"symbol":format!("{pair_symbol}-companion"),"template":SHARED_COMPANION,
        "file":file,"reference":Value::Null,"patches":[],"shared_owner":pair_symbol,
        "source_parent":family.native_pair,"inherited":inherited.iter().collect::<Vec<_>>()}),
    );
    Ok((
        pair_symbol,
        json!({"source":hex(entity),"family_root":hex(family.modern_root),
            "native_family":hex(family.native_entity),"names":names.iter().map(|n| hex(*n)).collect::<Vec<_>>(),
            "pieces":reports}),
    ))
}

fn convert_piece(
    m: &mut Reader,
    n: &mut Reader,
    twins: &Twins,
    tag: u32,
    symbol: &str,
    graph: &Path,
    nodes: &mut Vec<Value>,
) -> Result<(Target, Value)> {
    if let Some(&native) = twins.map.get(&tag) {
        return Ok((
            Target::Native(native),
            json!({"source":hex(tag),"twin":hex(native)}),
        ));
    }
    let source = twins::piece(m, tag, true)?;
    let classes = |p: &Piece| p.owners.iter().map(|(_, c)| *c).collect::<Vec<_>>();
    let (template, native) = twins
        .pieces
        .iter()
        .filter(|(t, _)| t.object == source.object && classes(t) == classes(&source))
        .max_by_key(|(t, _)| {
            t.owners
                .iter()
                .filter(|o| source.owners.contains(o))
                .count()
        })
        .context(format!(
            "no native crosshair piece has the shape of {tag:08X}"
        ))?;
    let mut unit = Unit::new();
    let mut entity = n.tag(native.tag, Some(twins::NATIVE_ENTITY))?.0.clone();
    let mut components = Vec::new();
    for (index, &(owner, class)) in source.owners.iter().enumerate() {
        let Some(native_class) = native_class(class) else {
            components.push(json!({"source":hex(owner),"class":hex(class),"result":"dropped"}));
            continue;
        };
        let carriers = native
            .owners
            .iter()
            .filter(|(_, c)| *c == native_class)
            .collect::<Vec<_>>();
        let [(carrier, _)] = carriers.as_slice() else {
            components.push(json!({"source":hex(owner),"class":hex(class),"result":"dropped"}));
            continue;
        };
        let carrier = *carrier;
        if let Some(&mapped) = twins.map.get(&owner) {
            if mapped != carrier {
                replace_word(&mut entity, carrier, mapped);
            }
            components.push(json!({"source":hex(owner),"class":hex(class),"result":"twin","native":hex(mapped)}));
            continue;
        }
        if owner == template.owners[index].0 {
            components.push(json!({"source":hex(owner),"class":hex(class),"result":"template","native":hex(carrier)}));
            continue;
        }
        let part = format!("{symbol}-{index}");
        let converted = match class {
            0x8080_8B5F => {
                controls::convert_unmapped(&m.tag(owner, None)?.0, &n.tag(carrier, None)?.0)
                    .map(|p| unit.add(&part, carrier, p.0, Vec::new()))
            }
            0x8080_81DE => {
                rig_convert::skeleton(&m.tag(owner, None)?.0, &n.tag(carrier, None)?.0, carrier)
                    .and_then(|(p, report)| {
                        let patches = report["owner_patches"]
                            .as_array()
                            .context("skeleton owner patches")?
                            .iter()
                            .map(|at| {
                                Ok((
                                    usize::try_from(at.as_u64().context("skeleton patch")?)?,
                                    part.clone(),
                                ))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        unit.add(&part, carrier, p.0, patches);
                        Ok(())
                    })
            }
            0x8080_819D => {
                let modern = m.tag(owner, None)?;
                let native_markers = n.tag(carrier, None)?;
                modern
                    .pointer(24)
                    .and_then(|resource| markers::read(&modern, resource + 0xB8, markers::SOURCE))
                    .and_then(|rows| markers::replace(&native_markers, &rows))
                    .map(|converted| unit.add(&part, carrier, converted.0, Vec::new()))
            }
            MODERN_LOOKUP => {
                let consumers = native
                    .owners
                    .iter()
                    .filter(|(_, c)| *c == NATIVE_CONSUMER)
                    .collect::<Vec<_>>();
                match consumers.as_slice() {
                    [(consumer, _)] => animation(m, n, owner, carrier, *consumer, &part, &mut unit),
                    _ => Err(anyhow::anyhow!(
                        "the native piece has no unique bank consumer"
                    )),
                }
            }
            MODERN_CONSUMER => {
                components
                    .push(json!({"source":hex(owner),"class":hex(class),"result":"with lookup"}));
                continue;
            }
            _ => Err(anyhow::anyhow!("no converter")),
        };
        components.push(match converted {
            Ok(()) => {
                json!({"source":hex(owner),"class":hex(class),"result":"converted","symbol":part})
            }
            Err(error) => json!({"source":hex(owner),"class":hex(class),"result":"carrier",
                "native":hex(carrier),"reason":format!("{error:#}")}),
        });
    }
    unit.add(symbol, native.tag, entity, Vec::new());
    unit.finish(graph, nodes)?;
    Ok((
        Target::New(symbol.to_owned()),
        json!({"source":hex(tag),"template":hex(template.tag),"native_template":hex(native.tag),
            "symbol":symbol,"components":components}),
    ))
}

/// Convert a piece's animation: the lookup owner names its bank and its lookup table, the bank
/// names its clips and describes them, and the bank consumer names the bank.
fn animation(
    m: &mut Reader,
    n: &mut Reader,
    modern_lookup: u32,
    native_lookup: u32,
    native_consumer: u32,
    symbol: &str,
    unit: &mut Unit,
) -> Result<()> {
    let ml = m.tag(modern_lookup, None)?;
    let mres = ml.pointer(24)?;
    ensure!(
        ml.u32(mres - 4)? == MODERN_LOOKUP,
        "source crosshair lookup layout differs"
    );
    let modern_bank = m.tag(ml.u32(mres + 0xA8)?, Some(0x8080_289F))?;
    let modern_table = m.tag(ml.u32(mres + 0xB4)?, Some(0x8080_2612))?;
    let nl = n.tag(native_lookup, None)?;
    let nres = nl.pointer(24)?;
    ensure!(
        nl.u32(nres - 4)? == NATIVE_LOOKUP,
        "native crosshair lookup layout differs"
    );
    let bank_tag = nl.u32(nres + 0x90)?;
    let table_tag = nl.u32(nres + 0x9C)?;
    let bank = n.tag(bank_tag, Some(0x8080_36F6))?;
    let table = n.tag(table_tag, Some(0x8080_3465))?;
    let consumer = n.tag(native_consumer, None)?;
    ensure!(
        consumer.u32(rig_convert::animation::first_person::consumers::bank_field(
            &consumer
        )?)? == bank_tag,
        "native crosshair bank consumer names another bank"
    );
    // Both versions share the name table, so a clip keeps its meaning in the native bank.
    let modern_names = modern_bank
        .array(0x48, 4, Some(0x8080_8AEA))?
        .into_iter()
        .map(|at| modern_bank.u32(at))
        .collect::<Result<Vec<_>>>()?;
    let native_names = bank
        .array(0x58, 4, Some(0x8080_8F1C))?
        .into_iter()
        .map(|at| bank.u32(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        modern_names == native_names,
        "crosshair bank name tables differ"
    );
    let native_clips = bank.array(8, 4, Some(0x8080_8F48))?;
    let clip_template = bank.u32(
        *native_clips
            .first()
            .context("native crosshair bank has no clip")?,
    )?;
    let per_clip = bank.array(0x18, 4, Some(0x8080_0007))?;
    let per_clip_value = bank.u32(
        *per_clip
            .first()
            .context("native crosshair bank per-clip value")?,
    )?;
    let descriptors = bank.array(0x68, 32, Some(0x8080_9002))?;
    let descriptor_template =
        bank.bytes::<32>(*descriptors.first().context("native crosshair descriptor")?)?;
    let mut frames = Vec::new();
    let mut clip_symbols = Vec::new();
    for (index, row) in modern_bank
        .array(8, 16, Some(0x8080_8BDF))?
        .into_iter()
        .enumerate()
    {
        let clip = m.tag(m.ref64(&modern_bank, row)?, Some(0x8080_8BE0))?;
        let (converted, _) = clips::convert(&clip.0)?;
        let count = converted.u16(0x13C)?;
        ensure!(count > 0, "crosshair clip has no frames");
        frames.push(count);
        let clip_symbol = format!("{symbol}-clip-{index}");
        unit.nodes
            .push((clip_symbol.clone(), clip_template, converted.0, Vec::new()));
        clip_symbols.push(clip_symbol);
    }
    let mut rows = Vec::new();
    for row in modern_bank.array(0x58, 48, Some(0x8080_8BDE))? {
        let slot = usize::try_from(modern_bank.u32(row + 0x28)?)?;
        let count = *frames
            .get(slot)
            .context("crosshair descriptor names a missing clip")?;
        let mut descriptor = descriptor_template;
        descriptor[16..20].copy_from_slice(&modern_bank.u32(row + 0x10)?.to_le_bytes());
        descriptor[20..24].copy_from_slice(&(f32::from(count - 1) / 30.0).to_le_bytes());
        descriptor[24..26].copy_from_slice(&u16::try_from(slot)?.to_le_bytes());
        rows.extend_from_slice(&descriptor);
    }
    let mut new_bank = bank.as_ref().clone();
    rig_convert::write_array(
        &mut new_bank.0,
        8,
        0x8080_8F48,
        clip_symbols.len(),
        &vec![0; 4 * clip_symbols.len()],
    )?;
    rig_convert::write_array(
        &mut new_bank.0,
        0x18,
        0x8080_0007,
        clip_symbols.len(),
        &per_clip_value.to_le_bytes().repeat(clip_symbols.len()),
    )?;
    rig_convert::write_array(&mut new_bank.0, 0x68, 0x8080_9002, rows.len() / 32, &rows)?;
    let size = new_bank.0.len() as u64;
    new_bank.0[..8].copy_from_slice(&size.to_le_bytes());
    let clip_patches = new_bank
        .array(8, 4, Some(0x8080_8F48))?
        .into_iter()
        .zip(&clip_symbols)
        .map(|(at, s)| (at, s.clone()))
        .collect::<Vec<_>>();
    // The lookup tables share one layout across versions apart from their classes.
    let mut new_table = modern_table.as_ref().clone();
    for (at, word) in words(&modern_table) {
        if let Some((_, native)) = TABLE_CLASSES.iter().find(|(modern, _)| *modern == word) {
            write_u32(&mut new_table.0, at, *native);
        }
    }
    for at in [8usize, 0x18] {
        let expected = table.u32(table.pointer(at + 8)? + 8)?;
        let actual = new_table.u32(new_table.pointer(at + 8)? + 8)?;
        ensure!(
            expected == actual,
            "crosshair lookup table layout differs at {at:X}"
        );
    }
    unit.add(
        &format!("{symbol}-lookup"),
        native_lookup,
        nl.0.clone(),
        Vec::new(),
    );
    unit.add(
        &format!("{symbol}-bank"),
        bank_tag,
        new_bank.0,
        clip_patches,
    );
    unit.add(
        &format!("{symbol}-table"),
        table_tag,
        new_table.0,
        Vec::new(),
    );
    unit.add(
        &format!("{symbol}-consumer"),
        native_consumer,
        consumer.0.clone(),
        Vec::new(),
    );
    Ok(())
}
