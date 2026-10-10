//! Package-backed preview decoding and layer composition, without editor state.
use super::{
    ICON_PREVIEW_SIZE, WeaponIconEdit,
    authoring::{
        read_icon_layer, read_primary_layer_tag, read_rgba8_texture_pair, read_tag,
        texture_reference_offsets,
    },
};
use crate::tag_payload::{read_u16, read_u32, write_u32};
use std::path::Path;
use sundial::image_processing::{blend_rgba_pixel, decode_bc1};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::{
    icon_schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_FOREGROUND_LAYER_OFFSET, ICON_PRIMARY_LAYER_OFFSET,
        ICON_WATERMARK_LAYER_OFFSET,
    },
    is_valid_package_tag, open_shadowkeep_package_manager,
};
use tiger_pkg::TagHash;

#[derive(Clone)]
pub(super) struct DecodedIconImage {
    pub(super) size: [usize; 2],
    pub(super) rgba: Vec<u8>,
}

pub(super) struct LoadedIconPreview {
    pub(super) background: Option<DecodedIconImage>,
    pub(super) primary: DecodedIconImage,
    pub(super) authored_watermark: DecodedIconImage,
    pub(super) foreground: Option<DecodedIconImage>,
    pub(super) warnings: Vec<String>,
    /// Keeps the container's own background and watermark, as the build does for a subclass.
    pub(super) plain: bool,
}

/// Decoded context retained by the watermark editor, without open package handles.
pub(crate) struct WatermarkPreview {
    icon: LoadedIconPreview,
    edit: WeaponIconEdit,
}

impl WatermarkPreview {
    pub(crate) fn load(
        packages: &Path,
        container: TagHash,
        rarity: crate::AuthoredWeaponRarity,
        edit: WeaponIconEdit,
    ) -> Result<Self, String> {
        let manager = open_shadowkeep_package_manager(packages)?;
        Ok(Self {
            icon: load_icon_preview(&manager, container, rarity, false)?,
            edit,
        })
    }

    pub(crate) fn render(
        &mut self,
        artwork: &crate::presentation::Artwork,
    ) -> Result<egui::ColorImage, String> {
        self.icon.authored_watermark.rgba = crate::watermark::render_custom_corner(artwork, 0)
            .map_err(|error| error.to_string())?;
        self.icon.render(&self.edit)
    }
}

impl LoadedIconPreview {
    pub(super) fn set_branding(
        &mut self,
        branding: crate::branding::Branding,
    ) -> Result<(), String> {
        if self.plain {
            return Ok(());
        }
        let image = branding.watermark().map_err(|error| error.to_string())?;
        self.authored_watermark = DecodedIconImage {
            size: [image.width() as usize, image.height() as usize],
            rgba: image.into_raw(),
        };
        Ok(())
    }
    /// Takes `art` as the image an edit starts from, in place of the container's own.
    pub(super) fn replace_primary(&mut self, art: &super::ImportedIcon) {
        let [width, height] = self.primary.size.map(|side| side as u32);
        self.primary.rgba = art.fit_to(width, height).into_raw();
    }

    pub(super) fn source_primary(&self, edit: &WeaponIconEdit) -> DecodedIconImage {
        let mut primary = self.primary.clone();
        if let Some(imported) = &edit.imported_image {
            primary.rgba = imported
                .fit_to(primary.size[0] as u32, primary.size[1] as u32)
                .into_raw();
        }
        primary
    }
    pub(super) fn render(&self, edit: &WeaponIconEdit) -> Result<egui::ColorImage, String> {
        let mut primary = self.primary.clone();
        edit.apply_to_rgba8_sized(&mut primary.rgba, primary.size[0], primary.size[1])
            .map_err(|error| error.to_string())?;
        Ok(composite_icon([
            self.background.as_ref(),
            Some(&primary),
            Some(&self.authored_watermark),
            self.foreground.as_ref(),
        ]))
    }
}

pub(super) fn load_icon_preview(
    manager: &PackageManager,
    container_tag: TagHash,
    rarity: crate::AuthoredWeaponRarity,
    plain: bool,
) -> Result<LoadedIconPreview, String> {
    // Reuse the backend's strict icon-definition audit before accepting any preview graph.
    read_primary_layer_tag(manager, container_tag).map_err(|error| error.to_string())?;
    let mut container = manager.read_tag(container_tag).map_err(|error| {
        format!("Could not read donor icon definition {container_tag}: {error}")
    })?;
    if !plain {
        write_u32(
            &mut container,
            ICON_BACKGROUND_LAYER_OFFSET,
            rarity.icon_background_layer().0,
        )
        .map_err(|error| error.to_string())?;
    }
    let primary = load_primary_preview_layer(manager, &container, container_tag)?;
    let mut warnings = Vec::new();
    let background = load_context_preview_layer(
        manager,
        &container,
        ICON_BACKGROUND_LAYER_OFFSET,
        "background",
        &mut warnings,
    );
    let foreground = load_context_preview_layer(
        manager,
        &container,
        ICON_FOREGROUND_LAYER_OFFSET,
        "foreground overlay",
        &mut warnings,
    );
    let authored_watermark = if plain {
        load_context_preview_layer(
            manager,
            &container,
            ICON_WATERMARK_LAYER_OFFSET,
            "watermark",
            &mut warnings,
        )
        .unwrap_or(DecodedIconImage {
            size: [0, 0],
            rgba: Vec::new(),
        })
    } else {
        load_bundled_preview_watermark()?
    };
    Ok(LoadedIconPreview {
        background,
        primary,
        authored_watermark,
        foreground,
        warnings,
        plain,
    })
}

pub(super) fn load_bundled_preview_watermark() -> Result<DecodedIconImage, String> {
    let image = crate::watermark::render_output_texture(0).map_err(|error| error.to_string())?;
    Ok(DecodedIconImage {
        size: [image.width() as usize, image.height() as usize],
        rgba: image.into_raw(),
    })
}

/// An icon's layers as read from the packages, with its watermark and corner in place. Reading
/// them opens the packages, which takes seconds, so an edit only composes them again.
pub(crate) struct IconLayers(LoadedIconPreview);

impl IconLayers {
    pub(crate) fn from_manager(
        manager: &PackageManager,
        container_tag: TagHash,
        rarity: crate::AuthoredWeaponRarity,
        corner: Option<&crate::presentation::Artwork>,
        branding: crate::branding::Branding,
        plain: bool,
    ) -> Result<Self, String> {
        let mut preview = load_icon_preview(manager, container_tag, rarity, plain)?;
        preview.set_branding(branding)?;
        if let Some(corner) = corner.filter(|_| !plain) {
            preview.authored_watermark.rgba = crate::watermark::render_custom_corner(corner, 0)
                .map_err(|error| error.to_string())?;
        }
        Ok(Self(preview))
    }

    pub(crate) fn render(&self, edit: &WeaponIconEdit) -> Result<egui::ColorImage, String> {
        self.0.render(edit)
    }
}

pub(crate) fn render_weapon_icon_preview_from_manager(
    manager: &PackageManager,
    container_tag: TagHash,
    rarity: crate::AuthoredWeaponRarity,
    edit: &WeaponIconEdit,
    corner: Option<&crate::presentation::Artwork>,
    branding: crate::branding::Branding,
    plain: bool,
) -> Result<egui::ColorImage, String> {
    IconLayers::from_manager(manager, container_tag, rarity, corner, branding, plain)?.render(edit)
}

/// Decodes a texture without inventory backgrounds or watermarks.
pub(crate) fn render_texture_preview(
    manager: &PackageManager,
    header: TagHash,
) -> Result<egui::ColorImage, String> {
    let image = load_preview_texture_pair(manager, header)?;
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        image.size,
        &image.rgba,
    ))
}

/// Decodes a texture to RGBA at its own size, with its alpha as stored, as an export writes it.
pub(crate) fn decode_texture(
    manager: &PackageManager,
    header: TagHash,
) -> Result<image::RgbaImage, String> {
    let image = load_preview_texture_pair(manager, header)?;
    let [width, height] = image
        .size
        .map(|side| u32::try_from(side).unwrap_or(u32::MAX));
    image::RgbaImage::from_raw(width, height, image.rgba)
        .ok_or_else(|| format!("Texture {header} holds fewer pixels than its size"))
}

pub(super) fn load_primary_preview_layer(
    manager: &PackageManager,
    container: &[u8],
    container_tag: TagHash,
) -> Result<DecodedIconImage, String> {
    let layer_tag = read_tag(container, ICON_PRIMARY_LAYER_OFFSET).map_err(|e| e.to_string())?;
    let layer = read_icon_layer(manager, layer_tag, container_tag).map_err(|e| e.to_string())?;
    let header_tag = texture_reference_offsets(&layer, layer_tag)
        .map_err(|e| e.to_string())?
        .first()
        .map(|(_, tag)| *tag)
        .ok_or_else(|| format!("Weapon icon primary layer {layer_tag} has no texture"))?;
    let (_, data, header) =
        read_rgba8_texture_pair(manager, layer_tag, header_tag).map_err(|e| e.to_string())?;
    decode_icon_texture(&header, &data, true)
}

fn load_context_preview_layer(
    manager: &PackageManager,
    container: &[u8],
    offset: usize,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<DecodedIconImage> {
    let loaded = (|| {
        let layer_tag = read_tag(container, offset).map_err(|error| error.to_string())?;
        if !is_valid_package_tag(layer_tag) {
            return Ok(None);
        }
        let layer =
            read_icon_layer(manager, layer_tag, layer_tag).map_err(|error| error.to_string())?;
        let header_tag = texture_reference_offsets(&layer, layer_tag)
            .map_err(|error| error.to_string())?
            .first()
            .map(|(_, tag)| *tag)
            .ok_or_else(|| format!("Icon {label} layer {layer_tag} has no texture"))?;
        load_preview_texture_pair(manager, header_tag).map(Some)
    })();
    match loaded {
        Ok(layer) => layer,
        Err(error) => {
            warnings.push(format!("Could not load donor {label}: {error}"));
            None
        }
    }
}

fn load_preview_texture_pair(
    manager: &PackageManager,
    header_tag: TagHash,
) -> Result<DecodedIconImage, String> {
    let entry = manager
        .get_entry(header_tag)
        .ok_or_else(|| format!("Icon texture header {header_tag} has no package entry"))?;
    let data_tag = TagHash(entry.reference);
    if !is_valid_package_tag(data_tag) {
        return Err(format!(
            "Icon texture header {header_tag} has no data resource"
        ));
    }
    let header = manager
        .read_tag(header_tag)
        .map_err(|error| format!("Could not read icon texture header {header_tag}: {error}"))?;
    let data = manager
        .read_tag(data_tag)
        .map_err(|error| format!("Could not read icon texture data {data_tag}: {error}"))?;
    decode_icon_texture(&header, &data, false)
}

fn decode_icon_texture(
    header: &[u8],
    data: &[u8],
    require_rgba8: bool,
) -> Result<DecodedIconImage, String> {
    let format = read_u32(header, 4).map_err(|error| error.to_string())?;
    let width = usize::from(read_u16(header, 0x0E).map_err(|error| error.to_string())?);
    let height = usize::from(read_u16(header, 0x10).map_err(|error| error.to_string())?);
    // Emblem nameplate backgrounds run to 2300 wide.
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err("Item icon texture dimensions are invalid".to_owned());
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| "Item icon texture dimensions overflowed".to_owned())?;
    let rgba = match format {
        28 | 29 => {
            let length = pixel_count
                .checked_mul(4)
                .ok_or_else(|| "Item icon RGBA8 size overflowed".to_owned())?;
            data.get(..length)
                .ok_or_else(|| "Item icon RGBA8 data is truncated".to_owned())?
                .to_vec()
        }
        71 | 72 if !require_rgba8 => decode_bc1(data, width, height)?,
        71 | 72 => {
            return Err(format!(
                "Primary item artwork uses unsupported DXGI format {format}; only audited RGBA8 formats 28 and 29 can be edited"
            ));
        }
        _ => return Err(format!("Unsupported item icon texture format {format}")),
    };
    Ok(DecodedIconImage {
        size: [width, height],
        rgba,
    })
}

pub(super) fn composite_icon<'a>(
    layers: impl IntoIterator<Item = Option<&'a DecodedIconImage>>,
) -> egui::ColorImage {
    let mut rgba = vec![0_u8; ICON_PREVIEW_SIZE * ICON_PREVIEW_SIZE * 4];
    for layer in layers.into_iter().flatten() {
        let [source_width, source_height] = layer.size;
        if source_width == 0 || source_height == 0 {
            continue;
        }
        for y in 0..ICON_PREVIEW_SIZE {
            let source_y = y * source_height / ICON_PREVIEW_SIZE;
            for x in 0..ICON_PREVIEW_SIZE {
                let source_x = x * source_width / ICON_PREVIEW_SIZE;
                let source_offset = (source_y * source_width + source_x) * 4;
                let destination_offset = (y * ICON_PREVIEW_SIZE + x) * 4;
                blend_rgba_pixel(
                    &mut rgba[destination_offset..destination_offset + 4],
                    layer.rgba[source_offset..source_offset + 4]
                        .try_into()
                        .expect("four-byte pixel"),
                );
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([ICON_PREVIEW_SIZE, ICON_PREVIEW_SIZE], &rgba)
}
