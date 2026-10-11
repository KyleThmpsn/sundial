//! Preserve resource members and decode source motion without inventing controller behavior.
mod codec;
pub(super) mod first_person;
pub(super) mod native;
mod playback;
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
    let zone = cache.only("zone")?.address()?;
    let entries = cache.block(zone + 100, 64)?;
    let mut graphs = Vec::new();
    for node in behavior["nodes"]
        .as_object()
        .context("Behavior nodes")?
        .values()
    {
        if node["tag"]["group"] != "jmad" {
            continue;
        }
        let tag = cache.find(
            "jmad",
            node["tag"]["path"].as_str().context("Animation path")?,
        )?;
        let skeleton = cache
            .block(tag.address()? + 0x48, 0x2c)?
            .into_iter()
            .map(|row| {
                Ok(json!({"name":cache.string_id(cache.u32(row)?)?,"parent":cache.i16(row + 8)?}))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut groups = Vec::new();
        for (index, row) in cache
            .block(tag.address()? + 0x1ac, 12)?
            .into_iter()
            .enumerate()
        {
            let handle = cache.u32(row + 4)?;
            if handle == u32::MAX {
                groups.push(json!({"index":index,"handle":handle,"status":"No resource"}));
                continue;
            }
            let resource = cache.resource(tag, handle)?;
            let entry = *entries
                .get((handle & 65535) as usize)
                .context("Animation resource index")?;
            let address = cache.u32(entry + 36)?;
            ensure!(address >> 28 == 2, "Animation root is outside control data");
            let base = resource.fixup_offset;
            let control = cache.meta(base, resource.fixup_size)?;
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
            let mut members = Vec::new();
            for member in 0..count {
                crate::cancellation::check()?;
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
                let file = format!("animations/{:08X}-{index:03}-{member:03}.bin", tag.datum);
                let path = root.join(&file);
                fs::create_dir_all(path.parent().context("Animation directory")?)?;
                fs::write(&path, &bytes)?;
                let static_size = sizes[0] as usize;
                let animated = if sizes[1] > 0 {
                    bytes.get(static_size).copied()
                } else {
                    None
                };
                let frames = i16::from_le_bytes(control[at + 8..at + 10].try_into()?);
                let motion = if bytes.is_empty() {
                    Value::Null
                } else {
                    let motion = codec::decode(
                        &bytes,
                        usize::try_from(frames)?,
                        control[at + 10] as usize,
                        &sizes,
                    )
                    .with_context(|| {
                        format!("Animation {} group {index} member {member}", tag.path)
                    })?;
                    let motion_file =
                        format!("animations/{:08X}-{index:03}-{member:03}.json", tag.datum);
                    fs::write(root.join(&motion_file), serde_json::to_vec(&motion)?)?;
                    json!({"file":motion_file,"output":fingerprint(&root.join(&motion_file))?,
                        "rotation":"xyzw","translation_unit":"metre","missing_channels":"Bind pose required",
                        "limits":["Source root movement and auxiliary sections remain in the compressed archive."]})
                };
                members.push(json!({"index":member,"checksum":word(at+4)?,"frames":frames,
                    "nodes":control[at+10],"movement":control[at+11],"sizes":sizes,"file":file,"output":fingerprint(&path)?,
                    "motion":motion,
                    "static_codec":if static_size>0 {bytes.first().copied()}else{None},"animated_codec":animated,
                    "status":if size==0 {"No resource payload"}else{"Source compressed motion preserved"}}));
            }
            groups.push(json!({"index":index,"handle":handle,"members":members}));
        }
        graphs.push(json!({"tag":tag,"skeleton":skeleton,"clips":node["data"]["clips"],"groups":groups,"native_playback":false}));
    }
    playback::attach(scene, &mut graphs, root)?;
    let report = json!(graphs);
    crate::io::write_json(&root.join("animations.json"), &report)?;
    Ok(report)
}
