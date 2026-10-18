//! Sequence lifecycle and direct damage events.
use super::*;

pub(super) fn spawn_parameters(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    inputs: &BTreeMap<u32, u32>,
) -> Result<()> {
    let rows = source.array(from + 0xA0, 72, Some(0x8080849C))?;
    // Native 58AFD0 accepts one through 32 creation-context rows. A larger
    // declaration silently skips the entire parameter block in the engine.
    ensure!(
        rows.len() <= 32,
        "spawn parameter capacity exceeds the native limit of 32"
    );
    let start = array(output, to + 0x88, 0x8080889B, &vec![0; rows.len() * 72], 72)?;
    let mut names = std::collections::BTreeSet::new();
    for (index, row) in rows.into_iter().enumerate() {
        let name = source.u32(row + 64)?;
        ensure!(
            name != 0x811C9DC5 && names.insert(name) && source.u32(row + 68)? == 0,
            "spawn parameter name or padding differs"
        );
        let target = start + index * 72;
        expression::bound(source, row, output, target, 1, inputs)?;
        put(output, target + 64, &name.to_le_bytes())?;
    }
    Ok(())
}

pub(super) fn write(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    inputs: &BTreeMap<u32, u32>,
    kind: u32,
) -> Result<()> {
    match kind {
        2 => {
            // Modern 1690291 and native 40D97D select the output at +28
            // and +3C respectively. Native 58C960 indexes 64-byte runtime
            // rows, so preserve the ordinal rather than a template's zero.
            let index = source.u32(from + 0x28)?;
            let count = source.u64(source.pointer(24)? + 0x1F8)?;
            ensure!(
                source.u32(from + 0x2C)? == 0x811C9DC5
                    && u64::from(index) < count
                    && output.u64(output.pointer(24)? + 0x178)? == count,
                "sequence numeric event output is unbound"
            );
            put(output, to + 0x38, &source.bytes::<4>(from + 0x24)?)?;
            put(output, to + 0x3C, &index.to_le_bytes())?;
            expression::bound(source, from + 0x30, output, to + 0x40, 1, inputs)?;
        }
        3 => {
            ensure!(
                source.u32(from + 0x24)? == 0
                    && source.u32(from + 0x6C)? == 0x811C9DC5
                    && source.u64(from + 0x70)? == 0,
                "sequence channel-write extension differs"
            );
            expression::bound(source, from + 0x28, output, to + 0x38, 1, inputs)?;
            put(output, to + 0x78, &source.bytes::<4>(from + 0x68)?)?;
        }
        27 => {
            ensure!(
                source.u32(from + 0x24)? == 0
                    && source.u64(from + 0x28)? == 0
                    && source.u64(from + 0x80)? == 0,
                "direct damage event extension differs"
            );
            let dependency = link(bindings, from + 0x30, 0x80809B06, 0x80809C36)?;
            put(output, to + 0x40, &source.bytes::<8>(from + 0x28)?)?;
            put(output, to + 0x48, &dependency.to_le_bytes())?;
            for (sd, nd) in [
                (0x40, 0x50),
                (0x88, 0x90),
                (0xC8, 0xD0),
                (0x108, 0x110),
                (0x148, 0x150),
            ] {
                expression::bound(source, from + sd, output, to + nd, 1, inputs)?;
            }
            put(output, to + 0x190, &source.bytes::<8>(from + 0x188)?)?;
        }
        38 => {
            put(output, to + 0x40, &source.bytes::<4>(from + 0x24)?)?;
        }
        _ => anyhow::bail!("unsupported functional sequence event {kind}"),
    }
    Ok(())
}
