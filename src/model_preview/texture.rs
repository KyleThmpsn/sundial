//! Shadowkeep pixel-shader texture bindings and bounded base-color decoding.
use super::*;
mod plate;
pub(super) use plate::{albedo, gearstack, normal};

pub(crate) struct Texture {
    pub tag: u32,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

pub(super) fn material(
    manager: &PackageManager,
    tag: u32,
    model: &mut Model,
) -> Result<usize, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    if u64_at(&bytes, 0x2D0)? == 0 {
        return Err("This material has no image texture bindings".into());
    }
    let (count, rows) = array(&bytes, 0x2D0, 0x8080_7211, 8, 256)?;
    // Prefer explicitly sRGB color bindings over linear normal/data bindings.
    // This is a preview policy, not an evaluation of the material's TFX shader.
    let mut candidates = Vec::new();
    for row in 0..count {
        let offset = rows + row * 8;
        let slot = u32_at(&bytes, offset)?;
        let tag = u32_at(&bytes, offset + 4)?;
        if let Some(entry) = manager.get_entry(tag)
            && entry.file_type == 32
            && matches!(entry.file_subtype, 1..=3)
        {
            let header = manager.read_tag(tag)?;
            let format = u32_at(&header, 4)?;
            if let Some(rank) = color_rank(format, slot) {
                candidates.push((rank, slot, tag, format));
            }
        }
    }
    candidates.sort_unstable();
    let &(_, _, tag, format) = candidates
        .first()
        .ok_or("No supported color texture binding")?;
    if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
        return Ok(index);
    }
    if model.textures.len() >= MAX_TEXTURES {
        return Err("The preview texture budget is full".into());
    }
    let texture = load(manager, tag)?;
    if matches!(format, 61 | 80) {
        model.notices.push(format!(
            "Texture 0x{tag:08X} is shown in grayscale. Shader coloring is not shown."
        ));
    }
    model.textures.push(texture);
    Ok(model.textures.len() - 1)
}

fn color_rank(format: u32, slot: u32) -> Option<u8> {
    match format {
        29 | 72 | 75 | 78 | 91 | 93 | 99 => Some(0),
        28 | 71 | 74 | 77 | 87 | 88 | 98 if slot == 0 => Some(1),
        61 | 80 if slot == 0 => Some(2),
        _ => None,
    }
}

/// The iridescence lookup from the render globals texture set (0x80806B99, slot 0x14).
pub(super) fn iridescence(manager: &PackageManager) -> Option<Texture> {
    let (tag, _) = manager
        .get_all_by_reference(0x8080_6B99)
        .into_iter()
        .next()?;
    let bytes = manager.read_tag(tag).ok()?;
    let texture = u32_at(&bytes, 0x14).ok()?;
    if matches!(texture, 0 | u32::MAX) {
        return None;
    }
    load(manager, texture).ok()
}

pub(super) fn load(manager: &PackageManager, tag: u32) -> Result<Texture, String> {
    let entry = manager.get_entry(tag).ok_or("Texture header is missing")?;
    if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
        return Err("Unsupported texture header type".into());
    }
    let header = manager.read_tag(tag)?;
    let width = usize::from(u16_at(&header, 0x0E)?);
    let height = usize::from(u16_at(&header, 0x10)?);
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u16_at(&header, 0x12)? != 1
        || u16_at(&header, 0x14)? != 1
    {
        return Err("Only 2D textures up to 8192 pixels are supported".into());
    }
    let large = u32_at(&header, 0x24)?;
    let data_tag = if matches!(large, 0 | u32::MAX) {
        entry.reference
    } else {
        large
    };
    let data_entry = manager
        .get_entry(data_tag)
        .ok_or("Texture pixels are missing")?;
    if data_entry.file_size > 32 * 1024 * 1024 {
        return Err("Texture data exceeds the preview budget".into());
    }
    let bytes = manager.read_tag(data_tag)?;
    let format = u32_at(&header, 4)?;
    let (width, height, offset) = preview_mip(format, width, height)?;
    let rgba = decode(
        bytes
            .get(offset..)
            .ok_or("Preview mip is missing from the texture payload")?,
        format,
        width,
        height,
    )?;
    Ok(Texture {
        tag,
        size: [width, height],
        rgba,
    })
}

fn preview_mip(
    format: u32,
    mut width: usize,
    mut height: usize,
) -> Result<(usize, usize, usize), String> {
    let mut offset = 0;
    while width > 2048 || height > 2048 {
        offset += match format {
            28 | 29 | 87 | 88 | 91 | 93 => width * height * 4,
            10 => width * height * 8,
            61 => width * height,
            71 | 72 | 80 => width.div_ceil(4) * height.div_ceil(4) * 8,
            74 | 75 | 77 | 78 | 83 | 98 | 99 => width.div_ceil(4) * height.div_ceil(4) * 16,
            _ => return Err(format!("Unsupported texture mip format {format}")),
        };
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok((width, height, offset))
}

pub(super) fn decode(
    bytes: &[u8],
    format: u32,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err("Invalid preview texture dimensions".into());
    }
    match format {
        28 | 29 => {
            return bytes
                .get(..width * height * 4)
                .map(<[u8]>::to_vec)
                .ok_or("Truncated RGBA texture".into());
        }
        87 | 88 | 91 | 93 => {
            let source = bytes
                .get(..width * height * 4)
                .ok_or("Truncated BGRA texture")?;
            return Ok(source
                .chunks_exact(4)
                .flat_map(|p| {
                    [
                        p[2],
                        p[1],
                        p[0],
                        if matches!(format, 88 | 93) { 255 } else { p[3] },
                    ]
                })
                .collect());
        }
        61 => {
            let source = bytes
                .get(..width * height)
                .ok_or("Truncated grayscale texture")?;
            return Ok(source.iter().flat_map(|&v| [v, v, v, 255]).collect());
        }
        // DXGI_FORMAT_R16G16B16A16_FLOAT, used by the lighting lookups. Clamped to 0..=1.
        10 => {
            let source = bytes
                .get(..width * height * 8)
                .ok_or("Truncated half-float texture")?;
            return Ok(source
                .chunks_exact(2)
                .map(|pair| {
                    let value = half_to_f32(u16::from_le_bytes([pair[0], pair[1]]));
                    (value.clamp(0.0, 1.0) * 255.0).round() as u8
                })
                .collect());
        }
        71 | 72 => return crate::image_processing::decode_bc1(bytes, width, height),
        74 | 75 | 77 | 78 | 80 | 83 | 98 | 99 => {}
        _ => return Err(format!("Unsupported base-color texture format {format}")),
    }
    let columns = width.div_ceil(4);
    let rows = height.div_ceil(4);
    let block_size = if format == 80 { 8 } else { 16 };
    let source = bytes
        .get(..columns * rows * block_size)
        .ok_or("Truncated block-compressed texture")?;
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in source.chunks_exact(block_size).enumerate() {
        let mut pixels = [0; 64];
        match format {
            74 | 75 => bcdec_rs::bc2(block, &mut pixels, 16),
            77 | 78 => bcdec_rs::bc3(block, &mut pixels, 16),
            80 => {
                let mut values = [0; 16];
                bcdec_rs::bc4(block, &mut values, 4, false);
                for (pixel, value) in pixels.chunks_exact_mut(4).zip(values) {
                    pixel.copy_from_slice(&[value, value, value, 255]);
                }
            }
            83 => {
                let mut values = [0; 32];
                bcdec_rs::bc5(block, &mut values, 8, false);
                for (pixel, value) in pixels.chunks_exact_mut(4).zip(values.chunks_exact(2)) {
                    // Blue is cavity for gear normals, not the reconstructed normal Z.
                    pixel.copy_from_slice(&[value[0], value[1], 255, 255]);
                }
            }
            _ => bcdec_rs::bc7(block, &mut pixels, 16),
        }
        for y in 0..4 {
            for x in 0..4 {
                let px = (index % columns) * 4 + x;
                let py = (index / columns) * 4 + y;
                if px < width && py < height {
                    let to = (py * width + px) * 4;
                    let from = (y * 4 + x) * 4;
                    rgba[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
        }
    }
    Ok(rgba)
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1F) as i32;
    let mantissa = (bits & 0x3FF) as f32;
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => {
            if mantissa == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
    }
}

#[cfg(test)]
mod tests;

impl Texture {
    pub(super) fn sample(&self, uv: [f32; 2]) -> [f32; 3] {
        let rgba = self.sample_rgba(uv);
        [rgba[0], rgba[1], rgba[2]]
    }

    pub(super) fn sample_rgba(&self, uv: [f32; 2]) -> [f32; 4] {
        let [width, height] = self.size;
        let x = uv[0].rem_euclid(1.0) * width as f32 - 0.5;
        let y = uv[1].rem_euclid(1.0) * height as f32 - 0.5;
        let (tx, ty) = (x - x.floor(), y - y.floor());
        let pixel = |dx: i32, dy: i32, channel: usize| {
            let px = (x.floor() as i32 + dx).rem_euclid(width as i32) as usize;
            let py = (y.floor() as i32 + dy).rem_euclid(height as i32) as usize;
            self.rgba[(py * width + px) * 4 + channel] as f32
        };
        std::array::from_fn(|c| {
            let top = pixel(0, 0, c) * (1.0 - tx) + pixel(1, 0, c) * tx;
            let bottom = pixel(0, 1, c) * (1.0 - tx) + pixel(1, 1, c) * tx;
            top * (1.0 - ty) + bottom * ty
        })
    }
}
