//! Descriptive labels proven by the native consumer, not recovered source spellings.

pub(super) fn behavior(schema: u32, offset: usize) -> Option<&'static str> {
    if let Some((_, _, name)) = declared_fields(schema)
        .1
        .iter()
        .find(|(at, _, _)| *at == offset)
    {
        return Some(name);
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
        return Err("Verified projectile field layout has an incompatible structure size".into());
    }
    Ok(fields)
}

#[derive(Clone, Copy)]
pub(super) enum Storage {
    Float32,
    Boolean,
}

type NativeField = (usize, Storage, &'static str);

fn declared_fields(schema: u32) -> (usize, &'static [NativeField]) {
    use Storage::{Boolean, Float32};
    match schema {
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
