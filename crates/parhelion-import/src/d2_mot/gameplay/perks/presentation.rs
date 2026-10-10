//! Source perk text and artwork, independent of runtime translation coverage.
use crate::d2_mot::{assets::item, icon, localization, lore, reader::Reader};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;

pub struct Presentation {
    pub name: String,
    pub description: String,
    pub icon_png: Option<Vec<u8>>,
    pub report: Value,
}

fn layer(reader: &mut Reader, tag: u32, slot: usize) -> Result<Option<Value>> {
    let Some(texture) = icon::layer_texture(reader, tag)? else {
        return Ok(None);
    };
    let layer = icon::read_texture(reader, texture)?;
    let rgba = icon::decode(&layer)?;
    let file = format!("perk-icon-{slot:02X}.png");
    let mut encoder = png::Encoder::new(
        fs::File::create(reader.output.join(&file))?,
        u32::from(layer.width),
        u32::from(layer.height),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
    Ok(Some(json!({"slot":slot,"texture":format!("{texture:08X}"),
        "format":layer.format,"width":layer.width,"height":layer.height,"file":file})))
}

fn artwork(reader: &mut Reader, index: usize) -> Result<(Option<Vec<u8>>, Value)> {
    let container = icon::container(reader, index)?;
    let mut layers = Vec::new();
    let mut primary = None;
    for slot in [0x20, 0x14, 0x24] {
        let result = match layer(reader, container.u32(slot)?, slot) {
            Ok(Some(value)) => {
                if slot == 0x14 {
                    primary =
                        Some(fs::read(reader.output.join(
                            value["file"].as_str().context("source icon filename")?,
                        ))?);
                }
                value
            }
            Ok(None) => json!({"slot":slot,"status":"absent"}),
            Err(error) => json!({"slot":slot,"status":"unavailable","reason":format!("{error:#}")}),
        };
        layers.push(result);
    }
    Ok((primary, json!({"index":index,"layers":layers})))
}

/// Preserve source presentation even when a controller needs further translation.
/// Missing optional artwork or lore remains an explicit report entry.
pub fn read(reader: &mut Reader, hash: u32) -> Result<Presentation> {
    let (index, tag) = item::find(reader, hash)?;
    let strings_tag = localization::item_strings(reader, hash, index)?;
    let strings = reader.tag(strings_tag, Some(0x8080_549F))?;
    let mut resolver = localization::Resolver::default();
    let name = resolver.label(reader, &strings, 0x80)?;
    let description = resolver.label(reader, &strings, 0x94)?;
    let item_type = resolver.label(reader, &strings, 0x8C)?;
    let (icon_png, artwork) = match artwork(reader, strings.u32(0x78)? as usize) {
        Ok(result) => result,
        Err(error) => (
            None,
            json!({"status":"unavailable","reason":format!("{error:#}")}),
        ),
    };
    let lore = match lore::for_item(reader, hash) {
        Ok(value) => json!(value),
        Err(error) => json!({"status":"unavailable","reason":format!("{error:#}")}),
    };
    let report = json!({"item_hash":format!("{hash:08X}"),"item_tag":format!("{tag:08X}"),
        "strings_tag":format!("{strings_tag:08X}"),"locale":"English",
        "name":name,"description":description,"item_type":item_type,
        "artwork":artwork,"lore":lore});
    Ok(Presentation {
        name,
        description,
        icon_png,
        report,
    })
}
