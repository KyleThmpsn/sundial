//! The map's atmosphere lookup volume, recolored to the level's own light. A map keeps its donor's
//! atmosphere component (`80807086`), whose volume texture at `+0x90` feeds Destiny's sky lookups
//! and colors its sun and sky light on players and other dynamic objects. The copy's texels come
//! from [`CLEAR_VOLUME`] when it has the donor's shape, else from the donor's, and each keeps its
//! luminance and alpha and takes the hue of the scenario sky's analytical light.
//! Its distance lookup at `+0x98` takes a black copy, since
//! the level's surfaces draw the level's own fog ([`clear_lookup`]).
use crate::{presentation::Graph, tiger::reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// A native texture header's byte count at `+0x00`, format at `+0x04`, its `CAFE` marker at
/// `+0x0C`, and width, height and depth as 16-bit values from `+0x0E`.
const HEADER_SIZE: usize = 40;
const BYTES: usize = 0x00;
const FORMAT: usize = 0x04;
const MARKER: usize = 0x0C;
const DIMENSIONS: usize = 0x0E;
/// DXGI's four 16-bit floats per texel, the format of the donor's volume.
const RGBA16_FLOAT: u32 = 10;
const TEXEL_BYTES: usize = 8;
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// The volume whose texels every copy takes: Distant Shore's (`pvp_greenhouse_2`, shared with The
/// Grove and Echo), 128 by 128 by 16. Its 16 depth slices, one per time of day the component's
/// 16 floats from `+0xA0` key, are equal, as a level whose light is baked needs. Along each slice's
/// rows its luminance rises from 0.07 to between 2.2 and 3.0. The Dead Cliffs' volume (`80B68CE6`)
/// instead falls from about 1.6 to 0.14 along the same rows, and its first two and last two
/// slices are nearly black, during which Paradiso drew dark and blue. Its green, 3.2 times its red,
/// gives way to the level's hue. Swapping the texels turned The Dead Cliffs' haze over Hang 'Em
/// High from gray to white without thinning it, so they color the haze but do not set it.
const CLEAR_VOLUME: u32 = 0x80C7_54C7;

/// A four half float volume texture: its header, its data tag, its texel data and texel count.
struct Volume {
    header: Vec<u8>,
    source: u32,
    data: Vec<u8>,
    texels: usize,
}

impl Volume {
    fn read(native: &mut Reader, volume: u32) -> Result<Self> {
        let header = native.tag(volume, None)?.0.clone();
        let word = |at: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                header
                    .get(at..at + 4)
                    .context("Atmosphere volume header boundary")?
                    .try_into()?,
            ))
        };
        let half = |at: usize| -> Result<usize> {
            Ok(usize::from(u16::from_le_bytes(
                header
                    .get(at..at + 2)
                    .context("Atmosphere volume header boundary")?
                    .try_into()?,
            )))
        };
        ensure!(
            header.len() == HEADER_SIZE && half(MARKER)? == 0xCAFE && word(FORMAT)? == RGBA16_FLOAT,
            "The atmosphere volume {volume:08X} is not a four half float texture"
        );
        let texels = half(DIMENSIONS)? * half(DIMENSIONS + 2)? * half(DIMENSIONS + 4)?;
        let source = native.reference(volume)?;
        let data = native.tag(source, None)?.0.clone();
        ensure!(
            texels > 0
                && data.len() == texels * TEXEL_BYTES
                && usize::try_from(word(BYTES)?)? == data.len(),
            "The atmosphere volume {volume:08X} holds {} bytes for {texels} texels",
            data.len()
        );
        Ok(Self {
            header,
            source,
            data,
            texels,
        })
    }

    /// Whether `other` has this volume's format, byte count and dimensions.
    fn same_shape(&self, other: &Self) -> bool {
        self.header[BYTES..BYTES + 8] == other.header[BYTES..BYTES + 8]
            && self.header[DIMENSIONS..DIMENSIONS + 6] == other.header[DIMENSIONS..DIMENSIONS + 6]
    }
}

/// Adds a copy of the donor's atmosphere volume `volume` whose texels, [`CLEAR_VOLUME`]'s where it
/// has the donor's shape, take `color`'s hue at their own luminance. Returns the copy's symbol and
/// a report of the source's and the copy's mean colors and alphas.
pub(super) fn add(
    native: &mut Reader,
    graph: &mut Graph,
    volume: u32,
    color: [f32; 3],
) -> Result<(String, Value)> {
    let donor = Volume::read(native, volume)?;
    let clear = Volume::read(native, CLEAR_VOLUME)
        .ok()
        .filter(|clear| donor.same_shape(clear));
    let (texel_source, mut data) = match clear {
        Some(clear) => (CLEAR_VOLUME, clear.data),
        None => (volume, donor.data.clone()),
    };
    let peak = color.iter().copied().fold(0f32, f32::max);
    ensure!(
        color.iter().all(|c| c.is_finite() && *c >= 0.) && peak > 0.,
        "The level's light color is not a color"
    );
    let hue = color.map(|c| c / peak);
    let hue_luminance = hue.iter().zip(LUMINANCE).map(|(c, w)| c * w).sum::<f32>();
    let mut before = [0f64; 4];
    let mut after = [0f64; 4];
    for texel in data.chunks_exact_mut(TEXEL_BYTES) {
        let rgba: [f32; 4] = std::array::from_fn(|k| {
            half::f16::from_le_bytes([texel[k * 2], texel[k * 2 + 1]]).to_f32()
        });
        let luminance = rgba[..3]
            .iter()
            .zip(LUMINANCE)
            .map(|(c, w)| c * w)
            .sum::<f32>();
        let [r, g, b] = hue.map(|c| c * luminance / hue_luminance);
        for (k, value) in [r, g, b, rgba[3]].into_iter().enumerate() {
            texel[k * 2..k * 2 + 2].copy_from_slice(&half::f16::from_f32(value).to_le_bytes());
            before[k] += f64::from(rgba[k]);
            after[k] += f64::from(value);
        }
    }
    let symbol = "atmosphere-volume".to_owned();
    let data_symbol = format!("{symbol}-data");
    graph.add(
        &symbol,
        volume,
        &donor.header,
        Some(&data_symbol),
        Vec::new(),
    )?;
    graph.add(&data_symbol, donor.source, &data, Some(&symbol), Vec::new())?;
    let mean = |sums: [f64; 4]| sums.map(|s| s / donor.texels as f64);
    Ok((
        symbol,
        json!({
            "donor_volume": format!("{volume:08X}"),
            "texel_source": format!("{texel_source:08X}"),
            "texels": donor.texels,
            "color": hue,
            "source_mean": mean(before),
            "mean": mean(after),
        }),
    ))
}

/// Adds a black copy of the donor atmosphere's 2D lookup `lookup` (`+0x98`). Destiny's deferred
/// program (`80B35738`) fogs a pixel by `exp(-depth * cb0[42])` and adds a sky lookup times
/// `1 - exp(-depth)`, where the optical depth is a 2D lookup's color, sampled at a linear
/// function of the pixel's distance, times the atmosphere's height-integrated density. Distant
/// Shore's `+0x98` lookup, 16 by 1,024, rises from black to white down its rows. That it is
/// the depth lookup the program samples, directly or through a generated copy, is an inference.
/// Black, it would hold no fog, and the level's surfaces draw their own. Returns the copy's symbol
/// and a report.
pub(super) fn clear_lookup(
    native: &mut Reader,
    graph: &mut Graph,
    lookup: u32,
) -> Result<(String, Value)> {
    let header = native.tag(lookup, None)?.0.clone();
    let marker = header
        .get(MARKER..MARKER + 2)
        .context("Atmosphere lookup header boundary")?;
    let bytes = header
        .get(BYTES..BYTES + 4)
        .context("Atmosphere lookup header boundary")?;
    let bytes = usize::try_from(u32::from_le_bytes(bytes.try_into()?))?;
    ensure!(
        header.len() == HEADER_SIZE && marker == 0xCAFEu16.to_le_bytes(),
        "The atmosphere lookup {lookup:08X} is not a texture"
    );
    let source = native.reference(lookup)?;
    let data = native.tag(source, None)?.0.clone();
    ensure!(
        data.len() == bytes,
        "The atmosphere lookup {lookup:08X} holds {} bytes, its header {bytes}",
        data.len()
    );
    let symbol = "atmosphere-lookup".to_owned();
    let data_symbol = format!("{symbol}-data");
    graph.add(&symbol, lookup, &header, Some(&data_symbol), Vec::new())?;
    graph.add(
        &data_symbol,
        source,
        &vec![0; data.len()],
        Some(&symbol),
        Vec::new(),
    )?;
    Ok((
        symbol,
        json!({
            "donor_lookup": format!("{lookup:08X}"),
            "bytes": data.len(),
            "source_nonzero": data.iter().filter(|&&b| b != 0).count(),
        }),
    ))
}
