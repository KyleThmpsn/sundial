//! Event-position weighted pickups using the native recipient's ammo configuration.
use super::*;

pub(super) fn read(data: &Payload, at: usize) -> Result<Action> {
    ensure!(
        data.bytes::<5>(at)? == [12, 0, 1, 1, 0]
            && data.bytes::<11>(at + 5)? == [0; 11]
            && data.u32(at + 16)? == u32::MAX
            && data.u32(at + 20)? == 0
            && data.u32(at + 60)? == 0,
        "Source weighted pickup requires event position, an owner recipient and no attachment"
    );
    let mut total = 0.0_f32;
    for category in 0..3 {
        let row = at + 24 + category * 12;
        let low = data.f32(row)?;
        let high = data.f32(row + 4)?;
        let weight = data.f32(row + 8)?;
        ensure!(
            low.is_finite()
                && high.is_finite()
                && weight.is_finite()
                && (0.0..=1.0).contains(&low)
                && (low..=1.0).contains(&high)
                && weight >= 0.0,
            "Source weighted pickup has an unsupported range or weight"
        );
        total += weight;
    }
    ensure!(
        total.is_finite() && total > 0.0,
        "Source weighted pickup has no finite positive category weight"
    );
    // Source 718A90 and native EC5420 use the same two random draws, ordered
    // categories and range interpolation. Their spawn consumers forward the
    // sampled value to pickup initialization +10. Native pickup configuration
    // determines the final ammunition award and remains separate from this range.
    let mut bytes = data.bytes::<64>(at)?.to_vec();
    bytes[0] = 13;
    Ok(Action::Native {
        node: NativeNode { kind: 13, bytes },
    })
}
