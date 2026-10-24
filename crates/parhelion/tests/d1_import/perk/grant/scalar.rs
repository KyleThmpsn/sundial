//! Independent source declarations and emitted numeric-provider readback.
use super::*;

pub(super) fn check(
    source: &Payload,
    row: usize,
    stock: &Payload,
    emitted: &Payload,
    native_row: usize,
    grant: &Value,
) -> CheckResult<()> {
    stock_values(stock, emitted)?;
    let m = source.pointer(row + 32)?;
    let index = number(&grant["parameter_index"])? as usize;
    let first = source.u32(m + 16)? as usize;
    let declarations = source.array(m, 8, Some(0x80802AA5))?;
    assert_eq!(declarations.len(), source.u32(m + 20)? as usize);
    let declaration = declarations[index.checked_sub(first).ok_or("Scalar source index")?];
    let inputs = source.array(source.pointer(24)? + 0xC0, 56, Some(0x808023B9))?;
    assert_eq!(
        source.u32(inputs[index] + 48)?,
        source.u32(declaration + 4)?
    );
    assert_eq!(
        source.u32(inputs[index] + 52)?,
        u32::from(source.u8(declaration + 1)?)
    );
    let input = source.u8(declaration)?;
    let change = source.f32(declaration + 4)?;
    let multiply = source.u8(declaration + 1)? != 0;
    let amount = if !multiply {
        change
    } else if change > 0.0 {
        1.0 + change
    } else {
        1.0 / (1.0 - change)
    };
    // The matching stock property independently qualifies the input's bank-local domain.
    let mut anchors = Vec::new();
    for r in stock.array(stock.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
        let sm = stock.pointer(r + 48)?;
        if stock.u32(r + 16)? == source.u32(row)? && stock.u32(sm + 4)? == 0x8080451B {
            anchors.push(sm);
        }
    }
    assert_eq!(anchors.len(), 1);
    let children = stock.array(anchors[0] + 16, 24, Some(0x8080451F))?;
    assert_eq!(children.len(), declarations.len());
    assert_eq!(stock.u8(children[index - first] + 16)?, input);
    assert_eq!(stock.u8(children[index - first] + 17)? != 0, multiply);
    let nm = emitted.pointer(native_row + 48)?;
    assert_eq!(emitted.u32(nm + 4)?, 0x8080451B);
    let children = emitted.array(nm + 16, 24, Some(0x8080451F))?;
    assert_eq!(children.len(), 1);
    let child = children[0];
    assert_eq!(emitted.u8(child + 16)?, input);
    assert_eq!(emitted.u8(child + 17)? != 0, multiply);
    assert_eq!(emitted.u32(child + 20)?, amount.to_bits());
    let values = emitted.array(emitted.pointer(24)? + 0xA8, 48, Some(0x808045B7))?;
    assert_eq!(emitted.u32(nm + 36)?, 1);
    let value = values[emitted.u32(nm + 32)? as usize];
    assert_eq!(emitted.u32(value + 40)?, amount.to_bits());
    assert_eq!(emitted.u32(value + 44)?, u32::from(multiply));
    let instance = usize::try_from(emitted.u64(value + 8)?)?;
    assert_eq!(emitted.u64(instance + 8)?, value as u64);
    assert_eq!(emitted.u32(instance + 64)?, 1.0f32.to_bits());
    assert_eq!(emitted.u8(instance + 68)?, 1);
    Ok(())
}

pub(super) fn stock_values(stock: &Payload, emitted: &Payload) -> CheckResult<()> {
    let before = stock.array(stock.pointer(24)? + 0xA8, 48, Some(0x808045B7))?;
    let after = emitted.array(emitted.pointer(24)? + 0xA8, 48, Some(0x808045B7))?;
    assert!(after.len() > before.len());
    for (old, new) in before.into_iter().zip(after) {
        // Providers, values and operations remain intact after input-array relocation.
        assert_eq!(stock.bytes::<24>(old + 24)?, emitted.bytes::<24>(new + 24)?);
        let old_instance = usize::try_from(stock.u64(old + 8)?)?;
        let new_instance = usize::try_from(emitted.u64(new + 8)?)?;
        assert_eq!(emitted.u64(new_instance + 8)?, new as u64);
        assert_eq!(
            stock.bytes::<8>(old_instance + 64)?,
            emitted.bytes::<8>(new_instance + 64)?
        );
    }
    // Keep every stock numeric declaration, including multiple rows under one property key.
    let declarations = |payload: &Payload| -> CheckResult<Vec<_>> {
        let mut values = Vec::new();
        for row in payload.array(payload.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
            let m = payload.pointer(row + 48)?;
            if payload.u32(m + 4)? != 0x8080451B {
                continue;
            }
            let mut children = Vec::new();
            for child in payload.array(m + 16, 24, Some(0x8080451F))? {
                children.push(payload.bytes::<8>(child + 16)?);
            }
            values.push((
                payload.u32(row + 16)?,
                payload.u32(row + 28)?,
                payload.u32(row + 32)?,
                payload.bytes::<8>(m + 32)?,
                children,
            ));
        }
        Ok(values)
    };
    let mut after = declarations(emitted)?;
    for before in declarations(stock)? {
        let at = after
            .iter()
            .position(|row| *row == before)
            .ok_or("Stock numeric property changed during import")?;
        after.remove(at);
    }
    Ok(())
}
