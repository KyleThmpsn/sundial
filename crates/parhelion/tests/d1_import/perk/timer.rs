//! Retain observed timer event routing and armor damage gates in the staged artifact.
use super::*;

pub(super) fn evidence(source: &Payload, native: &Payload) -> CheckResult<Value> {
    let mut timers = Vec::new();
    let mut damage = Vec::new();
    for row in native.array(0x38, 8, None)? {
        let at = native.pointer(row)?;
        match native.u8(at)? {
            32 => {
                let mut conditions = Vec::new();
                for row in native.array(at + 16, 8, Some(0x808040BA))? {
                    let child = native.pointer(row)?;
                    let kind = native.u8(child + 5)?;
                    let mut condition = json!({"kind":kind,"class":native.u32(child - 4)?,
                        "probability":native.f32(child)?,"retained":native.u8(child + 6)?,
                        "state_index":native.u8(child + 7)?});
                    if kind == 12 {
                        condition["category"] = json!(native.u32(child + 8)?);
                        condition["event"] = json!(native.u32(child + 12)?);
                    } else if kind == 19 {
                        condition["weapon_flags"] = json!(native.bytes::<2>(child + 8)?);
                        condition["timeout_seconds"] = json!(native.f32(child + 12)?);
                    }
                    conditions.push(condition);
                }
                timers.push(json!({"extension_seconds":native.f32(at + 4)?,
                    "cap_seconds":native.f32(at + 8)?,"event_mask":native.u64(at + 32)?,
                    "conditions":conditions}));
            }
            40 => {
                let value = native.pointer(at + 0x140)?;
                damage.push(json!({"owner_gate":native.u8(at + 0xB8)?,
                    "literal_bits":[native.u32(value)?,native.u32(value + 4)?],
                    "assignment_rows":native.array(at + 0x120,12,Some(0x80803E22))?.len(),
                    "multiplication_rows":native.array(at + 0x130,12,Some(0x80803E22))?.len()}));
            }
            _ => {}
        }
    }
    let source_count = source.array(0x38, 8, None)?.into_iter().try_fold(
        0,
        |count, row| -> CheckResult<usize> {
            Ok(count + usize::from(source.u8(source.pointer(row)?)? == 27))
        },
    )?;
    assert_eq!(source_count, timers.len());
    Ok(json!({"timers":timers,"damage_events":damage}))
}
