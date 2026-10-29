//! Follow the source active rate to its private pair and independently sized priority storage.
use super::*;

pub(super) fn check(
    source: &Payload,
    source_row: usize,
    stock: &Payload,
    emitted: &Payload,
    row: usize,
) -> CheckResult<Value> {
    let source_body = source.pointer(source_row + 32)?;
    assert_eq!(source.u32(source_body - 4)?, 0x8080243C);
    assert_eq!(source.u32(source.pointer(16)? - 4)?, 0x808031E7);
    assert_eq!(source.u32(source.pointer(24)? - 4)?, 0x80803210);
    assert_eq!(source.u32(source_row + 8)?, 255);
    assert_eq!(source.u32(source_row + 16)?, 255);
    assert_eq!(source.u64(source_row + 24)?, 0);
    let bits = source.u32(source_body)?;
    assert!(f32::from_bits(bits).is_finite() && f32::from_bits(bits) >= 0.0);
    assert_eq!(emitted.u32(row + 20)?, source.u32(source_row + 4)?);
    assert_eq!(emitted.u32(row + 24)?, 255);
    assert_eq!(emitted.u32(row + 32)?, 255);
    assert_eq!(emitted.u64(row + 40)?, 0);

    let stock_runtime = stock.pointer(16)?;
    let runtime = emitted.pointer(16)?;
    let split = emitted.pointer(24)?;
    assert_eq!(stock.u32(stock_runtime - 4)?, 0x8080428A);
    assert_eq!(stock.u32(stock.pointer(24)? - 4)?, 0x8080428B);
    assert_eq!(emitted.u32(runtime - 4)?, 0x8080428A);
    assert_eq!(emitted.u32(split - 4)?, 0x8080428B);
    let callbacks = |p: &Payload| -> CheckResult<Vec<(usize, u32, u32)>> {
        let runtime = p.pointer(16)?;
        p.array(0x20, 8, None)?
            .into_iter()
            .map(|entry| {
                let at = p.pointer(entry)? - 8;
                Ok((at - runtime, p.u32(at)?, p.u32(at + 4)?))
            })
            .collect()
    };
    assert_eq!(callbacks(stock)?, callbacks(emitted)?);
    assert_eq!(
        callbacks(emitted)?,
        vec![
            (0x30, 0x8080428A, 0),
            (0x48, 0x8080428A, 1),
            (0x60, 0x808044FC, 12)
        ]
    );

    let old_rows = stock.array(stock.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let emitted_rows = emitted.array(split + 0x88, 56, Some(0x8080453B))?;
    let mut slots = BTreeSet::new();
    let mut witnesses = 0;
    for &old in &old_rows {
        let body = stock.pointer(old + 48)?;
        assert_ne!(stock.u32(body + 4)?, 0x8080452E);
        let retained = emitted_rows
            .iter()
            .copied()
            .filter(|&r| emitted.u32(r + 16).ok() == stock.u32(old + 16).ok())
            .collect::<Vec<_>>();
        assert_eq!(retained.len(), 1);
        let kept = emitted.pointer(retained[0] + 48)?;
        assert_eq!(
            stock.bytes::<24>(old + 16)?,
            emitted.bytes::<24>(retained[0] + 16)?
        );
        assert_eq!(stock.u32(body + 4)?, emitted.u32(kept + 4)?);
        match stock.u32(body + 4)? {
            0x80804516 => assert_eq!(
                stock.bytes::<24>(body + 16)?,
                emitted.bytes::<24>(kept + 16)?
            ),
            0x80804291 => {
                let before = stock.array(body + 16, 4, None)?;
                let after = emitted.array(kept + 16, 4, None)?;
                assert_eq!(before.len(), after.len());
                for (a, b) in before.into_iter().zip(after) {
                    assert_eq!(stock.u32(a)?, emitted.u32(b)?);
                }
            }
            _ => return Err("Unverified stock Stormtrance modifier".into()),
        }
        slots.insert(stock.u32(old + 28)? as usize);
        if stock.u32(old + 16)? == source.u32(source_row)? {
            assert_eq!(stock.u32(body + 4)?, 0x80804516);
            assert_eq!(stock.u32(old + 24)?, 255);
            assert_eq!(stock.u32(old + 32)?, 255);
            assert_eq!(stock.u64(old + 40)?, 0);
            witnesses += 1;
        }
    }
    assert_eq!(witnesses, 1);
    let old_priority = stock.array(stock_runtime + 0x190, 1, Some(0x80800009))?;
    let priority = emitted.array(runtime + 0x190, 1, Some(0x80800009))?;
    assert_eq!(
        stock.u64(stock.pointer(24)? + 0xB8)? as usize,
        old_priority.len()
    );
    assert_eq!(emitted.u64(split + 0xB8)? as usize, priority.len());
    assert_eq!(slots, (0..old_priority.len()).collect::<BTreeSet<_>>());
    assert_eq!(priority.len(), old_priority.len() + 1);
    assert_eq!(emitted.u32(row + 28)? as usize, old_priority.len());
    assert!(priority.iter().all(|&at| at >= runtime && at < split));
    for (old, current) in old_priority.iter().zip(&priority) {
        assert_eq!(stock.u8(*old)?, emitted.u8(*current)?);
    }
    assert_eq!(
        emitted.u8(*priority.last().ok_or("Rate priority storage")?)?,
        0
    );

    let body = emitted.pointer(row + 48)?;
    let twin = usize::try_from(emitted.u64(body + 8)?)?;
    assert!(twin + 32 <= split && body >= split);
    assert_eq!(emitted.u32(body - 4)?, 0x8080452F);
    assert_eq!(emitted.u32(body + 4)?, 0x8080452E);
    assert_eq!(emitted.u32(twin - 4)?, 0x8080452E);
    assert_eq!(emitted.u32(twin + 4)?, 0x8080452F);
    assert_eq!(emitted.u32(twin)?, emitted.u32(body)?);
    assert_eq!(emitted.u64(twin + 8)?, body as u64);
    let runtime_row = usize::try_from(emitted.u64(row + 8)?)?;
    assert_eq!(emitted.pointer(twin + 16)?, runtime_row);
    assert_eq!(emitted.pointer(runtime_row + 48)?, twin);
    assert_eq!(emitted.u32(body + 16)?, bits);
    assert_eq!(emitted.u32(body + 20)?, 0);
    assert_eq!(emitted.bytes::<8>(twin + 24)?, [0; 8]);
    Ok(json!({
        "source_bits":bits,
        "emitted_bits":emitted.u32(body + 16)?,
        "priority_slot":emitted.u32(row + 28)?,
        "priority_bytes":priority.len(),
        "priority_declaration":emitted.u64(split + 0xB8)?,
        "callback_class":0x8080428Au32,
        "gameplay_verified":false
    }))
}
