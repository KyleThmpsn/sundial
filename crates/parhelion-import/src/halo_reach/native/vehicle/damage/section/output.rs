//! Preserve public scalar identities while replacing their underlying health pools.
use super::*;

pub(super) fn remap(
    payload: &mut Payload,
    instance: usize,
    definition: usize,
    names: &[u32],
    main: usize,
) -> Result<Vec<Value>> {
    let sections = payload.array(definition + 0x1b0, 0x138, Some(0x80804c11))?;
    let recoveries = payload.array(definition + 0x368, 0x50, Some(0x80804c5f))?;
    let outputs = payload.array(definition + 0x398, 56, Some(0x80804b92))?;
    ensure!(
        payload.array(instance + 0x368, 32, Some(0x80804b91))?.len() == outputs.len(),
        "Native health publications have mismatched runtime storage"
    );
    let mut report = Vec::new();
    for at in outputs {
        let old_index = usize::try_from(payload.u32(at + 0x30)?)?;
        let recovery = *recoveries
            .get(old_index)
            .context("Native health output recovery is absent")?;
        let section = *sections
            .get(usize::try_from(payload.u32(recovery + 0x10)?)?)
            .context("Native health output section is absent")?;
        let old_name = payload.u32(section + 0x10)?;
        // Carrier-only pools such as hull still serve the retained HUD and
        // effects. Their public names now expose the source's primary health.
        let index = names
            .iter()
            .position(|&name| name == old_name)
            .unwrap_or(main);
        put(
            &mut payload.0,
            at + 0x30,
            &u32::try_from(index)?.to_le_bytes(),
        )?;
        report.push(json!({"name":format!("{:08X}",payload.u32(at+0x28)?),
            "previous_recovery":old_index,"recovery":index,
            "section":format!("{:08X}",names[index])}));
    }
    Ok(report)
}
