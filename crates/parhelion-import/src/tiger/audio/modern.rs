//! Resolve what a modern Wwise event plays in its default game state.
//!
//! Weapon events route through switch containers (environment, then layer set) before
//! reaching random containers of variations. Every switch follows its default state, and
//! every other container plays all of its children, so the result is the set of layers
//! that sound together, each a list of alternative media.
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

const SOUND: u8 = 2;
const ACTION: u8 = 3;
const EVENT: u8 = 4;
const RANDOM: u8 = 5;
const SWITCH: u8 = 6;
const MIXER: u8 = 7;
const LAYER: u8 = 9;
const SOURCE_MEDIA: usize = 9;

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn half(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

/// Switch assignments: each state with the children it plays.
type States = Vec<(u32, Vec<u32>)>;

struct Container {
    children: Vec<u32>,
    /// Switch containers: default state and the children assigned to it.
    default: Option<Vec<u32>>,
}

fn children_at(payload: &[u8], at: usize) -> Option<(Vec<u32>, usize)> {
    let count = word(payload, at)? as usize;
    if count > 4096 || at + 4 + 4 * count > payload.len() {
        return None;
    }
    let ids = (0..count)
        .map(|i| word(payload, at + 4 + 4 * i))
        .collect::<Option<Vec<_>>>()?;
    ids.windows(2)
        .all(|pair| pair[0] < pair[1])
        .then_some((ids, at + 4 + 4 * count))
}

/// The bytes after a child list, which must consume the payload exactly.
fn tail(kind: u8, payload: &[u8], at: usize, children: &[u32]) -> Option<Option<States>> {
    let end = payload.len();
    match kind {
        MIXER => (at == end).then_some(None),
        RANDOM => {
            let count = half(payload, at)? as usize;
            let items = (0..count)
                .map(|i| word(payload, at + 2 + 8 * i))
                .collect::<Option<Vec<_>>>()?;
            (at + 2 + 8 * count == end && items.iter().all(|item| children.contains(item)))
                .then_some(None)
        }
        SWITCH => {
            let mut cursor = at;
            let groups = word(payload, cursor)? as usize;
            cursor += 4;
            let mut states = Vec::new();
            for _ in 0..groups {
                let state = word(payload, cursor)?;
                let items = word(payload, cursor + 4)? as usize;
                cursor += 8;
                if items > 4096 {
                    return None;
                }
                let nodes = (0..items)
                    .map(|i| word(payload, cursor + 4 * i))
                    .collect::<Option<Vec<_>>>()?;
                if !nodes.iter().all(|node| children.contains(node)) {
                    return None;
                }
                cursor += 4 * items;
                states.push((state, nodes));
            }
            let params = word(payload, cursor)? as usize;
            cursor += 4;
            [14, 13, 12, 10]
                .iter()
                .any(|size| cursor + params * size == end)
                .then_some(Some(states))
        }
        LAYER => (at <= end).then_some(None),
        _ => None,
    }
}

fn container(kind: u8, payload: &[u8], known: &BTreeMap<u32, (u8, Vec<u8>)>) -> Result<Container> {
    let mut best: Option<(usize, Container)> = None;
    for at in (9..payload.len().saturating_sub(3)).rev() {
        let Some((children, after)) = children_at(payload, at) else {
            continue;
        };
        let Some(states) = tail(kind, payload, after, &children) else {
            continue;
        };
        let score = children
            .iter()
            .filter(|child| known.contains_key(child))
            .count();
        let default = match states {
            Some(states) => {
                let state = word(payload, at.checked_sub(5).context("switch header")?)
                    .context("switch default")?;
                Some(
                    states
                        .into_iter()
                        .filter(|(candidate, _)| *candidate == state)
                        .flat_map(|(_, nodes)| nodes)
                        .collect(),
                )
            }
            None => None,
        };
        let complete = score == children.len();
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, Container { children, default }));
        }
        if complete {
            break;
        }
    }
    best.map(|(_, found)| found)
        .context("container has no consistent child list")
}

/// The media each default layer of `event` alternates between.
pub fn default_layers(bank: &[u8], event: u32) -> Result<Vec<Vec<u32>>> {
    let mut objects = BTreeMap::new();
    let mut version = 0;
    let mut at = 0;
    while at + 8 <= bank.len() {
        let size = word(bank, at + 4).context("bank section size")? as usize;
        let body = bank
            .get(at + 8..at + 8 + size)
            .context("bank section exceeds payload")?;
        match &bank[at..at + 4] {
            b"BKHD" => version = word(body, 0).context("bank version")?,
            b"HIRC" => {
                let count = word(body, 0).context("HIRC count")? as usize;
                let mut cursor = 4;
                for _ in 0..count {
                    let kind = *body.get(cursor).context("short HIRC object")?;
                    let length = word(body, cursor + 1).context("HIRC length")? as usize;
                    let payload = body
                        .get(cursor + 5..cursor + 5 + length)
                        .context("HIRC object exceeds section")?
                        .to_vec();
                    let id = word(&payload, 0).context("HIRC object ID")?;
                    objects.insert(id, (kind, payload));
                    cursor += 5 + length;
                }
                ensure!(cursor == body.len(), "HIRC count disagrees with its size");
            }
            _ => {}
        }
        at += 8 + size;
    }
    ensure!(version > 125, "bank version {version} is not a modern bank");
    let (kind, payload) = objects.get(&event).context("event is not in its bank")?;
    ensure!(*kind == EVENT, "event ID names another object");
    let mut cursor = 4;
    let mut count = 0usize;
    loop {
        ensure!(
            cursor < 9 && count <= 65536 >> 7,
            "event count exceeds limit"
        );
        let byte = *payload.get(cursor).context("event count")?;
        cursor += 1;
        count = (count << 7) | usize::from(byte & 127);
        if byte & 128 == 0 {
            break;
        }
    }
    ensure!(
        cursor + 4 * count == payload.len(),
        "event action list length"
    );
    let mut layers = Vec::new();
    for index in 0..count {
        let action = word(payload, cursor + 4 * index).context("event action")?;
        let (kind, action) = objects
            .get(&action)
            .context("event action is not in its bank")?;
        ensure!(*kind == ACTION, "event names a non-action");
        // The high byte is the action kind, the low byte is its scope.
        // Stop (0x0103) and Play (0x0403) have the same scope.
        if half(action, 4).context("action type")? >> 8 != 0x04 {
            continue;
        }
        let target = word(action, 6).context("action target")?;
        walk(&objects, target, 0, &mut layers)?;
    }
    ensure!(!layers.is_empty(), "event plays nothing by default");
    Ok(layers)
}

fn walk(
    objects: &BTreeMap<u32, (u8, Vec<u8>)>,
    id: u32,
    depth: usize,
    layers: &mut Vec<Vec<u32>>,
) -> Result<()> {
    ensure!(depth < 16, "container nesting is too deep");
    let (kind, payload) = objects
        .get(&id)
        .with_context(|| format!("object {id:08X} is outside its bank"))?;
    match *kind {
        SOUND => {
            layers.push(vec![word(payload, SOURCE_MEDIA).context("source media")?]);
            Ok(())
        }
        SWITCH | RANDOM | MIXER | LAYER => {
            let found = container(*kind, payload, objects)?;
            if let Some(nodes) = found.default {
                for node in nodes {
                    walk(objects, node, depth + 1, layers)?;
                }
                return Ok(());
            }
            let sounds = found
                .children
                .iter()
                .all(|child| objects.get(child).is_some_and(|(kind, _)| *kind == SOUND));
            if *kind == RANDOM && sounds {
                let media = found
                    .children
                    .iter()
                    .map(|child| word(&objects[child].1, SOURCE_MEDIA).context("variation media"))
                    .collect::<Result<Vec<_>>>()?;
                layers.push(media);
                return Ok(());
            }
            if *kind == RANDOM {
                bail!("random container {id:08X} alternates between containers");
            }
            for child in found.children {
                walk(objects, child, depth + 1, layers)?;
            }
            Ok(())
        }
        other => bail!("object {id:08X} of kind {other} cannot play"),
    }
}
