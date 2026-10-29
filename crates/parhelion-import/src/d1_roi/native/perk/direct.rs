//! Direct fixed-damage and health-response effects through privately authored profiles.
use super::*;
use sundial::package_authoring::{
    ability_damage,
    sandbox_perk::program::{FixedDamageProfile, HealthResponse},
};
mod health;

// A stock neutral profile supplies only its checked runtime envelope and allocation.
// Neither the original amount nor the template's damage type reaches the imported effect.
const TEMPLATE: u32 = 0x815B2062;

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    p: &Payload,
    at: usize,
) -> Result<(Action, FixedDamageProfile)> {
    ensure!(
        p.u8(at + 1)? == 0
            && matches!(p.u8(at + 2)?, 1 | 3)
            && p.bytes::<5>(at + 3)? == [0; 5]
            && p.u32(at + 20)? == 0,
        "Direct source effect requires a nonretained player or event-object target"
    );
    if p.u64(at + 8)? != 0 {
        let path = p.pointer(at + 8)?;
        ensure!(
            p.0.get(path..)
                .is_some_and(|bytes| bytes.iter().take(4096).any(|b| *b == 0)),
            "Source direct resource path is unterminated"
        );
    }
    let tag = p.u32(at + 16)?;
    ensure!(
        source.reference(tag)? == 0x80800861,
        "Direct source effect is not a typed profile"
    );
    let owner = source.read(tag)?;
    let runtime = owner.pointer(16)?;
    let root = owner.pointer(24)?;
    ensure!(
        runtime == 0x68 && root == 0x90 && matches!(owner.0.len(), 0x1B0 | 0x230),
        "Source direct profile has an unqualified runtime envelope"
    );
    let instance_class = owner.u32(runtime - 4)?;
    let definition_class = owner.u32(root - 4)?;
    for (class, binding, base, size) in [
        (instance_class, 0xAC6BA0EF, 0x80802BF8, 0x20),
        (definition_class, 0x7F9BA830, 0x80802364, 0x120),
    ] {
        ensure!(
            source.reference(class)? == 0x80800000,
            "Source profile declaration class differs"
        );
        let declaration = source.read(class)?;
        ensure!(
            declaration.u32(8)? == binding
                && declaration.u32(16)? == base
                && declaration.u32(20)? == size,
            "Source direct effect has another profile declaration"
        );
    }
    let healing = owner.0.len() == 0x230;
    let amount = owner.f32(root + 0x80)?;
    let mode = owner.u8(root + 0x8C)?;
    ensure!(
        amount.is_finite()
            && (if healing {
                amount.to_bits() == 0 && mode == 0
            } else {
                amount > 0.0
            })
            && mode <= 3,
        "Source direct profile needs positive fixed damage or a qualified zero-damage health response"
    );

    // Compare every serialized field. The health family fills its response and category
    // records below. Additional predicates, attacker behavior or feedback remain rejected.
    let mut expected = vec![0u8; owner.0.len()];
    for (field, value) in [
        (0, owner.0.len() as u64),
        (16, 0x58),
        (24, 0x78),
        (0x48, 0x28),
        (0x58, 0x0000_0105_0000_0008),
        (runtime + 8, root as u64),
        (root + 8, runtime as u64),
        (root + 0x18, 0x68),
        (root + 0x20, 0xB8),
        (root + 0x38, 0xB8),
        (root + 0x40, 0xC8),
    ] {
        put(&mut expected, field, &value.to_le_bytes())?;
    }
    for (field, value) in [
        (0x40, 0x822A4174u32),
        (0x44, u32::MAX),
        (runtime - 4, instance_class),
        (runtime, tag),
        (runtime + 16, definition_class),
        (root - 4, definition_class),
        (root, tag),
        (root + 16, instance_class),
        (root + 0x50, 1.0f32.to_bits()),
        (root + 0x80, amount.to_bits()),
        (root + 0x84, amount.to_bits()),
        (root + 0xB0, 1.0f32.to_bits()),
        (root + 0xDC, 1.0f32.to_bits()),
        (root + 0x100, u32::MAX),
    ] {
        put(&mut expected, field, &value.to_le_bytes())?;
    }
    expected[root + 0x8C] = mode;
    let health = if healing {
        ensure!(
            p.u8(at + 2)? == 1,
            "Source healing requires the owning player target"
        );
        Some(health::read(source, native, &owner, &mut expected)?)
    } else {
        ensure!(
            p.u8(at + 2)? == 3,
            "Source damage requires the event-object target"
        );
        None
    };
    ensure!(
        owner.0 == expected,
        "Source direct profile has additional scaling, filters, responses or feedback"
    );
    let template_tag = if healing { health::TEMPLATE } else { TEMPLATE };
    let template = native.tag(template_tag, Some(0x80809C36))?;
    if healing {
        ability_damage::healing_profile(template_tag, &template.0)
    } else {
        ability_damage::neutral_profile(template_tag, &template.0)
    }
    .map_err(anyhow::Error::msg)?;
    let mut bytes = vec![0; 24];
    bytes[0] = 4;
    bytes[2] = p.u8(at + 2)?;
    put(&mut bytes, 16, &template_tag.to_le_bytes())?;
    Ok((
        Action::Native {
            node: NativeNode { kind: 4, bytes },
        },
        FixedDamageProfile {
            action_index: 0,
            template: template_tag,
            amount_bits: amount.to_bits(),
            damage_type: mode,
            health,
        },
    ))
}
