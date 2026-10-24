//! Scalar and four-lane channels with identical encoded samples in the two runtimes.
use super::*;

fn array(
    source: &Payload,
    native: &mut [u8],
    at: usize,
    stride: usize,
    classes: (u32, u32),
) -> Result<std::ops::Range<usize>> {
    let rows = source.array_range(at, stride, Some(classes.0))?;
    move_array(source, native, at, at + SHIFT, classes)?;
    Ok(rows)
}

fn uniform(
    source: &Payload,
    native: &mut [u8],
    at: usize,
    tracks: usize,
    width: usize,
    frames: usize,
) -> Result<()> {
    ensure!(
        source.u32(at + 12)? as usize == frames,
        "Source channel frame count differs"
    );
    let selectors = array(source, native, at + 16, 2, (0x80800F33, 0x80808F82))?;
    let samples = array(source, native, at + 32, 2, (0x8080000A, 0x8080000A))?;
    ensure!(
        selectors.len() == tracks * 2,
        "Source channel selector count differs"
    );
    for offset in [48, 64] {
        let values = array(source, native, at + offset, 4, (0x8080000F, 0x8080000F))?;
        ensure!(
            values.len() == tracks * width * 4,
            "Source channel interval count differs"
        );
        for row in values.step_by(4) {
            ensure!(
                source.f32(row)?.is_finite(),
                "Source channel interval is nonfinite"
            );
        }
    }
    for row in selectors.step_by(2) {
        let selector = source.u16(row)?;
        match selector >> 14 {
            1 => ensure!(
                (usize::from(selector & 0x3FFF) + 1) * frames * width * 2 <= samples.len(),
                "Source channel sampled selector exceeds its array"
            ),
            2 => {}
            _ => anyhow::bail!("Source channel has an unsupported selector"),
        }
    }
    Ok(())
}

fn curve(
    source: &Payload,
    native: &mut [u8],
    at: usize,
    tracks: usize,
    width: usize,
    frames: usize,
) -> Result<()> {
    let raw = array(source, native, at + 16, 2, (0x80800006, 0x80800006))?;
    let compressed = array(source, native, at + 32, 2, (0x80800006, 0x80800006))?;
    let times = array(source, native, at + 48, 1, (0x80800009, 0x80800009))?;
    let tangents = array(source, native, at + 64, 1, (0x80800009, 0x80800009))?;
    let mappings = array(source, native, at + 80, 2, (0x80800006, 0x80800006))?;
    ensure!(
        mappings.len() == (tracks + 1) * 2
            && usize::from(source.u16(mappings.end - 2)?) == compressed.len() / 2,
        "Source curve channel map count or compressed endpoint differs"
    );
    for row in (mappings.start..mappings.end - 2).step_by(2) {
        let index = source.i16(row)?;
        ensure!(index != 0, "Source curve channel has no samples");
        if index < 0 {
            let first = usize::from(index.unsigned_abs()) - 1;
            ensure!(
                (first + frames * width) * 2 <= raw.len(),
                "Source dense channel exceeds its samples"
            );
            continue;
        }
        let first = usize::try_from(index)? - 1;
        ensure!(
            (first + 3) * 2 <= compressed.len(),
            "Source curve header exceeds its array"
        );
        let start = compressed.start + first * 2;
        let time = usize::try_from(source.i16(start)?)?;
        let tangent = usize::try_from(source.i16(start + 2)?)?;
        let segments = usize::try_from(source.i16(start + 4)?)?;
        ensure!(
            segments > 0
                && segments < frames
                && time + segments <= times.len()
                && tangent + segments * width <= tangents.len()
                && (first + 3 + (segments + 1) * width) * 2 <= compressed.len(),
            "Source channel curve exceeds its arrays"
        );
        let deltas = &source.0[times.start + time..times.start + time + segments];
        ensure!(
            deltas.iter().all(|v| *v > 0)
                && deltas.iter().map(|v| usize::from(*v)).sum::<usize>() == frames - 1,
            "Source channel curve timing differs from its clip"
        );
    }
    Ok(())
}

pub(super) fn convert(source: &Payload, native: &mut [u8], frames: usize) -> Result<Vec<Value>> {
    let mut report = Vec::new();
    for (from, to, width) in [
        (0x20, 0x20, 1),
        (0x28, 0x28, 4),
        (0x38, 0x38, 1),
        (0x50, 0x58, 1),
    ] {
        if source.u64(from)? == 0 {
            continue;
        }
        let at = relocate(source, native, from, to)?;
        let class = source.u32(at - 4)?;
        let encoding = source.u16(at)?;
        let target = match (width, class, encoding) {
            (1, 0x80800770, 2) => 0x80808F7Cu32,
            (4, 0x8080055A, 2) => 0x80808F77,
            (1, 0x8080040F, 1) => 0x80808F7E,
            (4, 0x8080089E, 1) => 0x80808F78,
            _ => anyhow::bail!("Source channel codec {class:08X} at {from:X} has no converter"),
        };
        let tracks = usize::from(source.u16(at + 2)?);
        ensure!(
            tracks > 0 && tracks <= 255,
            "Source channel count exceeds its byte selector"
        );
        put(native, at + SHIFT - 4, &target.to_le_bytes());
        if encoding == 2 {
            uniform(source, native, at, tracks, width, frames)?;
        } else {
            curve(source, native, at, tracks, width, frames)?;
        }
        report.push(json!({"field":to,"source_field":from,"source_class":format!("{class:08X}"),
            "native_class":format!("{target:08X}"),"tracks":tracks,"width":width,"encoded_tracks_preserved":true}));
    }
    Ok(report)
}
