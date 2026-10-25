//! Resolve shared clip resources through their explicit provider-to-local node maps.
use super::{
    codec::{Motion, Track},
    *,
};
use crate::halo_reach::{behavior, cache::Tag};
use std::collections::BTreeSet;

struct Resolved {
    tag: Tag,
    clip: Value,
    sounds: Vec<Value>,
    nodes: Vec<Option<usize>>,
}

fn skeleton(cache: &Cache, tag: &Tag) -> Result<Vec<(String, i16)>> {
    cache
        .block(tag.address()? + 0x48, 0x2c)?
        .into_iter()
        .map(|r| Ok((cache.string_id(cache.u32(r)?)?, cache.i16(r + 8)?)))
        .collect()
}

fn resolve(
    cache: &Cache,
    tag: &Tag,
    index: usize,
    chain: &mut BTreeSet<(u32, usize)>,
    data_cache: &mut BTreeMap<u32, Value>,
) -> Result<Resolved> {
    ensure!(
        chain.len() < 64 && chain.insert((tag.datum, index)),
        "Cyclic or excessive shared animation inheritance"
    );
    if let std::collections::btree_map::Entry::Vacant(entry) = data_cache.entry(tag.datum) {
        entry.insert(behavior::animation_data(cache, tag)?);
    }
    let data = &data_cache[&tag.datum];
    let clip = data["clips"]
        .as_array()
        .context("Shared animation clips")?
        .get(index)
        .context("Shared animation index outside provider")?
        .clone();
    let local = skeleton(cache, tag)?;
    let result = if clip["shared_graph"].is_null() {
        Resolved {
            tag: tag.clone(),
            clip,
            sounds: data["sounds"]
                .as_array()
                .context("Animation sounds")?
                .clone(),
            nodes: (0..local.len()).map(Some).collect(),
        }
    } else {
        ensure!(
            clip["shared"]
                .as_array()
                .context("Local shared variants")?
                .is_empty(),
            "Animation has both local and inherited motion"
        );
        let provider: Tag = serde_json::from_value(clip["shared_graph"].clone())?;
        let inherited = cache
            .block(tag.address()? + 0x134, 0x2c)?
            .into_iter()
            .map(|r| Ok((r, cache.reference(r, Some("jmad"))?)))
            .collect::<Result<Vec<_>>>()?;
        let matching = inherited
            .iter()
            .filter(|(_, t)| t.as_ref().is_some_and(|t| t.datum == provider.datum))
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1,
            "Shared graph has no unique inheritance map"
        );
        let row = matching[0].0;
        ensure!(
            cache.u32(row + 0x28)? == 0,
            "Shared graph requires unsupported inheritance flags"
        );
        let source = skeleton(cache, &provider)?;
        let entries = cache.block(row + 0x10, 2)?;
        let nodes = if entries.is_empty() {
            ensure!(
                source == local,
                "Shared graph without a node map has a different skeleton"
            );
            (0..source.len()).map(Some).collect::<Vec<_>>()
        } else {
            ensure!(
                entries.len() == source.len(),
                "Shared node map has the wrong provider size"
            );
            let mut used = BTreeSet::new();
            entries
                .into_iter()
                .map(|r| {
                    let index = cache.i16(r)?;
                    ensure!(
                        index >= -1 && index < local.len() as i16,
                        "Shared node map escapes the local skeleton"
                    );
                    if index == -1 {
                        return Ok(None);
                    }
                    let index = index as usize;
                    ensure!(
                        used.insert(index),
                        "Shared node map aliases two provider nodes"
                    );
                    Ok(Some(index))
                })
                .collect::<Result<Vec<_>>>()?
        };
        let flags = cache
            .block(row + 0x1c, 4)?
            .into_iter()
            .map(|r| cache.u32(r))
            .collect::<Result<Vec<_>>>()?;
        if !flags.is_empty() {
            ensure!(
                flags.len() * 32 >= nodes.len(),
                "Shared node flags are truncated"
            );
            for (index, node) in nodes.iter().enumerate() {
                ensure!(
                    (flags[index / 32] & (1 << (index % 32)) != 0) == node.is_some(),
                    "Shared node flags disagree with the node map"
                );
            }
        }
        let index = usize::try_from(
            clip["shared_index"]
                .as_i64()
                .context("Shared animation index")?,
        )?;
        let mut resolved = resolve(cache, &provider, index, chain, data_cache)?;
        resolved.nodes = resolved
            .nodes
            .iter()
            .map(|index| index.and_then(|i| nodes[i]))
            .collect();
        resolved
    };
    chain.remove(&(tag.datum, index));
    Ok(result)
}

pub(super) fn complete(
    cache: &Cache,
    pages: &mut Pages,
    graph: &mut Value,
    root: &Path,
) -> Result<()> {
    let tag: Tag = serde_json::from_value(graph["tag"].clone())?;
    let local_nodes = graph["skeleton"]
        .as_array()
        .context("Local animation skeleton")?
        .len();
    let mut providers = BTreeMap::new();
    let mut data_cache = BTreeMap::new();
    let mut clips = graph["clips"]
        .as_array()
        .context("Animation clips")?
        .clone();
    let mut sounds = graph["sounds"].as_array().cloned().unwrap_or_default();
    let mut groups = graph["groups"]
        .as_array()
        .context("Animation groups")?
        .clone();
    let mut next_group = groups
        .iter()
        .filter_map(|g| g["index"].as_u64())
        .max()
        .map_or(0, |n| n + 1);
    for clip in &mut clips {
        if clip["shared_graph"].is_null() {
            continue;
        }
        let index = usize::try_from(clip["index"].as_u64().context("Local clip index")?)?;
        let resolved = resolve(cache, &tag, index, &mut BTreeSet::new(), &mut data_cache)?;
        let mut variants = resolved.clip["shared"]
            .as_array()
            .context("Resolved clip variants")?
            .clone();
        ensure!(!variants.is_empty(), "Shared clip has no resource variants");
        for (variant, shared) in variants.iter_mut().enumerate() {
            let group = usize::try_from(
                shared["resource_group"]
                    .as_u64()
                    .context("Shared resource group")?,
            )?;
            let key = (resolved.tag.datum, group);
            if let std::collections::btree_map::Entry::Vacant(entry) = providers.entry(key) {
                let data = &data_cache[&resolved.tag.datum];
                entry.insert(super::export_local(
                    cache,
                    pages,
                    &json!({"tag":resolved.tag,"data":data}),
                    Some(&BTreeSet::from([group])),
                    root,
                )?);
            }
            let provider = &providers[&key];
            let mut member = super::playback::member(provider, shared)?.clone();
            let file = member["motion"]["file"]
                .as_str()
                .context("Shared clip has no decoded motion")?;
            let mut motion: Motion = serde_json::from_slice(&fs::read(root.join(file))?)?;
            ensure!(
                motion.tracks.len() == resolved.nodes.len(),
                "Shared motion and provider skeleton disagree"
            );
            let mut tracks = vec![Track::default(); local_nodes];
            for (track, target) in motion.tracks.into_iter().zip(&resolved.nodes) {
                if let Some(target) = target {
                    tracks[*target] = track;
                }
            }
            motion.tracks = tracks;
            let file = format!(
                "animations/{:08X}-shared-{index:04}-{variant:02}.json",
                tag.datum
            );
            fs::write(root.join(&file), serde_json::to_vec(&motion)?)?;
            member["motion"]["file"] = json!(file);
            member["motion"]["output"] = json!(fingerprint(&root.join(&file))?);
            member["source_nodes"] = member["nodes"].clone();
            member["nodes"] = json!(local_nodes);
            member["index"] = json!(0);
            groups.push(json!({"index":next_group,"provider":resolved.tag,"provider_group":group,"members":[member]}));
            shared["resource_group"] = json!(next_group);
            shared["resource_member"] = json!(0);
            next_group += 1;
            for event in shared["sound_events"]
                .as_array_mut()
                .context("Shared sound events")?
            {
                let old = event["sound_index"]
                    .as_i64()
                    .context("Shared sound index")?;
                if old == -1 {
                    continue;
                }
                let sound = resolved
                    .sounds
                    .get(usize::try_from(old)?)
                    .context("Shared sound index outside provider")?;
                let new = sounds.iter().position(|s| s == sound).unwrap_or_else(|| {
                    sounds.push(sound.clone());
                    sounds.len() - 1
                });
                event["sound_index"] = json!(new);
            }
        }
        clip["shared"] = json!(variants);
        clip["resolved_provider"] =
            json!({"tag":resolved.tag,"clip":resolved.clip["name"],"node_map":resolved.nodes});
    }
    graph["clips"] = json!(clips);
    graph["sounds"] = json!(sounds);
    graph["groups"] = json!(groups);
    Ok(())
}
