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

pub(crate) fn vertex_colors_for(r: &mut Reader, entry: &Value) -> Result<Value> {
    let tag = crate::d2_mot::profile::hash(entry, "model")?;
    let model = r.tag(tag, Some(0x80806F07))?;
    let mesh = super::geometry::selected_mesh(&model, entry)?;
    let tag = model.u32(mesh + 20)?;
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

fn texture_binding(
    r: &mut Reader,
    p: &Payload,
    row: usize,
    modern: bool,
    tag: u32,
) -> Result<Value> {
    let slot = p.u32(row)?;
    let texture = if modern {
        r.ref64(p, row + 8)
            .with_context(|| format!("material {tag:08X} texture slot {slot}"))?
    } else {
        p.u32(row + 4)?
    };
    if [0, u32::MAX, 0x811C9DC5].contains(&texture) {
        return Ok(json!({"slot":slot,"unbound":true}));
    }
    let header = r.tag(texture, None)?;
    let reference = r.reference(texture)?;
    r.tag(reference, None)?;
    let large = if modern { header.u32(60)? } else { u32::MAX };
    if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
        r.tag(large, None)?;
    }
    Ok(
        json!({"slot":slot,"tag":format!("{texture:08X}"),"buffer":format!("{reference:08X}"),"large_buffer":large,"header":hex::encode(&header.0)}),
    )
}

fn attached_bindings(
    r: &mut Reader,
    p: &Payload,
    tag: u32,
    modern: bool,
    vertex: bool,
    textures: &mut Vec<Value>,
) -> Result<bool> {
    let mut implicit_dyes = false;
    if modern {
        for row in p.array(0x38, 4, Some(0x80800014))? {
            let attached = p.u32(row)?;
            let class = r.reference(attached)?;
            let scope = r.tag(attached, None)?;
            match class {
                0x80806DBA => {
                    let base = if vertex { 0xD0 } else { 0x48 };
                    for offset in [0x18, 0x28, 0x38, 0x48] {
                        ensure!(
                            scope.u64(base + offset)? == 0,
                            "attached material scope requires dynamic bindings"
                        );
                    }
                    ensure!(
                        [0, u32::MAX, 0x811C9DC5].contains(&scope.u32(base + 0x6C)?),
                        "attached material scope has external constants"
                    );
                    for row in scope.array(base, 24, Some(0x80806DCF))? {
                        let binding = texture_binding(r, &scope, row, true, tag)?;
                        if !textures.iter().any(|t| t["slot"] == binding["slot"]) {
                            textures.push(binding);
                        }
                    }
                }
                0x808022B6 => {
                    ensure!(
                        scope.0.len() == 24
                            && scope.u64(0)? == 24
                            && [8, 12, 16]
                                .iter()
                                .all(|at| scope.u32(*at).ok() == Some(u32::MAX)),
                        "attached material dye palette overrides need conversion"
                    );
                    // A vertex/pixel material with the default palette uses the
                    // weapon's combined dye bank through its bind mode.
                    implicit_dyes = p.u32(8)? == 1;
                }
                _ => anyhow::bail!("uninspected attached material scope {class:08X}"),
            }
        }
    }
    Ok(implicit_dyes)
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
    let mut textures = p
        .array(shader + 8, if modern { 24 } else { 8 }, None)?
        .into_iter()
        .map(|row| texture_binding(r, p, row, modern, tag))
        .collect::<Result<Vec<_>>>()?;
    let implicit_dyes = attached_bindings(r, p, tag, modern, vertex, &mut textures)?;
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
        json!({"tag":format!("{tag:08X}"),"shader":format!("{:08X}",p.u32(shader)?),"textures":textures,"implicit_dyes":implicit_dyes,"samplers":samplers,"constants":constants,"external_constants":external,"tfx_bytecode_bytes":p.u64(shader+0x20)?,"gameplay_verified":false}),
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
