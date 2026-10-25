//! Health threshold responses retain each private graph and its activation flags.
use super::*;

fn actions(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
) -> Result<()> {
    let rows = source.array(from, 8, Some(0x808040A3))?;
    ensure!(
        rows.len() <= 256 && (!rows.is_empty() || source.u64(from + 8)? == 0),
        "health response action list extent differs"
    );
    append_array(&mut output.0, to, 0x80804C2C, &vec![0; rows.len() * 8], 8)?;
    let targets = output.array(to, 8, Some(0x80804C2C))?;
    for (&row, &field) in rows.iter().zip(&targets) {
        let from = source.pointer(row)?;
        let class = source.u32(from.checked_sub(4).context("health response prefix")?)?;
        let (native, size) = match class {
            0x808040AC => (0x80804C33, 32),
            0x808040AD => (0x80804C34, 64),
            class => anyhow::bail!("health response action {class:08X} requires translation"),
        };
        ensure!(
            source.u32(from)? == 0 && source.u32(from + 36)? == 0,
            "health response action extension differs"
        );
        if class == 0x808040AD {
            ensure!(
                source.u64(from + 40)? == 0
                    && source.u64(from + 48)? <= 1
                    && source.bytes::<16>(from + 56)? == [0; 16],
                "health response launch parameters require translation"
            );
        }
        let target = typed(output, native, size)?;
        put(&mut output.0, target, &source.bytes::<8>(from)?)?;
        string(source, from + 8, output, target + 8)?;
        resource(
            source,
            from + 16,
            output,
            target + 16,
            (0x80809AD8, 0x80809C0F),
            bindings,
        )?;
        put(&mut output.0, target + 24, &source.bytes::<8>(from + 32)?)?;
        if class == 0x808040AD {
            put(&mut output.0, target + 32, &source.bytes::<16>(from + 40)?)?;
        }
        relative(output, field, target)?;
    }
    Ok(())
}

pub(super) fn emit(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
) -> Result<usize> {
    let rows = source.array(from, 208, Some(0x80804063))?;
    ensure!(
        rows.len() <= 4096 && (!rows.is_empty() || source.u64(from + 8)? == 0),
        "health response table extent differs"
    );
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        to,
        0x80804BB9,
        &vec![0; rows.len() * 200],
        200,
    )?;
    let targets = candidate.array(to, 200, Some(0x80804BB9))?;
    for (&row, &target) in rows.iter().zip(&targets) {
        ensure!(
            source.f32(row + 4)?.is_finite()
                && source.f32(row + 8)?.is_finite()
                && source.u8(row + 12)? <= 3
                && source.bytes::<3>(row + 13)? == [0; 3]
                && source.u64(row + 16)? == 0
                && source.u64(row + 24)? == 0,
            "health response threshold or conditional extension differs"
        );
        put(&mut candidate.0, target, &source.bytes::<24>(row)?)?;
        filter::damage(
            source,
            row + 32,
            &mut candidate,
            target + 32,
            &bindings.categories,
        )?;
        filter::ability(
            source,
            row + 0x98,
            &mut candidate,
            target + 0x90,
            &bindings.categories,
        )?;
        actions(source, row + 0xB8, &mut candidate, target + 0xB0, bindings)?;
        put(
            &mut candidate.0,
            target + 0xC0,
            &source.bytes::<8>(row + 0xC8)?,
        )?;
    }
    *output = candidate;
    Ok(rows.len())
}
