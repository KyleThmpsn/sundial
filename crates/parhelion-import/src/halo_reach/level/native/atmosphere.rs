//! The map's atmosphere lookup volume: the donor's, recolored to the level's own light. A map keeps
//! its donor's atmosphere component (`80807086`), whose volume texture at `+0x90` colors Destiny's
//! sun and sky light on players and other dynamic objects. Distant Shore's volume averages
//! (0.59, 1.86, 0.95), green at 3.2 times red, the greenest of the 31 Crucible maps' volumes, and
//! lit players green on Hang 'Em High. The copy keeps every texel's luminance and alpha and takes
//! the hue of the level's bright baked lobes ([`super::sky::baked_light_color`]), so its brightness
//! and structure stay the donor's.
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

/// Adds a copy of the donor's atmosphere volume `volume` whose texels take `color`'s hue at their
/// own luminance. Returns the copy's symbol and a report of both volumes' mean colors.
pub(super) fn add(
    native: &mut Reader,
    graph: &mut Graph,
    volume: u32,
    color: [f32; 3],
) -> Result<(String, Value)> {
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
        "The donor's atmosphere volume {volume:08X} is not a four half float texture"
    );
    let texels = half(DIMENSIONS)? * half(DIMENSIONS + 2)? * half(DIMENSIONS + 4)?;
    let source = native.reference(volume)?;
    let mut data = native.tag(source, None)?.0.clone();
    ensure!(
        texels > 0
            && data.len() == texels * TEXEL_BYTES
            && usize::try_from(word(BYTES)?)? == data.len(),
        "The donor's atmosphere volume {volume:08X} holds {} bytes for {texels} texels",
        data.len()
    );
    let peak = color.iter().copied().fold(0f32, f32::max);
    ensure!(
        color.iter().all(|c| c.is_finite() && *c >= 0.) && peak > 0.,
        "The level's light color is not a color"
    );
    let hue = color.map(|c| c / peak);
    let hue_luminance = hue.iter().zip(LUMINANCE).map(|(c, w)| c * w).sum::<f32>();
    let mut before = [0f64; 3];
    let mut after = [0f64; 3];
    for texel in data.chunks_exact_mut(TEXEL_BYTES) {
        let rgb: [f32; 3] = std::array::from_fn(|k| {
            half::f16::from_le_bytes([texel[k * 2], texel[k * 2 + 1]]).to_f32()
        });
        let luminance = rgb.iter().zip(LUMINANCE).map(|(c, w)| c * w).sum::<f32>();
        for (k, value) in hue
            .map(|c| c * luminance / hue_luminance)
            .into_iter()
            .enumerate()
        {
            texel[k * 2..k * 2 + 2].copy_from_slice(&half::f16::from_f32(value).to_le_bytes());
            before[k] += f64::from(rgb[k]);
            after[k] += f64::from(value);
        }
    }
    let symbol = "atmosphere-volume".to_owned();
    let data_symbol = format!("{symbol}-data");
    graph.add(&symbol, volume, &header, Some(&data_symbol), Vec::new())?;
    graph.add(&data_symbol, source, &data, Some(&symbol), Vec::new())?;
    let mean = |sums: [f64; 3]| sums.map(|s| s / texels as f64);
    Ok((
        symbol,
        json!({
            "donor_volume": format!("{volume:08X}"),
            "texels": texels,
            "color": hue,
            "donor_mean": mean(before),
            "mean": mean(after),
        }),
    ))
}
