//! Positioning fields and the automated path form validated by native banks.
use super::*;

fn number(bytes: &[u8]) -> f32 {
    f32::from_le_bytes(bytes.try_into().expect("four-byte path scalar"))
}

fn paths(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let start = r.at;
    ensure!(
        r.u8()? <= 3,
        "automated path mode has no native counterpart"
    );
    ensure!((r.u32()? as i32) >= 0, "negative automated path transition");
    let vertices = r.count()?;
    ensure!(vertices > 0, "automated path has no vertices");
    for _ in 0..vertices {
        let row = r.take(16)?;
        ensure!(
            row[..12].chunks_exact(4).all(|v| number(v).is_finite()),
            "nonfinite automated path vertex"
        );
        ensure!(
            i32::from_le_bytes(row[12..].try_into()?) >= 0,
            "negative automated path vertex duration"
        );
    }
    let items = r.count()?;
    ensure!(items > 0, "automated path has no playlist");
    for _ in 0..items {
        let offset = usize::try_from(r.u32()?)?;
        let count = usize::try_from(r.u32()?)?;
        ensure!(
            count > 0 && offset.checked_add(count).is_some_and(|end| end <= vertices),
            "automated playlist exceeds vertex table"
        );
    }
    for _ in 0..items {
        let row = r.take(12)?;
        ensure!(
            row.chunks_exact(4)
                .all(|v| number(v).is_finite() && number(v) >= 0.0),
            "invalid automated path randomization range"
        );
    }
    w.bytes.extend_from_slice(&r.bytes[start..r.at]);
    Ok(())
}

/// A native positioning type is binary. A modern continuous blend has no
/// equivalent, even when both authored curve endpoints happen to be binary.
pub(super) fn switch_curve(r: &mut Read<'_>, w: &mut Write, count: u16) -> Result<()> {
    ensure!(count > 0, "empty positioning switch curve");
    let mut previous = None;
    for index in 0..count {
        let row = r.take(12)?;
        let x = number(&row[..4]);
        let y = number(&row[4..8]);
        let interpolation = u32::from_le_bytes(row[8..].try_into()?);
        ensure!(
            x.is_finite()
                && previous.is_none_or(|p| x > p)
                && (y == 0.0 || y == 100.0)
                && (index + 1 == count || interpolation == 9),
            "continuous or invalid positioning blend curve"
        );
        previous = Some(x);
        w.bytes.extend_from_slice(&row[..4]);
        w.bytes.extend_from_slice(&(y / 100.0).to_le_bytes());
        w.bytes.extend_from_slice(&row[8..]);
    }
    Ok(())
}

pub(super) fn emit(
    r: &mut Read<'_>,
    w: &mut Write,
    attenuation: Option<u32>,
    position_switch: bool,
) -> Result<()> {
    let bits = r.u8()?;
    ensure!(
        !position_switch || bits == 3,
        "positioning switch requires direct spatial routing"
    );
    ensure!(bits & 0x90 == 0, "unsupported positioning flags {bits:02X}");
    if bits & 1 == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "inherited position overrides attenuation"
        );
        w.u8(0xC0);
        return Ok(());
    }
    let panner = (bits >> 2) & 3;
    ensure!(panner <= 1, "unsupported speaker panner {panner}");
    if bits & 2 == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "2D position has attenuation"
        );
        w.u8(0xC1 | if panner == 1 { 6 } else { 0 });
        return Ok(());
    }
    let spatial = r.u8()?;
    ensure!(
        !position_switch || spatial == 0x0A,
        "unvalidated positioning switch flags"
    );
    ensure!(
        spatial & 0x80 == 0,
        "diffraction needs a native implementation"
    );
    let mode = spatial & 3;
    ensure!(mode <= 2, "unknown spatialization mode");
    if bits & 0x60 != 0 {
        // Three independently exported native bank twins establish this full
        // flag combination and the unchanged path table. Other combinations
        // need their own evidence rather than a guessed legacy bit packing.
        ensure!(
            bits == 0x43 && spatial == 0x6A,
            "unvalidated automated positioning flags {bits:02X}/{spatial:02X}"
        );
        w.u8(0x79);
        w.u8(0);
        w.reference(attenuation.unwrap_or(0), Kind::Object);
        return paths(r, w);
    }
    if mode == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "nonspatial audio has attenuation"
        );
        w.u8(0xC7);
    } else {
        w.u8(if position_switch { 0xDB } else { 0xD9 });
        w.u8(1 | ((spatial & 0x70) >> 1));
        w.reference(
            if spatial & 8 != 0 {
                attenuation.unwrap_or(0)
            } else {
                0
            },
            Kind::Object,
        );
    }
    Ok(())
}
