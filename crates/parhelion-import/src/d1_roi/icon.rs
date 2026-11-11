//! Source inventory artwork shared by browsing and native recipe preparation.
use super::*;
use crate::tiger::icon::Layer;

pub struct Icon {
    pub texture: u32,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

impl Source {
    /// Primary artwork used by recipes. Native rarity and foreground remain separate layers.
    pub fn item_icon(&self, strings: u32, xg: Option<&Path>) -> Result<Option<Icon>> {
        self.container_icon(self.read(strings)?.u32(0x60)?, xg)
    }

    /// Resting inventory tile, bottom first. D1's background and foreground slots differ
    /// from D2's. Slot 0x24 is a recoloring mask, not a visible overlay.
    pub fn item_icon_layers(&self, strings: u32, xg: Option<&Path>) -> Result<Vec<Layer>> {
        let Some(container) = self.icon_container(self.read(strings)?.u32(0x60)?)? else {
            return Ok(Vec::new());
        };
        let mut layers = Vec::new();
        for slot in [0x18, 0x14, 0x20, 0x1C] {
            let read = || -> Result<Option<Layer>> {
                let Some(texture) = self.icon_texture(container.u32(slot)?, false)? else {
                    return Ok(None);
                };
                Ok(Some(self.icon_layer(slot, texture, xg)?))
            };
            match read() {
                Ok(Some(layer)) => layers.push(layer),
                Ok(None) if slot == 0x14 => {
                    anyhow::bail!("Source inventory icon has no primary artwork")
                }
                Err(error) if slot == 0x14 => return Err(error),
                _ => {}
            }
        }
        Ok(layers)
    }

    /// An icon container's primary artwork as a PNG, as talent and grid steps name it. A
    /// container with several frames, as a subclass item's has, gives its first.
    pub fn icon_png(&self, container: u32, xg: Option<&Path>) -> Result<Option<Vec<u8>>> {
        let Some(found) = self.icon_container(container)? else {
            return Ok(None);
        };
        let Some(texture) = self.icon_texture(found.u32(0x14)?, false)? else {
            return Ok(None);
        };
        let layer = self.icon_layer(0x14, texture, xg)?;
        let icon = Icon {
            texture,
            width: layer.width,
            height: layer.height,
            rgba: crate::tiger::icon::decode(&layer)?,
        };
        let image = image::RgbaImage::from_raw(icon.width.into(), icon.height.into(), icon.rgba)
            .context("Source icon size differs from its pixels")?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(Some(bytes.into_inner()))
    }

    /// A single-image texture's top mip, for UI art no icon container names.
    pub fn texture_image(&self, texture: u32) -> Result<image::RgbaImage> {
        let layer = self.icon_layer(0, texture, None)?;
        let rgba = crate::tiger::icon::decode(&layer)?;
        image::RgbaImage::from_raw(layer.width.into(), layer.height.into(), rgba)
            .context("Source texture size differs from its pixels")
    }

    /// [`Self::texture_image`] as a PNG.
    pub fn texture_png(&self, texture: u32) -> Result<Vec<u8>> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        self.texture_image(texture)?
            .write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(bytes.into_inner())
    }

    /// The white art the source HUD draws for glyph `key`, as a PNG. Glyph tables, class
    /// `808022D2`, hold 112-byte rows of class `8080295C` with the key at +0 and the primary layer
    /// at +4, a layer of the class icon containers name. The HUD record writer `0048A0A3` copies
    /// the key a controller names at instance +1BC unless byte +1C5 hides it.
    pub fn hud_glyph_png(&self, key: u32) -> Result<Option<Vec<u8>>> {
        let glyphs = self.hud_glyphs(key)?;
        let image = match glyphs.as_slice() {
            [] => return Ok(None),
            [(_, image)] => image,
            _ => anyhow::bail!("Source HUD glyph {key:08X} is ambiguous"),
        };
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(Some(bytes.into_inner()))
    }

    /// Every distinct static glyph row for `key`, with its table and art. Several tables can draw
    /// the same key at different sizes, as the weapon silhouettes are.
    pub fn hud_glyphs(&self, key: u32) -> Result<Vec<(u32, image::RgbaImage)>> {
        let mut rows = Vec::new();
        for table in self.classes(0x808022D2) {
            let data = self.read(table)?;
            for at in data.array(8, 112, Some(0x8080295C))? {
                if data.u32(at)? == key {
                    let row = data.bytes::<112>(at)?;
                    if !rows.iter().any(|(_, other)| *other == row) {
                        rows.push((table, row));
                    }
                }
            }
        }
        rows.into_iter()
            .map(|(table, row)| {
                let row = Payload(row.to_vec());
                // A static row: no further layer at +08 and no colors or display policy from +18.
                ensure!(
                    row.bytes::<16>(8)? == [0xFF; 16] && row.bytes::<88>(24)? == [0; 88],
                    "Source HUD glyph {key:08X} has additional display policy"
                );
                let icon = self.layer_icon(row.u32(4)?)?;
                let image =
                    image::RgbaImage::from_raw(icon.width.into(), icon.height.into(), icon.rgba)
                        .context("Source HUD glyph size differs from its pixels")?;
                Ok((table, image))
            })
            .collect()
    }

    pub(super) fn container_icon(&self, container: u32, xg: Option<&Path>) -> Result<Option<Icon>> {
        let Some(container) = self.icon_container(container)? else {
            return Ok(None);
        };
        let Some(texture) = self.icon_texture(container.u32(0x14)?, true)? else {
            return Ok(None);
        };
        let layer = self.icon_layer(0x14, texture, xg)?;
        Ok(Some(Icon {
            texture,
            width: layer.width,
            height: layer.height,
            rgba: crate::tiger::icon::decode(&layer)?,
        }))
    }

    fn icon_container(&self, tag: u32) -> Result<Option<Payload>> {
        if matches!(tag, 0 | u32::MAX) {
            return Ok(None);
        }
        let class = self.reference(tag)?;
        if class != 0x80802797 {
            let definition = self.read(class)?;
            ensure!(
                definition.u32(8)? == class && definition.u32(12)? == 0x80802797,
                "Source icon container ancestry differs"
            );
        }
        Ok(Some(self.read(tag)?))
    }

    /// One static layer, without inventory background, tint or foreground slots.
    pub(super) fn layer_icon(&self, tag: u32) -> Result<Icon> {
        let texture = self
            .icon_texture(tag, true)?
            .context("Source icon layer is empty")?;
        let layer = self.icon_layer(0, texture, None)?;
        Ok(Icon {
            texture,
            width: layer.width,
            height: layer.height,
            rgba: crate::tiger::icon::decode(&layer)?,
        })
    }

    fn icon_texture(&self, tag: u32, single: bool) -> Result<Option<u32>> {
        if matches!(tag, 0 | u32::MAX) {
            return Ok(None);
        }
        ensure!(
            self.reference(tag)? == 0x80802070,
            "Source icon layer class differs"
        );
        let layer = self.read(tag)?;
        let resource = layer.pointer(16)?;
        ensure!(
            layer.u32(
                resource
                    .checked_sub(4)
                    .context("Source icon resource pointer")?
            )? == 0x808029CD,
            "Source icon resource differs"
        );
        let lanes = layer.array(resource, 16, Some(0x80802478))?;
        ensure!(
            !single || lanes.len() == 1,
            "Source inventory icon has multiple lanes"
        );
        let Some(&lane) = lanes.first() else {
            return Ok(None);
        };
        let textures = layer.array(lane, 4, Some(0x80802B99))?;
        ensure!(
            !single || textures.len() == 1,
            "Source inventory icon has multiple frames"
        );
        let Some(&frame) = textures.first() else {
            return Ok(None);
        };
        let texture = layer.u32(frame)?;
        Ok((!matches!(texture, 0 | u32::MAX)).then_some(texture))
    }

    fn icon_layer(&self, slot: usize, tag: u32, xg: Option<&Path>) -> Result<Layer> {
        let texture = self.linear_texture(tag, xg)?;
        ensure!(
            texture.layers == 1 && texture.depth == 1,
            "Source inventory icon is not a single image"
        );
        let (_, length) = super::texture::surface(
            texture.format,
            texture.width as usize,
            texture.height as usize,
        )?;
        Ok(Layer {
            slot,
            texture: tag,
            format: texture.format,
            width: texture.width,
            height: texture.height,
            data: texture
                .bytes
                .get(..length)
                .context("Source inventory icon top mip")?
                .to_vec(),
        })
    }
}
