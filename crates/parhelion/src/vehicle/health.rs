//! Capacity and recovery settings, preserving region selection and disabled recovery.
use super::{Durability, values};
use crate::{AuthoringResult, error::invalid, tag_payload::array_at};

pub(super) fn tune(
    payload: &mut Vec<u8>,
    roots: impl IntoIterator<Item = usize>,
    settings: &Durability,
) -> AuthoringResult<()> {
    let capacity = f32::from(settings.health_percent) / 100.0;
    let delay = f32::from(settings.repair_delay_percent) / 100.0;
    let duration = 100.0 / f32::from(settings.repair_rate_percent);
    for root in roots {
        for (at, factor, label) in [
            (0x48, capacity, "Health"),
            (0xA8, capacity, "Health"),
            (0x68, delay, "Repair Delay"),
            (0xC8, delay, "Repair Delay"),
            (0x88, duration, "Repair Rate"),
            (0xE8, duration, "Repair Rate"),
        ] {
            values::scale_number(payload, root + at, factor, label)?;
            values::scale_float(payload, root + at + 12, factor, label)?;
        }
        // The native pointer is at +0x370. The count-first array descriptor begins at +0x368.
        let (count, _, rows, class) = array_at(payload, root + 0x368)?;
        if class != 0x8080_4C5F
            || count == 0
            || count > 64
            || payload.get(rows..rows.saturating_add(count * 80)).is_none()
        {
            return Err(invalid(
                "Durability requires supported vehicle health recovery regions",
            ));
        }
        for index in 0..count {
            let region = rows + index * 80;
            // Numeric-input regions bypass these scalars. Both paths are edited, retaining
            // the flags and the region's original base-capacity source.
            for (at, factor, label) in [
                (0x14, capacity, "Health"),
                (0x38, delay, "Repair Delay"),
                (0x3C, delay, "Repair Delay"),
                (0x40, duration, "Repair Rate"),
            ] {
                values::scale_float(payload, region + at, factor, label)?;
            }
        }
    }
    Ok(())
}
