//! Value meanings established by the native consumers, separate from storage types.
use super::{Field, Format};

pub struct ValueContract {
    pub description: &'static str,
    pub suffix: &'static str,
    pub choices: &'static [(u8, &'static str)],
    /// Values the installed stock perks set for this byte, most used first. Present when a
    /// selector has no recovered names, so a user can still pick a value the engine is
    /// known to accept instead of guessing inside 0 to 255.
    pub observed: &'static [(u8, u32, &'static str)],
}

// F04A50's input contract is shared by these specific consumers. It is not
// the input contract of every value program (notably Named Property).
const COMMON_INPUTS: &[(u8, &str)] = &[
    (0, "Action Numeric State"),
    (1, "Ammunition Complement"),
    (2, "Current Ammunition Value"),
    (3, "Native Stat Slot 1"),
    (4, "Native Stat Slot 2"),
    (5, "Native Stat Slot 3"),
    (6, "Component Byte 401"),
    (7, "Component Byte 402"),
    (8, "Component Bytes 403 + 404"),
    (9, "Component Byte 403"),
    (10, "Component Byte 404"),
    (11, "Selected Object Value"),
    (12, "Mapped Object Property Value"),
];

/// This does not infer selector meanings from a field's numeric representation.
pub fn contract(class: u32, field: &Field) -> ValueContract {
    let mut result = ValueContract {
        description: match field.format {
            Format::Byte => "Native selector. Its gameplay values have not been identified.",
            Format::Flag => "False is 0. True is 1.",
            Format::Float => {
                "Stored as a 32-bit float. Its gameplay units have not been identified."
            }
            Format::Integer => "Signed 32-bit integer. Negative values are preserved.",
            Format::Unsigned => "Unsigned 32-bit integer. All 32 bits are preserved.",
            Format::Mask32 => {
                "A 32-bit mask. Set bits select matching event bits. Individual bit meanings are unresolved."
            }
            Format::Key => "Native 32-bit name key. This is an identifier, not an amount.",
            Format::Tag => "Native package resource identifier.",
            Format::Bytes => "Uninterpreted native bytes. No gameplay meaning is established.",
            Format::Pointer => "Native allocation reference maintained by the compiler.",
        },
        suffix: "",
        choices: &[],
        observed: if field.format == Format::Byte {
            super::stock_values::observed(class, field.offset)
        } else {
            &[]
        },
    };
    match (class, field.offset) {
        (0x80803E44, 0x50) | (0x80803E4D, 0x48) | (0x80802F18, 0x38) | (0x808029EC, 0x6B) => {
            result.description = "Source passed through F04A50 to the value program. Ammunition inputs use native accessor values and optional normalization. Native stat slots and component-byte roles remain unresolved. Unsupported selectors or missing data use the caller's fallback.";
            result.choices = COMMON_INPUTS;
        }
        // Kind 3's spawn position. The workbench's own typed spawn action writes this byte
        // and reads it back, so both values are established by round trip, not inference.
        (0x80803E43, 4) => {
            result.description = "Where the spawned object appears. The owner position is the player. The event position is the place the triggering event reports, such as a defeated enemy.";
            result.choices = &[(0, "At the Owner"), (1, "At the Event")];
        }
        // Kind 8's ability target. Several stock perks identify each slot independently; see
        // `action::roles`. A census of all 11 selector, flag and option combinations the
        // stock perks use shows the selector alone decides the ability, so the names hold
        // whatever the other two bytes are.
        (0x80803E4D, 2) => {
            result.description = "Which ability the adjustment applies to. Each slot is identified by several stock perks, and the flag and option bytes do not change it. Other values are unidentified and keep their number.";
            result.choices = &[
                (0, "Grenade Energy"),
                (1, "Super Energy"),
                (2, "Melee Energy"),
                (7, "Class Ability Energy"),
            ];
        }
        // Kind 6's mode is the weapon's damage type. Three stock plugs that set it are
        // named after the element they apply (Solar, Arc and Void Damage Mod), The
        // Fundamentals sets all three and its own description reads "Change this weapon's
        // damage type", and Play with Your Prey sets 1 and 3 while describing a Solar and a
        // Void rocket. Mode 0 is set only by Bad Juju, a kinetic weapon.
        (0x80803E41, 2) => {
            result.description = "The weapon's damage type. Established by the stock plugs that set each value: Solar, Arc and Void Damage Mod name their element, and The Fundamentals, which sets all three, is described in game as changing the weapon's damage type.";
            result.choices = &[(0, "Kinetic"), (1, "Solar"), (2, "Arc"), (3, "Void")];
        }
        // Kind 8's flag byte is a separate axis from the ability. Every combination of it and
        // the target selector adjusts the ability the selector names, so it does not change
        // the target. What it does select is not established.
        (0x80803E4D, 3) => {
            result.description = "A second setting on the adjustment. It does not change which ability is adjusted, which the target selector alone decides. Its own values are not identified.";
        }
        // Condition kind 6's second mask is the ammunition type of the pickup that raised the
        // event. Three stock perks are named after the bit they set and say so in their own
        // description: Primary Ammo Scavenger sets 1, Special Ammo Scavenger sets 2 and Heavy
        // Ammo Scavenger sets 4. Every other perk on each bit agrees, by weapon family: the
        // Auto Rifle, Pulse Rifle, Scout Rifle, Sidearm and Submachine Gun Scavengers sit on
        // 1; the Fusion Rifle, Grenade Launcher, Shotgun and Sniper Rifle Scavengers, which
        // all read "Special ammo", sit on 2; and the Rocket Launcher, Sword and Machine Gun
        // Scavengers, with Lead from Gold, sit on 4.
        (0x80803DFB, 9) => {
            result.description = "Which ammunition the pickup carried. Each bit is named by a stock perk that sets it alone, and a value with several bits accepts any of those types.";
            result.choices = &[(1, "Primary"), (2, "Special"), (4, "Heavy")];
        }
        // The ability mask says which ability raised the event, so it is what makes a perk
        // fire. Two condition classes carry it with the same layout. Each bit is identified
        // by the stock perks that set it: value 1 by grenade perks ("throwing grenades grants
        // energy", "improved charging for Void grenades"), value 2 by super perks
        // ("activating an Arc Super", "ability energy on Super cast", "casting your Super"),
        // and value 128 unanimously by class ability perks. On class 0x80803E01 those are
        // Dodge, Barricade and four perks reading "when using your class ability"; on class
        // 0x80803DFD they are Solar Rampart, Planetary Torrent and Burning Souls, which name
        // the Barricade, the Rift and the Dodge, one class ability per class.
        (0x80803E01, 8) => {
            result.description = "Which ability raised the event. The bits are identified by the stock perks that set them. A value with several bits accepts any of those abilities.";
            result.choices = &[(1, "Grenade"), (2, "Super"), (128, "Class Ability")];
        }
        (0x80803DFD, 8) => {
            result.description = "Which ability raised the event. Class Ability is identified by the stock perks that set it on this condition. Grenade and Super carry over from the sibling condition that shares this layout, and no stock perk sets them here. A value with several bits accepts any of those abilities.";
            result.choices = &[(1, "Grenade"), (2, "Super"), (128, "Class Ability")];
        }
        // Kind 7's ability slot. The slot numbering reproduces kind 8's, which four stock
        // perks establish independently (see `action::roles`), and the perks that set each
        // value here agree: grenade perks on 0 (Fastball, And Another Thing, Bring the
        // Heat), melee perks on 2 (Cross Counter, Scissor Fingers, The Whispers) and Double
        // Dodge on 7. Values with no agreeing set keep their number.
        (0x80803E1D, 2) => {
            result.description = "Which ability bank holds the property. Slots 0, 2 and 7 are each identified by several stock perks and match the numbering kind 8 uses. Other values are unidentified and keep their number.";
            result.choices = &[
                (0, "Grenade"),
                (1, "Super"),
                (2, "Melee"),
                (7, "Class Ability"),
            ];
        }
        // Kind 10's target. Every stock perk that sets 2 changes a fired projectile
        // (Cluster Bomb, Timed Payload, Explosive Payload and the grenade-payload perks),
        // which identifies that value. The other values carry no such agreeing set.
        (0x808029ED, 2) => {
            result.description = "What the named property is read from. Value 2 is identified: every stock perk that uses it changes the fired projectile, as Explosive Payload and the grenade payload perks do. The other values are unidentified and keep their number.";
            result.choices = &[(2, "The Fired Projectile")];
        }
        // Kind 15's capacity basis, established the same way: the perks on 0 are the
        // Scavenger and Armaments families, which grant reserves, and the perks on 1 are
        // Grave Robber, Together Forever and Relentless Strikes, which fill the magazine.
        (0x80803E3E, 0x6A) => {
            result.description = "Which capacity the share is taken from. Every stock perk that stores 0 describes granting reserves, and every perk that stores 1 describes filling the magazine.";
            result.choices = &[(0, "Reserve Capacity"), (1, "Magazine Capacity")];
        }
        (0x808029EC, 0x69) => {
            result.description = "Applicable slot-program outputs are added and multiplied by this capacity. Zero uses magazine plus reserve capacity. Any nonzero value uses magazine capacity. Transfer still obeys reserve availability and ammunition-unit rounding.";
            result.choices = &[(0, "Magazine + Reserves"), (1, "Magazine")];
        }
        (0x808029ED, 0x48) => {
            result.description = "Named Property uses its own input contract, read from the client. 0 supplies zero, 1 reads the selected target, and 2 reads the activation-context object. Stock perks set 0 and 2 only. This differs from the action-state and ammunition input selectors used by other effects.";
            result.choices = &[
                (0, "Zero"),
                (1, "Selected Target Value"),
                (2, "Activation Object Value"),
            ];
        }
        // Kind 23's event byte, read off which slot each stock perk uses it in. The twelve
        // aiming perks (Rangefinder, Moving Target, Slug Rifle, Dual Speed Receiver, Spread
        // Shot Package, Tracking Module and more) start on 1 and end on 0. Hip-Fire Grip
        // and Freehand Grip, which apply while not aiming, start on 0 and end on 1.
        (0x80803DDA, 8) => {
            result.description = "Whether aiming down sights has started or stopped. Every stock aiming perk starts on 1 and ends on 0, and the hip fire perks do the reverse.";
            result.choices = &[(1, "Aiming Started"), (0, "Aiming Stopped")];
        }
        // Kind 22's event byte: Field Prep, Firmly Planted and Sneak Bow, the crouching
        // perks, all start on 1 and end on 0.
        (0x80803DF9, 8) => {
            result.description = "Whether crouching has started or ended. Field Prep, Firmly Planted and Sneak Bow all start on 1 and end on 0.";
            result.choices = &[(1, "Crouching Started"), (0, "Crouching Ended")];
        }
        // Kind 42's event byte: Bulwark Finisher and Empowered Finish read the finisher's
        // final blow and start on 1. Reactive Pulse, an overshield while performing the
        // finisher, starts on 0 and ends on 2.
        (0x808029E6, 8) => {
            result.description = "Which part of a finisher fires the event. Bulwark Finisher and Empowered Finish read the final blow on 1. Reactive Pulse, active while performing a finisher, starts on 0 and ends on 2.";
            result.choices = &[
                (0, "Finisher Started"),
                (1, "Finisher Final Blow"),
                (2, "Finisher Ended"),
            ];
        }
        // Kind 27's mode: Mulligan, Reversal of Fortune and Looks Can Kill, the perks that
        // read a missed shot, set 1. Nine perks that fire on any shot set 0. Taken Predator
        // alone sets 3, so that value keeps its number.
        (0x808029E0, 0x90) => {
            result.description = "Which shots fire the event. Mulligan and Reversal of Fortune, which read a missed shot, set 1. Tap the Trigger, Box Breathing and seven others set 0. Value 3 is set by Taken Predator alone and keeps its number.";
            result.choices = &[(0, "Any Shot"), (1, "Missed Shot")];
        }
        // Kind 9's slot bits reproduce kind 8's ability numbering: bit 1 is set by Resolute
        // ("casting Fists of Havoc") and Volatile Conduction ("Arc Super ... cast"), bit 7 by
        // Aeon Energy ("dodging"). Bits 2 and 3 are each set by one undescribed perk.
        (0x80803E00, 8) => {
            result.description = "Which ability slot's bit the event must carry. Super is identified by Resolute and Volatile Conduction, Class Ability by Aeon Energy. Other bits are unidentified and keep their number.";
            result.choices = &[(2, "Super"), (128, "Class Ability")];
        }
        // A one-byte damage type record nested in kill, damage and accumulator conditions.
        // Its numbering is kind 6's: Crystalline Transistor ("Kinetic precision kills") sets
        // 0, the Solar grenade disruption perk sets 1, Transfusion Matrix ("Arc melee
        // kills"), Conduction Tines ("Arc ability kills") and the Arc grenade perk set 2,
        // and Horns of Doom ("Void melee kills"), Abyssal Extractors ("Void kills") and
        // Oppressive Darkness ("Void grenade") set 3. Powered melee perks such as Vanishing
        // Execution carry one row each for 1, 2 and 3.
        (0x80806B02, 0) => {
            result.description = "The damage type the event must carry, numbered as the weapon damage type: Crystalline Transistor reads Kinetic on 0, Transfusion Matrix and Conduction Tines Arc on 2, Horns of Doom and Oppressive Darkness Void on 3. Value 4 is set by Arc Conductor alone and keeps its number.";
            result.choices = &[(0, "Kinetic"), (1, "Solar"), (2, "Arc"), (3, "Void")];
        }
        // A one-byte enemy faction record nested in the damage condition. Each value is set
        // by exactly one Repurposing mod, and each mod names its faction.
        (0x80806829, 0) => {
            result.description = "The enemy faction the damaged target must belong to. Fallen Repurposing sets 2, Hive Repurposing 4 and Taken Repurposing 5, each reading \"destroying a <faction> shield\". Other values are unidentified and keep their number.";
            result.choices = &[(2, "Fallen"), (4, "Hive"), (5, "Taken")];
        }
        // The general predicate's player state mask. Every stock perk that sets a bit reads
        // as that state: Icarus Grip, Air Assault, Mask Upgrade, Peregrine Strike and Tome of
        // Dawn ("airborne", "in midair") set 2, Slideshot and Slideways ("sliding") set 4,
        // Rapid Cooldown, Tesseract and Sprint Grip ("sprinting") set 8.
        (0x80803DCE, 0x38) | (0x80803DCC, 0x38) => {
            result.description = "The player state this predicate requires. Airborne is set by Icarus Grip, Air Assault and Mask Upgrade, Sliding by Slideshot and Slideways, Sprinting by Rapid Cooldown and Tesseract. Values 1 and 32 are set by undescribed perks and keep their number.";
            result.choices = &[(2, "Airborne"), (4, "Sliding"), (8, "Sprinting")];
        }
        // The weapon state at +81. Value 4 is set by Upgraded Sensor Pack, Box Breathing,
        // MIDA Radar and Tome of Dawn, all reading "while aiming". Values 0, 1, 2 and 5 split
        // perks that share no reading.
        (0x80803DCE, 0x81) | (0x80803DCC, 0x81) => {
            result.description = "The weapon state this predicate requires. Aiming Down Sights is set by Upgraded Sensor Pack, Box Breathing, MIDA Radar and Tome of Dawn. The other values are set by perks that share no reading and keep their number.";
            result.choices = &[(4, "Aiming Down Sights")];
        }
        // Key fields whose stock values are named by the perks that use them; the names
        // themselves live in `fields::keys`.
        (0x80803DEA, 8) => {
            result.description = "The event value this condition matches. The named values are established by the stock perks that listen to them, such as the Orb of Light pickup Innervation and Recuperation read.";
        }
        (0x80803DEA, 0xC) => {
            result.description = "The context key resolved against the matched event. Each named value pairs with the event value of the same name.";
        }
        (0x80803DEC, 8) => {
            result.description = "The signal this condition passes on. The named values are established by the stock perks that start on them, such as collecting a Warmind Cell.";
        }
        (0x80803DEB, 8) => {
            result.description = "The signal that ends the effect. The compiler writes an always-active effect's own end key here, and the named values are the signals stock perks end on.";
        }
        (0x80803E1C, 4) => {
            result.description = "The key written over the weapon property while the effect is active. Every stock use writes the full auto key.";
        }
        (0x80803E4D, 8) => {
            result.description = "Adjustment equals this scale multiplied by the value-program output. The result uses the selected component's native units. Do not assume every target uses percentages."
        }
        // EC500C..EC5080 bypasses the cap for a negative limit, otherwise
        // restricts positive or negative movement toward the configured value.
        (0x80803E4D, 12) => {
            result.description = "Negative values disable the limit. Zero is a real limit. With a nonnegative limit, the adjustment only moves toward that value without overshooting it."
        }
        // 107CAAF..107CB30 selects the pass/fail lane, then adds (0),
        // replaces (1) or multiplies (2) the running accumulator value.
        (0x80803E32, 8 | 20) => {
            result.description = "Operation on the counter when this condition passes or fails. Add uses a delta, Replace uses an absolute value, Multiply uses a factor.";
            result.choices = &[(0, "Add"), (1, "Replace"), (2, "Multiply")];
        }
        (0x80803E32, 9 | 21) => {
            result.description =
                "Use the child condition's numeric event result instead of its literal value."
        }
        (0x80803E32, 12 | 24) => {
            result.description = "Literal counter contribution. Add 1 counts one event. Replace 0 clears the counter. Multiply 1 leaves it unchanged. Ignored when the corresponding event-value flag is enabled."
        }
        (0x80803E22, 0) => {
            result.description = "Index of the event numeric slot. Slots 0 through 3 are decoded. Their gameplay meaning depends on the event source."
        }
        (0x80803E22, 4) => {
            result.description = "Used only with stat selector 255. Assignment replaces the selected event slot. Multiplication scales it, where 1 leaves it unchanged and 0 clears it."
        }
        (0x80803E22, 8) => {
            result.description = "255 uses the literal value. Other values select a native stat, whose individual identities remain unresolved.";
            result.choices = &[(255, "Literal Value")];
        }
        (0x80802F1A, 0) => {
            result.description = "The event scalar is multiplied by 1 + this value. 0 leaves it unchanged, 0.25 gives 1.25 times the original, and -1 gives zero."
        }
        (0x80802F1A, 4) => {
            result.description = "Positive values supply an alternate scalar adjustment. Zero or negative values fall back to the default adjustment. The multiplier is 1 + the selected adjustment."
        }
        (0x80803E42, 2) => {
            result.description =
                "Select the owner or activation-event position for the generation request.";
            result.choices = &[(0, "Owner Position"), (1, "Event Position")];
        }
        (0x80803E42, 4) => {
            result.description =
                "Number of Orbs of Light requested by the player-recipient generation path."
        }
        (0x80803E42, 8) => {
            result.description = "Native value supplied to orb generation. Its conversion to Super energy has not been established."
        }
        (0x80803E42, 0x18) => {
            result.description = "Resource reference supplied to orb generation. Changing it is not confirmed to change the pickup type. Use a spawn action for health pickups and other objects."
        }
        (0x80803E30, 0x20) => {
            result.description = "Counter threshold that satisfies this condition, after applying contributions and clamping the stored value."
        }
        (0x80803E30, 0x24) => {
            result.description = "Counter reset threshold. Its effect depends on the accumulator's event and state path."
        }
        (0x80803E30, 0x28 | 0x2C) => {
            result.description = "Lower or upper clamp on the stored counter value."
        }
        // Confirmed by the in-game descriptions of every stock perk that sets this byte: all
        // 14 on 0 describe reserves ("bonus reserves when you pick up ammo"), and all 40 on
        // 1 describe the magazine ("return 1 round to the magazine").
        (0x80803E3F | 0x80803E3E, 0x68) => {
            result.description = "Ammunition destination. Zero selects reserves and one selects the magazine, which every stock perk's own in-game description agrees on. Other values have no identified storage contract.";
            result.choices = &[(0, "Reserves"), (1, "Magazine")];
        }
        (0x80803E3F, 0x6C..=0x84) => {
            result.description = "Signed ammunition contribution. The owning-weapon, matching weapon-slot and matching ammo-type amounts are added together. Positive values add ammunition, negative values remove it. Destination and scaling settings still apply."
        }
        (0x80803E3E, 0x6C..=0x84) => {
            result.description = "Proportional ammunition contribution. The owning-weapon, matching weapon-slot and matching ammo-type values are added together. 0.25 requests one quarter of the selected capacity. Destination and scaling settings still apply."
        }
        _ if matches!(
            field.label.as_str(),
            "Duration" | "Hold Duration" | "Extend By" | "Up To"
        ) =>
        {
            result.suffix = " s";
            result.description = match (class, field.offset) {
                (0x80803E32, 16) => {
                    "Seconds to retain a successful contribution. Positive values schedule the hold path. Zero does not schedule it."
                }
                (0x80803E3B, 4) => "Seconds added to eligible running timers, limited by Up To.",
                (0x80803E3B, 8) => {
                    "Maximum duration in seconds after extending eligible running timers."
                }
                _ => "Duration in seconds, stored as a 32-bit float.",
            };
        }
        _ if field.label == "Probability" => {
            result.description = "Literal probability when Probability Source is 255. 1 always passes, nonpositive values fail, and 0.25 means a 25% chance. Other sources ignore this literal."
        }
        _ if field.label == "Probability Source" => {
            result.description = "255 uses the literal probability. Other values obtain probability from a native stat whose identity remains unresolved.";
            result.choices = &[(255, "Literal Probability")];
        }
        _ => {}
    }
    result
}
