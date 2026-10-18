//! Three retained object inputs and one explicitly unbound modern extension.
use super::*;

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
) -> Result<(Vec<Relocation>, Vec<Object>)> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let mut objects = Vec::new();
    let mut unbound = Vec::new();
    for index in 0..4 {
        let from = si + 0x6D0 + index * 128;
        let definition = sd + 0x488 + index * 96;
        check_pair(source, from, definition, 0x808093A3, 0x808093A4, si)?;
        check_pair(
            source,
            from + 32,
            definition + 16,
            0x80809A9C,
            0x80809A9D,
            from,
        )?;
        ensure!(
            source.u64(from + 24)? == 0
                && source.u64(from + 56)? == 0
                && source.u64(from + 64)? == u64::MAX
                && source.u64(from + 72)? == 0
                && source.u64(from + 80)? == u32::MAX as u64
                && source.u64(from + 88)? == u64::MAX
                && source.bytes::<16>(from + 96)? == [0; 16]
                && source.pointer(from + 112)? == si
                && source.u64(from + 120)? == 0,
            "health object input has active state"
        );
        ensure!(
            source.u64(definition + 32)? == 2
                && source.u64(definition + 40)? == 0x80809AA8
                && source.u64(definition + 48)? == 0xFFFFFFFF00000000
                && source.u64(definition + 56)? == 0
                && source.u64(definition + 72)? == 1
                && source.bytes::<16>(definition + 80)? == [0; 16],
            "health object input contract differs"
        );
        let source_class = [0x80803FE5, 0x80803FE4, 0x80803FE3, 0x80809AC9][index];
        ensure!(
            source.u32(definition + 64)? == source_class,
            "health object input selector class differs"
        );
        if index == 3 {
            for (offset, class) in [
                (from, 0x808093A3),
                (definition, 0x808093A4),
                (from + 32, 0x80809A9C),
                (definition + 16, 0x80809A9D),
            ] {
                unbound.push(Object {
                    owner: source.u32(si)?,
                    class,
                    offset: offset as u64,
                });
            }
            continue;
        }
        let to = ni + 0x560 + index * 128;
        let target_definition = nd + 0x3E0 + index * 96;
        ensure!(
            output.u64(target_definition + 40)? == 0x80809BE2
                && output.u32(target_definition + 64)?
                    == [0x80804B74, 0x80804B73, 0x80804B72][index],
            "native health object input contract differs"
        );
        pair(output, to, target_definition, 0x80809663, 0x80809664, ni)?;
        pair(
            output,
            to + 32,
            target_definition + 16,
            0x80809BD6,
            0x80809BD7,
            to,
        )?;
        relative(output, to + 112, ni)?;
        put(
            &mut output.0,
            target_definition + 68,
            &source.bytes::<4>(definition + 68)?,
        )?;
        for (s, sc, n, nc) in [
            (from, 0x808093A3, to, 0x80809663),
            (definition, 0x808093A4, target_definition, 0x80809664),
            (from + 32, 0x80809A9C, to + 32, 0x80809BD6),
            (
                definition + 16,
                0x80809A9D,
                target_definition + 16,
                0x80809BD7,
            ),
        ] {
            objects.push(object(source, output, s, sc, n, nc)?);
        }
    }
    Ok((objects, unbound))
}
