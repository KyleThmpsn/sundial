//! Check the native interpolation state actually consumed by channel procedures.
use super::*;

pub(crate) fn interpolation(bank: &Payload, allocation: &Payload) -> Result<()> {
    let instance = bank.pointer(16)?;
    let schema = bank.pointer(24)?;
    ensure!(
        bank.u32(instance - 4)? == 0x8080979F && bank.u32(schema - 4)? == 0x80809790,
        "channel interpolation component types differ"
    );
    let states = bank.array(instance + 0x60, 16, Some(0x80800090))?;
    ensure!(
        states.len() == bank.u32(schema + 0x138)? as usize,
        "channel interpolation state capacity differs"
    );
    if let Some(&end) = states.last() {
        ensure!(
            states[0] >= instance
                && end + 16 <= schema
                && (states[0] - instance).is_multiple_of(16),
            "channel interpolation state is outside the aligned mutable instance"
        );
    }
    for row in bank.array(schema + 0xD8, 112, Some(0x808097A1))? {
        if bank.u64(row + 96)? == 0 {
            continue;
        }
        let p = bank.pointer(row + 96)?;
        let kind = bank.u32(p)?;
        ensure!(
            (1..=7).contains(&kind) && bank.u32(p - 4)? == 0x808097B0 - kind,
            "unrecognized native interpolation parameter type"
        );
        let index = bank.u32(p + 4)?;
        ensure!(
            index == u32::MAX || (index as usize) < states.len(),
            "channel procedure {:08X} references missing interpolation state {index}",
            bank.u32(row)?
        );
    }
    let entries = allocation
        .array(0x20, 40, Some(0x80808852))?
        .into_iter()
        .filter(|at| allocation.u32(*at).ok() == Some(0xB6515162))
        .collect::<Vec<_>>();
    ensure!(entries.len() <= 1, "duplicate interpolation allocation");
    if let Some(&at) = entries.first() {
        ensure!(
            allocation.u32(at + 16)? == 0x80800090
                && allocation.u32(at + 20)? as usize == states.len(),
            "interpolation allocation size differs"
        );
    } else {
        ensure!(states.is_empty(), "interpolation allocation is absent");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(p: &mut Payload, at: usize, bytes: &[u8]) {
        p.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn relative(p: &mut Payload, at: usize, target: usize) {
        let delta = target as i64 - at as i64;
        p.0[at..at + 8].copy_from_slice(&delta.to_le_bytes());
    }
    fn array(p: &mut Payload, descriptor: usize, header: usize, class: u32, count: u64) {
        le(p, descriptor, &count.to_le_bytes());
        relative(p, descriptor + 8, header);
        le(p, header, &count.to_le_bytes());
        le(p, header + 8, &class.to_le_bytes());
    }

    /// A channel bank whose only procedure interpolates through history slot 2
    /// while the mutable instance holds `history` vec4 slots.
    fn bank(history: u64) -> Payload {
        let instance = 0x40;
        let states = 0xB0;
        let schema = 0x100;
        let procedures = 0x240;
        let parameter = 0x2C4;
        let mut p = Payload(vec![0; 0x2D0]);
        relative(&mut p, 16, instance);
        relative(&mut p, 24, schema);
        le(&mut p, instance - 4, &0x8080979Fu32.to_le_bytes());
        le(&mut p, schema - 4, &0x80809790u32.to_le_bytes());
        if history > 0 {
            array(&mut p, instance + 0x60, states, 0x80800090, history);
        }
        le(&mut p, schema + 0x138, &(history as u32).to_le_bytes());
        array(&mut p, schema + 0xD8, procedures, 0x808097A1, 1);
        let row = procedures + 16;
        le(&mut p, row, &0x808097B5u32.to_le_bytes());
        relative(&mut p, row + 96, parameter);
        le(&mut p, parameter - 4, &(0x808097B0u32 - 3).to_le_bytes());
        le(&mut p, parameter, &3u32.to_le_bytes());
        le(&mut p, parameter + 4, &2u32.to_le_bytes());
        p
    }
    fn allocation(history: Option<u32>) -> Payload {
        let mut p = Payload(vec![0; 0xB0]);
        let rows = history.map_or(1, |_| 2);
        array(&mut p, 0x20, 0x40, 0x80808852, rows);
        le(&mut p, 0x50, &0xFC2F3D6Fu32.to_le_bytes());
        le(&mut p, 0x50 + 16, &0x80800090u32.to_le_bytes());
        le(&mut p, 0x50 + 20, &7u32.to_le_bytes());
        if let Some(count) = history {
            le(&mut p, 0x78, &0xB6515162u32.to_le_bytes());
            le(&mut p, 0x78 + 16, &0x80800090u32.to_le_bytes());
            le(&mut p, 0x78 + 20, &count.to_le_bytes());
        }
        p
    }

    #[test]
    fn procedures_may_not_interpolate_through_unallocated_history() {
        // The Eternal Blazon defect: procedures kept their source history slots
        // while the private instance and allocation carried none.
        let error = interpolation(&bank(0), &allocation(None)).unwrap_err();
        assert!(
            error.to_string().contains("missing interpolation state 2"),
            "{error}"
        );
        // Enough slots in the instance but no matching allocation row.
        let error = interpolation(&bank(3), &allocation(None)).unwrap_err();
        assert!(
            error.to_string().contains("allocation is absent"),
            "{error}"
        );
        // Instance and allocation disagree about the capacity.
        let error = interpolation(&bank(3), &allocation(Some(1))).unwrap_err();
        assert!(
            error.to_string().contains("allocation size differs"),
            "{error}"
        );
        // Two slots leave slot 2 unaddressable even with a consistent allocation.
        let error = interpolation(&bank(2), &allocation(Some(2))).unwrap_err();
        assert!(
            error.to_string().contains("missing interpolation state 2"),
            "{error}"
        );
        interpolation(&bank(3), &allocation(Some(3))).unwrap();
    }
}
