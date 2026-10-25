//! Health-region configuration semantics established by the native consumers.

pub(super) fn field_help(schema: u32, offset: u32) -> Option<&'static str> {
    Some(match (schema, offset) {
        // CDA780 initializes eleven numeric inputs from 32-byte configurations.
        // F3F8F0 uses +0C only when a referenced numeric value does not supply it.
        // B8B6E0 selects input 0 for shield regions and input 3 for health regions.
        (0x8080_4B8A, 0x54 | 0xB4) => {
            "Fallback capacity for this component's matching regions. A referenced numeric value takes precedence. Region selection and player scaling can change the final capacity. This does not set current health or shields."
        }
        // B8BED0 selects inputs 1/4. B8A950 adds the resulting duration to now.
        (0x8080_4B8A, 0x74 | 0xD4) => {
            "Fallback delay in seconds before this component's matching regions can regenerate. A referenced numeric value takes precedence. Player recovery and tuning can scale the delay. Final delays at or below 0.0001 add no waiting time in this consumer."
        }
        // B8BA30 selects inputs 2/5. B91ED0 adds elapsed seconds / duration
        // to the region fraction and caps it at that region's recovery limit.
        (0x8080_4B8A, 0x94 | 0xF4) => {
            "Fallback seconds to recover one full region fraction once regeneration starts. A referenced numeric value takes precedence. A smaller positive value recovers faster. Player recovery and tuning can scale the duration. A final duration at or below 0.0001 stops this regeneration update."
        }
        (0x8080_4C5F, 0x14) => {
            "Multiplies the component's base capacity for this region. Regions configured to use the health or shield numeric inputs use those capacities instead."
        }
        (0x8080_4C5F, 0x38) => {
            "Delay in seconds before regeneration when this region has remaining health. Regions configured to use the health or shield numeric inputs use their regeneration delay instead."
        }
        (0x8080_4C5F, 0x3C) => {
            "Delay in seconds before regeneration when this region's remaining fraction is at or below 0.0001. Regions configured to use the health or shield numeric inputs use their regeneration delay instead."
        }
        (0x8080_4C5F, 0x40) => {
            "Seconds to recover one full region fraction once regeneration starts. Player recovery and tuning can scale the duration. A final duration at or below 0.0001 stops this regeneration update. Regions configured to use the health or shield numeric inputs use their regeneration duration instead."
        }
        _ => return None,
    })
}
