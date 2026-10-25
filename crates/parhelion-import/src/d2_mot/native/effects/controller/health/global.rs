//! Component-wide health properties and their graph dependencies.
use super::*;

pub(super) fn emit(source: &Payload, output: &mut Payload, bindings: &Bindings) -> Result<()> {
    let sd = source.pointer(24)?;
    let nd = output.pointer(24)?;
    let rows = source.array(sd + 0x230, 8, Some(0x8080403B))?;
    ensure!(
        rows.len() <= 256,
        "health global property count exceeds limit"
    );
    let mut targets = Vec::new();
    for row in rows {
        let from = source.pointer(row)?;
        let class = source.u32(
            from.checked_sub(4)
                .context("health global property prefix")?,
        )?;
        let (native, size) = match class {
            0x80804044 => (0x80804C1B, 64),
            0x8080404B => (0x80804C22, 24),
            0x8080404D => (0x80804C24, 4),
            0x80804043 => (0x80804C1A, 16),
            0x8080404C => (0x80804C23, 4),
            class => anyhow::bail!("health global property {class:08X} requires translation"),
        };
        let to = typed(output, native, size)?;
        if class == 0x80804043 {
            string(source, from, output, to)?;
            resource(
                source,
                from + 8,
                output,
                to + 8,
                (0x80809AD8, 0x80809C0F),
                bindings,
            )?;
        } else {
            if class == 0x80804044 {
                ensure!(
                    source.bytes::<24>(from + 40)? == [0; 24],
                    "health global property contains nested conditions"
                );
            }
            put(
                &mut output.0,
                to,
                source
                    .0
                    .get(from..from + size)
                    .context("health global property extent")?,
            )?;
        }
        targets.push(to);
    }
    append_array(
        &mut output.0,
        nd + 0x1D0,
        0x80804C12,
        &vec![0; targets.len() * 8],
        8,
    )?;
    for (field, target) in output
        .array(nd + 0x1D0, 8, Some(0x80804C12))?
        .into_iter()
        .zip(targets)
    {
        relative(output, field, target)?;
    }
    Ok(())
}
