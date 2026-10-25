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
