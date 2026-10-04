//! Shadowkeep pixel-shader texture bindings and bounded base-color decoding.
use super::*;
mod gear;
mod plate;
pub(super) use gear::{DyeMap, Gear, gear};
pub(super) use plate::{albedo, gearstack, normal};

#[derive(Clone)]
pub(crate) struct Texture {
    pub tag: u32,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AddressMode {
    Wrap,
    Mirror,
    Clamp,
    Border,
    MirrorOnce,
}

impl AddressMode {
    fn read(value: u32) -> Option<Self> {
        Some(match value {
            1 => Self::Wrap,
            2 => Self::Mirror,
            3 => Self::Clamp,
            4 => Self::Border,
            5 => Self::MirrorOnce,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sampler {
    pub u: AddressMode,
    pub v: AddressMode,
    pub border: [f32; 4],
}

pub(super) fn material(
    manager: &PackageManager,
    tag: u32,
    model: &mut Model,
) -> Result<usize, String> {
    // Prefer explicitly sRGB color bindings over linear normal/data bindings.
    // This is a preview policy, not an evaluation of the material's TFX shader.
    let mut candidates = Vec::new();
    let mut failures = Vec::new();
    for (slot, tag) in material_bindings(manager, tag)? {
        if let Some(entry) = manager.get_entry(tag)
            && entry.file_type == 32
            && matches!(entry.file_subtype, 1..=3)
        {
            let format = match manager.read_tag(tag).and_then(|header| u32_at(&header, 4)) {
                Ok(format) => format,
                Err(error) => {
                    failures.push(format!("Texture 0x{tag:08X} could not be read: {error}"));
                    continue;
                }
            };
            if let Some(rank) = color_rank(format, slot) {
                candidates.push((rank, slot, tag, format));
            }
        }
    }
    candidates.sort_unstable();
    for (_, _, tag, format) in candidates {
        if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
            model.notices.extend(failures);
            return Ok(index);
        }
        if model.textures.len() >= MAX_TEXTURES {
            // A later binding may reuse a texture already in the model.
            continue;
        }
        let texture = match load(manager, tag) {
            Ok(texture) => texture,
            Err(error) => {
                failures.push(format!("Texture 0x{tag:08X} could not be shown: {error}"));
                continue;
            }
        };
        model.notices.extend(failures);
        if matches!(format, 61 | 80) {
            model.notices.push(format!(
                "Texture 0x{tag:08X} is shown in grayscale. Shader coloring is not shown."
            ));
        }
        model.textures.push(texture);
        return Ok(model.textures.len() - 1);
    }
    if model.textures.len() >= MAX_TEXTURES {
        failures.push("The preview texture budget is full".into());
    }
    Err(if failures.is_empty() {
        "No supported color texture binding".into()
    } else {
        failures.join("\n")
    })
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
pub(crate) fn iridescence(manager: &PackageManager) -> Option<Texture> {
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

pub(crate) fn load(manager: &PackageManager, tag: u32) -> Result<Texture, String> {
    let entry = manager.get_entry(tag).ok_or("Texture header is missing")?;
    if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
        return Err("Unsupported texture header type".into());
    }
    let header = manager.read_tag(tag)?;
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
    from_payload(tag, &header, &manager.read_tag(data_tag)?)
}

/// Decode a complete local native texture without looking up a package tag.
pub(crate) fn from_payload(tag: u32, header: &[u8], bytes: &[u8]) -> Result<Texture, String> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Texture data exceeds the preview budget".into());
    }
    let width = usize::from(u16_at(header, 0x0E)?);
    let height = usize::from(u16_at(header, 0x10)?);
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u16_at(header, 0x12)? != 1
        || u16_at(header, 0x14)? != 1
    {
        return Err("Only 2D textures up to 8192 pixels are supported".into());
    }
    let format = u32_at(header, 4)?;
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
            26 | 28 | 29 | 35 | 87 | 88 | 91 | 93 => width * height * 4,
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
        // DXGI_FORMAT_R11G11B10_FLOAT stores three unsigned floating-point channels.
        26 => {
            let source = bytes
                .get(..width * height * 4)
                .ok_or("Truncated packed-float texture")?;
            return Ok(source
                .chunks_exact(4)
                .flat_map(|pixel| {
                    let bits = u32::from_le_bytes(pixel.try_into().unwrap());
                    let red = unsigned_float(bits & 0x7FF, 6);
                    let green = unsigned_float((bits >> 11) & 0x7FF, 6);
                    let blue = unsigned_float((bits >> 22) & 0x3FF, 5);
                    [red, green, blue, 255]
                })
                .collect());
        }
        // DXGI_FORMAT_R16G16_UNORM stores two normalized 16-bit channels.
        35 => {
            let source = bytes
                .get(..width * height * 4)
                .ok_or("Truncated two-channel texture")?;
            return Ok(source
                .chunks_exact(4)
                .flat_map(|pixel| {
                    let red = u16::from_le_bytes([pixel[0], pixel[1]]);
                    let green = u16::from_le_bytes([pixel[2], pixel[3]]);
                    [
                        ((u32::from(red) * 255 + 32767) / 65535) as u8,
                        ((u32::from(green) * 255 + 32767) / 65535) as u8,
                        0,
                        255,
                    ]
                })
                .collect());
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

pub(super) fn material_color_ramp(
    manager: &PackageManager,
    material: u32,
) -> Result<Option<Texture>, String> {
    let Some((_, tag)) = material_bindings(manager, material)?
        .into_iter()
        .find(|&(candidate, _)| candidate == 2)
    else {
        return Ok(None);
    };
    let Some(entry) = manager.get_entry(tag) else {
        return Ok(None);
    };
    if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
        return Ok(None);
    }
    let header = manager.read_tag(tag)?;
    let width = usize::from(u16_at(&header, 0x0E)?);
    let height = usize::from(u16_at(&header, 0x10)?);
    let format = u32_at(&header, 4)?;
    if !(16..=1024).contains(&width) || height != 1 || color_rank(format, 2) != Some(0) {
        return Ok(None);
    }
    let texture = load(manager, tag)?;
    Ok(Some(texture))
}

/// Decode the declared texture slots in shader order for effect inspection.
/// A generic color pick cannot describe a particle material that combines
/// masks, distortion, and a color ramp in separate shader slots.
pub(super) fn material_slots(
    manager: &PackageManager,
    material: u32,
) -> Result<(Vec<(u32, Texture)>, usize), String> {
    const MAX_SLOTS: usize = 8;
    const MAX_PIXELS: usize = 4 * 1024 * 1024;
    let bindings = material_bindings(manager, material)?;
    let mut textures = Vec::new();
    let mut omitted = bindings.len().saturating_sub(MAX_SLOTS);
    let mut pixels = 0usize;
    for (slot, tag) in bindings.into_iter().take(MAX_SLOTS) {
        let Some(entry) = manager.get_entry(tag) else {
            omitted += 1;
            continue;
        };
        if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
            omitted += 1;
            continue;
        }
        let Ok(texture) = load(manager, tag) else {
            omitted += 1;
            continue;
        };
        let area = texture.size[0].saturating_mul(texture.size[1]);
        if area > MAX_PIXELS.saturating_sub(pixels) {
            omitted += 1;
            continue;
        }
        pixels += area;
        textures.push((slot, texture));
    }
    Ok((textures, omitted))
}

/// The pixel shader's native sampler list follows its texture assignments.
pub(super) fn material_samplers(manager: &PackageManager, material: u32) -> Vec<Sampler> {
    let Ok(bytes) = checked(manager, material, 0x8080_71E8) else {
        return Vec::new();
    };
    let Ok((count, rows)) = array(&bytes, 0x308, 0x8080_73F3, 16, 8) else {
        return Vec::new();
    };
    (0..count)
        .map(|index| {
            let tag = u32_at(&bytes, rows + index * 16).ok()?;
            let entry = manager.get_entry(tag)?;
            if entry.file_type != 34 || entry.file_subtype != 1 {
                return None;
            }
            let header = manager.get_entry(entry.reference)?;
            if header.file_type != 42
                || header.file_subtype != 1
                || header.reference != tag
                || header.file_size != 52
            {
                return None;
            }
            let data = manager.read_tag(entry.reference).ok()?;
            let u = AddressMode::read(u32_at(&data, 4).ok()?)?;
            let v = AddressMode::read(u32_at(&data, 8).ok()?)?;
            let mut border = [0.0; 4];
            for (lane, value) in border.iter_mut().enumerate() {
                *value = f32::from_bits(u32_at(&data, 28 + lane * 4).ok()?) * 255.0;
            }
            if !border.iter().all(|value| value.is_finite()) {
                return None;
            }
            Some(Sampler { u, v, border })
        })
        // A native resource table can retain unused non-sampler entries after the
        // shader's sampler prefix. Preserve its registers without compacting holes.
        // Material contracts still reject any sampled slot beyond this prefix.
        .take_while(Option::is_some)
        .flatten()
        .collect()
}

fn material_bindings(manager: &PackageManager, tag: u32) -> Result<Vec<(u32, u32)>, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    if u64_at(&bytes, 0x2D0)? == 0 {
        return Err("This material has no image texture bindings".into());
    }
    let (count, rows) = array(&bytes, 0x2D0, 0x8080_7211, 8, 256)?;
    (0..count)
        .map(|row| {
            let offset = rows + row * 8;
            Ok((u32_at(&bytes, offset)?, u32_at(&bytes, offset + 4)?))
        })
        .collect()
}

fn unsigned_float(bits: u32, mantissa_bits: u32) -> u8 {
    let mantissa_mask = (1 << mantissa_bits) - 1;
    let exponent = ((bits >> mantissa_bits) & 0x1F) as i32;
    let mantissa = (bits & mantissa_mask) as f32 / (1 << mantissa_bits) as f32;
    let value = match exponent {
        0 => mantissa * 2f32.powi(-14),
        31 => f32::INFINITY,
        _ => (1.0 + mantissa) * 2f32.powi(exponent - 15),
    };
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests;

impl Texture {
    pub(super) fn sample_ramp(&self, position: f32) -> [f32; 4] {
        let width = self.size[0];
        let x = (position.clamp(0.0, 1.0) * width as f32 - 0.5).clamp(0.0, (width - 1) as f32);
        let left = x.floor() as usize;
        let right = (left + 1).min(width - 1);
        let blend = x - left as f32;
        std::array::from_fn(|channel| {
            self.rgba[left * 4 + channel] as f32 * (1.0 - blend)
                + self.rgba[right * 4 + channel] as f32 * blend
        })
    }

    pub(super) fn sample(&self, uv: [f32; 2]) -> [f32; 3] {
        let rgba = self.sample_rgba(uv);
        [rgba[0], rgba[1], rgba[2]]
    }

    pub(super) fn sample_rgba(&self, uv: [f32; 2]) -> [f32; 4] {
        self.sample_with_sampler(
            uv,
            &Sampler {
                u: AddressMode::Wrap,
                v: AddressMode::Wrap,
                border: [0.0; 4],
            },
        )
    }

    pub(super) fn sample_with_sampler(&self, uv: [f32; 2], sampler: &Sampler) -> [f32; 4] {
        self.sample_filtered(uv, sampler, false)
    }

    pub(super) fn sample_material(&self, uv: [f32; 2], sampler: &Sampler, color: bool) -> [f32; 4] {
        let value = self.sample_filtered(uv, sampler, color);
        if color {
            value
        } else {
            value.map(|v| v / 255.0)
        }
    }

    /// Color plates and detail color are decoded before interpolation, like an sRGB GPU
    /// texture. RGB is linear light in 0..1. Alpha remains linear coverage in 0..1.
    pub(super) fn sample_color(&self, uv: [f32; 2]) -> [f32; 4] {
        self.sample_filtered(
            uv,
            &Sampler {
                u: AddressMode::Wrap,
                v: AddressMode::Wrap,
                border: [0.0; 4],
            },
            true,
        )
    }

    fn sample_filtered(&self, uv: [f32; 2], sampler: &Sampler, color: bool) -> [f32; 4] {
        static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
        let linear = color.then(|| {
            LINEAR.get_or_init(|| {
                std::array::from_fn(|value| super::shader::linear(value as f32 / 255.0))
            })
        });
        let decode = |channel: usize, value: f32| match linear {
            Some(table) if channel < 3 => table[value.clamp(0.0, 255.0) as usize],
            Some(_) => value / 255.0,
            None => value,
        };
        let [width, height] = self.size;
        if !uv.iter().all(|value| value.is_finite()) || width == 0 || height == 0 {
            return std::array::from_fn(|channel| decode(channel, sampler.border[channel]));
        }
        let position = |coordinate: f32, size: usize, mode: AddressMode| {
            let coordinate = match mode {
                AddressMode::Wrap => coordinate.rem_euclid(1.0),
                AddressMode::Mirror => coordinate.rem_euclid(2.0),
                AddressMode::Clamp => coordinate.clamp(0.0, 1.0),
                AddressMode::Border => coordinate.clamp(-1.0, 2.0),
                AddressMode::MirrorOnce => coordinate.abs().clamp(0.0, 1.0),
            };
            coordinate * size as f32 - 0.5
        };
        let x = position(uv[0], width, sampler.u);
        let y = position(uv[1], height, sampler.v);
        let (tx, ty) = (x - x.floor(), y - y.floor());
        let address = |value: i32, size: usize, mode: AddressMode| {
            let size = size as i32;
            Some(match mode {
                AddressMode::Wrap => value.rem_euclid(size),
                AddressMode::Mirror => {
                    let at = value.rem_euclid(size * 2);
                    if at < size { at } else { size * 2 - 1 - at }
                }
                AddressMode::Clamp | AddressMode::MirrorOnce => value.clamp(0, size - 1),
                AddressMode::Border if (0..size).contains(&value) => value,
                AddressMode::Border => return None,
            } as usize)
        };
        let pixel = |dx: i32, dy: i32, channel: usize| {
            let Some(px) = address(x.floor() as i32 + dx, width, sampler.u) else {
                return decode(channel, sampler.border[channel]);
            };
            let Some(py) = address(y.floor() as i32 + dy, height, sampler.v) else {
                return decode(channel, sampler.border[channel]);
            };
            decode(channel, self.rgba[(py * width + px) * 4 + channel] as f32)
        };
        std::array::from_fn(|c| {
            let top = pixel(0, 0, c) * (1.0 - tx) + pixel(1, 0, c) * tx;
            let bottom = pixel(0, 1, c) * (1.0 - tx) + pixel(1, 1, c) * tx;
            top * (1.0 - ty) + bottom * ty
        })
    }
}
