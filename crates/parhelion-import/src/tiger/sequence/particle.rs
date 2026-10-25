//! Particle events with explicit systems, locators and neutral gain expressions.
use super::*;

/// Return the static system-reference fields for private resource fixups.
pub(crate) fn write(
    payload: &mut Payload,
    event: usize,
    systems: &[String],
    locator: u8,
) -> Result<Vec<(usize, String)>> {
    ensure!(
        payload.u32(event - 4)? == 0x80806cc6 && payload.u32(event + 0x30)? == 4,
        "Particle event class differs"
    );
    let root = payload.pointer(24)?;
    ensure!(
        usize::from(locator) < payload.array(root + 0x1b0, 6, Some(0x80808617))?.len(),
        "Particle event locator is undeclared"
    );
    ensure!(!systems.is_empty(), "Particle event has no systems");
    put(&mut payload.0, event + 0x40, &[0; 0xe8])?;
    let rows = array(payload, event + 0x40, 0x80806cc8, 24, systems.len())?;
    let mut resources = Vec::new();
    for (row, system) in rows.into_iter().zip(systems) {
        append(&mut payload.0, row, 0x80800009, &[locator], 1)?;
        put(&mut payload.0, row + 16, &[0xff; 8])?;
        resources.push((row + 16, system.clone()));
    }
    // The native 80FC7DFB CPU sprite events use the same literal-one program
    // in all three embedded expression slots. Preserve their storage contract.
    for offset in [0x50, 0x98, 0xe0] {
        append(
            &mut payload.0,
            event + offset,
            0x80800009,
            &[0x34, 0, 0x3e, 0],
            1,
        )?;
        let values = [1f32; 4]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        append(&mut payload.0, event + offset + 16, 0x80800090, &values, 16)?;
        put(&mut payload.0, event + offset + 32, &1u64.to_le_bytes())?;
        put(&mut payload.0, event + offset + 40, &1u64.to_le_bytes())?;
    }
    put(&mut payload.0, event + 0x120, &u32::MAX.to_le_bytes())?;
    Ok(resources)
}
