//! Read-only discovery of Quicktag's 137x76 weapon-icon texture family.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use tiger_pkg::TagHash;
pub fn roots(r: &mut Reader, target: u32) -> Result<Value> {
    let mut result = vec![];
    for tag in r.classes(0x80809EF9) {
        if r.manager.package_paths[&TagHash(tag).pkg_id()].name != "ui" {
            continue;
        }
        let p = Payload(r.manager.read_tag(TagHash(tag))?);
        let deps = crate::d2_mot::audit::dependencies(&p)?;
        if deps.contains(&target) {
            r.tag(tag, None)?;
            result.push(json!({"companion":format!("{tag:08X}"),"owner":format!("{:08X}",p.u32(12)?),"dependencies":deps.len()}));
        }
    }
    let result = json!({"roots":result});
    write_json(&r.output.join("roots.json"), &result)?;
    Ok(result)
}
pub fn references(r: &mut Reader, target: u32, ui_only: bool) -> Result<Value> {
    let wide: Vec<_> = r
        .manager
        .lookup
        .tag64_entries
        .iter()
        .filter_map(|(&k, v)| (v.hash32.0 == target).then_some(k))
        .collect();
    let mut tags = vec![];
    for (&pkg, entries) in &r.manager.lookup.tag32_entries_by_pkg {
        if ui_only
            && !["ui", "client_startup"].contains(&r.manager.package_paths[&pkg].name.as_str())
        {
            continue;
        }
        for (i, e) in entries.iter().enumerate() {
            if [8, 16].contains(&e.file_type) {
                tags.push(TagHash::new(pkg, u16::try_from(i)?).0);
            }
        }
    }
    tags.sort_unstable();
    let mut matches = vec![];
    let mut failures = 0;
    for (i, &tag) in tags.iter().enumerate() {
        if i % 20000 == 0 {
            eprintln!("Structured tags {i}/{}", tags.len());
        }
        let bytes = match r.manager.read_tag(TagHash(tag)) {
            Ok(b) => b,
            Err(_) => {
                failures += 1;
                continue;
            }
        };
        let mut offsets = vec![];
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            if u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) == target {
                offsets.push(json!({"offset":at,"width":4}));
            } else if !wide.is_empty()
                && at + 8 <= bytes.len()
                && wide.contains(&u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()))
            {
                offsets.push(json!({"offset":at,"width":8}));
            }
        }
        if !offsets.is_empty() {
            r.tag(tag, None)?;
            matches.push(json!({"tag":format!("{tag:08X}"),"class":format!("{:08X}",r.reference(tag)?),"package":r.manager.package_paths[&TagHash(tag).pkg_id()].name,"offsets":offsets}));
        }
    }
    let report = json!({"target":format!("{target:08X}"),"wide_ids":wide,"matches":matches,"read_failures":failures,"note":"aligned byte matches; semantic references require schema validation"});
    write_json(&r.output.join("references.json"), &report)?;
    Ok(report)
}
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn scan(r: &mut Reader, modern: bool) -> Result<Value> {
    let mut tags = vec![];
    for (&pkg, entries) in &r.manager.lookup.tag32_entries_by_pkg {
        for (i, e) in entries.iter().enumerate() {
            if e.file_type == 32 && e.file_subtype == 1 {
                tags.push(TagHash::new(pkg, u16::try_from(i)?).0);
            }
        }
    }
    tags.sort_unstable();
    let dim = if modern { 34 } else { 14 };
    let mut candidates = vec![];
    let mut failures = vec![];
    let mut sheet = vec![0u8; 1096 * 912 * 4];
    let mut sheet_rows = vec![];
    let mut pages = 0;
    for (i, &tag) in tags.iter().enumerate() {
        if i % 10000 == 0 {
            eprintln!("Texture headers {i}/{}", tags.len());
        }
        let data = match r.manager.read_tag(TagHash(tag)) {
            Ok(data) => Payload(data),
            Err(e) => {
                failures.push(json!({"tag":format!("{tag:08X}"),"error":format!("{e:#}")}));
                continue;
            }
        };
        if data.u16(dim - 2).ok() != Some(0xcafe)
            || data.u16(dim).ok() != Some(137)
            || data.u16(dim + 2).ok() != Some(76)
        {
            continue;
        }
        let p = r.tag(tag, None)?;
        let format = p.u32(4)?;
        let mut row = json!({"tag":format!("{tag:08X}"),"format":format,"size":[137,76],"package":r.manager.package_paths[&TagHash(tag).pkg_id()].name,"usage":"candidate; binding not yet traced"});
        if [28, 29].contains(&format) && p.u16(dim + 4)? == 1 && p.u16(dim + 6)? == 1 {
            let large = p.u32(if modern { 60 } else { 36 })?;
            let buffer = if [0, u32::MAX, 0x811C9DC5].contains(&large) {
                r.reference(tag)?
            } else {
                large
            };
            let bytes = r.tag(buffer, None)?;
            let rgba = bytes.0.get(..137 * 76 * 4).context("truncated icon")?;
            let name = format!("{tag:08X}.png");
            save(&r.output.join(&name), 137, 76, rgba)?;
            row["png"] = json!(name);
            row["buffer"] = json!(format!("{buffer:08X}"));
            let slot = sheet_rows.len();
            for y in 0..76 {
                for x in 0..137 {
                    let from = (y * 137 + x) * 4;
                    let to = (((slot / 8) * 76 + y) * 1096 + (slot % 8) * 137 + x) * 4;
                    let a = rgba[from + 3] as u32;
                    for c in 0..3 {
                        sheet[to + c] = ((rgba[from + c] as u32 * a + 35 * (255 - a)) / 255) as u8;
                    }
                    sheet[to + 3] = 255;
                }
            }
            sheet_rows.push(format!("{tag:08X}"));
            row["sheet"] = json!({"page":pages,"slot":slot});
            if sheet_rows.len() == 96 {
                save(
                    &r.output.join(format!("sheet-{pages}.png")),
                    1096,
                    912,
                    &sheet,
                )?;
                write_json(
                    &r.output.join(format!("sheet-{pages}.json")),
                    &json!(sheet_rows),
                )?;
                pages += 1;
                sheet_rows.clear();
                sheet.fill(0);
            }
        }
        candidates.push(row);
    }
    if !sheet_rows.is_empty() {
        let height = sheet_rows.len().div_ceil(8) * 76;
        save(
            &r.output.join(format!("sheet-{pages}.png")),
            1096,
            height as u32,
            &sheet[..1096 * height * 4],
        )?;
        write_json(
            &r.output.join(format!("sheet-{pages}.json")),
            &json!(sheet_rows),
        )?;
    }
    let report = json!({"headers_scanned":tags.len(),"candidates":candidates,"read_failures":failures,"installed":false});
    write_json(&r.output.join("hud-candidates.json"), &report)?;
    Ok(report)
}
fn save(path: &std::path::Path, w: u32, h: u32, bytes: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(fs::File::create(path)?, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(bytes)?;
    Ok(())
}
