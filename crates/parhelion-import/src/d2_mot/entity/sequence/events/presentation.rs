//! Presentation events retain explicit dependencies and authored transforms.
use super::*;

pub(super) fn write(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    kind: u32,
) -> Result<()> {
    let (sd, nd) = match kind {
        5 => {
            let resource = link(bindings, from + 0x24, 0x80806CEF, 0x808071D9)?;
            put(output, to + 0x40, &resource.to_le_bytes())?;
            put(output, to + 0x44, &source.bytes::<4>(from + 0x28)?)?;
            ensure!(
                source.u32(from + 0x2C)? == 0 && source.bytes::<24>(from + 0x38)? == [0; 24],
                "presentation event selector needs translation"
            );
            put(output, to + 0x48, &source.bytes::<8>(from + 0x30)?)?;
            (0x50, 0x68)
        }
        23 => {
            let resource = link(bindings, from + 0x28, 0x80806BB7, 0x8080707E)?;
            ensure!(
                source.u32(from + 0x2C)? == 0,
                "presentation event padding differs"
            );
            put(output, to + 0x40, &source.bytes::<4>(from + 0x24)?)?;
            put(output, to + 0x44, &resource.to_le_bytes())?;
            (0x30, 0x48)
        }
        _ => anyhow::bail!("unsupported presentation sequence event {kind}"),
    };
    expression::write(source, from + sd, output, to + nd)?;
    ensure!(
        source.u64(from + sd + 0x40)? == 0 && source.u64(from + sd + 0x88)? == 0,
        "presentation rotation modes require translation"
    );
    expression::write(source, from + sd + 0x48, output, to + nd + 0x48)?;
    expression::write(source, from + sd + 0x90, output, to + nd + 0x90)?;
    Ok(())
}
