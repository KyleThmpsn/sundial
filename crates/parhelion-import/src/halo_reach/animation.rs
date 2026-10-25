//! Preserve resource members and decode source motion without inventing controller behavior.
pub(super) mod character;
mod codec;
pub(super) mod first_person;
pub(super) mod native;
mod playback;
mod route;
mod shared;
use super::{
    cache::Cache,
    resource::{Pages, fingerprint},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn export(
    cache: &Cache,
    pages: &mut Pages,
    behavior: &Value,
    scene: &super::Scene,
    root: &Path,
) -> Result<Value> {
    let mut graphs = Vec::new();
    for node in behavior["nodes"]
        .as_object()
        .context("Behavior nodes")?
        .values()
    {
        if node["tag"]["group"] != "jmad" {
            continue;
        }
        graphs.push(export_graph(cache, pages, node, None, root)?);
    }
    playback::attach(scene, &mut graphs, root)?;
    let report = json!(graphs);
    crate::io::write_json(&root.join("animations.json"), &report)?;
    Ok(report)
}

fn export_graph(
    cache: &Cache,
    pages: &mut Pages,
    node: &Value,
    selected_groups: Option<&std::collections::BTreeSet<usize>>,
    root: &Path,
) -> Result<Value> {
    let mut graph = export_local(cache, pages, node, selected_groups, root)?;
    shared::complete(cache, pages, &mut graph, root)?;
    Ok(graph)
}

fn export_local(
    cache: &Cache,
    pages: &mut Pages,
    node: &Value,
    selected_groups: Option<&std::collections::BTreeSet<usize>>,
    root: &Path,
) -> Result<Value> {
    let tag = cache.find(
        "jmad",
        node["tag"]["path"].as_str().context("Animation path")?,
    )?;
    let skeleton = cache
        .block(tag.address()? + SKELETON.0, SKELETON.1)?
        .into_iter()
        .map(|row| {
            Ok(json!({"name":cache.string_id(cache.u32(row)?)?,"parent":cache.i16(row + 8)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut groups = Vec::new();
    for (index, row) in cache
        .block(tag.address()? + GROUPS.0, GROUPS.1)?
        .into_iter()
        .enumerate()
    {
        if selected_groups.is_some_and(|groups| !groups.contains(&index)) {
            continue;
        }
        let Some((handle, group)) = group_members(cache, pages, tag, row)? else {
            groups.push(json!({"index":index,"handle":u32::MAX,"status":"No resource"}));
            continue;
        };
        let mut members = Vec::new();
        for (member, data) in group.into_iter().enumerate() {
            crate::cancellation::check()?;
            let file = format!("animations/{:08X}-{index:03}-{member:03}.bin", tag.datum);
            let path = root.join(&file);
            fs::create_dir_all(path.parent().context("Animation directory")?)?;
            fs::write(&path, &data.bytes)?;
            let static_size = data.sizes[0] as usize;
            let animated = if data.sizes[1] > 0 {
                data.bytes.get(static_size).copied()
            } else {
                None
            };
            let motion = if data.bytes.is_empty() {
                Value::Null
            } else {
                let motion = codec::decode(
                    &data.bytes,
                    usize::try_from(data.frames)?,
                    usize::from(data.nodes),
                    &data.sizes,
                )
                .with_context(|| format!("Animation {} group {index} member {member}", tag.path))?;
                let motion_file =
                    format!("animations/{:08X}-{index:03}-{member:03}.json", tag.datum);
                fs::write(root.join(&motion_file), serde_json::to_vec(&motion)?)?;
                json!({"file":motion_file,"output":fingerprint(&root.join(&motion_file))?,
                    "rotation":"xyzw","translation_unit":"metre","missing_channels":"Bind pose required",
                    "limits":["Source root movement and auxiliary sections remain in the compressed archive."]})
            };
            members.push(json!({"index":member,"checksum":data.checksum,"frames":data.frames,
                "nodes":data.nodes,"movement":data.movement,"sizes":data.sizes,"file":file,"output":fingerprint(&path)?,
                "motion":motion,
                "static_codec":if static_size>0 {data.bytes.first().copied()}else{None},"animated_codec":animated,
                "status":if data.bytes.is_empty() {"No resource payload"}else{"Source compressed motion preserved"}}));
        }
        groups.push(json!({"index":index,"handle":handle,"members":members}));
    }
    Ok(
        json!({"tag":tag,"skeleton":skeleton,"clips":node["data"]["clips"],"routes":route::read(cache, tag.address()?)?,"sounds":node["data"]["sounds"],"groups":groups,"native_playback":false}),
    )
}

/// An animation graph's skeleton (`0x2C` rows: name, parent at `+0x08`), its clips (`0x3C` rows:
/// name, shared graph `+0x1C`, its clip index `+0x2C`, shared rows `+0x30`), each shared row
/// (`0xD4`: frame count `+0x02`, resource group `+0x20` and member `+0x22`), its resource groups
/// (`12` rows: handle `+0x04`), and the parent graph it inherits clips from (`+0x00`).
const SKELETON: (usize, usize) = (0x48, 0x2C);
const CLIPS: (usize, usize) = (0x94, 0x3C);
const CLIP_SHARED_GRAPH: usize = 0x1C;
const CLIP_SHARED_INDEX: usize = 0x2C;
const CLIP_SHARED: (usize, usize) = (0x30, 0xD4);
const SHARED_GROUP: usize = 0x20;
const SHARED_MEMBER: usize = 0x22;
const GROUPS: (usize, usize) = (0x1AC, 12);

/// One member of an animation resource group: its control words and compressed payload.
struct Member {
    checksum: u32,
    frames: i16,
    nodes: u8,
    movement: u8,
    sizes: Vec<u32>,
    bytes: Vec<u8>,
}

/// The handle and members of `tag`'s resource group whose block row is `row`, none when the group
/// has no resource.
fn group_members(
    cache: &Cache,
    pages: &mut Pages,
    tag: &super::Tag,
    row: usize,
) -> Result<Option<(u32, Vec<Member>)>> {
    let handle = cache.u32(row + 4)?;
    if handle == u32::MAX {
        return Ok(None);
    }
    let zone = cache.only("zone")?.address()?;
    let entries = cache.block(zone + 100, 64)?;
    let resource = cache.resource(tag, handle)?;
    let entry = *entries
        .get((handle & 65535) as usize)
        .context("Animation resource index")?;
    let address = cache.u32(entry + 36)?;
    ensure!(address >> 28 == 2, "Animation root is outside control data");
    let control = cache.meta(resource.fixup_offset, resource.fixup_size)?;
    let word = |at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            control
                .get(at..at + 4)
                .context("Animation control boundary")?
                .try_into()?,
        ))
    };
    let fixups = cache
        .block(entry + 40, 8)?
        .into_iter()
        .map(|r| Ok((cache.u32(r)? as usize, cache.u32(r + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let block = (address & 0x0fff_ffff) as usize;
    let count = word(block)? as usize;
    ensure!(count <= 16384, "Oversized animation resource group");
    let pointer = *fixups
        .get(&(block + 4))
        .context("Animation member array has no relocation")?;
    ensure!(
        pointer >> 28 == 2,
        "Animation member array is outside control data"
    );
    let start = (pointer & 0x0fff_ffff) as usize;
    ensure!(
        start <= control.len() && count * 100 <= control.len() - start,
        "Animation members exceed control data"
    );
    let mut members = Vec::with_capacity(count);
    for member in 0..count {
        let at = start + member * 100;
        let size = word(at + 80)? as usize;
        let sizes = (0..17)
            .map(|i| word(at + 12 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            size == 0 || sizes.iter().map(|s| u64::from(*s)).sum::<u64>() <= size as u64,
            "Animation section sizes exceed payload"
        );
        let bytes = if size == 0 {
            Vec::new()
        } else {
            let pointer = *fixups
                .get(&(at + 92))
                .context("Animation payload has no relocation")?;
            ensure!(
                pointer >> 28 == 4,
                "Animation payload is outside resource data"
            );
            pages.bytes(cache, &resource, (pointer & 0x0fff_ffff) as usize, size)?
        };
        members.push(Member {
            checksum: word(at + 4)?,
            frames: i16::from_le_bytes(control[at + 8..at + 10].try_into()?),
            nodes: control[at + 10],
            movement: control[at + 11],
            sizes,
            bytes,
        });
    }
    Ok(Some((handle, members)))
}

/// One skeleton node's local transform in a sampled pose: its name, and its rotation (xyzw) and
/// translation (metres) where the clip animates them. An absent channel keeps the bind pose.
pub(super) struct PoseNode {
    pub name: String,
    pub rotation: Option<[f32; 4]>,
    pub translation: Option<[f32; 3]>,
}

/// The clip `graph` names `name`, searched in `graph` and the graphs it inherits from: the graph
/// holding its shared row, and that row's address.
fn find_clip(cache: &Cache, graph: &super::Tag, name: &str) -> Result<Option<(super::Tag, usize)>> {
    let mut current = Some(graph.clone());
    let mut visited = 0;
    while let Some(tag) = current {
        visited += 1;
        ensure!(visited <= 16, "Animation graph inheritance does not end");
        let at = tag.address()?;
        for row in cache.block(at + CLIPS.0, CLIPS.1)? {
            if cache.string_id(cache.u32(row)?)? != name {
                continue;
            }
            if let Some(&shared) = cache.block(row + CLIP_SHARED.0, CLIP_SHARED.1)?.first() {
                return Ok(Some((tag, shared)));
            }
            // A clip without its own rows names another graph's clip by index.
            let Some(other) = cache.reference(row + CLIP_SHARED_GRAPH, Some("jmad"))? else {
                return Ok(None);
            };
            let index = usize::try_from(cache.i16(row + CLIP_SHARED_INDEX)?)?;
            let clip = *cache
                .block(other.address()? + CLIPS.0, CLIPS.1)?
                .get(index)
                .context("Shared animation clip outside its graph")?;
            return Ok(cache
                .block(clip + CLIP_SHARED.0, CLIP_SHARED.1)?
                .first()
                .map(|&shared| (other, shared)));
        }
        current = cache.reference(at, Some("jmad"))?;
    }
    Ok(None)
}

/// The pose clip `name` of animation graph `graph` holds at its last frame, one entry per node of
/// the skeleton of the graph that stores the clip, none when no graph in its inheritance names it.
pub(super) fn pose(
    cache: &Cache,
    pages: &mut Pages,
    graph: &super::Tag,
    name: &str,
) -> Result<Option<Vec<PoseNode>>> {
    let Some((owner, shared)) = find_clip(cache, graph, name)? else {
        return Ok(None);
    };
    let group = usize::try_from(cache.i16(shared + SHARED_GROUP)?)?;
    let member = usize::try_from(cache.i16(shared + SHARED_MEMBER)?)?;
    let row = *cache
        .block(owner.address()? + GROUPS.0, GROUPS.1)?
        .get(group)
        .context("Animation clip names a missing resource group")?;
    let Some((_, mut members)) = group_members(cache, pages, &owner, row)? else {
        return Ok(None);
    };
    ensure!(
        member < members.len(),
        "Animation clip names a missing member"
    );
    let data = members.swap_remove(member);
    if data.bytes.is_empty() {
        return Ok(None);
    }
    let frames = usize::try_from(data.frames)?.max(1);
    let motion = codec::decode(&data.bytes, frames, usize::from(data.nodes), &data.sizes)
        .with_context(|| format!("Animation {} clip {name}", owner.path))?;
    final_pose(cache, &owner, &motion, name).map(Some)
}

fn final_pose(
    cache: &Cache,
    owner: &super::Tag,
    motion: &codec::Motion,
    name: &str,
) -> Result<Vec<PoseNode>> {
    let last = motion.frames.max(1) - 1;
    let sample = |count: usize| if count == 1 { 0 } else { last.min(count - 1) };
    let skeleton = cache.block(owner.address()? + SKELETON.0, SKELETON.1)?;
    skeleton
        .iter()
        .enumerate()
        .map(|(index, &row)| {
            let track = motion.tracks.get(index);
            ensure!(
                track
                    .is_none_or(|track| track.scale.iter().all(|scale| (scale - 1.).abs() < 0.001)),
                "Scenery pose {name} has nonunit animated bone scale"
            );
            Ok(PoseNode {
                name: cache.string_id(cache.u32(row)?)?,
                rotation: track
                    .filter(|track| !track.rotation.is_empty())
                    .map(|track| track.rotation[sample(track.rotation.len())]),
                translation: track
                    .filter(|track| !track.translation.is_empty())
                    .map(|track| track.translation[sample(track.translation.len())]),
            })
        })
        .collect()
}
