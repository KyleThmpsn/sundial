//! Read grouped ability declarations and their actual flattened values independently.
use super::*;

pub(super) fn check(
    source: &Payload,
    row: usize,
    stock: &Payload,
    emitted: &Payload,
    native_row: usize,
) -> CheckResult<()> {
    scalar::stock_values(stock, emitted)?;
    let m = source.pointer(row + 32)?;
    let children = source.array(m, 8, Some(0x8080299D))?;
    let nm = emitted.pointer(native_row + 48)?;
    assert_eq!(emitted.u32(nm + 4)?, 0x8080447C);
    let target = emitted.array(nm + 16, 8, Some(0x80804485))?;
    assert_eq!(target.len(), children.len());
    let first = emitted.u32(nm + 32)? as usize;
    assert_eq!(emitted.u32(nm + 36)? as usize, children.len());
    let inputs = emitted.array(emitted.pointer(24)? + 0xA8, 48, Some(0x808045B7))?;
    let original_inputs = source.array(source.pointer(24)? + 0xC0, 56, Some(0x808023B9))?;
    let source_first = source.u32(m + 16)? as usize;
    assert_eq!(source.u32(m + 20)? as usize, children.len());
    for (index, (s, n)) in children.into_iter().zip(target).enumerate() {
        let s = source.pointer(s)?;
        let n = emitted.pointer(n)?;
        let (class, input, value) = match source.u32(s - 4)? {
            0x80802862 => (0x80804483, source.u16(s)?, source.f32(s + 4)?),
            0x808022D6 => (0x80804484, source.u16(s)?, source.f32(s + 4)?),
            0x80802C2D => {
                assert_eq!(source.u16(s)?, 0x120);
                (0x80804481, 32, 1.0 + source.f32(s + 4)?)
            }
            _ => return Err("Unqualified grouped ability declaration".into()),
        };
        assert_eq!(emitted.u32(n - 4)?, class);
        assert_eq!(emitted.u32(n)?, u32::from(input));
        assert_eq!(emitted.u32(n + 4)?, value.to_bits());
        let original = original_inputs[source_first + index];
        assert_eq!(source.u32(original + 48)?, source.u32(s + 4)?);
        assert_eq!(source.u32(original + 52)?, u32::from(source.u8(s + 1)?));
        let flat = inputs[first + index];
        assert_eq!(emitted.u32(flat + 40)?, value.to_bits());
        assert_eq!(emitted.u32(flat + 44)?, 0);
        let live = usize::try_from(emitted.u64(flat + 8)?)?;
        assert_eq!(emitted.u64(live + 8)?, flat as u64);
        assert_eq!(emitted.u32(live + 64)?, 1.0f32.to_bits());
        assert_eq!(emitted.u8(live + 68)?, 1);
    }
    // Compare every pre-existing grouped declaration after relocation, including
    // gated duplicates and native-only children that the imported group must omit.
    let declarations = |p: &Payload| -> CheckResult<Vec<_>> {
        let mut result = Vec::new();
        for r in p.array(p.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
            let m = p.pointer(r + 48)?;
            if p.u32(m + 4)? != 0x8080447C {
                continue;
            }
            let mut children = Vec::new();
            for c in p.array(m + 16, 8, Some(0x80804485))? {
                let c = p.pointer(c)?;
                children.push((p.u32(c - 4)?, p.bytes::<8>(c)?));
            }
            result.push((p.bytes::<24>(r + 16)?, p.bytes::<8>(m + 32)?, children));
        }
        Ok(result)
    };
    let mut after = declarations(emitted)?;
    for old in declarations(stock)? {
        let i = after
            .iter()
            .position(|v| *v == old)
            .ok_or("Stock group changed")?;
        after.remove(i);
    }
    assert!(!after.is_empty());
    Ok(())
}
