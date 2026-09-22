//! Read rendering channel identities and defaults for cross-version verification.
pub mod channels;
pub mod program;
use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub fn context(r: &mut Reader, modern: bool) -> Result<Value> {
    let name = if modern {
        "render_globals"
    } else {
        "client_bootstrap_patchable"
    };
    let named = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == name)
        .with_context(|| format!("missing named tag {name}"))?
        .hash
        .0;
    let bootstrap = r.tag(named, None)?;
    let globals_tag = bootstrap.u32(if modern { 0x48 } else { 0x4C })?;
    let globals = r.tag(globals_tag, None)?;
    let mut scopes = vec![];
    for (index, at) in globals.array(0x10, 16, None)?.into_iter().enumerate() {
        let start = globals.pointer(at)?;
        let tail = globals.0.get(start..).context("scope name pointer")?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .context("unterminated scope name")?;
        anyhow::ensure!(end <= 128, "scope name too long");
        let name = std::str::from_utf8(&tail[..end])?;
        let tag = globals.u32(at + 12)?;
        if matches!(
            name,
            "transparent"
                | "transparent_advanced"
                | "gear_plated_textures"
                | "frame"
                | "view"
                | "gear_dye_0"
                | "gear_dye_1"
                | "gear_dye_2"
                | "gear_dye_012"
        ) {
            let payload = r.tag(tag, None)?;
            scopes.push(json!({"name":name,"index":index,"tag":format!("{tag:08X}"),"class":format!("{:08X}",r.reference(tag)?),"bytes":payload.0.len()}));
        }
    }
    let lookup_tag = globals.u32(0x30)?;
    let lookup = r.tag(lookup_tag, None)?;
    let mut textures = vec![];
    for (name, offset) in [
        ("specular_tint", 8),
        ("specular_lobe", 12),
        ("specular_lobe_3d", 16),
        ("iridescence", 20),
    ] {
        let tag = lookup.u32(offset)?;
        if [0, u32::MAX, 0x811C9DC5].contains(&tag) {
            continue;
        }
        let header = r.tag(tag, None)?;
        let buffer = r.reference(tag)?;
        r.tag(buffer, None)?;
        textures.push(json!({"name":name,"tag":format!("{tag:08X}"),"buffer":format!("{buffer:08X}"),"header":hex::encode(&header.0)}));
    }
    let defaults = r.tag(globals.u32(0x34)?, None)?;
    let hashes = defaults.array(8, 4, None)?;
    let values = defaults.array(24, 16, None)?;
    anyhow::ensure!(hashes.len() == values.len(), "global channel count differs");
    let mut channels = vec![];
    for (index, (h, v)) in hashes.into_iter().zip(values).enumerate() {
        channels.push(json!({"index":index,"hash":format!("{:08X}",defaults.u32(h)?),"value":[defaults.f32(v)?,defaults.f32(v+4)?,defaults.f32(v+8)?,defaults.f32(v+12)?]}));
    }
    Ok(
        json!({"globals":format!("{globals_tag:08X}"),"channels":channels,"lookup_textures":textures,"scopes":scopes}),
    )
}
