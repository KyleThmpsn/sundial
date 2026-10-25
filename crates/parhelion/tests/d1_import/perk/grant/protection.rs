//! Follow the source fixed factor into the emitted value program, including its absent filter.
use super::*;

pub(super) fn check(
    source: &Payload,
    source_row: usize,
    stock: &Payload,
    emitted: &Payload,
    row: usize,
) -> CheckResult<()> {
    let source_body = source.pointer(source_row + 32)?;
    assert_eq!(source.u32(source_body - 4)?, 0x808026D1);
    assert_eq!(source.u32(source_row + 8)?, 255);
    assert_eq!(source.u64(source_row + 24)?, 0);
    let key = source.u32(source_row)?;
    let witnesses = stock
        .array(stock.pointer(24)? + 0x88, 56, Some(0x8080453B))?
        .into_iter()
        .filter(|&r| {
            stock.u32(r + 16).ok() == Some(key)
                && stock
                    .pointer(r + 48)
                    .ok()
                    .and_then(|m| stock.u32(m + 4).ok())
                    == Some(0x80804520)
        })
        .collect::<Vec<_>>();
    assert_eq!(witnesses.len(), 1);
    let witness = witnesses[0];
    assert_eq!(stock.u32(witness + 24)?, 255);
    assert_eq!(stock.u64(witness + 40)?, 0);
    assert_eq!(emitted.u32(row + 28)?, stock.u32(witness + 28)?);
    assert_eq!(emitted.u32(row + 24)?, 255);
    assert_eq!(emitted.u64(row + 40)?, 0);
    let body = emitted.pointer(row + 48)?;
    let twin = usize::try_from(emitted.u64(body + 8)?)?;
    let split = emitted.pointer(24)?;
    assert!(twin + 40 <= split && body >= split);
    assert_eq!(emitted.u32(body - 4)?, 0x80804521);
    assert_eq!(emitted.u32(body + 4)?, 0x80804520);
    assert_eq!(emitted.u32(twin + 4)?, 0x80804521);
    assert_eq!(emitted.u64(twin + 8)?, body as u64);
    assert_eq!(emitted.u32(twin)?, emitted.u32(body)?);
    assert_eq!(emitted.u32(body + 16)?, source.u32(source_body)?);
    assert_eq!(emitted.u32(body + 20)?, source.u32(source_body + 4)?);
    assert_eq!(emitted.u64(body + 24)?, 0);
    assert_eq!(
        emitted.bytes::<16>(body + 64)?,
        [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(emitted.u64(body + 80)?, 0);
    assert_eq!(emitted.u32(body + 88)?, u32::MAX);
    assert_eq!(emitted.u32(body + 92)?, 0);
    let code = emitted.array(body + 32, 1, Some(0x80800009))?;
    assert_eq!(code.len(), 4);
    assert!(code[0] >= split);
    assert_eq!(emitted.bytes::<4>(code[0])?, [0x34, 0, 0x3E, 0]);
    let values = emitted.array(body + 48, 16, Some(0x80800090))?;
    assert_eq!(values.len(), 1);
    assert!(values[0] >= split);
    for lane in 0..4 {
        assert_eq!(
            emitted.u32(values[0] + lane * 4)?,
            source.u32(source_body + 4)?
        );
    }
    // A new private property must leave the complete stock expression and filter intact.
    let retained = emitted
        .array(split + 0x88, 56, Some(0x8080453B))?
        .into_iter()
        .find(|&r| {
            emitted.u32(r + 16).ok() == Some(key)
                && emitted
                    .pointer(r + 48)
                    .ok()
                    .and_then(|m| emitted.u32(m + 4).ok())
                    == Some(0x80804520)
        })
        .ok_or("Original ability protection property disappeared")?;
    let old = stock.pointer(witness + 48)?;
    let kept = emitted.pointer(retained + 48)?;
    assert_eq!(
        &stock.0[old + 16..old + 96],
        &emitted.0[kept + 16..kept + 96]
    );
    for (offset, stride, class) in [(32, 1, 0x80800009), (48, 16, 0x80800090)] {
        let old_rows = stock.array(old + offset, stride, Some(class))?;
        let kept_rows = emitted.array(kept + offset, stride, Some(class))?;
        assert_eq!(old_rows.len(), kept_rows.len());
        for (a, b) in old_rows.into_iter().zip(kept_rows) {
            assert_eq!(&stock.0[a..a + stride], &emitted.0[b..b + stride]);
        }
    }
    Ok(())
}
