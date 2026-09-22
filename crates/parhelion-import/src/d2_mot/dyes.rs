use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result};
use serde_json::{Value, json};
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn inspect(r: &mut Reader, item_tag: u32, modern: bool) -> Result<Value> {
    let item = r.tag(item_tag, None)?;
    let root = item.pointer(if modern { 0x70 } else { 0x88 })?;
    let table = r.tag(
        *r.classes(if modern { 0x808055C2 } else { 0x80805DE8 })
            .first()
            .context("dye table")?,
        None,
    )?;
    let rows = table.array(8, 8, None)?;
    let maps = r.classes(if modern { 0x8080978C } else { 0x80809780 });
    let mut result = vec![];
    for descriptor in [0x28, 0x38, 0x48] {
        for row in item.array(root + descriptor, if modern { 8 } else { 4 }, None)? {
            let (channel, index) = if modern {
                (item.u32(row)?, item.u32(row + 4)?)
            } else {
                (item.u16(row)? as u32, item.u16(row + 2)? as u32)
            };
            let d = *rows.get(index as usize).context("dye index")?;
            let manifest = table.u32(d + 4)?;
            let mut found = vec![];
            for &m in &maps {
                let Ok(map) = r.tag(m, None) else {
                    continue;
                };
                for a in map.array(8, if modern { 24 } else { 8 }, None)? {
                    if map.u32(a)? != manifest {
                        continue;
                    }
                    let relation_tag = if modern {
                        r.ref64(&map, a + 8)?
                    } else {
                        map.u32(a + 4)?
                    };
                    let relation = r.tag(relation_tag, None)?;
                    let dye_tag = if modern {
                        r.ref64(&relation, 8)?
                    } else {
                        relation.u32(16)?
                    };
                    let dye = r.tag(dye_tag, None)?;
                    let scope_tag = dye.u32(12)?;
                    let scope = r.tag(scope_tag, None)?;
                    let mut textures = vec![];
                    for tex in scope.array(
                        if modern { 0x48 } else { 0x40 },
                        if modern { 24 } else { 8 },
                        None,
                    )? {
                        let tag = if modern {
                            r.ref64(&scope, tex + 8)?
                        } else {
                            scope.u32(tex + 4)?
                        };
                        let header = r.tag(tag, None)?;
                        let buffer = r.reference(tag)?;
                        r.tag(buffer, None)?;
                        if modern && ![0, u32::MAX, 0x811C9DC5].contains(&header.u32(60)?) {
                            r.tag(header.u32(60)?, None)?;
                        }
                        textures.push(json!({"slot":scope.u32(tex)?,"tag":format!("{tag:08X}"),"header":hex::encode(&header.0),"buffer":format!("{buffer:08X}"),"offset":tex}));
                    }
                    let header_tag = scope.u32(if modern { 0xB4 } else { 0xBC })?;
                    let header = r.tag(header_tag, None)?;
                    let data = r.tag(r.reference(header_tag)?, None)?;
                    let constants = floats(&data);
                    found.push(json!({"relation":format!("{relation_tag:08X}"),"dye":format!("{dye_tag:08X}"),"scope":format!("{scope_tag:08X}"),"buffer_header":format!("{header_tag:08X}"),"buffer_header_bytes":hex::encode(&header.0),"constants":constants,"textures":textures}));
                }
            }
            result.push(json!({"descriptor":descriptor,"channel":channel,"index":index,"manifest":format!("{manifest:08X}"),"found":found}));
        }
    }
    Ok(json!(result))
}
fn floats(p: &Payload) -> Vec<Vec<f32>> {
    p.0.chunks_exact(16)
        .map(|v| {
            v.chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect()
        })
        .collect()
}
