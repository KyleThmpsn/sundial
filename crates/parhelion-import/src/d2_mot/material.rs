//! Export explicit pixel bindings without inferring resources from slot numbers.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn vectors(p: &Payload, rows: Vec<usize>) -> Result<Vec<[Value; 4]>> {
    // Constant buffers can contain packed integers, including NaN bit patterns.
    // Preserve those bits instead of treating every slot as a finite transform.
    // The source shader adapter consumes the original raw buffer bytes.
    let component = |o| -> Result<Value> {
        let bits = p.u32(o)?;
        let value = f32::from_bits(bits);
        Ok(if value.is_finite() {
            json!(value)
        } else {
            json!({"bits": format!("{bits:08X}")})
        })
    };
    rows.into_iter()
        .map(|o| {
            Ok([
                component(o)?,
                component(o + 4)?,
                component(o + 8)?,
                component(o + 12)?,
            ])
        })
        .collect()
}

/// Locate native render-state/scope examples without loading every texture.
pub fn search_native(r: &mut Reader, scope_mask: u32, blend: u8, limit: usize) -> Result<Value> {
    ensure!(
        limit > 0 && limit <= 64,
        "material search limit must be 1..64"
    );
    ensure!(blend < 128, "blend selector must fit seven bits");
    let mut tags = vec![];
    for (&package, entries) in &r.manager.lookup.tag32_entries_by_pkg {
        for (index, entry) in entries.iter().enumerate() {
            if entry.reference == 0x808071E8 {
                tags.push(tiger_pkg::TagHash::new(package, u16::try_from(index)?).0);
            }
        }
    }
    tags.sort_unstable();
    let mut matches = vec![];
    let mut unreadable = 0;
    let mut examined = 0;
    for tag in tags {
        let bytes = match r.manager.read_tag(tiger_pkg::TagHash(tag)) {
            Ok(bytes) => bytes,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        examined += 1;
        let payload = Payload(bytes);
        if payload.u32(24)? & scope_mask != scope_mask || payload.u8(32)? != (0x80 | blend) {
            continue;
        }
        r.tag(tag, Some(0x808071E8))?;
        matches.push(json!({"material":format!("{tag:08X}"),"scopes":format!("{:08X}",payload.u32(24)?),
            "states":format!("{:08X}",payload.u32(32)?),"bind_mode":payload.u32(8)?,
            "vertex":format!("{:08X}",payload.u32(0x48)?),"pixel":format!("{:08X}",payload.u32(0x2C8)?)}));
        if matches.len() == limit {
            break;
        }
    }
    Ok(json!({"examined":examined,"unreadable":unreadable,"matches":matches}))
}

/// Find model draw records that demonstrate how selected native materials run.
pub fn native_usage(r: &mut Reader, wanted: &[u32]) -> Result<Value> {
    native_draws(r, Some(wanted), None, None, None, wanted.len())
}

pub fn native_stage(r: &mut Reader, stage: usize, layout: i16, limit: usize) -> Result<Value> {
    ensure!(
        stage < 23 && (1..=64).contains(&limit),
        "invalid native stage search"
    );
    native_draws(r, None, Some(stage), Some(layout), None, limit)
}

/// Find native draws for a stage and vertex layout whose material declares a
/// given render state. Converting a source draw needs a native record that
/// already runs the same blend equation, so the state is the search key.
pub fn native_stage_state(
    r: &mut Reader,
    stage: usize,
    layout: i16,
    state: u8,
    limit: usize,
) -> Result<Value> {
    ensure!(
        stage < 23 && (1..=64).contains(&limit),
        "invalid native stage search"
    );
    native_draws(r, None, Some(stage), Some(layout), Some(state), limit)
}

fn native_draws(
    r: &mut Reader,
    wanted: Option<&[u32]>,
    selected_stage: Option<usize>,
    layout: Option<i16>,
    state: Option<u8>,
    limit: usize,
) -> Result<Value> {
    let mut tags = vec![];
    for (&package, entries) in &r.manager.lookup.tag32_entries_by_pkg {
        for (index, entry) in entries.iter().enumerate() {
            if entry.reference == 0x808073A5 {
                tags.push(tiger_pkg::TagHash::new(package, u16::try_from(index)?).0);
            }
        }
    }
    tags.sort_unstable();
    let mut found = std::collections::BTreeSet::new();
    let mut matches = vec![];
    let mut unreadable = 0;
    for tag in tags {
        let model = match r.manager.read_tag(tiger_pkg::TagHash(tag)) {
            Ok(bytes) => Payload(bytes),
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        for mesh in model.array(16, 136, Some(0x80807378))? {
            let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
            for stage in 0..23 {
                if selected_stage.is_some_and(|s| s != stage)
                    || layout.is_some_and(|l| model.i16(mesh + 88 + stage * 2).ok() != Some(l))
                {
                    continue;
                }
                let start = model.u16(mesh + 40 + stage * 2)? as usize;
                let end = model.u16(mesh + 42 + stage * 2)? as usize;
                ensure!(
                    start <= end && end <= parts.len(),
                    "invalid native stage ranges"
                );
                for &part in &parts[start..end] {
                    let material = model.u32(part)?;
                    if wanted.is_some_and(|wanted| !wanted.contains(&material)) {
                        continue;
                    }
                    if let Some(state) = state
                        && r.manager
                            .read_tag(tiger_pkg::TagHash(material))
                            .ok()
                            .map(Payload)
                            .and_then(|m| m.u8(32).ok())
                            != Some(0x80 | state)
                    {
                        continue;
                    }
                    if !found.insert(material) {
                        continue;
                    }
                    r.tag(tag, Some(0x808073A5))?;
                    matches.push(json!({"model":format!("{tag:08X}"),"mesh":mesh,
                        "material":format!("{material:08X}"),"stage":stage,
                        "layout":model.i16(mesh+88+stage*2)?,"record":hex::encode(&model.0[part..part+32])}));
                    if found.len() >= limit {
                        return Ok(json!({"matches":matches,"unreadable":unreadable}));
                    }
                }
            }
        }
        if found.len() >= limit {
            break;
        }
    }
    Ok(json!({"matches":matches,"unreadable":unreadable}))
}

pub fn vertex_colors(r: &mut Reader, tag: u32) -> Result<Value> {
    let model = r.tag(tag, Some(0x80806F07))?;
    let meshes = model.array(16, 128, Some(0x80806EC5))?;
    ensure!(meshes.len() == 1, "expected single mesh for color export");
    let tag = model.u32(meshes[0] + 20)?;
    if [0, u32::MAX, 0x811C9DC5].contains(&tag) {
        return Ok(json!({"buffer":null,"source_has_color_buffer":false}));
    }
    let header = r.tag(tag, None)?;
    ensure!(
        header.u16(4)? == 4 && header.u16(6)? == 5,
        "unsupported vertex color format"
    );
    let reference = r.reference(tag)?;
    let data = r.tag(reference, None)?;
    ensure!(
        !data.0.is_empty() && data.0.len() % 4 == 0 && header.u32(0)? as usize == data.0.len(),
        "vertex color buffer size differs"
    );
    Ok(
        json!({"buffer":format!("{reference:08X}"),"source_has_color_buffer":true,"count":data.0.len()/4,"format":"R8G8B8A8_UNORM"}),
    )
}

pub fn inspect(r: &mut Reader, tag: u32, modern: bool) -> Result<Value> {
    let p = r.tag(tag, Some(if modern { 0x80806DAA } else { 0x808071E8 }))?;
    let mut pixel = inspect_stage(r, tag, &p, modern, false)?;
    if modern {
        pixel["vertex"] = inspect_stage(r, tag, &p, modern, true)?;
    }
    Ok(pixel)
}

fn inspect_stage(
    r: &mut Reader,
    tag: u32,
    p: &Payload,
    modern: bool,
    vertex: bool,
) -> Result<Value> {
    let shader = match (modern, vertex) {
        (true, true) => 0x70,
        (true, false) => 0x2B0,
        (false, true) => 0x48,
        (false, false) => 0x2C8,
    };
    let mut textures = vec![];
    for row in p.array(shader + 8, if modern { 24 } else { 8 }, None)? {
        let slot = p.u32(row)?;
        let texture = if modern {
            r.ref64(p, row + 8)
                .with_context(|| format!("material {tag:08X} texture slot {slot}"))?
        } else {
            p.u32(row + 4)?
        };
        if [0, u32::MAX, 0x811C9DC5].contains(&texture) {
            textures.push(json!({"slot":slot,"unbound":true}));
            continue;
        }
        let header = r.tag(texture, None)?;
        let reference = r.reference(texture)?;
        r.tag(reference, None)?;
        let large = if modern { header.u32(60)? } else { u32::MAX };
        if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
            r.tag(large, None)?;
        }
        textures.push(json!({"slot":slot,"tag":format!("{texture:08X}"),"buffer":format!("{reference:08X}"),"large_buffer":large,"header":hex::encode(&header.0)}));
    }
    let mut samplers = vec![];
    for (index, row) in p.array(shader + 0x40, 16, None)?.into_iter().enumerate() {
        let tag = if modern {
            r.ref64(p, row)
                .with_context(|| format!("material {tag:08X} sampler {index}"))?
        } else {
            p.u32(row)?
        };
        if [0, u32::MAX, 0x811C9DC5].contains(&tag) {
            samplers.push(json!({"slot":index+1,"unbound":true}));
            continue;
        }
        let entry = r
            .manager
            .get_entry(tiger_pkg::TagHash(tag))
            .context("sampler entry")?;
        let file_type = entry.file_type;
        let reference = entry.reference;
        let header = r.tag(tag, None)?;
        let data = r.tag(reference, None)?;
        samplers.push(json!({"slot":index+1,"tag":format!("{tag:08X}"),"buffer":format!("{reference:08X}"),"type":file_type,"header":hex::encode(&header.0),"data":hex::encode(&data.0),"direct_sampler":file_type==34}));
    }
    let external = p.u32(shader + if modern { 0x74 } else { 0x84 })?;
    let constants = if [0, u32::MAX, 0x811C9DC5].contains(&external) {
        vectors(p, p.array(shader + 0x50, 16, None)?)?
    } else {
        r.tag(external, None)?;
        let data = r.tag(r.reference(external)?, None)?;
        ensure!(
            data.0.len() % 16 == 0,
            "external material constants are not float4 vectors"
        );
        vectors(&data, (0..data.0.len()).step_by(16).collect())?
    };
    Ok(
        json!({"tag":format!("{tag:08X}"),"shader":format!("{:08X}",p.u32(shader)?),"textures":textures,"samplers":samplers,"constants":constants,"external_constants":external,"tfx_bytecode_bytes":p.u64(shader+0x20)?,"gameplay_verified":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constants_preserve_packed_nonfinite_bits() {
        let words = [1.0f32.to_bits(), 0xFFFFFFFF, 0x7F800000, 0xFFC12345];
        let p = Payload(words.into_iter().flat_map(u32::to_le_bytes).collect());
        let values = vectors(&p, vec![0]).unwrap();
        assert_eq!(values[0][0], json!(1.0));
        assert_eq!(values[0][1], json!({"bits":"FFFFFFFF"}));
        assert_eq!(values[0][2], json!({"bits":"7F800000"}));
        assert_eq!(values[0][3], json!({"bits":"FFC12345"}));
        assert!(vectors(&p, vec![4]).is_err());
    }
}
