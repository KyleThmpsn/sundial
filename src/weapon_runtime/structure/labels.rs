//! Descriptive labels proven by the native consumer, not recovered source spellings.

pub(super) fn behavior(schema: u32, offset: usize) -> Option<&'static str> {
    if let Some((_, _, name)) = declared_fields(schema)
        .1
        .iter()
        .find(|(at, _, _)| *at == offset)
    {
        return Some(name);
    }
    // F83E80 multiplies the base value program by each matching row's program.
    // F89CF0 checks that row's damage-source and object filters. The names describe
    // the actual components, not the whole perk that happens to attach them.
    let mapped = match (schema, offset) {
        (0x8080_3F8C, 0x78) => Some("Conditional Damage Multipliers"),
        (0x8080_3F8C, 0x80) => Some("Base Damage Multiplier"),
        (0x8080_2A1C, 0x10) => Some("Damage Source Filter"),
        (0x8080_2A1C, 0x30) => Some("Source Object Filter"),
        (0x8080_2A1C, 0x50) => Some("Damage Multiplier"),
        // The stored 43E1 references pair these settings with runtime curves
        // +20/+40/+70/+90/+C0. D24AC0 consumes normalized damage. D2B880
        // decays both accumulators per second and evaluates the movement curve.
        (0x8080_43E2, 0x18) | (0x8080_43E1, 0x20) => Some("Damage Break Response"),
        (0x8080_43E2, 0x60) | (0x8080_43E1, 0x40) => Some("Damage Break Recovery Rate"),
        (0x8080_43E2, 0xC8) | (0x8080_43E1, 0x70) => Some("Damage Strength Loss"),
        (0x8080_43E2, 0x110) | (0x8080_43E1, 0x90) => Some("Strength Recovery Rate"),
        (0x8080_43E2, 0x188) | (0x8080_43E1, 0xC0) => Some("Movement Strength Multiplier"),
        _ => None,
    };
    if mapped.is_some() {
        return mapped;
    }
    if schema != 0x8080_37C9 {
        return None;
    }
    // This 0xB0-byte record is inline at +0xF0 of moving-projectile instance 0x80803B73.
    // CE43B0 interpolates speed (+54 -> +5C) and multiplies it by +50.
    // CE4290 interpolates gravity (+58 -> +60) before applying world gravity.
    // Both normalize (projectile +184 - +64) * +6C and clamp to [0, 1].
    // CFC2CA..CFC2DB accumulates +184 as speed * elapsed time, proving distance,
    // not a lifetime timer. CFC2F2..CFC36A applies gravity to velocity.
    // D00CE0's executed path through 487FAD..487FF6 stores both distance
    // endpoints, then D00D25..D00D58 derives and clamps the reciprocal span.
    Some(match offset {
        0x50 => "Speed Curve Multiplier",
        0x54 => "Initial Speed",
        0x58 => "Initial Gravity Multiplier",
        0x5C => "Speed Curve Endpoint",
        0x60 => "Gravity Curve Endpoint",
        0x64 => "Curve Start Distance",
        0x68 => "Curve End Distance",
        0x6C => "Curve Distance Scale",
        _ => return None,
    })
}

/// Native configuration fields omitted from the network descriptor. These offsets
/// come from the reset/initialization consumers, not from inferred wire layouts.
pub(super) fn native_fields(schema: u32, size: usize) -> Result<&'static [NativeField], String> {
    let (expected, fields) = declared_fields(schema);
    if expected != 0 && expected != size {
        return Err("Verified native field layout has an incompatible structure size".into());
    }
    Ok(fields)
}

#[derive(Clone, Copy)]
pub(super) enum Storage {
    Float32,
    Boolean,
    Key,
    Byte,
    Signed16,
}

type NativeField = (usize, Storage, &'static str);

fn declared_fields(schema: u32) -> (usize, &'static [NativeField]) {
    use Storage::{Boolean, Byte, Float32, Key, Signed16};
    match schema {
        // CDA780/F3F8F0 initialize numeric inputs from these fallback scalars.
        // B8B6E0, B8BED0 and B8BA30 select capacity, delay and duration.
        // Stock 8161D5EE ties flag 4 to shields and flag 2 to health.
        0x8080_4B8A => (
            0x5C8,
            &[
                (0x54, Float32, "Default Shield Capacity"),
                (0x74, Float32, "Default Shield Regeneration Delay"),
                (0x94, Float32, "Default Shield Regeneration Duration"),
                (0xB4, Float32, "Default Health Capacity"),
                (0xD4, Float32, "Default Health Regeneration Delay"),
                (0xF4, Float32, "Default Health Regeneration Duration"),
            ],
        ),
        // Per-region alternatives used when neither numeric-input flag is set.
        // B8A950 selects the depleted delay using the actual region fraction.
        0x8080_4C5F => (
            0x50,
            &[
                (0x14, Float32, "Region Capacity Multiplier"),
                (0x38, Float32, "Regeneration Delay"),
                (0x3C, Float32, "Depleted Regeneration Delay"),
                (0x40, Float32, "Regeneration Duration"),
            ],
        ),
        // D20A40 reads initial strength. D26D40/D2B800 consume retirement time.
        0x8080_43EC => (
            0x2F0,
            &[
                (0x290, Float32, "Invisibility Strength"),
                (0x294, Float32, "Retirement Delay"),
            ],
        ),
        // D24AC0: damage break threshold and suppression request.
        // D29500/D2B880: grace period and movement response. All are native
        // settings, not live accumulators or guesses from stock default values.
        0x8080_43E2 => (
            0x248,
            &[
                (0x10, Float32, "Damage Break Threshold"),
                (0x170, Float32, "Disruption Suppression Duration"),
                (0x174, Float32, "Disruption Grace Period"),
                (0x178, Float32, "Minimum Movement Speed"),
                (0x1D8, Boolean, "Ignore Movement"),
            ],
        ),
        // F40810: scalar +28 and operation +2C. CA9850: ability slot +48,
        // input +4A (word for stats, byte otherwise), component category +4C.
        // These are native configuration, absent from the network declaration.
        0x8080_3B06 => (
            0x58,
            &[
                (0x28, Float32, "Adjustment Value"),
                (0x2C, Byte, "Operation"),
                (0x48, Signed16, "Ability Slot"),
                (0x4A, Signed16, "Property Input"),
                (0x4C, Byte, "Component"),
            ],
        ),
        // F83F36..F83FFA checks this property on the damage source, then applies
        // the inversion flag. F83F7A..F83FBA compares source owners. Empty keys
        // skip the property test, including its inversion flag.
        0x8080_3F8C => (
            0xF0,
            &[
                (0xE0, Key, "Required Source Property"),
                (0xE4, Boolean, "Invert Source Property"),
                (0xE8, Boolean, "Require Matching Source Owner"),
            ],
        ),
        // CEC3C0 reset copies +74/+88/+D8 into the paired instance.
        0x8080_388F => (
            0x5D0,
            &[
                (0x74, Float32, "Travel Distance Limit"),
                (0x88, Float32, "Initial Speed"),
                (0xD8, Float32, "Initial Gravity Multiplier"),
            ],
        ),
        // CEC4A9..CEC4C9 loads this object from definition +C8 and calls
        // D00CE0(start, end, speed, gravity). Its executed body stores all four.
        0x8080_3803 => (
            16,
            &[
                (0, Float32, "Speed Curve Endpoint"),
                (4, Float32, "Gravity Curve Endpoint"),
                (8, Float32, "Curve Start Distance"),
                (12, Float32, "Curve End Distance"),
            ],
        ),
        // CF4B50..CF4B6A walks the 0x210-byte pool. CF4C55 passes element +10
        // through D03800 into CFC000 and its curve consumers. Their +184/+1C1
        // therefore mean element +194/+1D1. CEC5AE..CEC5BB sets the enable
        // flag from the presence of definition +C8's curve configuration.
        0x8080_37BA => (
            0x210,
            &[
                (0x194, Float32, "Curve Travel Distance"),
                (0x1D1, Boolean, "Distance Curve Enabled"),
            ],
        ),
        _ => (0, &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_labels_require_the_exact_native_float_declarations() {
        let codecs = super::super::codecs().unwrap();
        let declaration = &codecs[&0x8080_37C9];
        assert_eq!(declaration.size, 0xB0);
        for offset in [0x50, 0x54, 0x58, 0x5C, 0x60, 0x64, 0x68, 0x6C] {
            assert!(
                declaration
                    .fields
                    .iter()
                    .any(|field| field.advance == offset
                        && field.code == 11
                        && field.child == u32::MAX)
            );
            assert!(behavior(0x8080_37C9, offset).is_some());
            assert_eq!(behavior(0x8080_37CA, offset), None);
        }
        assert_eq!(behavior(0x8080_37C9, 0x70), None);
    }
}
