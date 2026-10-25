//! Root health settings whose nested records are emitted by the owning helpers.
use super::*;

fn empty_reference(source: &Payload, at: usize) -> Result<()> {
    ensure!(
        source.u32(at)? == u32::MAX
            && matches!(source.u32(at + 4)?, 1 | 2)
            && source.u64(at + 8)? == 0,
        "health extension resource requires translation"
    );
    Ok(())
}

pub(super) fn emit(source: &Payload, output: &mut Payload, bindings: &Bindings) -> Result<()> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    ensure!(
        source.bytes::<32>(si + 16)? == output.bytes::<32>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == output.bytes::<56>(nd + 16)?,
        "health root base defaults differ"
    );
    // The first eleven input definitions keep their order. Only literal values
    // are admitted here, so a source pointer cannot escape into the native tag.
    for index in 0..14 {
        let at = sd + 0x48 + index * 32;
        ensure!(
            source.bytes::<12>(at)? == [0; 12]
                && source.bytes::<16>(at + 16)? == [0; 16]
                && source.f32(at + 12)?.is_finite(),
            "health stat needs an expression or provider"
        );
        let runtime = si + 0x30 + index * 56;
        ensure!(
            source.bytes::<16>(runtime)? == [0; 16]
                && source.u64(runtime + 16)? == u32::MAX as u64
                && source.u64(runtime + 24)? == u64::MAX
                && source.u64(runtime + 32)? == 0
                && source.u64(runtime + 40)? == u64::MAX
                && source.u64(runtime + 48)? == 0,
            "health stat input contains active state"
        );
        if index < 11 {
            put(
                &mut output.0,
                nd + 0x48 + index * 32,
                &source.bytes::<32>(at)?,
            )?;
        } else {
            ensure!(
                source.f32(at + 12)? == if index == 13 { 1.0 } else { 0.0 },
                "additional health stat requires translation"
            );
        }
    }
    // Both initializers use this capacity when the selected configuration has
    // no positive value. It is separate from the serialized runtime capacity.
    let capacity = source.f32(sd + 0x208)?;
    ensure!(
        capacity.is_finite() && capacity >= 0.0 && source.u32(sd + 0x20C)? == 0,
        "health fallback capacity or adjacent padding differs"
    );
    put(&mut output.0, nd + 0x1A8, &capacity.to_le_bytes())?;
    put(&mut output.0, nd + 0x1AC, &0u32.to_le_bytes())?;
    for (from, to) in [
        (0x220, 0x1C0),
        (0x3E0, 0x338),
        (0x420, 0x378),
        (0x430, 0x388),
        (0x450, 0x3A8),
        (0x668, 0x530),
        (0x678, 0x540),
    ] {
        ensure!(
            source.bytes::<16>(sd + from)? == [0; 16],
            "health root array at {from:X} requires translation"
        );
        put(&mut output.0, nd + to, &[0; 16])?;
    }
    for (from, to, sc, nc, stride) in [
        (0x3D0, 0x328, 0x8080401C, 0x80804BA9, 12),
        (0x3F0, 0x348, 0x80804002, 0x80804B90, 4),
    ] {
        copy_array(source, output, sd + from, nd + to, sc, nc, stride)?;
    }
    string(source, sd + 0x240, output, nd + 0x1E0)?;
    resource(
        source,
        sd + 0x248,
        output,
        nd + 0x1E8,
        (0x8080873D, 0x80808BCB),
        bindings,
    )?;
    put(&mut output.0, nd + 0x1F0, &source.bytes::<8>(sd + 0x258)?)?;
    put(&mut output.0, nd + 0x3B8, &source.bytes::<40>(sd + 0x460)?)?;
    ensure!(
        source.u64(sd + 0x608)? == 0
            && source.u64(sd + 0x610)? == 0
            && source.u64(sd + 0x618)? == 255
            && source.bytes::<16>(sd + 0x620)? == [0; 16]
            && source.u64(sd + 0x640)? == 0,
        "health additional target filter requires translation"
    );
    put(&mut output.0, nd + 0x500, &0u32.to_le_bytes())?;
    put(&mut output.0, nd + 0x504, &source.bytes::<16>(sd + 0x630)?)?;
    string(source, sd + 0x648, output, nd + 0x518)?;
    empty_reference(source, sd + 0x650)?;
    put(
        &mut output.0,
        nd + 0x520,
        &u64::from(u32::MAX).to_le_bytes(),
    )?;
    put(&mut output.0, nd + 0x528, &source.bytes::<8>(sd + 0x660)?)?;
    put(&mut output.0, nd + 0x550, &source.bytes::<8>(sd + 0x688)?)?;
    for (from, to) in [(0x690, 0x558), (0x6A8, 0x568)] {
        string(source, sd + from, output, nd + to)?;
        empty_reference(source, sd + from + 8)?;
        put(
            &mut output.0,
            nd + to + 8,
            &u64::from(u32::MAX).to_le_bytes(),
        )?;
    }
    ensure!(
        source.u64(sd + 0x6C0)? == 0 && source.u64(sd + 0x6D0)? == 0,
        "health root action extension requires translation"
    );
    put(&mut output.0, nd + 0x578, &0u64.to_le_bytes())?;
    put(&mut output.0, nd + 0x580, &source.bytes::<8>(sd + 0x6C8)?)?;
    ensure!(
        source.u64(sd + 0x710)? == 0 && source.u32(sd + 0x71C)? == 0,
        "health root additional state requires translation"
    );
    put(&mut output.0, nd + 0x5A8, &[0; 8])?;
    put(&mut output.0, nd + 0x5B4, &0u32.to_le_bytes())?;
    string(source, sd + 0x720, output, nd + 0x5B8)?;
    resource(
        source,
        sd + 0x728,
        output,
        nd + 0x5C0,
        (
            crate::d2_mot::native::categories::MODERN_CLASS,
            crate::d2_mot::native::categories::NATIVE_CLASS,
        ),
        bindings,
    )?;
    // These ranges are inactive caches and empty runtime arrays, not settings.
    for (from, to, length) in [
        (0x4E8, 0x378, 48),
        (0x518, 0x3A8, 440),
        (0x8E0, 0x6F0, 24),
        (0x910, 0x718, 80),
    ] {
        ensure!(
            source
                .0
                .get(si + from..si + from + length)
                .context("health runtime extent")?
                == output
                    .0
                    .get(ni + to..ni + to + length)
                    .context("native health runtime extent")?,
            "health root runtime defaults differ at {from:X}"
        );
    }
    ensure!(
        source.u64(si + 0x4C0)? == 0 && source.u64(si + 0x8F8)? == u32::MAX as u64,
        "health root runtime extension is initialized"
    );
    Ok(())
}
