//! D1's air-evade flag selects the reachable Shadowkeep air-evade profile.
use super::*;
use sundial::package_authoring::ability_bank::AbilityTarget;

pub(super) fn lower(
    native: &mut Reader,
    source: &Payload,
    row: usize,
    slot: AbilityTarget,
    bank: &Payload,
    graph: u32,
) -> Result<u32> {
    let body = source.pointer(row + 32)?;
    ensure!(
        slot == AbilityTarget::Movement
            && source.u32(source.pointer(16)? - 4)? == 0x808027BE
            && source.u32(source.pointer(24)? - 4)? == 0x808029B5
            && source.bytes::<4>(body)? == [0, 1, 0, 0]
            && source.u32(row + 12)? == 10
            && source.u32(row + 16)? == 255,
        "Only the ungated source airborne evade has a qualified native profile"
    );
    ensure!(
        bank.u32(bank.pointer(16)? - 4)? == 0x80804112
            && bank.u32(bank.pointer(24)? - 4)? == 0x80804113,
        "Native movement bank pair differs"
    );

    // A retained source key can name a removed profile. Resolve the current controller's
    // profile first, then the bank property that enables it. No shared bytes are changed.
    let entity = native.tag(graph, Some(0x80809C0F))?;
    let mut profiles = BTreeSet::new();
    for component in entity.array(16, 12, None)? {
        let data = native.tag(entity.u32(component)?, Some(0x80809C36))?;
        let settings = data.pointer(24)?;
        if data.u32(settings - 4)? != 0x808044E0 {
            continue;
        }
        ensure!(
            data.u32(data.pointer(16)? - 4)? == 0x808043AE,
            "Movement controller pair differs"
        );
        let config = data.pointer(settings + 0x800)?;
        ensure!(
            data.u32(config - 4)? == 0x8080410F,
            "Movement profile configuration differs"
        );
        let candidates = data.array(config + 16, 128, Some(0x80804106))?;
        ensure!(
            candidates.len() <= 32,
            "Movement profile mask exceeds its native capacity"
        );
        for at in candidates {
            // Original request admission uses mask 0x02 for airborne, 0x01 for grounded,
            // and this input key for air evade. The qualified profile has no extra gate.
            if data.u32(at + 0x58)? != 2
                || data.u32(at + 0x70)? != 0x476D0367
                || data.u64(at + 0x30)? != 0
                || data.u64(at + 0x38)? != 0
            {
                continue;
            }
            ensure!(
                data.u32(at + 0x14)? == 0.5f32.to_bits()
                    && data.u32(at + 0x18)? == 5.0f32.to_bits()
                    && data.u32(at + 0x1C)? == 100.0f32.to_bits()
                    && data.u32(at + 0x40)? == 0.5f32.to_bits()
                    && data.u32(at + 0x44)? == 1.0f32.to_bits(),
                "Air-evade energy profile differs from the qualified native contract"
            );
            native.tag(data.u32(at + 0x50)?, Some(0x80809C0F))?;
            ensure!(
                profiles.insert(data.u32(at + 16)?),
                "Duplicate air-evade profile identity"
            );
        }
    }
    ensure!(
        profiles.len() == 1,
        "Native air-evade profile is missing or ambiguous"
    );
    let profile = *profiles.first().context("Native air-evade profile")?;
    let mut selected = Vec::new();
    let rows = bank.array(bank.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    for &at in &rows {
        let modifier = bank.pointer(at + 48)?;
        if bank.u32(modifier + 4)? != 0x80804121 || bank.u32(modifier + 16)? != profile {
            continue;
        }
        ensure!(
            bank.u32(at + 24)? == 255
                && bank.u32(at + 32)? == 255
                && bank.u32(at + 28)? == 11
                && bank.u32(at + 36)? == 0
                && bank.u64(at + 40)? == 0,
            "Air-evade property has additional gates or flags"
        );
        selected.push(bank.u32(at + 16)?);
    }
    ensure!(
        selected.len() == 1,
        "Native air-evade property is missing or ambiguous"
    );
    let key = selected[0];
    ensure!(
        rows.iter()
            .filter(|&&at| bank.u32(at + 16).ok() == Some(key))
            .count()
            == 1,
        "Native air-evade key also grants an unqualified companion"
    );
    Ok(key)
}
