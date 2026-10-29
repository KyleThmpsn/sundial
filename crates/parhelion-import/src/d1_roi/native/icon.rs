use super::*;

impl Assembly<'_> {
    pub(super) fn icon(&mut self, item: &Armor) -> Result<Option<String>> {
        self.item_icon(item.strings)
    }

    pub(super) fn item_icon(&mut self, strings: u32) -> Result<Option<String>> {
        let strings = self.source.read(strings)?;
        let container = strings.u32(0x60)?;
        self.container_icon(container)
    }

    /// The talent grid display of `ornament` on the item whose definition is `owner`: its name
    /// and icon container.
    pub(super) fn ornament_display(
        &mut self,
        owner: u32,
        ornament: &Value,
    ) -> Result<(String, u32)> {
        let definition = self.source.read(owner)?;
        let holder = definition.pointer(0x58)?;
        ensure!(
            definition.u32(holder - 4)? == 0x808017BD,
            "Source talent holder type differs"
        );
        let ordinal = usize::from(definition.u16(holder)?);
        let (_, grids) = self.source.table(0x808018C2)?;
        let (_, clients) = self.source.table(0x80803461)?;
        let grids_rows = grids.array(8, 24, Some(0x808017CB))?;
        let clients_rows = clients.array(8, 16, None)?;
        ensure!(
            grids_rows.len() == clients_rows.len(),
            "Source shared and client grids disagree"
        );
        let shared_tag =
            grids.u32(*grids_rows.get(ordinal).context("Source ornament grid")? + 16)?;
        ensure!(
            ornament["grid"].as_u64() == Some(u64::from(shared_tag)),
            "Source ornament grid differs"
        );
        let client_tag = clients.u32(
            *clients_rows
                .get(ordinal)
                .context("Source ornament client grid")?
                + 8,
        )?;
        ensure!(
            self.source.reference(client_tag)? == 0x80803397,
            "Source client talent type differs"
        );
        let shared = self.source.read(shared_tag)?;
        let client = self.source.read(client_tag)?;
        let nodes = shared.array(16, 64, Some(0x80801728))?;
        let displays = client.array(8, 72, Some(0x8080348B))?;
        ensure!(
            nodes.len() == displays.len(),
            "Source ornament node ordinals disagree"
        );
        let node = usize::try_from(ornament["node"].as_u64().context("Source ornament node")?)?;
        let steps = shared.array(
            *nodes.get(node).context("Source ornament node ordinal")? + 24,
            144,
            Some(0x80801758),
        )?;
        let labels = client.array(
            *displays
                .get(node)
                .context("Source ornament display ordinal")?,
            48,
            Some(0x80803468),
        )?;
        ensure!(
            steps.len() == labels.len(),
            "Source ornament step ordinals disagree"
        );
        let step = usize::try_from(ornament["step"].as_u64().context("Source ornament step")?)?;
        let display = *labels.get(step).context("Source ornament step ordinal")?;
        let container = client.u32(display + 32)?;
        let name = super::super::localization::Labels::new(self.source)?.text(
            self.source,
            &client,
            display,
        )?;
        self.evidence.push(json!({"source_ornament_client":client_tag,"node":node,"step":step,"name":name,"icon_container":container}));
        Ok((name, container))
    }

    pub(super) fn container_icon(&mut self, container: u32) -> Result<Option<String>> {
        let Some((width, height, rgba)) = self.container_rgba(container)? else {
            return Ok(None);
        };
        let file = "source-icon.png";
        let mut encoder = png::Encoder::new(
            fs::File::create(self.folder.join(file))?,
            width.into(),
            height.into(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
        Ok(Some(file.to_owned()))
    }

    /// A talent's icon as a square PNG at the recipe's artwork size, base64 encoded.
    pub(super) fn talent_icon(&mut self, container: u32) -> Result<Option<String>> {
        use base64::Engine;
        const EDGE: u32 = 96;
        let Some((width, height, rgba)) = self.container_rgba(container)? else {
            return Ok(None);
        };
        let image = image::RgbaImage::from_raw(width.into(), height.into(), rgba)
            .context("Source talent icon size differs from its pixels")?;
        ensure!(
            image.width() == image.height(),
            "Source talent icon is not square"
        );
        let image = if image.width() == EDGE {
            image
        } else {
            image::imageops::resize(&image, EDGE, EDGE, image::imageops::FilterType::Lanczos3)
        };
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()),
        ))
    }

    /// An icon container's single image as RGBA pixels.
    fn container_rgba(&mut self, container: u32) -> Result<Option<(u16, u16, Vec<u8>)>> {
        let Some(icon) = self
            .source
            .container_icon(container, self.options.xg_library.as_deref())?
        else {
            return Ok(None);
        };
        self.evidence
            .push(json!({"source_icon":icon.texture,"width":icon.width,"height":icon.height}));
        Ok(Some((icon.width, icon.height, icon.rgba)))
    }
}
