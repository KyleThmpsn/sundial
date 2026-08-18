use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use tiger_pkg::{DestinyVersion, GameVersion, PackageManager, TagHash};

use super::package::{array_at, i64_at, relative_offset, u16_at, u32_at};

const ICON_PRIMARY_CONTAINER_OFFSET: usize = 0x14;
// Shadowkeep stores the opaque rarity background before its translucent watermark treatment.
const ICON_BACKGROUND_CONTAINER_OFFSET: usize = 0x1C;
const ICON_BACKGROUND_OVERLAY_CONTAINER_OFFSET: usize = 0x20;
const ICON_OVERLAY_CONTAINER_OFFSET: usize = 0x24;
const CATALOG_ICON_SIZE: usize = 96;
const MAX_CACHED_CATALOG_ICONS: usize = 512;
const FAILED_ICON_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct IconRuntime {
    manager: Option<PackageManager>,
    textures: HashMap<u64, (CachedIcon, u64)>,
    access_counter: u64,
}

impl IconRuntime {
    pub(super) fn texture(
        &mut self,
        context: &eframe::egui::Context,
        install_path: &Path,
        hash: u64,
        container: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        self.access_counter = self.access_counter.wrapping_add(1);
        let access = self.access_counter;
        let now = Instant::now();
        if let Some((cached, last_access)) = self.textures.get_mut(&hash) {
            *last_access = access;
            match cached {
                CachedIcon::Loaded { texture, .. } => return Some(texture.clone()),
                CachedIcon::Failed { retry_after, .. } if now < *retry_after => return None,
                CachedIcon::Failed { .. } => {}
            }
        }
        self.textures.remove(&hash);
        if self.manager.is_none() {
            match PackageManager::new(
                install_path.join("packages"),
                GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
                None,
            ) {
                Ok(manager) => self.manager = Some(manager),
                Err(error) => {
                    self.cache(
                        hash,
                        CachedIcon::Failed {
                            error: format!("Could not open the installed packages: {error}"),
                            retry_after: now + FAILED_ICON_RETRY_DELAY,
                        },
                        access,
                    );
                    return None;
                }
            }
        }
        let loaded = load_catalog_icon(
            self.manager
                .as_ref()
                .expect("icon package manager was initialized"),
            TagHash(container),
        );
        let cached = match loaded {
            Ok(loaded) => CachedIcon::Loaded {
                texture: context.load_texture(
                    format!("catalog-icon-{hash:08X}"),
                    loaded.image,
                    eframe::egui::TextureOptions::LINEAR,
                ),
                warnings: loaded.warnings,
            },
            Err(error) => CachedIcon::Failed {
                error,
                retry_after: now + FAILED_ICON_RETRY_DELAY,
            },
        };
        let texture = match &cached {
            CachedIcon::Loaded { texture, .. } => Some(texture.clone()),
            CachedIcon::Failed { .. } => None,
        };
        self.cache(hash, cached, access);
        texture
    }

    pub(super) fn diagnostic(&self, hash: u64) -> Option<String> {
        let (cached, _) = self.textures.get(&hash)?;
        match cached {
            CachedIcon::Loaded { warnings, .. } if !warnings.is_empty() => {
                Some(warnings.join("; "))
            }
            CachedIcon::Failed { error, .. } => Some(error.clone()),
            CachedIcon::Loaded { .. } => None,
        }
    }

    fn cache(&mut self, hash: u64, icon: CachedIcon, access: u64) {
        if self.textures.len() >= MAX_CACHED_CATALOG_ICONS
            && let Some(oldest) = self
                .textures
                .iter()
                .min_by_key(|(_, (_, last_access))| *last_access)
                .map(|(hash, _)| *hash)
        {
            self.textures.remove(&oldest);
        }
        self.textures.insert(hash, (icon, access));
    }
}

enum CachedIcon {
    Loaded {
        texture: eframe::egui::TextureHandle,
        warnings: Vec<String>,
    },
    Failed {
        error: String,
        retry_after: Instant,
    },
}

struct LoadedCatalogIcon {
    image: eframe::egui::ColorImage,
    warnings: Vec<String>,
}

fn load_catalog_icon(
    manager: &PackageManager,
    container_tag: TagHash,
) -> Result<LoadedCatalogIcon, String> {
    let container = manager
        .read_tag(container_tag)
        .map_err(|error| format!("Could not read icon container: {error}"))?;
    let mut warnings = Vec::new();
    let background = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_BACKGROUND_CONTAINER_OFFSET,
        "background",
        &mut warnings,
    );
    let background_overlay = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_BACKGROUND_OVERLAY_CONTAINER_OFFSET,
        "watermark",
        &mut warnings,
    );
    let primary = load_catalog_icon_layer(manager, &container, ICON_PRIMARY_CONTAINER_OFFSET)?
        .ok_or("Item icon has no primary texture")?;
    let overlay = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_OVERLAY_CONTAINER_OFFSET,
        "foreground overlay",
        &mut warnings,
    );
    Ok(LoadedCatalogIcon {
        image: composite_catalog_icon(
            [background, Some(primary), background_overlay, overlay]
                .into_iter()
                .flatten(),
        ),
        warnings,
    })
}

fn load_optional_catalog_icon_layer(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<eframe::egui::ColorImage> {
    match load_catalog_icon_layer(manager, icon_container, layer_offset) {
        Ok(layer) => layer,
        Err(error) => {
            warnings.push(format!("Could not load icon {label}: {error}"));
            None
        }
    }
}

fn load_catalog_icon_layer(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
) -> Result<Option<eframe::egui::ColorImage>, String> {
    let layer_tag = TagHash(u32_at(icon_container, layer_offset)?);
    if !layer_tag.is_valid() {
        return Ok(None);
    }
    let layer = manager
        .read_tag(layer_tag)
        .map_err(|error| format!("Could not read icon layer container: {error}"))?;
    let resource = relative_offset(0x10, 0, i64_at(&layer, 0x10)?)?;
    let (lane_count, lanes, _) = array_at(&layer, resource)?;
    if lane_count == 0 {
        return Ok(None);
    }
    let (texture_count, textures, _) = array_at(&layer, lanes)?;
    if texture_count == 0 {
        return Ok(None);
    }
    let texture_tag = TagHash(u32_at(&layer, textures)?);
    let header = manager
        .read_tag(texture_tag)
        .map_err(|error| format!("Could not read icon layer texture header: {error}"))?;
    let entry = manager
        .get_entry(texture_tag)
        .ok_or("Icon layer texture is missing from the package index")?;
    let data_tag = TagHash(entry.reference);
    if !data_tag.is_valid() {
        return Err("Icon layer texture has no data resource".into());
    }
    let data = manager
        .read_tag(data_tag)
        .map_err(|error| format!("Could not read icon layer texture: {error}"))?;
    decode_catalog_texture(&header, &data).map(Some)
}

fn composite_catalog_icon(
    layers: impl IntoIterator<Item = eframe::egui::ColorImage>,
) -> eframe::egui::ColorImage {
    let mut rgba = vec![0_u8; CATALOG_ICON_SIZE * CATALOG_ICON_SIZE * 4];
    for layer in layers {
        let [source_width, source_height] = layer.size;
        if source_width == 0 || source_height == 0 {
            continue;
        }
        for y in 0..CATALOG_ICON_SIZE {
            let source_y = y * source_height / CATALOG_ICON_SIZE;
            for x in 0..CATALOG_ICON_SIZE {
                let source_x = x * source_width / CATALOG_ICON_SIZE;
                let source =
                    layer.pixels[source_y * source_width + source_x].to_srgba_unmultiplied();
                let destination_offset = (y * CATALOG_ICON_SIZE + x) * 4;
                blend_rgba_pixel(
                    &mut rgba[destination_offset..destination_offset + 4],
                    source,
                );
            }
        }
    }
    eframe::egui::ColorImage::from_rgba_unmultiplied([CATALOG_ICON_SIZE, CATALOG_ICON_SIZE], &rgba)
}

fn blend_rgba_pixel(destination: &mut [u8], source: [u8; 4]) {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    let destination_alpha = u32::from(destination[3]);
    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha = source_alpha + (destination_alpha * inverse_source_alpha + 127) / 255;
    for channel in 0..3 {
        let premultiplied = u32::from(source[channel]) * source_alpha
            + (u32::from(destination[channel]) * destination_alpha * inverse_source_alpha + 127)
                / 255;
        destination[channel] = ((premultiplied + output_alpha / 2) / output_alpha) as u8;
    }
    destination[3] = output_alpha as u8;
}

fn decode_catalog_texture(header: &[u8], data: &[u8]) -> Result<eframe::egui::ColorImage, String> {
    let format = u32_at(header, 4)?;
    let width = usize::from(u16_at(header, 0x0E)?);
    let height = usize::from(u16_at(header, 0x10)?);
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err("Item icon texture dimensions are invalid".into());
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or("Item icon texture dimensions overflowed")?;
    let rgba = match format {
        // DXGI_FORMAT_R8G8B8A8_UNORM and _SRGB.
        28 | 29 => {
            let length = pixel_count
                .checked_mul(4)
                .ok_or("Item icon texture size overflowed")?;
            data.get(..length)
                .ok_or("Item icon texture data is truncated")?
                .to_vec()
        }
        // DXGI_FORMAT_BC1_UNORM and _SRGB.
        71 | 72 => decode_bc1(data, width, height)?,
        _ => return Err(format!("Unsupported item icon texture format {format}")),
    };
    Ok(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        &rgba,
    ))
}

fn decode_bc1(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let block_width = width.div_ceil(4);
    let block_height = height.div_ceil(4);
    let required = block_width
        .checked_mul(block_height)
        .and_then(|blocks| blocks.checked_mul(8))
        .ok_or("BC1 item icon size overflowed")?;
    if data.len() < required {
        return Err("BC1 item icon data is truncated".into());
    }
    let mut rgba = vec![0; width * height * 4];
    for block_y in 0..block_height {
        for block_x in 0..block_width {
            let offset = (block_y * block_width + block_x) * 8;
            let color_0 = u16::from_le_bytes([data[offset], data[offset + 1]]);
            let color_1 = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let mut colors = [[0_u8; 4]; 4];
            colors[0] = rgb565(color_0);
            colors[1] = rgb565(color_1);
            if color_0 > color_1 {
                for channel in 0..3 {
                    colors[2][channel] = ((2 * u16::from(colors[0][channel])
                        + u16::from(colors[1][channel]))
                        / 3) as u8;
                    colors[3][channel] = ((u16::from(colors[0][channel])
                        + 2 * u16::from(colors[1][channel]))
                        / 3) as u8;
                }
                colors[2][3] = 255;
                colors[3][3] = 255;
            } else {
                for channel in 0..3 {
                    colors[2][channel] =
                        ((u16::from(colors[0][channel]) + u16::from(colors[1][channel])) / 2) as u8;
                }
                colors[2][3] = 255;
            }
            let indices = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            for pixel_y in 0..4 {
                for pixel_x in 0..4 {
                    let x = block_x * 4 + pixel_x;
                    let y = block_y * 4 + pixel_y;
                    if x >= width || y >= height {
                        continue;
                    }
                    let pixel = pixel_y * 4 + pixel_x;
                    let color = colors[((indices >> (pixel * 2)) & 3) as usize];
                    rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
                }
            }
        }
    }
    Ok(rgba)
}

fn rgb565(color: u16) -> [u8; 4] {
    let red = ((color >> 11) & 0x1F) as u8;
    let green = ((color >> 5) & 0x3F) as u8;
    let blue = (color & 0x1F) as u8;
    [
        (u16::from(red) * 255 / 31) as u8,
        (u16::from(green) * 255 / 63) as u8,
        (u16::from(blue) * 255 / 31) as u8,
        255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_catalog_texture_decodes_package_pixels() {
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&28_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&2_u16.to_le_bytes());
        header[0x10..0x12].copy_from_slice(&1_u16.to_le_bytes());
        let image = decode_catalog_texture(&header, &[255, 0, 0, 255, 0, 255, 0, 128]).unwrap();

        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            [
                eframe::egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255),
                eframe::egui::Color32::from_rgba_unmultiplied(0, 255, 0, 128),
            ]
        );
    }

    #[test]
    fn bc1_catalog_texture_decodes_package_blocks() {
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&71_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&4_u16.to_le_bytes());
        header[0x10..0x12].copy_from_slice(&4_u16.to_le_bytes());
        let block = [0x00, 0xF8, 0xE0, 0x07, 0, 0, 0, 0];
        let image = decode_catalog_texture(&header, &block).unwrap();

        assert_eq!(image.size, [4, 4]);
        assert!(
            image
                .pixels
                .iter()
                .all(|pixel| *pixel == eframe::egui::Color32::RED)
        );
    }

    #[test]
    fn catalog_icon_layers_composite_in_package_display_order() {
        let background =
            eframe::egui::ColorImage::new([1, 1], eframe::egui::Color32::from_rgb(255, 0, 0));
        let overlay = eframe::egui::ColorImage::new(
            [1, 1],
            eframe::egui::Color32::from_rgba_unmultiplied(0, 0, 255, 128),
        );
        let image = composite_catalog_icon([background, overlay]);

        assert_eq!(image.size, [CATALOG_ICON_SIZE, CATALOG_ICON_SIZE]);
        assert!(image.pixels.iter().all(|pixel| {
            *pixel == eframe::egui::Color32::from_rgba_unmultiplied(127, 0, 128, 255)
        }));
    }
}
