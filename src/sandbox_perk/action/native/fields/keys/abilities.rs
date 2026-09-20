//! Ability-bank properties corroborated by the stock actions and their native modifiers.
//!
//! These are bank lookups, not universal flags. A slot selects a bank, and the current
//! subclass must actually define the key. The native add/remove path returns failure for
//! missing keys. Keep subtype restrictions in the choice's help rather than promising
//! that a Dodge property adds charges to every class ability.
use super::EventKey;

// A key may be defined in several slots. Empty membership records an identified but
// inactive stock operation, so saved records retain their explanation without offering
// that operation as a working property.
macro_rules! properties {
    ($([$($slot:literal),*] => $key:expr),* $(,)?) => {
        pub(super) const ALL: &[EventKey] = &[$($key),*];
        const SLOTS: &[&[u8]] = &[$(&[$($slot),*]),*];
    };
}

properties! {
    // Grenade.
    [0] => EventKey {
        hash: 0xBDA0_ACD6,
        name: "Extra Grenade Charge",
        evidence: "Adds one grenade charge. All 21 stock grenade banks define this key with a +1 charge modifier. Used by And Another Thing, Fusion Harness and New Tricks.",
    },
    [0] => EventKey {
        hash: 0x804E_DDF2,
        name: "Increased Grenade Throw Distance",
        evidence: "Changes the grenade's throw configuration. Defined in all 21 stock grenade banks and used by Fastball, which increases grenade throw distance. Also used by Wish-Dragon Teeth, Helium Spirals and Bring the Heat. The launch settings depend on the grenade.",
    },
    [0] => EventKey {
        hash: 0x804E_DDF1,
        name: "Increased Grenade Blast Radius",
        evidence: "Adds 0.14 to explosion_radius_scalar, whose base value is 1.0, in all 21 stock grenade banks. Wish-Dragon Teeth independently identifies the increased blast radius. This changes the radius scalar, not grenade damage or energy returned.",
    },
    [0] => EventKey {
        hash: 0x776F_1AAC,
        name: "Increased Skip Grenade Blast Radius",
        evidence: "Sets explosion_radius_scalar to 1.1 in the Skip Grenade bank. Used by New Tricks. This replaces the scalar rather than adding to it. The extra charge and improved tracking use separate properties.",
    },
    [0] => EventKey {
        hash: 0x60C8_4B16,
        name: "Improved Skip Grenade Tracking",
        evidence: "Sets improved_tracking to 1 in the Skip Grenade bank. Used by New Tricks. Requires Skip Grenade and does not itself add a charge or return grenade energy.",
    },
    [0] => EventKey {
        hash: 0x7BC2_406C,
        name: "Longer Solar Grenades",
        evidence: "Sets dot_duration to 4 in the three stock Warlock Solar grenade banks. Helium Spirals identifies the longer Solar Grenade duration. This is the lingering-damage duration parameter, separate from grenade recharge and throw distance. The script consumes this value, so it is not a universal four-second lifetime for any grenade.",
    },
    [0] => EventKey {
        hash: 0xD10B_AB1B,
        name: "Additional Arcbolt Chains",
        evidence: "Sets additional_chain to 2 in the Arcbolt Grenade bank. Probability Matrix identifies the increased chaining. This property does not itself supply the separate chance to recharge the grenade.",
    },
    [0] => EventKey {
        hash: 0x99E0_42AE,
        name: "Fast-Throw Fusion Grenade Tracking",
        evidence: "Sets fast_throw_tracking to 1 in the Fusion Grenade bank. Used by Bring the Heat alongside its separate throw and impact properties. This selects tracking for that throw behavior and does not convert other grenades into Fusion Grenades.",
    },
    [0] => EventKey {
        hash: 0x9CF6_A045,
        name: "Consume Grenade for Arc Soul",
        evidence: "Sets enable_grenade_consume to 1, changes the ability modes and selects entity 0x80BC058C in four stock Arc grenade banks. Dynamic Duo identifies this combination as consuming the grenade for a supercharged Arc Soul. Requires a matching Arc grenade bank.",
    },
    // Super.
    [1] => EventKey {
        hash: 0x7BB6_1D8B,
        name: "Super Damage Resistance",
        evidence: "Applies the ability-bank damage-response modifier used by Actual Grandeur during Nova Bomb. This is the resistance property, not the separate energy refund on kills. Requires a Super bank that defines this property.",
    },
    [1] => EventKey {
        hash: 0x9FAE_DBFA,
        name: "Additional Shadowshot Shots",
        evidence: "Selects the Shadowshot bank's eight-shot configuration with its 12-second timing value. Uncanny Arrows identifies this as the additional Moebius Quiver shots. This property does not itself supply the separate Super-energy refund.",
    },
    [1] => EventKey {
        hash: 0x0E3A_A789,
        name: "Early Arc Staff Deactivation",
        evidence: "The property resolves to enable_cancel. It changes the activation mode and sets an Arc Staff script flag to 1. Used by Mobius Conduit to deactivate Whirlwind Guard early. Its guarding-cost and replacement-entity properties are separate.",
    },
    [1] => EventKey {
        hash: 0x2FDF_E308,
        name: "Single-Shot Golden Gun",
        evidence: "Selects replacement entity 0x80BAA312 in both stock Golden Gun banks. Hawkeye Hack identifies this variant as a single high-damage shot. This selects the authored Golden Gun variant rather than applying a universal shot-count or damage multiplier to other Supers.",
    },
    // Melee.
    [2] => EventKey {
        hash: 0xD80D_2810,
        name: "Extra Melee Charge",
        evidence: "Adds one charge in the Warlock melee bank. Used by The Whispers. The key is absent from the other stock melee banks, so it does not add a charge to every class's melee.",
    },
    [2] => EventKey {
        hash: 0xA2C6_D0FB,
        name: "Extra Throwing Knife Charge",
        evidence: "Adds one charge in the Hunter melee bank. Scissor Fingers uses this property for its second throwing knife. Requires the corresponding throwing-knife ability.",
    },
    [2] => EventKey {
        hash: 0x16B9_A58E,
        name: "Increased Melee Lunge Range",
        evidence: "Selects melee reach modifiers in the Titan and Warlock melee banks. Biotic Enhancements and Cobra Totemic independently identify the increased lunge range. The bank chooses the applicable range configuration. It is not a projectile-range modifier.",
    },
    [2] => EventKey {
        hash: 0x8247_8EC7,
        name: "Increased Melee Blast Radius",
        evidence: "Adds 0.14 to explosion_radius_scalar, whose base value is 1.0, in the Hunter melee bank. Used by Wish-Dragon Teeth. This affects a melee's explosion where that ability consumes the scalar, not its lunge distance or direct-hit damage. The other stock melee banks do not define this key.",
    },
    [2] => EventKey {
        hash: 0x9C89_99AF,
        name: "Arc Melee Damage",
        evidence: "The key resolves to enable_arc_damage. In the Hunter melee bank it selects damage-type value 2, Arc, and enables the native damage-type override. It does not change the weapon's damage type or add a new melee ability.",
    },
    // Jump.
    [3] => EventKey {
        hash: 0x05EB_2F76,
        name: "Increased Blink Distance",
        evidence: "Raises the Blink distance setting to at least 7.5 native units in both stock Blink banks. Used by Move to Survive. This property does not include that perk's separate recharge, weapon-ready or radar behavior.",
    },
    [3] => EventKey {
        hash: 0x3FE0_B848,
        name: "Radar During Blink",
        evidence: "The property name blink_radar and its script parameter enable_radar both resolve exactly. It sets enable_radar to 1 in the Blink bank. Move to Survive independently confirms that Radar remains up. Blink distance and recharge use separate properties.",
    },
    // Movement.
    [4] => EventKey {
        hash: 0x5D21_CF68,
        name: "Increased Sprint Speed",
        evidence: "Raises the movement bank's sprint-speed setting to at least 8.5 native units and enables its associated script flag. Used by Rapid Cooldown, Hydraulic Boosters, Linear Actuators and other sprint-speed perks.",
    },
    [4] => EventKey {
        hash: 0xA329_CD86,
        name: "Improved Slide",
        evidence: "Selects slide mode 2 in the movement bank and enables its associated script flag. Reflective Vents identifies the improved slide, independently of the sprint and jump modifiers used alongside it by Hydraulic Boosters.",
    },
    [4] => EventKey {
        hash: 0xBDAB_B5A1,
        name: "Tighter Sprint Turning",
        evidence: "Writes the movement bank's turning setting to 0.8. Traction identifies the tighter turn radius while sprinting. This is separate from Traction's Mobility stat bonus.",
    },
    // Class ability.
    [7] => EventKey {
        hash: 0x8354_9A10,
        name: "Extra Dodge Charge",
        evidence: "Adds one charge in the Dodge bank and sets dodge_recharge_scalar to approximately 0.02136752. That value is a script parameter, not a Boolean flag. Used by Double Dodge. Does not add a Rift or Barricade charge.",
    },
    [0] => EventKey {
        hash: 0x927D_7EAD,
        name: "Longer Tripmine Duration",
        evidence: "Sets exotic_armor_extend_duration_enabled to 1 in the Tripmine bank. The property key also resolves to exotic_armor_extend_duration. Wish-Dragon Teeth confirms the longer duration. Blast radius and projectile selection are separate properties.",
    },
    [0] => EventKey {
        hash: 0x83E9_CE97,
        name: "Tripmine Projectile Override",
        evidence: "Adds the thermal_prox projectile 0x80BC03EE to the Tripmine launch configuration and enables its override flag. Used by Wish-Dragon Teeth. This selects a native projectile configuration. It is not the separately mapped duration or blast-radius scalar, and its remaining launch options have not been assigned gameplay names.",
    },
    [0] => EventKey {
        hash: 0x2F08_CD3D,
        name: "Detonate Grenades on Impact",
        evidence: "Resolves to explode_on_impact. Sets the same script parameter to 1 in the Fusion Grenade bank and changes the impact modifier in the Magnetic Grenade bank. Used by Bring the Heat. Requires one of these banks and does not add impact detonation to every grenade.",
    },
    [0] => EventKey {
        hash: 0x74B6_8D3C,
        name: "Improved Fusion Grenade Regeneration",
        evidence: "Resolves to exotic_armor_improved_regen. Adds 0.2 to the Fusion Grenade regeneration input. This is an additive recharge-rate modifier, not a 0.2-second cooldown. The additional grenade charge in Fusion Harness uses a different property.",
    },
    [1] => EventKey {
        hash: 0x7F4B_A6E0,
        name: "Sentinel Blinding and Overshields",
        evidence: "Enables script parameter 0x7417499F in the Sentinel bank. This is the live ability-bank switch used by Starless Night, whose behavior blinds nearby enemies and grants overshields through Ward of Dawn. The script owns the individual conditions. This is not a general blind or overshield action for unrelated abilities.",
    },
    [1] => EventKey {
        hash: 0x3C1A_A3C8,
        name: "Multiple Arc Staff Hits",
        evidence: "The property resolves to enable_multi_hit and sets multi_hit_enabled to 1 in the Arc Staff bank. The stock effect applies it after a dodge during the Super for five seconds. The property enables the multiple-hit behavior. A custom effect supplies its own trigger and duration.",
    },
    [1, 7] => EventKey {
        hash: 0x5055_A1E1,
        name: "Rift and Well Weapon Bonuses",
        evidence: "Enables the same script switch in the Rift and Well of Radiance banks. Alchemical Etchings uses it for weapon reload bonuses and the Empowering Rift range bonus. It selects the authored ability behavior rather than supplying independent reload or range multipliers.",
    },
    [1, 2] => EventKey {
        hash: 0x7CC8_FBDC,
        name: "Shared and Extended Sun Warrior",
        evidence: "Enables the same script parameter in the Hammer of Sol and Solar melee banks. Beacons of Empowerment uses it to extend Sun Warrior and share it with allies passing through Sunspots. Stock also attempts this key on grenades, but no stock grenade bank defines it. It does not create a Sunspot by itself.",
    },
    [1] => EventKey {
        hash: 0xB1A3_1197,
        name: "Reduced-Cost Whirlwind Guard",
        evidence: "Sets the Arc Staff guard switch and selects entity 0x8162C963. Its guard component differs from the standard variant in one tuning value, 0.03 instead of 0.06. Mobius Conduit identifies the replacement as removing the extra guarding cost. Stock applies this key while removing Standard Whirlwind Guard. Early deactivation is separate.",
    },
    [1] => EventKey {
        hash: 0x5DFF_DE8D,
        name: "Standard Whirlwind Guard",
        evidence: "Sets the Arc Staff guard switch and selects entity 0x80BC40B6, the standard guard configuration. Mobius Conduit removes this property while applying Reduced-Cost Whirlwind Guard. Applying both is not the stock configuration. A Remove operation is reversed when its effect ends.",
    },
    [2] => EventKey {
        hash: 0xBA4E_0300,
        name: "Titan Arc Melee Replacement",
        evidence: "Selects the Titan melee attack configuration using Arc damage profile 0x80BC32E6 with base damage 100. The stock effect activates on qualifying damage and lasts six seconds. This is an attack replacement. Its separate attached effect must not be attributed to this property.",
    },
    [2] => EventKey {
        hash: 0x6A02_D03D,
        name: "Increased Hunter Melee Lunge Range",
        evidence: "Selects the Hunter melee reach modifier with range 6.5 native units. The stock effect activates after a dodge outside the Super and lasts eight seconds. The property changes reach. A custom effect supplies its own trigger and duration.",
    },
    [2] => EventKey {
        hash: 0xCBE7_F856,
        name: "Melee Counterpunch",
        evidence: "Selects the Hunter counterpunch attack profiles 0x80BC31FC and 0x80BC31FD, with base damage 300 and Kinetic or Arc damage respectively. Cross Counter applies this while removing three normal melee properties. This is an attack replacement, not a universal 3x multiplier. The perk supplies its activation and other actions separately.",
    },
    [2] => EventKey {
        hash: 0xB632_5EAB,
        name: "Hunter Arc and Kinetic Melee",
        evidence: "Selects the normal Hunter Arc and Kinetic melee profiles 0x80BC31FF and 0x81A6B618, with base damage 100. Cross Counter temporarily removes this property so its counterpunch replacement can take over. It is a paired attack configuration, not a choice of weapon damage type.",
    },
    [2] => EventKey {
        hash: 0xB8F4_A574,
        name: "Hunter Kinetic Melee",
        evidence: "Selects the Hunter Kinetic attack profile 0x81A6B618 with base damage 100. Cross Counter temporarily removes this property along with its other normal melee replacements. The property selects the native attack configuration rather than changing the weapon.",
    },
    [2] => EventKey {
        hash: 0x0B2C_D7CE,
        name: "Hunter Arc Melee with a Hit Effect",
        evidence: "Selects Arc melee profile 0x80BC3201 with base damage 100 and the additional hit-effect set 0x80B80FB7. Cross Counter temporarily removes this property. The replacement and hit-effect references are decoded, but the individual gameplay consequences of that hit-effect set are not yet established.",
    },
    [3] => EventKey {
        hash: 0x2C3E_9751,
        name: "Fire During Lift and Glide",
        evidence: "Clears the ability weapon-restriction bit, mask 0x01, in both Lift and Glide banks. Jump Jets independently confirms firing during Lift. This does not change airborne accuracy or aiming bonuses by itself.",
    },
    [3] => EventKey {
        hash: 0xAC68_7DC6,
        name: "Improved Lift",
        evidence: "Resolves to exotic_armor_improved_lift. Changes two energy-use settings in the Lift bank to 0.2. Used by Jump Jets alongside the separate weapon-restriction property. This is Lift tuning, not an additional jump charge or a universal movement-speed multiplier.",
    },
    [3] => EventKey {
        hash: 0x90C8_BF09,
        name: "Improved Hunter Jump",
        evidence: "Resolves to exotic_armor_improved_jump. Selects the improved Hunter jump mode through two native mode records. Hydraulic Boosters confirms the improvements to High Jump, Strafe Jump and Triple Jump. It does not modify Lift, Glide or Blink.",
    },
    [3] => EventKey {
        hash: 0xCD00_9FB2,
        name: "Super Glide",
        evidence: "Selects the Warlock Glide configuration used by the stock effect while super_active is true. It clears activation and sustained energy-use settings and replaces the jump and movement tuning. This is the complete Super movement profile, not a single height or speed bonus.",
    },
    [3] => EventKey {
        hash: 0x94ED_82B7,
        name: "Well of Radiance Glide",
        evidence: "Selects the Warlock Glide configuration tied to thermal_sword_healing.pattern.tft. It supplies a 0.09 energy-use setting and a distinct movement profile. The stock effect uses a value from the healing Super to control activation. This property alone does not cast a Well or grant its healing.",
    },
    [3] => EventKey {
        hash: 0x8480_91F3,
        name: "Blink Recovery and Cost",
        evidence: "Subtracts 0.55 seconds from the Blink recovery-timer input and sets the energy deducted when the ability ends to 1.5 native units. Used by Move to Survive. Distance and Radar use separate properties. This is not a universal weapon-ready multiplier.",
    },
    [4] => EventKey {
        hash: 0xF7F7_55A1,
        name: "Sprint Transition Time",
        evidence: "Sets the movement bank transition-time value to 0.5. The native movement code uses it to blend into and out of the sprint state. Used alongside Increased Sprint Speed by several movement perks. It does not itself set the maximum sprint speed.",
    },
    [7] => EventKey {
        hash: 0x5418_CF3C,
        name: "Class Ability Recharge Multiplier",
        evidence: "Multiplies the recharge-rate input by 0.7 for Barricade, 0.5 for Dodge or 0.8 for Rift. Stock applies it while is_arc is true. These are rate multipliers below 1, not faster-recharge bonuses or cooldown durations. A custom effect controls when the modifier applies.",
    },
    [] => EventKey {
        hash: 0xDD7C_D3E0,
        name: "Unused Glide Flag",
        evidence: "The stock Glide modifier clears mask 0x00. The native bit operation therefore changes nothing. This key is retained for reading existing records and is not offered as a working property.",
    },
    [] => EventKey {
        hash: 0xD132_46A1,
        name: "Unavailable Sentinel Property",
        evidence: "Starless Night attempts this key, but no captured stock ability bank defines it. The native add-property path rejects a missing key without applying a property. Occurrences in script resources are not ability-bank definitions. Retained for reading existing records rather than offered as a working property.",
    },
}

/// Named properties defined by this ability slot. Changing slots never rewrites an authored key.
#[must_use]
pub fn for_slot(slot: u8) -> &'static [EventKey] {
    // Build each small list once. Slot membership lives with the record instead of fragile
    // array ranges, and properties shared by two banks still have one name and explanation.
    static CHOICES: std::sync::LazyLock<[Vec<EventKey>; 8]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|slot| {
            ALL.iter()
                .zip(SLOTS)
                .filter_map(|(key, slots)| slots.contains(&(slot as u8)).then_some(*key))
                .collect()
        })
    });
    CHOICES.get(usize::from(slot)).map_or(&[], Vec::as_slice)
}
