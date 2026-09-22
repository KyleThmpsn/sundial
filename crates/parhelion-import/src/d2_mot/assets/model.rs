//! Preserve all dynamic meshes and typed component evidence, without guessed references.
use crate::d2_mot::{geometry, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn export(r: &mut Reader, entities: &[u32], direct: Option<u32>) -> Result<Value> {
    ensure!(entities.len() <= 256, "too many art entities");
    let mut models = BTreeSet::new();
    let mut components = vec![];
    let mut blockers = vec![];
    let mut plates = vec![];
    if let Some(tag) = direct {
        models.insert(tag);
    }
    for &entity in entities {
        let class = r.reference(entity)?;
        if class != 0x80809AD8 {
            blockers.push(json!({"entity":format!("{entity:08X}"),"class":format!("{class:08X}"),"reason":"unsupported entity class"}));
            continue;
        }
        let e = r.tag(entity, Some(class))?;
        for row in e.array(8, 12, None)? {
            let tag = e.u32(row)?;
            let p = r.tag(tag, Some(0x80809B06))?;
            let at = p.pointer(24)?;
            ensure!(at >= 4, "invalid component pointer");
            let class = p.u32(at - 4)?;
            components.push(json!({"entity":format!("{entity:08X}"),"owner":format!("{tag:08X}"),"class":format!("{class:08X}")}));
            if class == 0x80806D8F {
                let model = p.u32(at + 0x264)?;
                if ![0, u32::MAX, 0x811C9DC5].contains(&model) {
                    models.insert(model);
                }
                let pt = p.u32(at + 0x350)?;
                if ![0, u32::MAX, 0x811C9DC5].contains(&pt) {
                    let table = r.tag(pt, Some(0x80806E1C))?;
                    for n in 0..4 {
                        let plate = table.u32(0x28 + n * 4)?;
                        if [0, u32::MAX, 0x811C9DC5].contains(&plate) {
                            continue;
                        }
                        let p = r.tag(plate, Some(0x80809E91))?;
                        for row in p.array(16, 20, None)? {
                            let texture = p.u32(row)?;
                            let surface =
                                crate::d2_mot::texture::export(r, texture).with_context(|| {
                                    format!("plate {plate:08X} texture {texture:08X}")
                                })?;
                            plates.push(json!({"owner":format!("{tag:08X}"),"channel":n,"plate":format!("{plate:08X}"),"placement":[p.u32(row+4)?,p.u32(row+8)?,p.u32(row+12)?,p.u32(row+16)?],"surface":surface}));
                        }
                    }
                }
            } else if [0x808081D6, 0x808081DE, 0x8080853E, 0x80808546].contains(&class) {
                let skeleton = crate::d2_mot::rig::decode(&p, at, class, tag, true)?;
                components.last_mut().unwrap()["skeleton"] = skeleton;
            } else {
                blockers.push(json!({"owner":format!("{tag:08X}"),"class":format!("{class:08X}"),"reason":"component retained as raw evidence; animation/attachment semantics not converted"}));
            }
        }
    }
    let mut output = vec![];
    for tag in models {
        let p = r.tag(tag, None)?;
        let class = r.reference(tag)?;
        if class != 0x80806F07 {
            blockers.push(json!({"model":format!("{tag:08X}"),"class":format!("{class:08X}"),"reason":"unsupported model schema; raw source retained"}));
            continue;
        }
        let meshes = p.array(16, 128, Some(0x80806EC5))?;
        ensure!(meshes.len() <= 256, "mesh count exceeds export bound");
        for i in 0..meshes.len() {
            match geometry::export_mesh(r, tag, i, false) {
                Ok(mut report) => {
                    report["mesh_index"] = json!(i);
                    for material in report["materials"].as_array().unwrap() {
                        let t = u32::from_str_radix(material.as_str().unwrap(), 16)?;
                        if [0, u32::MAX, 0x811C9DC5].contains(&t) {
                            blockers.push(json!({"model":format!("{tag:08X}"),"mesh":i,"reason":"mesh contains an unbound material slot"}));
                            continue;
                        }
                        r.tag(t, Some(0x80806DAA)).with_context(|| {
                            format!("model {tag:08X} mesh {i} material {t:08X}")
                        })?;
                    }
                    output.push(report);
                }
                Err(error) => blockers.push(
                    json!({"model":format!("{tag:08X}"),"mesh":i,"reason":format!("{error:#}")}),
                ),
            }
        }
    }
    let status = if output.is_empty() {
        "blocked"
    } else if blockers.is_empty() {
        "source_exported"
    } else {
        "partial_source_export"
    };
    Ok(
        json!({"status":status,"models":output,"components":components,"plates":plates,"blockers":blockers,"dependency_closure_complete":false,"native_conversion_complete":false}),
    )
}
