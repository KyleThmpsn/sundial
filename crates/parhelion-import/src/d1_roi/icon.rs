//! Source inventory artwork shared by browsing and native recipe preparation.
use super::*;

pub struct Icon {
    pub texture: u32,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

impl Source {
    pub fn item_icon(&self, strings: u32, xg: Option<&Path>) -> Result<Option<Icon>> {
        self.container_icon(self.read(strings)?.u32(0x60)?, xg)
    }

    pub(super) fn container_icon(&self, container: u32, xg: Option<&Path>) -> Result<Option<Icon>> {
        if matches!(container, 0 | u32::MAX) {
            return Ok(None);
        }
        let class = self.reference(container)?;
        if class != 0x80802797 {
            let definition = self.read(class)?;
            ensure!(
                definition.u32(8)? == class && definition.u32(12)? == 0x80802797,
                "Source icon container ancestry differs"
            );
        }
        let container = self.read(container)?;
        let layer = container.u32(0x14)?;
        ensure!(
            self.reference(layer)? == 0x80802070,
            "Source icon layer class differs"
        );
        let layer = self.read(layer)?;
        let resource = layer.pointer(16)?;
        ensure!(
            layer.u32(resource - 4)? == 0x808029CD,
            "Source icon resource differs"
        );
        let lanes = layer.array(resource, 16, Some(0x80802478))?;
        ensure!(lanes.len() == 1, "Source inventory icon has multiple lanes");
        let textures = layer.array(lanes[0], 4, Some(0x80802B99))?;
        ensure!(
            textures.len() == 1,
            "Source inventory icon has multiple frames"
        );
        let tag = layer.u32(textures[0])?;
        let texture = self.linear_texture(tag, xg)?;
        ensure!(
            texture.layers == 1,
            "Source inventory icon is not a single image"
        );
        let (_, length) = super::texture::surface(
            texture.format,
            texture.width as usize,
            texture.height as usize,
        )?;
        let rgba = crate::d2_mot::icon::decode(&crate::d2_mot::icon::Layer {
            slot: 0x14,
            texture: tag,
            format: texture.format,
            width: texture.width,
            height: texture.height,
            data: texture.bytes[..length].to_vec(),
        })?;
        Ok(Some(Icon {
            texture: tag,
            width: texture.width,
            height: texture.height,
            rgba,
        }))
    }
}
