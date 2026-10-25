//! Read the staged weighted pickup operation against its original source operands.
use super::*;

pub(super) fn check(source: &Payload, s: usize, native: &Payload, n: usize) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x80802482);
    assert_eq!(native.u32(n - 4)?, 0x80803E47);
    assert_eq!(source.bytes::<5>(s)?, [12, 0, 1, 1, 0]);
    assert_eq!(native.bytes::<5>(n)?, [13, 0, 1, 1, 0]);
    // A source resource is not a native attachment. The qualified operation uses
    // the recipient's native category pickup and has no authored attachment.
    assert_eq!(source.u64(s + 8)?, 0);
    assert_eq!(source.u32(s + 16)?, u32::MAX);
    assert_eq!(native.u64(n + 8)?, 0);
    assert_eq!(native.u32(n + 16)?, u32::MAX);
    for offset in [5, 6, 7, 20, 21, 22, 23, 60, 61, 62, 63] {
        assert_eq!(source.u8(s + offset)?, 0);
        assert_eq!(native.u8(n + offset)?, 0);
    }
    let mut total = 0.0_f32;
    for category in 0..3 {
        let row = 24 + category * 12;
        let low = source.f32(s + row)?;
        let high = source.f32(s + row + 4)?;
        let weight = source.f32(s + row + 8)?;
        assert!(low.is_finite() && high.is_finite() && weight.is_finite());
        assert!((0.0..=1.0).contains(&low) && (low..=1.0).contains(&high));
        assert!(weight >= 0.0);
        total += weight;
        for lane in 0..3 {
            assert_eq!(
                native.u32(n + row + lane * 4)?,
                source.u32(s + row + lane * 4)?,
                "The emitted category's range or weight differs from the source"
            );
        }
    }
    assert!(total.is_finite() && total > 0.0);
    Ok(())
}
