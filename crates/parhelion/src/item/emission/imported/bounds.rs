//! Keep copied native radius fields consistent when an imported model replaces its carrier.
use super::*;

pub(super) fn repair(
    manager: &sundial::package_authoring::PackageManager,
    template: TagHash,
    payload: &mut [u8],
) -> AuthoringResult<()> {
    let radius = read_u32(payload, 0x8c)?;
    let copied = read_u32(payload, 0x28)?;
    let scale = f32::from_bits(read_u32(payload, 0x6c)?);
    // Only the checked, uniform-radius model layout is eligible. A collapsed
    // model or independently authored radius retains its own semantics.
    if copied == radius
        || read_u32(payload, 0x20)? != radius
        || read_u32(payload, 0x24)? != radius
        || !f32::from_bits(radius).is_finite()
        || f32::from_bits(radius) <= 0.
        || !scale.is_finite()
        || scale <= 0.
        || !manager
            .get_entry(template)
            .is_some_and(|entry| entry.reference == 0x808073a5)
    {
        return Ok(());
    }
    let original = manager
        .read_tag(template)
        .map_err(|error| invalid(format!("Imported model bounds template: {error}")))?;
    for offset in [0x20, 0x24, 0x28, 0x8c] {
        if read_u32(&original, offset)? != copied {
            return Ok(());
        }
    }
    // The three recomputed bounds agree and the fourth still matches every
    // radius in the stock carrier. Repair the private emitted copy only.
    write_u32(payload, 0x28, radius)
}
