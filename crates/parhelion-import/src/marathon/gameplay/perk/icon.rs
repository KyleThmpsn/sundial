//! Slot glyphs. Each source socket type has a display row that names an inventory icon, and the
//! icon's primary layer holds the white pictogram the source draws for that slot: a chip, a
//! magazine, a barrel and so on. Every mod a slot accepts wears its glyph.
use super::*;
use crate::tiger::icon::Layer;
use base64::Engine as _;

/// Socket type displays, keyed by socket type hash, with an icon index at +0x20.
const DISPLAYS: u32 = 0x8080_3419;
const DISPLAY_ROW: u32 = 0x8080_341D;
const DISPLAY_SIZE: usize = 0x28;
const DISPLAY_ICON: usize = 0x20;
/// Inventory icons by index, with the icon container's tag64 at row +0x10.
const ICONS: u32 = 0x8080_71C0;
const ICON_ROW: u32 = 0x8080_71C6;
const CONTAINER: u32 = 0x8080_5335;
/// The container's primary layer, whose resource lists lanes of the same picture at several
/// sizes.
const PRIMARY: usize = 0x18;
const LAYER: u32 = 0x8080_5350;
const LANES: [u32; 2] = [0x8080_5348, 0x8080_534C];
/// A 2D Marathon texture header: 64 bytes marked `CAFE`, one layer of one depth slice.
const TEXTURE_HEADER: usize = 64;
const TEXTURE_MAGIC: u16 = 0xCAFE;
/// The recipe's perk icon edge. A glyph that fits keeps its pixels and is centred.
const EDGE: u32 = 96;

pub(super) struct Glyphs {
    icons: BTreeMap<u32, usize>,
    table: Arc<Payload>,
    rows: Vec<usize>,
    made: BTreeMap<usize, std::result::Result<String, String>>,
}

impl Glyphs {
    pub(super) fn read(r: &mut Reader) -> Result<Self> {
        let displays = table(r, DISPLAYS)?;
        let mut icons = BTreeMap::new();
        for row in displays.array(8, DISPLAY_SIZE, Some(DISPLAY_ROW))? {
            // A socket without a glyph stores -1.
            let index = i32::from_le_bytes(displays.bytes(row + DISPLAY_ICON)?);
            if let Ok(index) = usize::try_from(index) {
                icons.insert(displays.u32(row)?, index);
            }
        }
        let table = table(r, ICONS)?;
        let rows = table.array(8, 32, Some(ICON_ROW))?;
        Ok(Self {
            icons,
            table,
            rows,
            made: BTreeMap::new(),
        })
    }

    /// The glyph of the socket type `hash` as a base64 PNG, or why it has none.
    pub(super) fn png(&mut self, r: &mut Reader, hash: u32) -> std::result::Result<String, String> {
        let Some(&index) = self.icons.get(&hash) else {
            return Err("The socket type names no icon".into());
        };
        if let Some(made) = self.made.get(&index) {
            return made.clone();
        }
        let made = self.make(r, index).map_err(|error| format!("{error:#}"));
        self.made.insert(index, made.clone());
        made
    }

    fn make(&self, r: &mut Reader, index: usize) -> Result<String> {
        let row = *self
            .rows
            .get(index)
            .context("Icon index outside the icon table")?;
        let container = r.tag(r.ref64(&self.table, row + 0x10)?, Some(CONTAINER))?;
        let layer = r.tag(container.u32(PRIMARY)?, Some(LAYER))?;
        let lanes = layer.pointer(0x10)?;
        ensure!(
            lanes >= 4 && LANES.contains(&layer.u32(lanes - 4)?),
            "Unsupported icon layer"
        );
        let lane = *layer
            .array(lanes, 16, None)?
            .first()
            .context("The icon layer has no lanes")?;
        let mut largest: Option<Layer> = None;
        for at in layer.array(lane, 4, None)? {
            let texture = texture(r, layer.u32(at)?)?;
            if largest
                .as_ref()
                .is_none_or(|kept| texture.width > kept.width)
            {
                largest = Some(texture);
            }
        }
        encode(&largest.context("The icon lane has no texture")?)
    }
}

/// A texture's top mip. The first half of a split mip chain starts at mip 0.
fn texture(r: &mut Reader, tag: u32) -> Result<Layer> {
    let header = r.tag(tag, None)?;
    ensure!(
        header.0.len() == TEXTURE_HEADER
            && header.u16(32)? == TEXTURE_MAGIC
            && header.u16(38)? == 1
            && header.u16(40)? == 1,
        "Icon texture {tag:08X} is not a 2D texture"
    );
    let format = header.u32(4)?;
    let (width, height) = (header.u16(34)?, header.u16(36)?);
    let blocks = usize::from(width).div_ceil(4) * usize::from(height).div_ceil(4);
    let length = match format {
        28 | 29 => usize::from(width) * usize::from(height) * 4,
        71 | 72 => blocks * 8,
        74 | 75 | 77 | 78 | 98 | 99 => blocks * 16,
        _ => anyhow::bail!("Unsupported icon texture format {format}"),
    };
    let large = header.u32(60)?;
    let buffer = if [0, u32::MAX, 0x811C_9DC5].contains(&large) {
        r.reference(tag)?
    } else {
        large
    };
    let pixels = r.tag(buffer, None)?;
    Ok(Layer {
        slot: PRIMARY,
        texture: tag,
        format,
        width,
        height,
        data: pixels
            .0
            .get(..length)
            .context("Truncated icon pixels")?
            .to_vec(),
    })
}

/// The glyph centred on a transparent square at the recipe's edge, scaled down only when it
/// does not fit.
fn encode(layer: &Layer) -> Result<String> {
    let pixels = crate::tiger::icon::decode(layer)?;
    let mut glyph = image::RgbaImage::from_raw(layer.width.into(), layer.height.into(), pixels)
        .context("Icon size differs from its pixels")?;
    if glyph.width() > EDGE || glyph.height() > EDGE {
        let scale = f64::from(EDGE) / f64::from(glyph.width().max(glyph.height()));
        let width = ((f64::from(glyph.width()) * scale).round() as u32).max(1);
        let height = ((f64::from(glyph.height()) * scale).round() as u32).max(1);
        glyph =
            image::imageops::resize(&glyph, width, height, image::imageops::FilterType::Lanczos3);
    }
    let mut canvas = image::RgbaImage::new(EDGE, EDGE);
    image::imageops::overlay(
        &mut canvas,
        &glyph,
        i64::from((EDGE - glyph.width()) / 2),
        i64::from((EDGE - glyph.height()) / 2),
    );
    let mut bytes = std::io::Cursor::new(Vec::new());
    canvas.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}
