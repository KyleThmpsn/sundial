//! Stored component-property modifiers used by player buffs and ability attachments.
//! These describe native consumers, not recovered source names.

pub struct FieldMeaning {
    pub help: &'static str,
    pub choices: &'static [(i64, &'static str)],
}

/// The settings record of one component-property modifier.
pub const SETTINGS_SCHEMA: u32 = 0x8080_3B06;
/// The amount the modifier adds, or the factor it multiplies by.
pub const AMOUNT_OFFSET: u32 = 0x28;
/// Whether the amount adds or multiplies.
pub const OPERATION_OFFSET: u32 = 0x2C;
/// The numeric input within the selected component.
pub const INPUT_OFFSET: u32 = 0x4A;
/// The component interface whose input receives the modifier.
pub const COMPONENT_OFFSET: u32 = 0x4C;
/// The operation byte that adds the amount.
pub const OPERATION_ADD: i64 = 0;
/// The operation byte that multiplies by the amount.
pub const OPERATION_MULTIPLY: i64 = 1;
/// The component interface of the weapon's magazine. Its inputs 0 and 3 are magazine size, by
/// the stock perks that move them (Adaptive Frame -18, Rat Pack +6, Harsh Truths x1.667 for its
/// 5-round bursts), and input 1 is reserves (Large Arms Reserves and the reserve mods).
pub const MAGAZINE_COMPONENT: i64 = 1;
/// The component interface of the weapon's barrel.
pub const BARREL_COMPONENT: i64 = 2;
/// Barrel inputs that set how many rounds one pull of the trigger fires.
pub const BARREL_ROUNDS_PER_BURST: [i64; 4] = [22, 23, 24, 25];
/// Barrel inputs that set how quickly the weapon fires: Rate of Fire 1 to 4 in shots per second,
/// then Time Between Shots 1 and 2 in seconds.
pub const BARREL_FIRE_TIMING: [i64; 6] = [0, 1, 2, 3, 4, 5];

/// Exact schema/offset lookup. A similar-looking field on another type gets no meaning.
pub fn field_meaning(schema: u32, offset: u32) -> Option<FieldMeaning> {
    if schema != SETTINGS_SCHEMA {
        return None;
    }
    let (help, choices): (_, &'static [(i64, &'static str)]) = match offset {
        // F40810 reads +28/+2C and the instance's active weight. 36A100 is lerp.
        AMOUNT_OFFSET => (
            "Add contributes this value multiplied by the active weight. Multiply blends from 1 to this factor using the active weight, then multiplies the input. At full weight, Add 3 adds 3 and Multiply 3 triples the input. The selected component and input determine the units.",
            &[],
        ),
        OPERATION_OFFSET => (
            "Add changes the selected input by a weighted amount. Multiply applies a weighted factor. Other operation bytes leave the input unchanged in this native consumer.",
            &[(0, "Add"), (1, "Multiply")],
        ),
        // CA9850's category 9 branch uses +48 to select the ability before
        // dispatching +4A through 186B250. Stock Abyssal Extractors corroborates slots.
        // The runtime survey agrees: all 19 stock Abilities records use 0, 1, 2 or 7, and all
        // 117 records for other components hold -1.
        0x48 => (
            "Ability selected when Component is Abilities. Other components do not read it. Unidentified values are preserved.",
            &[
                (-1, "None"),
                (0, "Grenade"),
                (1, "Super"),
                (2, "Melee"),
                (3, "Jump"),
                (7, "Class Ability"),
            ],
        ),
        INPUT_OFFSET => (
            "Numeric input within the selected component.\nHealth and Shields: 0 = Shield Capacity, 1 = Shield Regeneration Delay, 2 = Shield Regeneration Duration, 3 = Health Capacity, 4 = Health Regeneration Delay, 5 = Health Regeneration Duration. Delays and durations are in seconds before player scaling. Halving a positive duration doubles the regeneration rate. A final duration at or below 0.0001 stops that update.\nAbilities: input 0 participates in recharge and input 4 controls the activation lockout.\nOther components have different input tables. Player Stats and Weapon Stats consume both bytes, while other mapped categories consume the low byte. Do not transfer input names between components.",
            &[],
        ),
        // CA2830's exact dispatch table resolves these interfaces. Unknown
        // categories remain numeric rather than borrowing an unrelated enum.
        COMPONENT_OFFSET => (
            "Component interface whose numeric input receives this modifier. The input number is local to that interface. The receiving object must provide the selected component.",
            &[
                (0, "Weapon Controller"),
                (1, "Magazine"),
                (2, "Barrel"),
                // Every stock perk that uses category 3 describes reload speed: Outlaw, Feeding
                // Frenzy, Drop Mag, Field Prep, Alloy Magazine, Rapid Hit and Underdog among 21.
                (3, "Reload"),
                // Category 4 perks describe charge and draw time: Conserve Momentum ("lessen
                // charge time"), Backup Plan, Archer's Tempo and Adamantine Brace.
                (4, "Charge"),
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

/// The order a table of these records reads in: what changes, then how.
#[must_use]
pub fn column_order(schema: u32) -> Option<&'static [u32]> {
    (schema == 0x8080_3B06).then_some(&[0x4C, 0x48, 0x4A, 0x2C, 0x28][..])
}

/// Whether a stored value leaves its field unread. A record for a component other than
/// Abilities holds -1 as its ability, which no consumer reads.
#[must_use]
pub fn unread(schema: u32, offset: u32, value: i64) -> bool {
    schema == 0x8080_3B06 && offset == 0x48 && value < 0
}

/// Names for a record's input, which is local to the component it changes. Health and
/// Shields and Abilities follow the traced consumer above. The rest follow the stock perks
/// that set them, in the game's own words. Inputs that several perks move together share a
/// name and a number, since the perks establish what they change but not how the lanes
/// divide it. An input whose perks disagree, or that no described perk sets, keeps its number.
///
/// Weapon Controller:
/// - Ready Time by Mecha Holster ("instant ready", its only timing input). Ready Time 2 by
///   Freehand Grip and Sprint Grip ("ready speed"), which set both, and eleven handling perks.
/// - Stow Time by Field Prep ("stow, and ready") and Illegally Modded Holster ("stowed"),
///   which set it and Ready Time but not Ready Time 2.
/// - Zoom by Rangefinder and Perk 384, the only two perks that set it, both about zoom.
/// - Aim Down Sights Time by Snapshot Sights ("faster time to aim down sights", its only
///   input) and Sprint Grip.
/// - Range and Range 2 move together under Killing Wind ("weapon range"), Magnificent Howl,
///   Opening Shot and The Roadborn, and Shrapnel Launcher ("short-range") halves both.
/// - Effective Range and Effective Range 2 are what Rangefinder and Perk 384 ("effective
///   range") raise, with Box Breathing and Killing Wind.
/// - The three Aim Assist Angles hold radians. `golden_gun_aim_assist`, the engine name of the
///   buff Perk 27 attaches, adds one degree and two half degrees, The Roadborn ("bonus handling,
///   range") and Harmonic Laser widen the first, and Micro-Missile ("fires in a straight line")
///   narrows all three. That buff and Micro-Missile also move Effective Range, Effective Range 2
///   and inputs 12 and 19 but never Range or Range 2, and the Barrel has its own Spread.
/// - Effective Range 3 to 6 (inputs 12, 14, 19 and 21) are the distances beside Effective Range
///   and Effective Range 2, and every described perk moves them the same way: Shrapnel Launcher
///   ("short-range") cuts all six by the same factor, The Roadborn ("range") raises 12 with 13,
///   Looks Can Kill cuts 12 with 13, and `golden_gun_aim_assist` adds 250 to inputs 12 and 13,
///   50 to input 19 and 20 to input 20. Rangefinder raises only 13 and 20, so those keep the
///   first two names. Input 22 stays numbered: Micro-Missile adds one to it and Release the
///   Wolves cuts it, and neither description says what it holds.
///
/// Barrel:
/// - Rate of Fire 1 to 4 rise with Thunderer ("significantly increased rate of fire"),
///   Aggressive Frame, Spinning Up, Lightning Rounds and Onslaught.
/// - Time Between Shots falls under the same perks: Desperado ("increases your rate of fire")
///   subtracts 0.23 and Thunderer multiplies by 0.35.
/// - Rounds per Burst: the four-round, five-round and three-round burst perks add exactly the
///   rounds their descriptions give.
/// - Accuracy 1 to 6 fall under Opening Shot, Eye of the Storm and Firmly Planted ("more
///   accurate"). Hip-Fire Accuracy rises with Freehand Grip, Hip-Fire Grip and Payday ("hip fire
///   accuracy"). Airborne Accuracy falls under Icarus Grip ("accuracy while airborne") and Tome
///   of Dawn ("holds you in midair").
/// - Damage Bonus and Damage Bonus 2 add Rampage's, Memento Mori's and Broadside's damage.
/// - Precision Damage is Headseeker's only modifier ("increase precision damage"), and Box
///   Breathing ("bonus range and precision damage") and Target Acquired ("more precision
///   damage") raise it beside the lanes their other effects name. Magnificent Howl, Ravenous
///   Beast and Release the Wolves cut it while raising input 32 for their empowered shots.
/// - Spread widens under Broadside ("more spread") and narrows under Spread Shot Package
///   ("reduces the spread").
/// - Projectile Speed by Micro-Missile ("massively increased projectile speed") and
///   Rangefinder ("increased projectile velocity").
/// - Projectile Drop is the one lane Micro-Missile sets that its speed does not account for.
///   It cuts the lane to a hundredth, and the rest of its description is "fires in a straight
///   line".
/// - Recoil 1 to 4 rise under Aggressive Frame ("high recoil") and fall under Precision Frame
///   ("recoil pattern") and Firmly Planted ("stability").
/// - Flinch and Flinch 2 fall under No Distractions and Perk 521 ("reduces flinch").
/// - Target Acquisition by Hip-Fire Grip ("precision hit targeting") and Gift of the Traveler.
///
/// Reload: Drop Mag, Field Prep and Rapid-Fire Frame shorten all three Reload Times.
/// Charge: Charge Time by Backup Plan and Conserve Momentum ("charge time") and Archer's Tempo
/// ("draw time"). Hold Time and Hold Time 2 by Sneak Bow ("increases hold time") and Adamantine
/// Brace ("charges can be held indefinitely").
/// Movement: Move Speed 1 to 5 by Lightweight Frame ("move faster"), MIDA Multi-Tool ("move
/// speed"), Assassin's Blade and The Scientific Method. Strange Protractor ("buffs sprint")
/// adds to Move Speed 4 and 5 and to two inputs no move speed perk sets, read as the Sprint
/// Speeds.
///
/// Player Stats: Mobility by Traction and Killing Wind ("increased mobility"), Lightweight
/// Frame and MIDA Multi-Tool. Recovery by Last Stand ("greatly increased recovery").
///
/// Weapon Stats:
/// - Stability by Dynamic Sway Reduction, Hip-Fire Grip, Firmly Planted and Slideshot.
/// - Range by Business Time, Opening Shot, Slideshot and Killing Wind.
/// - Reload Speed by Outlaw, Feeding Frenzy, Field Prep and Threat Detector.
/// - Inventory Size by Light Arms Reserves and Rifle Reserves.
/// - Handling by Quickdraw, Mecha Holster, Backup Plan and Firmly Planted.
/// - Aim Assistance by Target Acquired, Moving Target, Hip-Fire Grip and Opening Shot.
/// - Rounds Per Minute by Dual Speed Receiver and Perk 339 ("rate of fire slows"), which
///   subtract 100.
/// - Charge Rate by Lucent Blade ("charge rate"), Backup Plan and Adamantine Brace.
#[must_use]
pub fn input_choices(component: i64) -> &'static [(i64, &'static str)] {
    match component {
        0 => &[
            (0, "Ready Time"),
            (1, "Ready Time 2"),
            (2, "Stow Time"),
            (3, "Zoom"),
            (4, "Aim Down Sights Time"),
            (9, "Range"),
            (10, "Range 2"),
            (11, "Aim Assist Angle"),
            (12, "Effective Range 3"),
            (13, "Effective Range"),
            (14, "Effective Range 4"),
            (15, "Aim Assist Angle 2"),
            (17, "Aim Assist Angle 3"),
            (19, "Effective Range 5"),
            (20, "Effective Range 2"),
            (21, "Effective Range 6"),
        ],
        2 => &[
            (0, "Rate of Fire"),
            (1, "Rate of Fire 2"),
            (2, "Rate of Fire 3"),
            (3, "Rate of Fire 4"),
            (4, "Time Between Shots"),
            (5, "Time Between Shots 2"),
            (6, "Accuracy"),
            (7, "Accuracy 2"),
            (8, "Accuracy 3"),
            (9, "Accuracy 4"),
            (14, "Accuracy 5"),
            (15, "Accuracy 6"),
            (16, "Airborne Accuracy"),
            (22, "Rounds per Burst"),
            (23, "Rounds per Burst 2"),
            (24, "Rounds per Burst 3"),
            (25, "Rounds per Burst 4"),
            (30, "Hip-Fire Accuracy"),
            (31, "Hip-Fire Accuracy 2"),
            (36, "Precision Damage"),
            (37, "Damage Bonus"),
            (38, "Damage Bonus 2"),
            (39, "Spread"),
            (43, "Projectile Speed"),
            (48, "Projectile Drop"),
            (54, "Recoil"),
            (56, "Recoil 2"),
            (58, "Recoil 3"),
            (60, "Recoil 4"),
            (70, "Flinch"),
            (71, "Flinch 2"),
            (72, "Target Acquisition"),
        ],
        3 => &[
            (0, "Reload Time"),
            (1, "Reload Time 2"),
            (2, "Reload Time 3"),
        ],
        4 => &[(0, "Charge Time"), (1, "Hold Time"), (2, "Hold Time 2")],
        5 => &[
            (0, "Move Speed"),
            (1, "Move Speed 2"),
            (3, "Move Speed 3"),
            (4, "Move Speed 4"),
            (5, "Move Speed 5"),
            (6, "Sprint Speed"),
            (7, "Sprint Speed 2"),
        ],
        6 => &[
            (0, "Shield Capacity"),
            (1, "Shield Regeneration Delay"),
            (2, "Shield Regeneration Duration"),
            (3, "Health Capacity"),
            (4, "Health Regeneration Delay"),
            (5, "Health Regeneration Duration"),
        ],
        9 => &[(0, "Recharge"), (4, "Activation Lockout")],
        13 => &[(1, "Mobility"), (2, "Recovery")],
        14 => &[
            (0, "Rounds Per Minute"),
            (5, "Stability"),
            (7, "Range"),
            (9, "Reload Speed"),
            (10, "Charge Rate"),
            (13, "Inventory Size"),
            (14, "Handling"),
            (17, "Aim Assistance"),
        ],
        _ => &[],
    }
}

/// Names for a value that depend on another value of its record: a property adjustment's
/// input by the component it changes. `row` holds the record's numbers by offset.
#[must_use]
pub fn row_choices(
    schema: u32,
    offset: u32,
    row: &[(u32, i64)],
) -> Option<&'static [(i64, &'static str)]> {
    if schema != 0x8080_3B06 || offset != 0x4A {
        return None;
    }
    let component = row.iter().find(|(at, _)| *at == 0x4C)?.1;
    let choices = input_choices(component);
    (!choices.is_empty()).then_some(choices)
}
