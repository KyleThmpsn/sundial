//! Stored component-property modifiers used by player buffs and ability attachments.
//! These describe native consumers, not recovered source names.

pub struct FieldMeaning {
    pub help: &'static str,
    pub choices: &'static [(i64, &'static str)],
}

/// Exact schema/offset lookup. A similar-looking field on another type gets no meaning.
pub fn field_meaning(schema: u32, offset: u32) -> Option<FieldMeaning> {
    if schema != 0x8080_3B06 {
        return None;
    }
    let (help, choices): (_, &'static [(i64, &'static str)]) = match offset {
        // F40810 reads +28/+2C and the instance's active weight. 36A100 is lerp.
        0x28 => (
            "Add contributes this value multiplied by the active weight. Multiply blends from 1 to this factor using the active weight, then multiplies the input. At full weight, Add 3 adds 3 and Multiply 3 triples the input. The selected component and input determine the units.",
            &[],
        ),
        0x2C => (
            "Add changes the selected input by a weighted amount. Multiply applies a weighted factor. Other operation bytes leave the input unchanged in this native consumer.",
            &[(0, "Add"), (1, "Multiply")],
        ),
        // CA9850's category 9 branch uses +48 to select the ability before
        // dispatching +4A through 186B250. Stock Abyssal Extractors corroborates slots.
        0x48 => (
            "Ability selected when Component is Abilities. 0 is Grenade, 1 is Super, 2 is Melee, 3 is Jump and 7 is Class Ability. Other component categories do not use this as an ability selector. Negative and unidentified values are preserved.",
            &[],
        ),
        0x4A => (
            "Numeric input within the selected component.\nHealth and Shields: 0 = Shield Capacity, 1 = Shield Regeneration Delay, 2 = Shield Regeneration Duration, 3 = Health Capacity, 4 = Health Regeneration Delay, 5 = Health Regeneration Duration. Delays and durations are in seconds before player scaling. Halving a positive duration doubles the regeneration rate. A final duration at or below 0.0001 stops that update.\nAbilities: input 0 participates in recharge and input 4 controls the activation lockout.\nOther components have different input tables. Player Stats and Weapon Stats consume both bytes, while other mapped categories consume the low byte. Do not transfer input names between components.",
            &[],
        ),
        // CA2830's exact dispatch table resolves these interfaces. Unknown
        // categories remain numeric rather than borrowing an unrelated enum.
        0x4C => (
            "Component interface whose numeric input receives this modifier. The input number is local to that interface. The receiving object must provide the selected component.",
            &[
                (0, "Weapon Controller"),
                (1, "Magazine"),
                (2, "Barrel"),
                (5, "Movement"),
                (6, "Health and Shields"),
                (9, "Abilities"),
                (13, "Player Stats"),
                (14, "Weapon Stats"),
            ],
        ),
        _ => return None,
    };
    Some(FieldMeaning { help, choices })
}
