use crate::d2_mot::{
    geometry,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
fn unique<T>(mut values: Vec<T>, name: &str) -> Result<T> {
    ensure!(
        values.len() == 1,
        "expected one {name}, got {}",
        values.len()
    );
    Ok(values.remove(0))
}
pub fn extract(r: &mut Reader, item_hash: u32, recipe: Option<&Path>) -> Result<Value> {
    extract_with_progress(
        r,
        item_hash,
        recipe,
        geometry::Detail::default(),
        &mut |_| {},
    )
}

fn texture_plates(
    r: &mut Reader,
    owner: &crate::d2_mot::payload::Payload,
    resource: usize,
    mt: u32,
    report: &mut Value,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    report["texture_plates"] = json!({});
    let plate_tag = owner.u32(resource + 0x350)?;
    let plates = if [0, u32::MAX, 0x811C9DC5].contains(&plate_tag) {
        None
    } else {
        Some(r.tag(plate_tag, Some(0x80806E1C))?)
    };
    report["has_texture_plates"] = json!(plates.is_some());
    for (n, name) in ["albedo", "normal", "gstack", "dyemap"].iter().enumerate() {
        let Some(plates) = &plates else {
            report["texture_plates"][name] = json!([]);
            continue;
        };
        progress(format!("Saving {name} textures for model {mt:08X}…"));
        let plate_tag = plates.u32(0x28 + n * 4)?;
        if [0, u32::MAX, 0x811C9DC5].contains(&plate_tag) {
            report["texture_plates"][name] = json!([]);
            continue;
        }
        let plate = r.tag(plate_tag, Some(0x80809E91))?;
        let mut entries = vec![];
        for row in plate.array(16, 20, None)? {
            let tt = plate.u32(row)?;
            let th = r.tag(tt, None)?;
            r.tag(r.reference(tt)?, None)?;
            let large = th.u32(60)?;
            if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
                r.tag(large, None)?;
            }
            entries.push(json!({"texture":format!("{tt:08X}"),"format":th.u32(4)?,"dimensions":[th.u16(34)?,th.u16(36)?],"placement":[plate.u32(row+4)? as i32,plate.u32(row+8)? as i32,plate.u32(row+12)? as i32,plate.u32(row+16)? as i32],"surface":crate::d2_mot::texture::export(r,tt)?}));
        }
        report["texture_plates"][name] = json!(entries);
    }
    Ok(())
}

pub fn extract_with_progress(
    r: &mut Reader,
    item_hash: u32,
    recipe: Option<&Path>,
    detail: geometry::Detail,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    progress("Reading source item data and lore...".into());
    let mut matches = vec![];
    for t in r.classes(0x80807997) {
        let p = r.tag(t, None)?;
        for (index, row) in p.array(8, 32, None)?.into_iter().enumerate() {
            if p.u32(row)? == item_hash {
                matches.push((index, r.ref64(&p, row + 16)?))
            }
        }
    }
    let (index, tag) = unique(matches, "item")?;
    let localized = crate::d2_mot::localization::item_name(r, item_hash, index, 0)?;
    let flavor = crate::d2_mot::localization::item_label(r, item_hash, index, 0, 0xA4)?;
    let item = r.tag(tag, Some(0x8080799D))?;
    let gameplay = super::gameplay::source(r, item_hash, index, &item)?;
    write_json(&r.output.join("gameplay.json"), &gameplay)?;
    let lore = super::lore::read(r, &item)?;
    write_json(&r.output.join("lore.json"), &json!(lore))?;
    let (texture, w, h) = crate::d2_mot::icon::export(r, item_hash, index)?;
    let art_parts = super::artwork::source::read(r, &item)?;
    let art_rows = super::artwork::source::rows(r, &item)?;
    let entities = art_parts
        .iter()
        .filter(|part| part["entity_class"] == "80809AD8")
        .map(|part| {
            Ok(u32::from_str_radix(
                part["entity"].as_str().context("art entity")?,
                16,
            )?)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(!entities.is_empty(), "no model entities");
    let mut models = vec![];
    let mut skipped_models = vec![];
    let entity_count = entities.len();
    for (entity_index, entity) in entities.into_iter().enumerate() {
        progress(format!(
            "Extracting model group {} of {entity_count}…",
            entity_index + 1
        ));
        let p = r.tag(entity, Some(0x80809AD8))?;
        for row in p.array(8, 12, None)? {
            let ot = p.u32(row)?;
            let owner = r.tag(ot, Some(0x80809B06))?;
            let resource = owner.pointer(24)?;
            let resource_class =
                owner.u32(resource.checked_sub(4).context("invalid owner resource")?)?;
            if !matches!(resource_class, 0x80806D8F | 0x80806D6C) {
                continue;
            }
            let cloth = resource_class == 0x80806D6C;
            if cloth {
                let instance = owner.pointer(16)?;
                ensure!(
                    owner.u32(instance.checked_sub(4).context("invalid cloth instance")?)?
                        == 0x80806D5B,
                    "cloth instance and resource types differ"
                );
            }
            let mt = owner.u32(resource + 0x264)?;
            let mesh_count = r
                .tag(mt, Some(0x80806F07))?
                .array(16, 128, Some(0x80806EC5))?
                .len();
            for mesh_index in 0..mesh_count {
                let mut report = match geometry::export_mesh(
                    r,
                    mt,
                    mesh_index,
                    mesh_count == 1,
                    detail,
                ) {
                    Ok(report) => report,
                    Err(error) if error.to_string() == "no full-detail geometry" => {
                        skipped_models.push(json!({"model":format!("{mt:08X}"),"mesh_index":mesh_index,"owner":format!("{ot:08X}"),"entity":format!("{entity:08X}"),"reason":"no nondegenerate full-detail triangles"}));
                        continue;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                        format!("model {mt:08X}, mesh {mesh_index}, owner {ot:08X}, entity {entity:08X}")
                    });
                    }
                };
                report["entity"] = json!(format!("{entity:08X}"));
                report["art_parts"] = json!(
                    art_parts
                        .iter()
                        .filter(|part| part["entity"] == report["entity"])
                        .collect::<Vec<_>>()
                );
                report["owner"] = json!(format!("{ot:08X}"));
                report["cloth"] = json!(cloth);
                if cloth {
                    let definition_tag = owner.u32(resource + 0x4B0)?;
                    let definition = r.tag(definition_tag, Some(0x80806D60))?;
                    let solver = definition.u32(0x6A0)?;
                    r.tag(solver, None)?;
                    report["cloth_simulation"] = json!({"definition":format!("{definition_tag:08X}"),"solver":format!("{solver:08X}"),"converted":false,"geometry":"source skinned fallback"});
                }
                texture_plates(r, &owner, resource, mt, &mut report, progress)?;
                for mat in report["materials"].as_array().context("materials")? {
                    // Some source meshes select an implicit compute-skinning
                    // program with the null material tag. Draw-stage validation
                    // remains the converter's responsibility.
                    if mat.as_str() == Some("FFFFFFFF") {
                        continue;
                    }
                    r.tag(
                        u32::from_str_radix(mat.as_str().context("material")?, 16)?,
                        Some(0x80806DAA),
                    )?;
                }
                models.push(report);
            }
        }
    }
    ensure!(!models.is_empty(), "no supported model resources");
    if let Some(path) = recipe {
        ensure!(w == 96 && h == 96, "recipe requires 96x96 icon");
        let text = fs::read_to_string(path)?;
        let mut recipe: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
        recipe["overrides"]["icon_edit"]["imported_image"] = json!({"png_base64":base64::engine::general_purpose::STANDARD.encode(fs::read(r.output.join("item-icon.png"))?)});
        write_json(&r.output.join("with-icon.parhelion.json"), &recipe)?;
    }
    Ok(
        json!({"item_hash":item_hash,"item_tag":format!("{tag:08X}"),"item_index":index,"name":localized["name"],"localization":localized,"flavor":flavor["name"],"lore":lore,"flavor_localization":flavor,"icon":{"path":"item-icon.png","texture":format!("{texture:08X}"),"size":[w,h],"layer":"primary artwork with separate native rarity background and watermark"},"art_rows":art_rows,"art_parts":art_parts,"models":models,"skipped_models":skipped_models,"shadowkeep_ready":false,"remaining":["native mesh/part record conversion","Shadowkeep material bindings and texture headers","private model tag allocation and gear-art registration","attachment/animation and in-game verification"]}),
    )
}
