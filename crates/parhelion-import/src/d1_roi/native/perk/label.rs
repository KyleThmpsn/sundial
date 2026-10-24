//! Source label declarations are checked against their own compiled 256-bit masks.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{Block, Graph, labels};

const PATH: &[u8] = b"content/common/native/sandbox/label_globals.label_globals.tft\0";
const NATIVE: u32 = 0x80C70CA1;

/// A dictionary reference is a relative path and a tag. Query components leave the path empty
/// and name the dictionary by tag alone. Its masks are then checked against that dictionary.
fn reference(p: &Payload, at: usize, tag: u32) -> Result<()> {
    let named = p.u64(at)? == 0 || {
        let path = p.pointer(at)?;
        p.0.get(path..path + PATH.len()) == Some(PATH)
    };
    ensure!(
        named && p.u32(at + 8)? == tag && p.u32(at + 12)? == 0,
        "Source condition names another label dictionary"
    );
    Ok(())
}

/// The four operations of a named label filter: declaration, compiled mask, empty flag.
fn operations(at: usize) -> Vec<(usize, usize, usize, usize)> {
    (0..4)
        .map(|op| (at + op * 16, at + 0x50 + op * 32, at + 0xD0 + op, op))
        .collect()
}

pub(super) fn translate(
    source: &Source,
    p: &Payload,
    at: usize,
    graph: &mut Graph,
    to: usize,
) -> Result<()> {
    let tag = p.u32(at + 0x48)?;
    reference(p, at + 0x40, tag)?;
    let lists = declared(source, p, tag, &operations(at))?;
    write(&lists, graph, to)
}

/// D1 ability labels have only include-any and exclude-any lists. Their named
/// entries still identify the dictionary used by the adjacent object filter.
pub(super) fn ability(
    source: &Source,
    p: &Payload,
    at: usize,
    dictionary: usize,
    graph: &mut Graph,
    to: usize,
) -> Result<()> {
    let tag = p.u32(dictionary + 8)?;
    reference(p, dictionary, tag)?;
    let lists = declared(
        source,
        p,
        tag,
        &[
            (at + 0x20, at + 0x48, at + 0x8A, 0),
            (at + 0x30, at + 0x68, at + 0x8B, 2),
        ],
    )?;
    write(&lists, graph, to)
}

/// A query filter's labels as Shadowkeep objects can carry them. A source atom the destination
/// registry lacks is on no destination object, so match any drops it, require all can never
/// hold, exclude any drops it and exclude all together always holds. `None` means the filter
/// can never hold. A source group the registry lacks has no established expansion and rejects.
pub(in crate::d1_roi::native) fn destination(
    source: &Source,
    p: &Payload,
    at: usize,
    registry: &Payload,
) -> Result<Option<[Vec<u32>; 4]>> {
    let tag = p.u32(at + 0x48)?;
    reference(p, at + 0x40, tag)?;
    let mut lists = declared(source, p, tag, &operations(at))?;
    let held = registry
        .array(8, 4, Some(0x80800070))?
        .into_iter()
        .chain(registry.array(24, 44, Some(0x808094BE))?)
        .map(|row| registry.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    let dictionary = source.read(tag)?;
    for row in dictionary.array(24, 36, Some(0x80800513))? {
        let group = dictionary.u32(row)?;
        ensure!(
            held.contains(&group) || lists.iter().flatten().all(|&name| name != group),
            "Source label group {group:08X} has no Shadowkeep counterpart"
        );
    }
    for (operation, names) in lists.iter_mut().enumerate() {
        if names.iter().all(|name| held.contains(name)) {
            continue;
        }
        match operation {
            1 => return Ok(None),
            3 => names.clear(),
            _ => {
                names.retain(|name| held.contains(name));
                if operation == 0 && names.is_empty() {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(lists))
}

/// A standalone native object-label predicate with masks compiled against `registry`. Query
/// payloads have no package-assembly compile step, unlike perk conditions.
pub(in crate::d1_roi::native) fn predicate(
    lists: &[Vec<u32>; 4],
    registry: &Payload,
) -> Result<Vec<u8>> {
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80804D73,
            count: None,
            bytes: vec![0; 0x60],
            links: BTreeMap::new(),
        }],
    };
    graph.blocks[0].bytes[0x50] = 0xFF;
    write(lists, &mut graph, 0)?;
    labels::compile(&mut graph, &registry.0).map_err(anyhow::Error::msg)?;
    graph.emit().map_err(anyhow::Error::msg)
}

/// Declared names for each operation, after checking them against the source's compiled masks.
fn declared(
    source: &Source,
    p: &Payload,
    tag: u32,
    operations: &[(usize, usize, usize, usize)],
) -> Result<[Vec<u32>; 4]> {
    let dictionary = source.read(tag)?;
    let atoms = dictionary.array(8, 4, Some(0x80800070))?;
    ensure!(
        !atoms.is_empty() && atoms.len() <= 256,
        "Source label dictionary exceeds its mask width"
    );
    let mut registry = BTreeMap::new();
    for (index, row) in atoms.into_iter().enumerate() {
        let mut bits = [0; 32];
        bits[index / 8] = 1 << (index % 8);
        ensure!(
            registry.insert(dictionary.u32(row)?, bits).is_none(),
            "Duplicate source label"
        );
    }
    for row in dictionary.array(24, 36, Some(0x80800513))? {
        ensure!(
            registry
                .insert(dictionary.u32(row)?, dictionary.bytes::<32>(row + 4)?)
                .is_none(),
            "Duplicate source label group"
        );
    }
    let mut lists: [Vec<u32>; 4] = Default::default();
    for &(declaration, compiled, empty, operation) in operations {
        let rows = p.array(declaration, 24, Some(0x80800365))?;
        let mut mask = [0; 32];
        for row in rows.iter().copied() {
            let name = p.u32(row)?;
            ensure!(p.u32(row + 4)? == 0, "Unknown source label record flags");
            reference(p, row + 8, tag)?;
            let bits = registry
                .get(&name)
                .context("Source condition label is not registered")?;
            for (out, bits) in mask.iter_mut().zip(bits) {
                *out |= bits;
            }
            lists[operation].push(name);
        }
        ensure!(
            p.bytes::<32>(compiled)? == mask && p.u8(empty)? == u8::from(rows.is_empty()),
            "Source condition declarations and compiled label mask disagree"
        );
    }
    Ok(lists)
}

fn write(lists: &[Vec<u32>; 4], graph: &mut Graph, to: usize) -> Result<()> {
    let path = graph.blocks.len();
    graph.blocks.push(Block {
        class: 0,
        count: None,
        bytes: PATH.to_vec(),
        links: BTreeMap::new(),
    });
    graph.blocks[0].links.insert(to + 0x40, path);
    put(&mut graph.blocks[0].bytes, to + 0x48, &NATIVE.to_le_bytes())?;
    for (operation, names) in lists.iter().enumerate() {
        if names.is_empty() {
            continue;
        }
        let mut bytes = vec![0; names.len() * 24];
        let mut links = BTreeMap::new();
        for (index, name) in names.iter().enumerate() {
            put(&mut bytes, index * 24, &name.to_le_bytes())?;
            put(&mut bytes, index * 24 + 16, &NATIVE.to_le_bytes())?;
            links.insert(index * 24 + 8, path);
        }
        let index = graph.blocks.len();
        graph.blocks.push(Block {
            class: 0x808094B3,
            count: Some(names.len()),
            bytes,
            links,
        });
        let root = &mut graph.blocks[0];
        put(
            &mut root.bytes,
            to + operation * 16,
            &(names.len() as u64).to_le_bytes(),
        )?;
        root.links.insert(to + operation * 16 + 8, index);
    }
    // Package assembly's shared native compiler rebuilds masks against the destination
    // registry. D1's label bit ordinals and group expansions never enter native storage.
    Ok(())
}
