//! Follow stored feedback into HUD reports and private behavior after package staging.
use super::*;

fn kinds(p: &Payload, at: usize, found: &mut Vec<u8>) -> CheckResult<()> {
    let kind = p.u8(at + 5)?;
    found.push(kind);
    match kind {
        32 => {
            for row in p.array(at + 8, 32, Some(0x80803181))? {
                for child in p.array(row + 8, 8, None)? {
                    kinds(p, p.pointer(child)?, found)?;
                }
            }
        }
        36 => kinds(p, p.pointer(at + 0xD0)?, found)?,
        26 => {
            for row in p.array(at + 8, 32, Some(0x8080316C))? {
                kinds(p, p.pointer(row)?, found)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    native: &Payload,
) -> CheckResult<()> {
    let mut conditions = Vec::new();
    for descriptor in [0x20, 0x48, 0x58] {
        for row in source.array(descriptor, 8, None)? {
            kinds(source, source.pointer(row)?, &mut conditions)?;
        }
    }
    let removal = source.array(0x48, 8, None)?;
    let timed = removal.len() == 1
        && source.u8(source.pointer(removal[0])? + 5)? == 1
        && conditions.iter().filter(|&&kind| kind == 1).count() == 1
        && !conditions.contains(&26);
    // Follow original nested records across every phase. Attached Duration state
    // is separate from the action's linked countdowns selected by the reporters.
    let timerless = !conditions.contains(&1) && !conditions.contains(&26);
    let mut reports = Vec::new();
    let mut required_graphs = Vec::new();
    for row in source.array(0x10, 8, None)? {
        let at = source.pointer(row)?;
        if source.u32(at - 4)? == 0x80802908 && source.u32(at + 16)? != u32::MAX {
            let graph = reader.read(source.u32(at + 16)?)?;
            for row in graph.array(0x20, 12, Some(0x80800715))? {
                let owner = reader.read(graph.u32(row)?)?;
                if owner.u32(owner.pointer(24)? - 4)? == 0x808020DE {
                    required_graphs.push(at);
                    break;
                }
            }
        }
        if source.u32(at - 4)? == 0x8080314C
            && source.u8(at)? & !15 == 0
            && source.u8(at + 1)? == 255
            && source.u16(at + 2)? == 0
            && source.u8(at + 12)? <= 1
            && source.u8(at + 13)? <= 1
            && source.u8(at + 14)? == 255
            && source.u8(at + 15)? == 0
            && (source.u8(at + 13)? == 0 || timerless || (timed && source.u8(at)? == 2))
        {
            reports.push(at);
        }
    }
    let mut actual = Vec::new();
    let mut graphs = Vec::new();
    for row in native.array(0x10, 8, Some(0x80804083))? {
        let at = native.pointer(row)?;
        if native.u32(at - 4)? == 0x80804087 {
            graphs.push(at);
        } else {
            actual.push(row);
        }
    }
    assert_eq!(
        required_graphs.len(),
        graphs.len(),
        "A feedback modifier graph must survive as private behavior"
    );
    for (s, n) in required_graphs.into_iter().zip(graphs) {
        assert_eq!(source.bytes::<3>(s)?, native.bytes::<3>(n)?);
        pattern::graph(
            reader,
            stock,
            emitted,
            source.u32(s + 16)?,
            native.u32(n + 16)?,
        )?;
    }
    assert_eq!(
        reports.len(),
        actual.len(),
        "A qualified HUD report was lost"
    );
    for (s, row) in reports.into_iter().zip(actual) {
        let n = native.pointer(row)?;
        assert_eq!(native.u32(n - 4)?, 0x80804085);
        assert_eq!(source.bytes::<4>(s)?, native.bytes::<4>(n)?);
        assert_eq!(source.u32(s + 8)?, native.u32(n + 8)?);
        assert_eq!(native.bytes::<8>(n + 12)?, [0; 8]);
        assert_eq!(native.u8(n + 20)?, source.u8(s + 14)?);
        assert_eq!(native.u8(n + 21)?, source.u8(s + 12)?);
        assert_eq!(native.u8(n + 22)?, source.u8(s + 13)?);
        assert_eq!(native.u8(n + 23)?, 0);
        if source.u32(s + 4)? == 0x811C9DC5 {
            assert_eq!(native.u32(n + 4)?, 0x811C9DC5);
        } else {
            hud::check(
                reader,
                stock,
                emitted,
                source.u32(s + 4)?,
                native.u32(n + 4)?,
            )?;
        }
    }
    Ok(())
}
