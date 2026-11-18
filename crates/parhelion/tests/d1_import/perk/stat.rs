//! Read equipped stat units from the original budget table and compare them with private plugs.
use super::*;

pub(super) fn expected(
    source: &Source,
    hash: u32,
    bytes: &Payload,
    slot: u8,
) -> CheckResult<Option<i64>> {
    if bytes.bytes::<12>(0)? == [0; 12] && bytes.bytes::<3>(13)? == [0xFF, 0, 0] {
        return Ok(None);
    }
    let classes = source.classes(0x808017EC);
    assert_eq!(classes.len(), 1);
    let table = source.read(classes[0])?;
    let budget = table
        .array(8, 16, Some(0x80801870))?
        .into_iter()
        .find(|&row| {
            table.u32(row).ok() == Some(hash) && table.u64(row + 8).ok().is_some_and(|p| p != 0)
        });
    if let Some(row) = budget {
        if bytes.u8(13)? != 0xFF {
            return Ok(None);
        }
        if bytes.u32(0)? == 0 {
            let object = table.pointer(row + 8)?;
            assert_eq!(table.u32(object - 4)?, 0x8080173D);
            let curve = table.array(object, 8, None)?;
            let plateau = table.u32(*curve.last().ok_or("Source budget curve")? + 4)? as i32;
            let mut total = 0i32;
            let mut selected = None;
            for at in table.array(object + 32, 4, None)? {
                let weight = table.u16(at + 2)? as i16;
                total += i32::from(weight);
                if table.u8(at + 1)? == slot {
                    selected = Some(weight);
                }
            }
            let source_points = (f32::from(selected.ok_or("Source slot weight")?) / total as f32
                * plateau as f32
                * bytes.f32(4)?)
            .trunc();
            // Rise of Iron has 60 points per tier, Shadowkeep has ten.
            return Ok(Some((f64::from(source_points) / 6.0).round() as i64));
        }
    }
    assert_eq!(hash, 0x1769266F, "Unqualified equipped stat unit");
    assert_eq!(bytes.f32(4)?, 1.0);
    assert_eq!(bytes.f32(8)?, 1.0);
    Ok(Some(
        i64::from(i32::from_le_bytes(bytes.bytes::<4>(0)?)) * 10,
    ))
}
