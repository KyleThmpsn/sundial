//! Construction and initializer ownership for a reset-owned zero Timer.
use super::*;

pub(super) fn source(p: &Payload, live: usize) -> Result<()> {
    let rows = p.array(0x30, 16, Some(0x80800930))?;
    ensure!(
        rows.len() == 2,
        "Source reset-owned status has additional lifecycle methods"
    );
    for (&row, (target, class, methods)) in rows.iter().zip([
        (live + 0x50, 0x8080255D, 0x00030001),
        (live, 0x80802A75, 0x0004FFFF),
    ]) {
        ensure!(
            p.pointer(row)? == target && p.u32(row + 8)? == class && p.u32(row + 12)? == methods,
            "Source reset-owned status lifecycle differs"
        );
    }
    ensure!(
        p.u8(live + 0xD8)? == 1,
        "Source reset-owned Timer initial activation differs"
    );
    Ok(())
}

pub(super) fn native(
    native: &mut Reader,
    graph: &Payload,
    p: &Payload,
    owner: u32,
    definition: usize,
) -> Result<()> {
    let duration = usize::try_from(p.u64(definition + 8)?)?;
    let timer = duration + 0x30;
    let timer_definition = definition + 0xB0;
    let mut constructors = Vec::new();
    for row in p.array(0x30, 16, Some(0x808091A4))? {
        if p.u16(row + 12)? != u16::MAX {
            constructors.push((p.pointer(row)?, p.u32(row + 8)?, p.u32(row + 12)?));
        }
    }
    ensure!(
        constructors == [(duration, 0x80803B32, 0x00020000)]
            && p.u32(timer)? == owner
            && p.u32(timer + 4)? == 0x80803C51
            && p.u64(timer + 8)? == timer_definition as u64
            && p.u32(timer_definition)? == owner
            && p.u32(timer_definition + 4)? == 0x80803C50
            && p.u64(timer_definition + 8)? == timer as u64
            && p.bytes::<3>(timer + 0xB1)? == [1, 0, 0]
            && p.u32(timer + 0x88)? == u32::MAX,
        "Native reset-owned status lifecycle or Timer state differs"
    );
    let mut initializers = Vec::new();
    for row in graph.array(0x58, 40, Some(0x80809C22))? {
        let class = graph.u32(row + 12)?;
        if !matches!(class, 0x80803B32 | 0x80803C50) {
            continue;
        }
        let declaration = graph.u32(row + 32)?;
        let provider = native.tag(declaration, Some(0x80809C54))?;
        if provider.u32(12)? != 0x80809ADD {
            continue;
        }
        let methods = provider.array(16, 24, Some(0x80809C56))?;
        let descriptor = usize::try_from(graph.u64(row + 24)?)?;
        ensure!(
            methods.len() == 1
                && provider.u32(methods[0])? == class
                && provider.u32(methods[0] + 4)? == 1
                && graph.u32(row + 16)? == owner
                && p.u32(descriptor + 8)? == declaration,
            "Native reset-owned status initializer differs"
        );
        initializers.push((class, p.pointer(descriptor)?));
    }
    ensure!(
        initializers == [(0x80803B32, definition), (0x80803C50, timer_definition)],
        "Native reset-owned status initializer order differs"
    );
    Ok(())
}
