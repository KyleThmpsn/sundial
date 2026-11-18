//! The output bus a native version 113 bank plays a cue's media through.
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Objects<'a> = BTreeMap<u32, (u8, &'a [u8])>;

pub(super) fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("Native bank is truncated")?
            .try_into()?,
    ))
}

/// A version 113 bank's hierarchy objects by identity, each with its type and body.
pub(super) fn objects(bank: &[u8]) -> Result<Objects<'_>> {
    let mut at = 0;
    let mut version = None;
    let mut found = BTreeMap::new();
    while at < bank.len() {
        let size = word(bank, at + 4)? as usize;
        let body = bank
            .get(at + 8..at + 8 + size)
            .context("Native bank chunk exceeds payload")?;
        match &bank[at..at + 4] {
            b"BKHD" => version = Some(word(body, 0)?),
            b"HIRC" => {
                let mut cursor = 4;
                for _ in 0..word(body, 0)? {
                    let kind = *body.get(cursor).context("Native hierarchy type")?;
                    let size = word(body, cursor + 1)? as usize;
                    let object = body
                        .get(cursor + 5..cursor + 5 + size)
                        .context("Native hierarchy object exceeds chunk")?;
                    ensure!(
                        found.insert(word(object, 0)?, (kind, object)).is_none(),
                        "Native audio object is repeated"
                    );
                    cursor += 5 + size;
                }
                ensure!(cursor == body.len(), "Native hierarchy extent differs");
            }
            _ => {}
        }
        at += 8 + size;
    }
    ensure!(version == Some(113), "Native bank is not version 113");
    Ok(found)
}

/// A sound node's own output bus and its parent. A sound's node fields follow its eighteen
/// source bytes, or the inline parameters of a generator plugin, and another node's follow its
/// identity: the effect override, the effect count with a bypass byte and seven bytes per
/// effect when there are any, the attachment byte, then the bus and the parent.
pub(super) fn links(kind: u8, node: &[u8]) -> Result<(u32, u32)> {
    ensure!(
        matches!(kind, 2 | 5 | 6 | 7 | 9),
        "Native audio parent chain holds an object that is not a sound node"
    );
    let base = match kind {
        2 if (word(node, 4)? & 0xF) == 2 => 22 + word(node, 18)? as usize,
        2 => 18,
        _ => 4,
    };
    let effects = usize::from(*node.get(base + 1).context("Native sound node effects")?);
    let at = base + 2 + if effects > 0 { 1 + 7 * effects } else { 0 } + 1;
    Ok((word(node, at)?, word(node, at + 4)?))
}

/// The topmost ancestor of `target` this bank defines, and that ancestor's own bus.
fn top(objects: &Objects<'_>, mut target: u32) -> Result<u32> {
    let mut seen = BTreeSet::new();
    loop {
        ensure!(seen.insert(target), "Native audio parent chain cycles");
        let (kind, node) = objects
            .get(&target)
            .context("Native sound node is outside its bank")?;
        let (bus, parent) = links(*kind, node)?;
        if matches!(parent, 0 | u32::MAX) || !objects.contains_key(&parent) {
            return Ok(bus);
        }
        target = parent;
    }
}

/// The bus of the top mixer most of the cue's sounds play under. A stock shot keeps its layers
/// under top mixers that share the weapon fire bus, with a lone sound of its event under another
/// mixer. `media` holds the cue's media identities. A tie between buses is refused.
pub fn media_bus(bank: &[u8], media: &BTreeSet<u32>) -> Result<u32> {
    let objects = objects(bank)?;
    let mut buses = BTreeMap::<u32, usize>::new();
    for (identity, (kind, node)) in &objects {
        if *kind == 2 && media.contains(&word(node, 9)?) {
            *buses.entry(top(&objects, *identity)?).or_default() += 1;
        }
    }
    let most = buses.values().copied().max();
    let mut leading = buses.iter().filter(|(_, count)| Some(**count) == most);
    let (Some((bus, _)), None) = (leading.next(), leading.next()) else {
        anyhow::bail!(
            "Native cue's sounds play under {} top buses with no single leader",
            buses.len()
        );
    };
    ensure!(
        !matches!(*bus, 0 | u32::MAX),
        "Native cue's top mixer names no output bus"
    );
    Ok(*bus)
}
