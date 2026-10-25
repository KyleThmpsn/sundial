//! A partial bridge onto an intact native projectile. Unconverted behavior is
//! deliberately retained from the native graph, not reported as source import.
use super::*;

/// Transfer the corpus-validated ballistic fields without moving native records,
/// references or allocation metadata. Other movement behavior stays native.
pub fn ballistics(source: &Payload, template: &Payload) -> Result<Payload> {
    let tree = Source::read(source)?;
    ensure!(
        template.u64(0)? == template.0.len() as u64,
        "native movement size differs"
    );
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    ensure!(
        template.u32(ni + 4)? == 0x8080388F
            && template.u32(nd + 4)? == 0x80803B73
            && template.u32(ni)? == template.u32(nd)?
            && template.u64(ni + 8)? == nd as u64
            && template.u64(nd + 8)? == ni as u64,
        "native movement root or twins differ"
    );
    let sd = tree.definition;
    ensure!(
        source.u64(tree.instance + 8)? == sd as u64
            && source.u64(sd + 8)? == tree.instance as u64
            && source.u32(sd)? == source.u32(tree.instance)?,
        "source movement root twins differ"
    );
    let mut output = template.clone();
    for target in [
        0x5C, 0x60, 0x68, 0x74, 0x88, 0x8C, 0x90, 0xA0, 0xA4, 0xA8, 0xB0, 0xB4, 0xD8, 0xDC,
    ] {
        let from = ROOT_DEFINITION
            .iter()
            .find(|(to, _)| *to == target)
            .context("ballistic field has no validated mapping")?
            .1;
        let bits = source.u32(sd + from)?;
        let value = f32::from_bits(bits);
        ensure!(
            value.is_finite(),
            "nonfinite source ballistic field {from:X}"
        );
        ensure!(
            !matches!(target, 0x5C | 0x60 | 0x68 | 0x74 | 0xD8 | 0xDC) || value >= 0.0,
            "negative source ballistic field {from:X}"
        );
        ensure!(
            target != 0x88 || value > 0.0,
            "source speed multiplier is not positive"
        );
        put(&mut output.0, nd + target, &bits.to_le_bytes())?;
        // These launch values reset from the matching definition fields.
        let runtime = match target {
            0x88 => Some(0x144),
            0xD8 => Some(0x148),
            0x74 => Some(0x168),
            _ => None,
        };
        if let Some(runtime) = runtime {
            ensure!(
                ni + runtime + 4 <= nd,
                "native ballistic state crosses definition"
            );
            put(&mut output.0, ni + runtime, &bits.to_le_bytes())?;
        }
    }
    distance_curve(source, sd, &mut output, ni, nd)?;
    Ok(output)
}

/// The definition's distance curve at +C8 moves speed and gravity toward its endpoints
/// over travel distance, and reset copies it into instance +14C..+15C. The source names
/// the same four values at +170, or nothing when its speed and gravity hold. A curve kept
/// from the native donor would slow and drop a source bolt that flies straight.
///
/// The record stays even when the source has none. Clearing +C8 on the Skyburner's Oath
/// skeleton left the bolt motionless in game on October 1, so a source without a curve
/// gets an inert one instead: endpoints equal to the bolt's own speed and gravity, and a
/// span that starts at the travel distance limit, where the bolt ends.
fn distance_curve(
    source: &Payload,
    sd: usize,
    output: &mut Payload,
    ni: usize,
    nd: usize,
) -> Result<()> {
    ensure!(ni + 0x160 <= nd, "native curve state crosses definition");
    ensure!(
        output.u64(nd + 0xC8)? != 0,
        "native movement has no distance curve record"
    );
    let to = output.pointer(nd + 0xC8)?;
    ensure!(
        to >= 4 && output.u32(to - 4)? == 0x80803803,
        "native distance curve class differs"
    );
    let values = if source.u64(sd + 0x170)? == 0 {
        let speed = f32::from_bits(output.u32(nd + 0x88)?);
        let gravity = f32::from_bits(output.u32(nd + 0xD8)?);
        let limit = f32::from_bits(output.u32(nd + 0x74)?);
        let start = if limit > 0.0 { limit } else { 1.0e6 };
        [speed, gravity, start, start * 2.0]
    } else {
        let from = source.pointer(sd + 0x170)?;
        ensure!(
            from >= 4 && source.u32(from - 4)? == 0x80802A08,
            "source distance curve class differs"
        );
        let mut values = [0.0f32; 4];
        for (i, value) in values.iter_mut().enumerate() {
            *value = f32::from_bits(source.u32(from + i * 4)?);
        }
        values
    };
    let [speed, gravity, start, end] = values;
    ensure!(
        values.iter().all(|v| v.is_finite())
            && speed >= 0.0
            && (0.0..=10.0).contains(&gravity)
            && start >= 0.0
            && end > start,
        "invalid distance curve"
    );
    for (i, value) in values.iter().enumerate() {
        put(&mut output.0, to + i * 4, &value.to_le_bytes())?;
    }
    let lanes = [speed, gravity, start, end, 1.0 / (end - start)];
    for (i, value) in lanes.iter().enumerate() {
        put(&mut output.0, ni + 0x14C + i * 4, &value.to_le_bytes())?;
    }
    Ok(())
}
