//! Value semantics established by the native invisibility consumers.
//! Descriptive names do not claim to recover the original source spelling.

pub(super) fn field_help(schema: u32, offset: u32) -> Option<&'static str> {
    Some(match (schema, offset) {
        // D20A40 initializes the contribution. D2B880 subtracts disruption
        // and then applies the movement response to the resulting strength.
        (0x8080_43EC, 0x290) => {
            "Starting contribution to invisibility. Damage disruption is subtracted from this value before movement scaling. Zero disables the contribution. This is a component strength, not a percentage of visual transparency."
        }
        // D26D40 starts this timer only when configuration flag 0x20 is set.
        // D2B800 subtracts elapsed seconds, clamps at zero, then retires the object.
        (0x8080_43EC, 0x294) => {
            "Seconds before retiring the attachment after its retirement request. Requires the attachment's existing retirement flag (0x20). Zero retires immediately. This is a cleanup delay, separate from the perk's duration."
        }
        // D24AC0 accumulates the damage-response curve in [0, 1] and breaks
        // only when the accumulator is strictly greater than a positive threshold.
        (0x8080_43E2, 0x10) => {
            "Breaks invisibility when accumulated damage response exceeds this positive threshold. Zero and negative values disable this check. The accumulator is clamped from 0 to 1, so a threshold of 1 cannot be exceeded. This is a curve response, not a raw damage amount."
        }
        // D24AC0 and D2B880 pass this duration through interface 44E7 to D21190.
        // D21190 extends a shared suppression timer only if the new time is longer.
        (0x8080_43E2, 0x170) => {
            "Seconds of shared invisibility suppression requested by disruption. A shorter request does not shorten an existing suppression timer. Zero does not start a new timer."
        }
        // D29500 initializes the gate, D2B880 opens it after elapsed time.
        // D24AC0 and the movement section of D2B880 both require the gate.
        (0x8080_43E2, 0x174) => {
            "Seconds before damage and movement disruption become active. Invisibility can apply during this grace period. Zero enables disruption immediately. Negative values leave the disruption gate closed."
        }
        // D2BE0D compares speed before normalization by the movement input.
        (0x8080_43E2, 0x178) => {
            "Movement speed below which this contribution becomes zero. The comparison uses speed before the movement curve's normalization. Applies after the disruption grace period when Ignore Movement is off."
        }
        (0x8080_43E2, 0x1D8) => {
            "Skips both the minimum movement speed check and the movement strength curve. Damage disruption still applies."
        }
        _ => return None,
    })
}
