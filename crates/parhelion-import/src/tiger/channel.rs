use super::payload::Payload;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

/// Correct older appended inherited declarations before their native update runs.
/// A zero receiver range selects receiver zero, even when the array is empty.
pub(super) fn repair_inherited_receivers(p: &mut Payload) -> Result<usize> {
    let instance = p.pointer(16)?;
    if p.u32(instance - 4)? != 0x8080979F || p.u64(instance + 0x70)? != 0 {
        return Ok(0);
    }
    let original = usize::try_from(p.u64(instance + 8)?)?;
    let mut definitions = vec![original, p.pointer(24)?];
    definitions.sort_unstable();
    definitions.dedup();
    let mut repaired = 0;
    for definition in definitions {
        ensure!(
            definition >= 4 && p.u32(definition - 4)? == 0x80809790,
            "Native channel reciprocal definition differs"
        );
        for row in p.array(definition + 0xD8, 112, Some(0x808097A1))? {
            if p.0[row + 0x68] > p.0[row + 0x69] {
                continue;
            }
            ensure!(
                p.u16(row + 0x68)? == 0
                    && p.u64(row + 8)? == 0
                    && p.0[row + 16..row + 72].iter().all(|v| *v == 0)
                    && p.u64(row + 96)? == 0
                    && p.u16(row + 106)? == u16::MAX,
                "Channel without local receivers has an unsupported callback range"
            );
            p.0[row + 0x68..row + 0x6A].copy_from_slice(&[u8::MAX, u8::MAX - 1]);
            repaired += 1;
        }
    }
    Ok(repaired)
}

pub fn object_channel_map(payload: &[u8]) -> Result<BTreeMap<String, u8>> {
    let p = Payload(payload.to_vec());
    let instance = p.pointer(16)?;
    ensure!(
        instance >= 4 && p.u32(instance - 4)? == 0x808072B8,
        "native model owner type differs"
    );
    // PushObjectChannel addresses the model's ordered inputs. The independent
    // channel-bank declaration order and vector-storage indices both differ.
    let inputs = p.array(instance + 0x120, 96, Some(0x80809788))?;
    ensure!(
        inputs.len() <= 256,
        "native object input count exceeds byte indices"
    );
    let mut result = BTreeMap::new();
    for (index, input) in inputs.iter().copied().enumerate() {
        ensure!(
            p.u32(input + 4)? == 0x80809789,
            "native channel input link type differs"
        );
        let link = usize::try_from(p.u64(input + 8)?)?;
        ensure!(
            p.u32(link)? == p.u32(input)?
                && p.u32(link + 4)? == 0x80809788
                && p.u64(link + 8)? == input as u64
                && p.u64(link + 24)? == 0x808097C1,
            "native channel input lacks reciprocal vector property"
        );
        ensure!(
            result
                .insert(format!("{:08X}", p.u32(link + 32)?), u8::try_from(index)?)
                .is_none(),
            "duplicate native object channel"
        );
    }
    Ok(result)
}
