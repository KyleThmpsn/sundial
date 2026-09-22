//! Inventory icon artwork; the ammunition HUD silhouette is a separate mapping.
use crate::d2_mot::reader::Reader;
use anyhow::{Context, Result, ensure};
use std::fs;

/// Modern icon container layer slots, in the order the client composites them. Decoded from
/// the live packages on 2026-09-20: the rarity plate, the artwork, then the 20px watermark.
const BACKGROUND: usize = 0x20;
const PRIMARY: usize = 0x14;
const WATERMARK: usize = 0x24;

fn unique<T>(mut values: Vec<T>, name: &str) -> Result<T> {
    ensure!(
        values.len() == 1,
        "expected one {name}, got {}",
        values.len()
    );
    Ok(values.remove(0))
}
pub fn export(r: &mut Reader, item_hash: u32, index: usize) -> Result<(u32, u16, u16)> {
    let strings_tag = crate::d2_mot::localization::item_strings(r, item_hash, index)?;
    let strings = r.tag(strings_tag, Some(0x8080549F))?;
    let icon_index = strings.u32(0x78)? as usize;
    let image = read_index(r, icon_index)?;
    let mut encoder = png::Encoder::new(
        fs::File::create(r.output.join("item-icon.png"))?,
        u32::from(image.width),
        u32::from(image.height),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.rgba)?;
    Ok((image.texture, image.width, image.height))
}

pub struct Image {
    pub texture: u32,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

/// One undecoded texture: `format` is the DXGI format number from the header.
pub struct Layer {
    /// Container slot offset the layer came from.
    pub slot: usize,
    pub texture: u32,
    pub format: u32,
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

/// Primary artwork as RGBA8, for export.
pub fn read_index(r: &mut Reader, icon_index: usize) -> Result<Image> {
    let container = container(r, icon_index)?;
    let texture = layer_texture(r, container.u32(PRIMARY)?)?.context("no icon layers")?;
    let layer = read_texture(r, texture)?;
    ensure!(matches!(layer.format, 28 | 29), "icon must be RGBA8");
    Ok(Image {
        texture: layer.texture,
        width: layer.width,
        height: layer.height,
        rgba: layer.data,
    })
}

/// Every layer the client draws, bottom first: background, primary, watermark.
/// Optional layers that are missing or unreadable are skipped; the primary must exist.
pub fn read_layers(r: &mut Reader, icon_index: usize) -> Result<Vec<Layer>> {
    let container = container(r, icon_index)?;
    let mut layers = Vec::new();
    for offset in [BACKGROUND, PRIMARY, WATERMARK] {
        let texture = if offset == PRIMARY {
            layer_texture(r, container.u32(offset)?)?.context("no icon layers")?
        } else {
            match container
                .u32(offset)
                .ok()
                .and_then(|tag| layer_texture(r, tag).ok().flatten())
            {
                Some(texture) => texture,
                None => continue,
            }
        };
        match read_texture(r, texture) {
            Ok(mut layer) => {
                layer.slot = offset;
                layers.push(layer);
            }
            Err(_) if offset != PRIMARY => {}
            Err(error) => return Err(error),
        }
    }
    Ok(layers)
}

fn container(
    r: &mut Reader,
    icon_index: usize,
) -> Result<std::sync::Arc<crate::d2_mot::payload::Payload>> {
    let mut containers = vec![];
    for t in r.classes(0x80805A01) {
        let p = r.tag(t, None)?;
        if let Some(&row) = p.array(8, 32, None)?.get(icon_index) {
            containers.push(r.ref64(&p, row + 16)?);
        }
    }
    r.tag(unique(containers, "icon container")?, Some(0x80803EB8))
}

/// First texture of the first lane of a layer, or `None` when the slot is empty.
fn layer_texture(r: &mut Reader, layer_tag: u32) -> Result<Option<u32>> {
    if layer_tag == 0 || layer_tag == u32::MAX {
        return Ok(None);
    }
    let layer = r.tag(layer_tag, None)?;
    let resource = layer.pointer(16)?;
    ensure!(
        [0x80803ECD, 0x80803ECB]
            .contains(&layer.u32(resource.checked_sub(4).context("invalid icon resource")?)?),
        "unsupported icon layer"
    );
    let lists = layer.array(resource, 16, None)?;
    let Some(&lane) = lists.first() else {
        return Ok(None);
    };
    let textures = layer.array(lane, 4, None)?;
    let Some(&texture) = textures.first() else {
        return Ok(None);
    };
    let texture = layer.u32(texture)?;
    Ok((texture != 0 && texture != u32::MAX).then_some(texture))
}

fn read_texture(r: &mut Reader, texture: u32) -> Result<Layer> {
    let header = r.tag(texture, None)?;
    let format = header.u32(4)?;
    let w = header.u16(34)?;
    let h = header.u16(36)?;
    ensure!(
        w > 0 && h > 0 && w <= 4096 && h <= 4096 && header.u16(38)? == 1 && header.u16(40)? == 1,
        "unsupported icon dimensions"
    );
    let pixels = r.tag(r.reference(texture)?, None)?;
    let (width, height) = (w as usize, h as usize);
    let length = match format {
        28 | 29 => width * height * 4,
        71 | 72 => width.div_ceil(4) * height.div_ceil(4) * 8,
        74 | 75 | 77 | 78 | 98 | 99 => width.div_ceil(4) * height.div_ceil(4) * 16,
        _ => anyhow::bail!("unsupported icon texture format {format}"),
    };
    Ok(Layer {
        slot: 0,
        texture,
        format,
        width: w,
        height: h,
        data: pixels
            .0
            .get(..length)
            .context("truncated icon pixels")?
            .to_vec(),
    })
}
