//! Source label declarations are checked against their own compiled 256-bit masks.
use super::*;
use anyhow::bail;
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

/// The package build compiles the written lists against Shadowkeep's registry, so with the
/// native reader they first take the [`restrict`] rule.
pub(super) fn translate(
    source: &Source,
    p: &Payload,
    at: usize,
    graph: &mut Graph,
    to: usize,
    native: Option<&mut Reader>,
) -> Result<()> {
    let tag = p.u32(at + 0x48)?;
    reference(p, at + 0x40, tag)?;
    let lists = declared(source, p, tag, &operations(at))?;
    write(&native_lists(source, tag, lists, native)?, graph, to)
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
    native: Option<&mut Reader>,
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
    write(&native_lists(source, tag, lists, native)?, graph, to)
}

/// A perk filter's lists for Shadowkeep's registry. A filter that can never hold there has no
/// effect a translated perk could keep, so it refuses the perk.
fn native_lists(
    source: &Source,
    tag: u32,
    lists: [Vec<u32>; 4],
    native: Option<&mut Reader>,
) -> Result<[Vec<u32>; 4]> {
    let Some(native) = native else {
        return Ok(lists);
    };
    let registry = native.tag(NATIVE, None)?;
    restrict(source, tag, lists, &registry)?
        .context("Source label filter can never hold, since Shadowkeep lacks labels it requires")
}

/// A query filter's labels as Shadowkeep objects can carry them, by the [`restrict`] rule.
pub(in crate::d1_roi) fn destination(
    source: &Source,
    p: &Payload,
    at: usize,
    registry: &Payload,
) -> Result<Option<[Vec<u32>; 4]>> {
    let tag = p.u32(at + 0x48)?;
    reference(p, at + 0x40, tag)?;
    let lists = declared(source, p, tag, &operations(at))?;
    restrict(source, tag, lists, registry)
}

/// Rise of Iron combatant rank atoms Shadowkeep replaced with its own rank vocabulary, source then
/// native. Source atoms 175 to 180 form one rank block, which owners such as `816E6024` and matrix
/// `80AAD664` pair with each combatant, and none of them is in Shadowkeep's registry. Hive
/// Disruptor's "Increase damage to Hive Majors" filter matches `A4FB017C`. Shadowkeep authors
/// majors with `elite` (`574459CE`), as its per-race `base` and `elite` filters and stock kill
/// actions such as `80BC2945` do. The correspondence follows descriptions and rank structure, not a
/// shared asset, so it is a product choice rather than a structural witness.
const RANKS: [(u32, u32); 1] = [(0xA4FB_017C, 0x5744_59CE)];

/// A source atom the destination registry lacks is on no destination object, so match any
/// drops it, require all can never hold, exclude any drops it and exclude all together always
/// holds. `None` means the filter can never hold. A source group the registry lacks has no
/// established expansion and rejects. A [`RANKS`] atom first takes its native rank.
fn restrict(
    source: &Source,
    tag: u32,
    mut lists: [Vec<u32>; 4],
    registry: &Payload,
) -> Result<Option<[Vec<u32>; 4]>> {
    let held = held(registry)?;
    for names in &mut lists {
        let mut renamed = BTreeSet::new();
        for name in names.iter_mut() {
            if !held.contains(name)
                && let Some(&(_, rank)) = RANKS
                    .iter()
                    .find(|&&(from, to)| from == *name && held.contains(&to))
            {
                *name = rank;
                renamed.insert(rank);
            }
        }
        // A list naming a rank twice keeps its first occurrence, in source order.
        let mut seen = BTreeSet::new();
        names.retain(|name| !renamed.contains(name) || seen.insert(*name));
    }
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
pub(in crate::d1_roi) fn predicate(lists: &[Vec<u32>; 4], registry: &Payload) -> Result<Vec<u8>> {
    compiled(lists, registry)?
        .emit()
        .map_err(anyhow::Error::msg)
}

/// The labels and groups `registry` holds.
pub(in crate::d1_roi) fn held(registry: &Payload) -> Result<BTreeSet<u32>> {
    registry
        .array(8, 4, Some(0x80800070))?
        .into_iter()
        .chain(registry.array(24, 44, Some(0x808094BE))?)
        .map(|row| registry.u32(row))
        .collect()
}

/// The 320-bit mask `registry` gives `names`, as the shared native compiler builds a match-any
/// list's mask. Native outgoing profiles store this beside their category rows.
pub(in crate::d1_roi) fn mask(names: &[u32], registry: &Payload) -> Result<[u8; 40]> {
    if names.is_empty() {
        return Ok([0; 40]);
    }
    let graph = compiled(
        &[names.to_vec(), Vec::new(), Vec::new(), Vec::new()],
        registry,
    )?;
    let block = *graph.blocks[0]
        .links
        .get(&0x58)
        .context("Compiled label mask is absent")?;
    graph.blocks[block]
        .bytes
        .get(..40)
        .and_then(|bytes| bytes.try_into().ok())
        .context("Compiled label mask is truncated")
}

/// A source filter at `at` as a Shadowkeep inline filter holds it: the lists by the [`restrict`]
/// rule, the compiled predicate mode, and the `808093F6` or `808093F5` mask record unless the mode
/// is 255. Response filters inside a projectile movement have no package-assembly compile step.
#[allow(clippy::type_complexity)]
pub(in crate::d1_roi) fn inline(
    source: &Source,
    p: &Payload,
    at: usize,
    registry: &Payload,
) -> Result<([Vec<u32>; 4], u8, Option<(u32, Vec<u8>)>)> {
    let lists = destination(source, p, at, registry)?
        .context("Source label filter can never hold, since Shadowkeep lacks labels it requires")?;
    let graph = compiled(&lists, registry)?;
    let root = &graph.blocks[0];
    let record = root
        .links
        .get(&0x58)
        .map(|&block| (graph.blocks[block].class, graph.blocks[block].bytes.clone()));
    Ok((lists, root.bytes[0x50], record))
}

/// A source filter stored only as compiled 256-bit include-any and exclude-any masks over
/// dictionary `tag`, bit `i` being atom `i`, as Shadowkeep's inline filter holds it. The lists
/// follow the [`restrict`] rule and the shared compiler, as in [`inline`]. Masks carry no group
/// names, so a group's atoms translate one by one.
#[allow(clippy::type_complexity)]
pub(in crate::d1_roi) fn masks(
    source: &Source,
    tag: u32,
    include: [u8; 32],
    exclude: [u8; 32],
    registry: &Payload,
) -> Result<([Vec<u32>; 4], u8, Option<(u32, Vec<u8>)>)> {
    let dictionary = source.read(tag)?;
    let atoms = dictionary
        .array(8, 4, Some(0x80800070))?
        .into_iter()
        .map(|row| dictionary.u32(row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !atoms.is_empty() && atoms.len() <= 256,
        "Source label dictionary exceeds its mask width"
    );
    let names = |mask: [u8; 32]| -> Result<Vec<u32>> {
        (0..256)
            .filter(|bit| mask[bit / 8] & (1 << (bit % 8)) != 0)
            .map(|bit| {
                atoms
                    .get(bit)
                    .copied()
                    .context("Source label mask names an undeclared atom")
            })
            .collect()
    };
    let lists = [names(include)?, Vec::new(), names(exclude)?, Vec::new()];
    let lists = restrict(source, tag, lists, registry)?
        .context("Source label filter can never hold, since Shadowkeep lacks labels it requires")?;
    let graph = compiled(&lists, registry)?;
    let root = &graph.blocks[0];
    let record = root
        .links
        .get(&0x58)
        .map(|&block| (graph.blocks[block].class, graph.blocks[block].bytes.clone()));
    Ok((lists, root.bytes[0x50], record))
}

/// An `80804D73` predicate object over `lists`, compiled by the shared native compiler.
fn compiled(lists: &[Vec<u32>; 4], registry: &Payload) -> Result<Graph> {
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
    Ok(graph)
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
            let bits = match registry.get(&name) {
                Some(bits) => bits,
                None if name == 0x811C9DC5 => &[0; 32],
                None => bail!("Source condition label is not registered"),
            };
            for (out, bits) in mask.iter_mut().zip(bits) {
                *out |= bits;
            }
            if bits.iter().any(|&bit| bit != 0) {
                lists[operation].push(name);
            }
        }
        // Source 435C00 derives each bypass flag from its compiled mask through
        // 2760D0. An empty-name declaration can therefore have a row but no bits.
        // Remove only known zero contributions and require the stored mask and flag
        // to agree. Dropping an unchecked zero-mask match-any/exclude-all changes
        // admission when its bypass flag is clear.
        ensure!(
            p.bytes::<32>(compiled)? == mask && p.u8(empty)? == u8::from(mask == [0; 32]),
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
