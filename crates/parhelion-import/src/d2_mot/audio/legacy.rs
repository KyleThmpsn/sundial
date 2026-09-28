//! Resize the variation containers of a Dawn-version (v113) event bank.
//!
//! Native weapon foley banks play one event through random containers whose children are
//! all sound sources, one source per media variation. A modern event can carry a
//! different number of variations. Each container keeps its first source as the prototype
//! for every new variation, so bus routing, attenuation, positioning and the container's
//! own randomization stay native. Any other bank shape is refused.
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};

const SOUND: u8 = 2;
const RANDOM: u8 = 5;
/// Sound source payload: media ID after the ID, plugin and stream type.
const SOURCE_MEDIA: usize = 9;
/// Sound source payload: the direct parent after the source, effect, attachment and bus fields.
const SOURCE_PARENT: usize = 25;

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("bank read outside payload")?
            .try_into()?,
    ))
}

struct Object {
    kind: u8,
    payload: Vec<u8>,
}

impl Object {
    fn id(&self) -> Result<u32> {
        word(&self.payload, 0)
    }
}

/// A random container's child list and playlist, which end its payload.
struct Variations {
    at: usize,
    children: Vec<u32>,
    weight: i32,
}

fn variations(payload: &[u8], sounds: &BTreeSet<u32>) -> Option<Variations> {
    for at in (8..payload.len().checked_sub(4)?).rev() {
        let count = word(payload, at).ok()? as usize;
        if count == 0 || count > 4096 {
            continue;
        }
        let list = at + 4 + 4 * count;
        if list + 2 > payload.len() {
            continue;
        }
        let children = (0..count)
            .map(|i| word(payload, at + 4 + 4 * i).ok())
            .collect::<Option<Vec<_>>>()?;
        if children.windows(2).any(|pair| pair[0] >= pair[1])
            || !children.iter().all(|child| sounds.contains(child))
        {
            continue;
        }
        let entries = u16::from_le_bytes(payload[list..list + 2].try_into().ok()?) as usize;
        if list + 2 + 8 * entries != payload.len() {
            continue;
        }
        let mut weight = None;
        let complete = (0..entries).all(|i| {
            let item = word(payload, list + 2 + 8 * i).ok();
            weight.get_or_insert(word(payload, list + 6 + 8 * i).unwrap_or(0) as i32);
            item.is_some_and(|item| children.contains(&item))
        });
        if complete {
            return Some(Variations {
                at,
                children,
                weight: weight.unwrap_or(50_000),
            });
        }
    }
    None
}

/// Rebuild `bank` so every variation container plays `media`, in order.
///
/// `source_id(container, index)` names each new sound source. The caller keeps bank,
/// event and other object IDs, and assigns private IDs afterwards.
pub fn fit_variations(
    bank: &[u8],
    media: &[u32],
    source_id: impl Fn(u32, usize) -> u32,
) -> Result<Vec<u8>> {
    ensure!(!media.is_empty(), "an event needs at least one variation");
    let mut sections = Vec::new();
    let mut at = 0;
    while at < bank.len() {
        let name: [u8; 4] = bank
            .get(at..at + 4)
            .context("short bank section")?
            .try_into()?;
        let size = word(bank, at + 4)? as usize;
        let body = bank
            .get(at + 8..at + 8 + size)
            .context("bank section exceeds payload")?;
        sections.push((name, body.to_vec()));
        at += 8 + size;
    }
    let hirc = sections
        .iter_mut()
        .find(|(name, _)| name == b"HIRC")
        .context("bank has no HIRC section")?;
    let body = &hirc.1;
    let count = word(body, 0)? as usize;
    let mut objects = Vec::with_capacity(count);
    let mut cursor = 4;
    for _ in 0..count {
        let kind = *body.get(cursor).context("short HIRC object")?;
        let length = word(body, cursor + 1)? as usize;
        let payload = body
            .get(cursor + 5..cursor + 5 + length)
            .context("HIRC object exceeds section")?
            .to_vec();
        objects.push(Object { kind, payload });
        cursor += 5 + length;
    }
    ensure!(cursor == body.len(), "HIRC count disagrees with its size");

    let sounds = objects
        .iter()
        .filter(|o| o.kind == SOUND)
        .map(Object::id)
        .collect::<Result<BTreeSet<_>>>()?;
    let mut containers = BTreeMap::new();
    let mut owner = BTreeMap::new();
    for object in objects.iter().filter(|o| o.kind == RANDOM) {
        let Some(found) = variations(&object.payload, &sounds) else {
            continue;
        };
        let id = object.id()?;
        for child in &found.children {
            ensure!(
                owner.insert(*child, id).is_none(),
                "a sound source belongs to two containers"
            );
        }
        containers.insert(id, found);
    }
    ensure!(!containers.is_empty(), "bank has no variation container");
    for sound in &sounds {
        let parent = owner.get(sound).with_context(|| {
            format!("sound source {sound:08X} is outside a variation container")
        })?;
        let object = objects
            .iter()
            .find(|o| o.kind == SOUND && o.id().ok() == Some(*sound))
            .context("sound source vanished")?;
        ensure!(
            word(&object.payload, SOURCE_PARENT)? == *parent,
            "sound source {sound:08X} names another parent"
        );
    }

    let mut rebuilt = Vec::with_capacity(objects.len());
    let mut emitted = BTreeSet::new();
    for object in &objects {
        if object.kind == SOUND {
            let container = owner[&object.id()?];
            if !emitted.insert(container) {
                continue;
            }
            let prototype = &objects
                .iter()
                .find(|o| o.id().ok() == Some(containers[&container].children[0]))
                .context("prototype source vanished")?
                .payload;
            for (index, medium) in media.iter().enumerate() {
                let mut payload = prototype.clone();
                payload[0..4].copy_from_slice(&source_id(container, index).to_le_bytes());
                payload[SOURCE_MEDIA..SOURCE_MEDIA + 4].copy_from_slice(&medium.to_le_bytes());
                rebuilt.push(Object {
                    kind: SOUND,
                    payload,
                });
            }
        } else if let Some(found) = containers
            .get(&object.id()?)
            .filter(|_| object.kind == RANDOM)
        {
            let container = object.id()?;
            let mut ids = (0..media.len())
                .map(|index| source_id(container, index))
                .collect::<Vec<_>>();
            let playlist = ids.clone();
            ids.sort_unstable();
            ensure!(
                ids.windows(2).all(|pair| pair[0] < pair[1]),
                "new sound source IDs repeat"
            );
            let mut payload = object.payload[..found.at].to_vec();
            // A container that avoids repeating more variations than it holds cannot pick.
            let avoid = found
                .at
                .checked_sub(6)
                .context("container lacks its avoid-repeat count")?;
            let limit = u16::try_from(media.len() - 1).unwrap_or(u16::MAX);
            let current = u16::from_le_bytes(payload[avoid..avoid + 2].try_into()?);
            payload[avoid..avoid + 2].copy_from_slice(&current.min(limit).to_le_bytes());
            payload.extend(u32::try_from(ids.len())?.to_le_bytes());
            for id in &ids {
                payload.extend(id.to_le_bytes());
            }
            payload.extend(u16::try_from(playlist.len())?.to_le_bytes());
            for id in &playlist {
                payload.extend(id.to_le_bytes());
                payload.extend(found.weight.to_le_bytes());
            }
            rebuilt.push(Object {
                kind: RANDOM,
                payload,
            });
        } else {
            rebuilt.push(Object {
                kind: object.kind,
                payload: object.payload.clone(),
            });
        }
    }

    let mut body = u32::try_from(rebuilt.len())?.to_le_bytes().to_vec();
    for object in &rebuilt {
        body.push(object.kind);
        body.extend(u32::try_from(object.payload.len())?.to_le_bytes());
        body.extend(&object.payload);
    }
    hirc.1 = body;
    let mut output = Vec::new();
    for (name, body) in &sections {
        output.extend(name);
        output.extend(u32::try_from(body.len())?.to_le_bytes());
        output.extend(body);
    }
    if output.is_empty() {
        bail!("bank rebuilt empty");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: u8, payload: Vec<u8>) -> Vec<u8> {
        let mut bytes = vec![kind];
        bytes.extend(u32::try_from(payload.len()).unwrap().to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn source(id: u32, media: u32, parent: u32) -> Vec<u8> {
        let mut payload = vec![0; 32];
        payload[0..4].copy_from_slice(&id.to_le_bytes());
        payload[4..8].copy_from_slice(&0x0001_0001u32.to_le_bytes());
        payload[8] = 2;
        payload[SOURCE_MEDIA..SOURCE_MEDIA + 4].copy_from_slice(&media.to_le_bytes());
        payload[SOURCE_PARENT..SOURCE_PARENT + 4].copy_from_slice(&parent.to_le_bytes());
        object(SOUND, payload)
    }

    fn container(id: u32, children: &[u32]) -> Vec<u8> {
        let mut payload = id.to_le_bytes().to_vec();
        payload.extend([0xAA; 20]);
        payload.extend(1u16.to_le_bytes());
        payload.extend([0, 0, 0, 0x12]);
        payload.extend(u32::try_from(children.len()).unwrap().to_le_bytes());
        for child in children {
            payload.extend(child.to_le_bytes());
        }
        payload.extend(u16::try_from(children.len()).unwrap().to_le_bytes());
        for child in children.iter().rev() {
            payload.extend(child.to_le_bytes());
            payload.extend(50_000i32.to_le_bytes());
        }
        object(RANDOM, payload)
    }

    fn bank(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut hirc = u32::try_from(objects.len()).unwrap().to_le_bytes().to_vec();
        for object in objects {
            hirc.extend(object);
        }
        let mut bytes = b"BKHD".to_vec();
        bytes.extend(8u32.to_le_bytes());
        bytes.extend(113u32.to_le_bytes());
        bytes.extend(0xB00Du32.to_le_bytes());
        bytes.extend(b"HIRC");
        bytes.extend(u32::try_from(hirc.len()).unwrap().to_le_bytes());
        bytes.extend(hirc);
        bytes
    }

    fn native() -> Vec<u8> {
        bank(&[
            source(0x10, 0xA1, 0x100),
            source(0x11, 0xA2, 0x100),
            container(0x100, &[0x10, 0x11]),
            source(0x20, 0xA2, 0x200),
            source(0x21, 0xA1, 0x200),
            container(0x200, &[0x20, 0x21]),
            object(4, vec![0xEE; 12]),
        ])
    }

    fn objects_of(bank: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let body = &bank[bank.windows(4).position(|w| w == b"HIRC").unwrap() + 8..];
        let mut at = 4;
        let mut found = Vec::new();
        for _ in 0..word(body, 0).unwrap() {
            let length = word(body, at + 1).unwrap() as usize;
            found.push((body[at], body[at + 5..at + 5 + length].to_vec()));
            at += 5 + length;
        }
        found
    }

    fn media_of(bank: &[u8]) -> Vec<(u32, u32, u32)> {
        objects_of(bank)
            .into_iter()
            .filter(|(kind, _)| *kind == SOUND)
            .map(|(_, payload)| {
                (
                    word(&payload, 0).unwrap(),
                    word(&payload, SOURCE_MEDIA).unwrap(),
                    word(&payload, SOURCE_PARENT).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn every_container_plays_every_new_variation() {
        let id = |container: u32, index: usize| container * 16 + index as u32;
        let fitted = fit_variations(&native(), &[0xB1, 0xB2, 0xB3], id).unwrap();
        let sources = media_of(&fitted);
        assert_eq!(sources.len(), 6);
        for container in [0x100, 0x200] {
            let media: Vec<_> = sources
                .iter()
                .filter(|(_, _, parent)| *parent == container)
                .map(|(source, media, _)| (*source, *media))
                .collect();
            assert_eq!(
                media,
                [
                    (id(container, 0), 0xB1),
                    (id(container, 1), 0xB2),
                    (id(container, 2), 0xB3)
                ]
            );
        }
        let refit = fit_variations(&fitted, &[0xC1], id).unwrap();
        assert_eq!(media_of(&refit).len(), 2);
        assert!(
            refit.windows(12).any(|w| w == [0xEE; 12]),
            "other objects survive"
        );
    }

    #[test]
    fn a_single_variation_stops_avoiding_repeats() {
        let fitted = fit_variations(&native(), &[0xB1], |c, i| c * 16 + i as u32).unwrap();
        let containers: Vec<_> = objects_of(&fitted)
            .into_iter()
            .filter(|(kind, _)| *kind == RANDOM)
            .collect();
        assert_eq!(containers.len(), 2);
        for (_, payload) in containers {
            // ID and 20 property bytes, then the avoid-repeat count.
            assert_eq!(&payload[24..26], &[0, 0]);
        }
    }

    #[test]
    fn sources_outside_variation_containers_are_refused() {
        let stray = bank(&[
            source(0x10, 0xA1, 0x100),
            container(0x100, &[0x10]),
            source(0x30, 0xA3, 0x999),
        ]);
        assert!(fit_variations(&stray, &[0xB1], |c, i| c + i as u32).is_err());
    }
}
