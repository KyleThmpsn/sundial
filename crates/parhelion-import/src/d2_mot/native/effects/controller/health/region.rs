//! Default health-region configuration and bounded scalar folding.
use super::*;

fn literal(source: &Payload, at: usize) -> Result<f32> {
    ensure!(
        source.u64(at + 32)? == 1 && source.u64(at + 40)? == 1,
        "health-region scalar input or output count differs"
    );
    let code = source.array(at, 1, Some(0x80800009))?;
    let values = source.array(at + 16, 16, Some(0x80800090))?;
    ensure!(
        code.len() == 4
            && values.len() == 1
            && code
                .iter()
                .map(|&row| source.u8(row))
                .collect::<Result<Vec<_>>>()?
                == [0x42, 0, 0x4C, 0],
        "health-region scalar requires a runtime expression"
    );
    let value = source.f32(values[0])?;
    ensure!(
        value.is_finite()
            && (1..4).all(|lane| source.u32(values[0] + lane * 4).ok() == Some(value.to_bits())),
        "health-region scalar is not a finite broadcast constant"
    );
    Ok(value)
}

/// A default region has no conditional configuration. The source's first two
/// damage-history channels use the same accumulation routine as the native pair.
/// Additional source channels are never substituted for the native channels.
/// The containing health converter must translate every exposed provider and
/// refuse providers selecting either additional channel.
pub fn emit(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    ensure!(
        source.u32(from + 4)? == 0x808040E0 && output.u32(to + 4)? == 0x80804C5E,
        "health-region pair classes differ"
    );
    ensure!(
        source.bytes::<16>(from + 0xE8)? == [0; 16] && source.u32(from + 0x40)? == u32::MAX,
        "conditional health-region configuration requires translation"
    );
    ensure!(
        source.u32(from + 0x3C)? == 0
            && source.u32(from + 0x4C)? == 0
            && source.bytes::<24>(from + 0xB8)? == [0; 24]
            && source.u32(from + 0xD4)? == 0,
        "health-region extension requires translation"
    );
    ensure!(
        source.u32(from + 0x48)? & !0x1F == 0 && source.u32(from + 0x50)? == 0,
        "health-region flags or damage-history selector requires translation"
    );
    ensure!(
        source.u32(from + 0xD8)? == u32::MAX
            && source.u32(from + 0xDC)? == 1
            && source.u64(from + 0xE0)? == 0,
        "health-region recovery resource requires translation"
    );
    // Source 16D33D0 and native B87B60 consume consecutive Float32 delay/fade
    // pairs. The new fractional-damage histories have independent runtime state.
    ensure!(
        source.bytes::<16>(from + 0x18)? == source.bytes::<16>(from + 0x28)?,
        "health-region additional history configuration differs"
    );
    for field in [0x14, 0x18, 0x1C, 0x20, 0x24, 0x38, 0x44, 0x54, 0xD0] {
        let value = source.f32(from + field)?;
        ensure!(
            value.is_finite() && value >= 0.0,
            "invalid health-region value at {field:X}"
        );
    }
    ensure!(
        source.f32(from + 0x44)? <= 1.0,
        "health-region initial fraction exceeds one"
    );
    let delay = literal(source, from + 0x58)?;
    let alternate_delay = literal(source, from + 0x88)?;
    ensure!(
        delay >= 0.0 && alternate_delay >= 0.0,
        "negative health-region regeneration delay"
    );
    let mut candidate = output.clone();
    put(&mut candidate.0, to + 16, &[0; 64])?;
    for (source_field, native_field, length) in [
        (0x10, 0x10, 8),
        (0x44, 0x18, 4),
        (0x48, 0x1C, 4),
        (0x18, 0x20, 16),
        (0x50, 0x30, 8),
        (0xD0, 0x40, 4),
        (0xD8, 0x44, 4),
        (0x38, 0x48, 4),
    ] {
        put(
            &mut candidate.0,
            to + native_field,
            source
                .0
                .get(from + source_field..from + source_field + length)
                .context("health-region field outside source")?,
        )?;
    }
    put(&mut candidate.0, to + 0x38, &delay.to_le_bytes())?;
    put(&mut candidate.0, to + 0x3C, &alternate_delay.to_le_bytes())?;
    *output = candidate;
    Ok(())
}

/// The modern provider inserted two history channels before the existing dead
/// state. Mapping follows the paired getter dispatch, not matching enum numbers.
pub fn provider_kind(source: u8) -> Result<u8> {
    match source {
        0..=2 => Ok(source),
        5 => Ok(3),
        _ => anyhow::bail!("health output {source} has no native provider translation"),
    }
}

pub(super) struct Regions {
    pub count: usize,
    pub objects: Vec<Relocation>,
}

pub(super) fn arrays(source: &Payload, output: &mut Payload, sections: usize) -> Result<Regions> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let definitions = source.array(sd + 0x410, 248, Some(0x808040E1))?;
    let instances = source.array(si + 0x4B0, 160, Some(0x808040E0))?;
    ensure!(
        definitions.len() == instances.len(),
        "health region pair count differs"
    );
    let source_owner = source.u32(si)?;
    for (&d, &i) in definitions.iter().zip(&instances) {
        ensure!(
            source.u32(d)? == source_owner
                && source.u32(i)? == source_owner
                && source.u32(d + 4)? == 0x808040E0
                && source.u32(i + 4)? == 0x808040E1
                && source.u64(d + 8)? == i as u64
                && source.u64(i + 8)? == d as u64
                && source.pointer(i + 16)? == si
                && (source.u32(d + 16)? as usize) < sections,
            "health region pair, parent or section index differs"
        );
        ensure!(
            source.bytes::<104>(i + 24)? == [0; 104]
                && source.bytes::<16>(i + 0x80)? == [255; 16]
                && source.u32(i + 0x90)? == 1f32.to_bits()
                && source.u32(i + 0x94)? <= 1
                && source.u32(i + 0x9C)? == 0
                && source.u32(i + 0x98)? == source.u32(d + 0x44)?,
            "health region runtime is initialized or differs from its initial fraction"
        );
    }
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        nd + 0x368,
        0x80804C5F,
        &vec![0; definitions.len() * 80],
        80,
    )?;
    append_array(
        &mut candidate.0,
        ni + 0x340,
        0x80804C5E,
        &vec![0; definitions.len() * 112],
        112,
    )?;
    let targets = candidate.array(nd + 0x368, 80, Some(0x80804C5F))?;
    let runtimes = candidate.array(ni + 0x340, 112, Some(0x80804C5E))?;
    let mut objects = Vec::new();
    for (((&from, &source_i), &to), &native_i) in definitions
        .iter()
        .zip(&instances)
        .zip(&targets)
        .zip(&runtimes)
    {
        pair(&mut candidate, native_i, to, 0x80804C5E, 0x80804C5F, ni)?;
        put(&mut candidate.0, native_i + 0x50, &[255; 8])?;
        put(
            &mut candidate.0,
            native_i + 0x58,
            &source.bytes::<12>(source_i + 0x90)?,
        )?;
        emit(source, from, &mut candidate, to)?;
        objects.push(object(
            source, &candidate, source_i, 0x808040E0, native_i, 0x80804C5E,
        )?);
        objects.push(object(
            source, &candidate, from, 0x808040E1, to, 0x80804C5F,
        )?);
    }
    *output = candidate;
    Ok(Regions {
        count: definitions.len(),
        objects,
    })
}
