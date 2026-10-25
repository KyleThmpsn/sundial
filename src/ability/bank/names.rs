//! The recovered names of script parameters, and notes on the ones without a name.

/// A script parameter name: written out in package text, or an FNV-1 preimage of words the
/// packages use, corroborated by its bank, values, the name of the key that sets it or a stock
/// description.
#[derive(Clone, Copy, Debug)]
pub struct ParameterName {
    pub hash: u32,
    pub name: &'static str,
    /// The name as a control reads it.
    pub label: &'static str,
    pub meaning: &'static str,
}

pub const PARAMETER_NAMES: [ParameterName; 86] = [
    ParameterName {
        hash: 0x99B1_D826,
        name: "explosion_radius_scalar",
        label: "Blast Radius Scalar",
        meaning: "Blast radius. Base 1.0, Wish-Dragon Teeth adds 0.14 and Skip Grenade writes 1.1.",
    },
    ParameterName {
        hash: 0xF2CA_F468,
        name: "additional_chain",
        label: "Additional Chains",
        meaning: "Extra Arcbolt chains. Probability Matrix writes 2.",
    },
    ParameterName {
        hash: 0x1AA3_B13F,
        name: "improved_tracking",
        label: "Improved Tracking",
        meaning: "Skip Grenade tracking. New Tricks writes 1.",
    },
    ParameterName {
        hash: 0x8FB4_5D39,
        name: "dot_duration",
        label: "Burn Duration",
        meaning: "Solar grenade burn length. Helium Spirals writes 4 in the Warlock Solar banks.",
    },
    ParameterName {
        hash: 0x8136_C7A9,
        name: "enable_radar",
        label: "Radar During Blink",
        meaning: "Radar during Blink. Move to Survive writes 1.",
    },
    ParameterName {
        hash: 0xB709_0D60,
        name: "enable_grenade_consume",
        label: "Grenade Consume",
        meaning: "Hold the grenade to trade it for an Arc Soul. Getaway Artist writes 1.",
    },
    ParameterName {
        hash: 0x99E0_42AE,
        name: "fast_throw_tracking",
        label: "Fast-Throw Tracking",
        meaning: "Fusion Grenade fast-throw tracking. Bring the Heat writes 1.",
    },
    ParameterName {
        hash: 0xE42E_75C1,
        name: "dodge_recharge_scalar",
        label: "Dodge Recharge Scalar",
        meaning: "Dodge recharge input. Base 0.042, and Double Dodge writes 0.0214.",
    },
    ParameterName {
        hash: 0x9C89_99AF,
        name: "enable_arc_damage",
        label: "Arc Damage",
        meaning: "Arc damage on the Hunter melee. Written with the damage-type override.",
    },
    ParameterName {
        hash: 0x3FE0_B848,
        name: "blink_radar",
        label: "Blink Radar",
        meaning: "Selects enable_radar in the Blink bank.",
    },
    ParameterName {
        hash: 0x0E3A_A789,
        name: "enable_cancel",
        label: "Early Cancel",
        meaning: "Early Arc Staff deactivation. Mobius Conduit writes 1.",
    },
    ParameterName {
        hash: 0x6E26_71B0,
        name: "increase_duration",
        label: "Increase Duration",
        meaning: "Longer Flashbang and Storm Grenade effects. Magnitude writes 2 in the Flashbang bank.",
    },
    ParameterName {
        hash: 0xC410_3CB0,
        name: "increased_duration_enabled",
        label: "Increased Duration",
        meaning: "Longer Lightning and Pulse Grenade effects. Magnitude writes 1.",
    },
    ParameterName {
        hash: 0x682A_8140,
        name: "linger_time",
        label: "Linger Time",
        meaning: "Vortex Grenade linger. A stock row writes 3, and no node applies it.",
    },
    ParameterName {
        hash: 0x9345_17D9,
        name: "damage_wave_scalar",
        label: "Damage Wave Scalar",
        meaning: "Pulse Grenade damage waves. Base 1.0, and a stock row writes 1.5 that no node applies.",
    },
    ParameterName {
        hash: 0x9F87_2FBC,
        name: "additional_seeker",
        label: "Additional Seekers",
        meaning: "Axion Bolt seekers. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xB6C6_CA14,
        name: "longer_lifetime",
        label: "Longer Lifetime",
        meaning: "Spike Grenade lifetime. A stock row writes 1.5, and no node applies it.",
    },
    ParameterName {
        hash: 0xB5C4_E5C9,
        name: "enable_solar_consume",
        label: "Solar Grenade Consume",
        meaning: "Trade a Solar grenade for Glide time. Heat Rises writes 1.",
    },
    ParameterName {
        hash: 0xF6F1_C6C2,
        name: "enable_grenade_charge",
        label: "Grenade Charging",
        meaning: "Hold the grenade to charge it. Chaos Accelerant and Divine Protection write 1.",
    },
    ParameterName {
        hash: 0xEFB7_A789,
        name: "enable_grenade_charge_blast",
        label: "Charged Grenade Blast",
        meaning: "Release the grenade as a short-range blast. Handheld Supernova writes 1.",
    },
    ParameterName {
        hash: 0xF599_BB5A,
        name: "additional_splits",
        label: "Additional Splits",
        meaning: "Firebolt Grenade splits. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x22DD_02DC,
        name: "tether_max_distance",
        label: "Tether Max Distance",
        meaning: "Shadowshot anchor reach. Base 10, Deadfall writes 14 and Moebius Quiver 7.",
    },
    ParameterName {
        hash: 0x45C7_9901,
        name: "enable_nova_linger",
        label: "Lingering Nova",
        meaning: "Nova Bomb leaves a singularity. Vortex writes 1.",
    },
    ParameterName {
        hash: 0x0A75_6E85,
        name: "enable_nova_spread",
        label: "Nova Spread",
        meaning: "Nova Bomb spread. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xB6B2_42C4,
        name: "enable_tracking",
        label: "Tracking",
        meaning: "Nova Bomb tracking. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x99C8_23B3,
        name: "enable_precision_damage",
        label: "Precision Damage",
        meaning: "Golden Gun precision damage. Line 'Em Up writes 1.",
    },
    ParameterName {
        hash: 0x99C0_4C89,
        name: "enable_hammer_cluster_detonate",
        label: "Hammer Cluster Detonation",
        meaning: "Hammers shatter into molten embers. Vulcan's Rage writes 1.",
    },
    ParameterName {
        hash: 0xD18A_18C0,
        name: "hammer_lingering_fire",
        label: "Hammer Lingering Fire",
        meaning: "Hammers leave a Sunspot. Endless Siege writes 1.",
    },
    ParameterName {
        hash: 0x1084_D833,
        name: "enable_tracking_increase",
        label: "Increased Tracking",
        meaning: "Daybreak projectiles seek targets. Fated for the Flame writes 1.",
    },
    ParameterName {
        hash: 0xD953_C5CC,
        name: "enable_line_attack",
        label: "Line Attack",
        meaning: "A streak of flames on impact. Fated for the Flame writes 1.",
    },
    ParameterName {
        hash: 0x62EF_4753,
        name: "air_cast",
        label: "Air Cast",
        meaning: "A shockwave on casting Stormtrance. Landfall writes 1.",
    },
    ParameterName {
        hash: 0x65D8_F47E,
        name: "chain_enabled",
        label: "Chaining",
        meaning: "Lightning chains to nearby enemies. Chain Lightning writes 1, Stormtrance and Transcendence 2.",
    },
    ParameterName {
        hash: 0xB3D1_9BED,
        name: "enable_lingering_extend",
        label: "Extended Lingering",
        meaning: "Longer lingering effects. Magnitude writes 1.",
    },
    ParameterName {
        hash: 0xB35B_A4DC,
        name: "multi_hit_enabled",
        label: "Multi-Hit",
        meaning: "Arc Staff hits create lightning aftershocks. Lethal Current writes 1.",
    },
    ParameterName {
        hash: 0xAA42_90AB,
        name: "extra_orb",
        label: "Extra Orb",
        meaning: "One more Orb of Light. Stock rows write or add 1 in every Super bank, and no node applies them.",
    },
    ParameterName {
        hash: 0xA583_969F,
        name: "enable_heal_shield",
        label: "Healing Shield",
        meaning: "Sentinel Shield healing. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x0C69_7E26,
        name: "enable_heavy_knife",
        label: "Heavy Knife",
        meaning: "Weighted Knife's throw. Weighted Knife writes 1.",
    },
    ParameterName {
        hash: 0xCE32_40C0,
        name: "enable_hammer_throw",
        label: "Hammer Throw",
        meaning: "Throwing Hammer's throw. Throwing Hammer writes 1.",
    },
    ParameterName {
        hash: 0x1C2A_B5A0,
        name: "hammer_throw_enabled",
        label: "Hammer Throw Enabled",
        meaning: "Set with Hammer Throw. Throwing Hammer writes 1.",
    },
    ParameterName {
        hash: 0x2748_AA34,
        name: "enable_smoke_vanish",
        label: "Vanishing Smoke",
        meaning: "Smoke that makes allies invisible. Vanish in Smoke writes 1.",
    },
    ParameterName {
        hash: 0x3DDD_50F8,
        name: "enable_smoke_venom",
        label: "Venomous Smoke",
        meaning: "Smoke that damages over time. Corrosive Smoke writes 1.",
    },
    ParameterName {
        hash: 0x8457_AEC4,
        name: "enable_smoke_snare",
        label: "Snaring Smoke",
        meaning: "Smoke that waits for enemies. Snare Bomb writes 1.",
    },
    ParameterName {
        hash: 0xAF27_EE11,
        name: "enable_smoke_bomb",
        label: "Smoke Bomb",
        meaning: "Throw a Smoke Bomb. All three smoke melees write 1.",
    },
    ParameterName {
        hash: 0xC9F9_4455,
        name: "enable_smoke_wall",
        label: "Smoke Wall",
        meaning: "Hunter melee smoke. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x9118_DEA8,
        name: "enable_smoke_duration_increase",
        label: "Smoke Duration Increase",
        meaning: "Hunter melee smoke duration. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x9C33_629B,
        name: "enable_arc_ball",
        label: "Arc Ball",
        meaning: "Ball Lightning's projectile. Ball Lightning writes 1.",
    },
    ParameterName {
        hash: 0xFA42_D4AA,
        name: "enable_solar_ball",
        label: "Solar Ball",
        meaning: "Celestial Fire's blasts. Celestial Fire writes 1.",
    },
    ParameterName {
        hash: 0x743A_880B,
        name: "burning_punch",
        label: "Burning Punch",
        meaning: "Struck enemies catch fire. Mortar Blast writes 1.",
    },
    ParameterName {
        hash: 0x2497_9FFB,
        name: "power_fist_enabled",
        label: "Power Fist",
        meaning: "Frontal Assault's punch. Frontal Assault writes 1.",
    },
    ParameterName {
        hash: 0x004F_9EC7,
        name: "improved_lunge_enabled",
        label: "Improved Lunge",
        meaning: "Warlock melee lunge. Biotic Enhancements and Cobra Totemic write 1.",
    },
    ParameterName {
        hash: 0x686F_5CBA,
        name: "shoulder_charge_enabled",
        label: "Shoulder Charge",
        meaning: "Seismic Strike, Shield Bash, Hammer Strike and Ballistic Slam write 1.",
    },
    ParameterName {
        hash: 0xDF99_528A,
        name: "dodge_regen_enabled",
        label: "Dodge Regeneration",
        meaning: "A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x9686_D294,
        name: "enable_heal_sigil",
        label: "Healing Rift",
        meaning: "The Rift heals. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xFF7B_8505,
        name: "enable_damage_sigil",
        label: "Empowering Rift",
        meaning: "The Rift empowers. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xE676_4147,
        name: "enable_regen_sigil",
        label: "Regeneration Rift",
        meaning: "A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xF6C0_5CF4,
        name: "enable_radar_sigil",
        label: "Radar Rift",
        meaning: "A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x8294_8225,
        name: "enable_extended_sigil",
        label: "Extended Rift",
        meaning: "The Rift lasts longer. Electrostatic Surge writes 5.",
    },
    ParameterName {
        hash: 0x5055_A1E1,
        name: "enable_reload_sigil",
        label: "Auto Reload",
        meaning: "Rifts and Well of Radiance reload weapons. Alchemical Etchings writes 1.",
    },
    ParameterName {
        hash: 0xA957_F024,
        name: "enable_lingering_fire_apply_buff",
        label: "Sunspot Buff",
        meaning: "Sunspots grant Sun Warrior. Sun Warrior writes 1.",
    },
    ParameterName {
        hash: 0x07CB_40DD,
        name: "disable_palm_strike_bonus_damage",
        label: "Melee Bonus Damage Disabled",
        meaning: "Base 1. Combination Blow writes 0 to turn on its melee damage bonus.",
    },
    ParameterName {
        hash: 0xA4D0_E64E,
        name: "smoke_bomb_disabled",
        label: "Smoke Bomb Disabled",
        meaning: "Base 1. Corrosive Smoke, Snare Bomb and Vanish in Smoke write 0.",
    },
    ParameterName {
        hash: 0xFBEB_EF54,
        name: "knife_throw_disabled",
        label: "Knife Throw Disabled",
        meaning: "Base 1. Knife Trick, Proximity Explosive Knife and Weighted Knife write 0, as do Chain Lightning and Rising Storm.",
    },
    ParameterName {
        hash: 0xD5AC_C5E7,
        name: "aerial_slam_enabled",
        label: "Aerial Slam",
        meaning: "Ballistic Slam writes 1.",
    },
    ParameterName {
        hash: 0xF68F_F5F8,
        name: "enable_aoe_landing",
        label: "Landing Blast",
        meaning: "An explosive landing. Phoenix Dive writes 1.",
    },
    ParameterName {
        hash: 0x68E4_649C,
        name: "increased_sprint_speed_enabled",
        label: "Increased Sprint Speed",
        meaning: "Focused Breathing, Keen Scout and several exotic armor perks write 1.",
    },
    ParameterName {
        hash: 0x8DDD_ECCE,
        name: "thermal_hammer_strike_enabled",
        label: "Hammer Strike",
        meaning: "Hammer Strike writes 1.",
    },
    ParameterName {
        hash: 0xA593_E10D,
        name: "combo_bonus_damage_enabled",
        label: "Combo Bonus Damage",
        meaning: "Arc Staff combo damage. The Arc Staff node writes 1.",
    },
    ParameterName {
        hash: 0x19C4_F9C0,
        name: "knife_throw_apply_dot",
        label: "Burning Knives",
        meaning: "Thrown knives burn. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xD15E_686C,
        name: "knife_throw_enable_detonate",
        label: "Detonating Knives",
        meaning: "Thrown knives detonate. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xE1FE_4C07,
        name: "exotic_armor_extended_range",
        label: "Extended Lunge Range",
        meaning: "Melee lunge range from exotic armor. A stock row writes 0.5, and no node applies it.",
    },
    ParameterName {
        hash: 0x6A44_CC21,
        name: "enable_lingering_aoe_extend",
        label: "Extended Damage Field",
        meaning: "A longer slam damage field. Magnitude writes 1.",
    },
    ParameterName {
        hash: 0x0E98_B133,
        name: "exotic_armor_extend_duration_enabled",
        label: "Longer Tripmine Duration",
        meaning: "Wish-Dragon Teeth writes 1.",
    },
    ParameterName {
        hash: 0x2F08_CD3D,
        name: "explode_on_impact",
        label: "Explode on Impact",
        meaning: "Fusion Grenades explode on impact. Bring the Heat writes 1.",
    },
    ParameterName {
        hash: 0x3288_A2AC,
        name: "whirlwind_guard_enable_cancel",
        label: "Whirlwind Guard Cancel",
        meaning: "Lets the Arc Staff end early. Mobius Conduit writes 1.",
    },
    ParameterName {
        hash: 0x237A_CD7B,
        name: "enable_prox_tether",
        label: "Proximity Tether",
        meaning: "Void Anchors become traps. Deadfall writes 1.",
    },
    ParameterName {
        hash: 0x2850_4837,
        name: "tether_base_lifetime",
        label: "Tether Lifetime",
        meaning: "Void Anchor lifetime. Base 4.8, Moebius Quiver writes 6.8 and Deadfall 8.",
    },
    ParameterName {
        hash: 0x4DA2_D8C6,
        name: "tether_max_target_count",
        label: "Tether Max Targets",
        meaning: "Base 8, Moebius Quiver writes 4 and Deadfall 10.",
    },
    ParameterName {
        hash: 0x8136_C08E,
        name: "enable_multishot",
        label: "Multishot",
        meaning: "Fire Shadowshot several times. Moebius Quiver writes 2.",
    },
    ParameterName {
        hash: 0x9E20_CC5F,
        name: "enable_fan_of_knives",
        label: "Fan of Knives",
        meaning: "Knife Trick's fan of flaming knives. Knife Trick writes 1.",
    },
    ParameterName {
        hash: 0x2768_2C62,
        name: "enable_lingering_aoe",
        label: "Slam Damage Field",
        meaning: "The ground slam leaves a damage field. Terminal Velocity writes 1.",
    },
    ParameterName {
        hash: 0x0FEA_EA74,
        name: "enable_golden_dot",
        label: "Golden Gun Burn",
        meaning: "Golden Gun damage over time. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x6626_5C09,
        name: "enable_extended_nova_linger",
        label: "Extended Lingering Nova",
        meaning: "A longer Nova Bomb singularity. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xC5D3_BF41,
        name: "additional_fragment",
        label: "Additional Fragments",
        meaning: "Skip Grenade fragments. A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0xB7C0_0900,
        name: "enable_hammer_pickup_bonus",
        label: "Hammer Pickup Bonus",
        meaning: "Picking up a Throwing Hammer heals. Tireless Warrior writes 1.",
    },
    ParameterName {
        hash: 0xA820_9C1C,
        name: "exotic_armor_scatter_tracking",
        label: "Scatter Grenade Tracking",
        meaning: "A stock row writes 1, and no node applies it.",
    },
    ParameterName {
        hash: 0x7525_9D7C,
        name: "enable_double_line_attack",
        label: "Double Line Attack",
        meaning: "A stock row writes 1, and no node applies it.",
    },
];

/// What the stock rows do with each listed parameter no name was recovered for: its base
/// and what the nodes and perks that set it write.
pub const PARAMETER_NOTES: [(u32, &str); 51] = [
    (0x030F_A201, "A stock row writes 1, and no node applies it."),
    (0x0707_CBB8, "A stock row writes 1, and no node applies it."),
    (0x2243_1822, "Arc Soul writes 1."),
    (
        0x2F0D_4DBB,
        "Several exotic armor perks write 1, among them Hydraulic Boosters and Mecha Holster.",
    ),
    (0x321D_0F28, "Lightning Reflexes writes 1."),
    (0x34B8_5318, "A stock row writes 1, and no node applies it."),
    (0x3583_F41B, "A stock row writes 1, and no node applies it."),
    (0x3C90_754D, "Trample writes 1."),
    (0x4726_5B80, "Base 4. Moebius Quiver writes 1."),
    (0x4758_3B9B, "Ball Lightning and Celestial Fire write 1."),
    (0x4B51_39F9, "Base 1.5. Moebius Quiver writes 1."),
    (
        0x5A60_BD4C,
        "Base 1. Moebius Quiver writes 0.75 and Deadfall 1.5.",
    ),
    (0x5C71_97A7, "A stock row writes 1, and no node applies it."),
    (0x5DFF_DE8D, "Whirlwind Guard and Mobius Conduit write 1."),
    (0x6598_083A, "A stock row writes 1, and no node applies it."),
    (0x6848_7C2D, "Catapult Lift writes 1."),
    (0x686A_9475, "Base 1. Line 'Em Up writes 0."),
    (
        0x68EE_13F9,
        "A stock row writes 0.5, and no node applies it.",
    ),
    (0x6D73_A109, "Tactical Strike writes 1."),
    (
        0x7408_0878,
        "Base 1. Frontal Assault and Tactical Strike write 1, and most other Titan melees write 0.",
    ),
    (0x7417_499F, "Starless Night writes 1."),
    (0x76FF_82D4, "A stock row writes 1, and no node applies it."),
    (0x7A68_B366, "A stock row writes 1, and no node applies it."),
    (0x7C4A_C40B, "Base 10. Moebius Quiver writes 1."),
    (0x7CC8_FBDC, "Beacons of Empowerment writes 1."),
    (0x7CF4_56CD, "A stock row writes 1, and no node applies it."),
    (0x8335_45DE, "Base 0.5. Vulcan's Rage writes 1.5."),
    (
        0x860B_3B0F,
        "Base 1. A stock row writes 1.5, and no node applies it.",
    ),
    (
        0x8626_D0F0,
        "Shadowshot writes 1 and Moebius Quiver 0. Another stock row writes 2.",
    ),
    (
        0x8B61_92E4,
        "Base 1. A stock row writes 0.6667, and no node applies it.",
    ),
    (
        0x8BDC_18A7,
        "Base 1. A stock row writes 1.2, and no node applies it.",
    ),
    (
        0x9C78_8300,
        "Chaos Accelerant and Divine Protection write 1.",
    ),
    (0x9D6F_1C04, "Base 1. Vulcan's Rage writes 1.33."),
    (
        0x9ECB_F672,
        "Base 1. A stock row writes 2, and no node applies it.",
    ),
    (0x9F33_C2DC, "A stock row writes 1, and no node applies it."),
    (0xA21F_6B1C, "A stock row writes 1, and no node applies it."),
    (0xA305_AD8E, "A stock row writes 1, and no node applies it."),
    (0xA4FD_57A7, "Terminal Velocity writes 1."),
    (0xA85B_5246, "Tempest Strike writes 1."),
    (
        0xB0A4_D3F0,
        "Base 1. Chaos Accelerant and Divine Protection write 0.6, and Handheld Supernova 1.",
    ),
    (0xB900_4A2D, "A stock row writes 1, and no node applies it."),
    (0xBEF8_CFC2, "Deadfall writes 1."),
    (0xBF88_06CC, "A stock row writes 1, and no node applies it."),
    (0xC49F_5B23, "A stock row writes 1, and no node applies it."),
    (0xC7B5_C10A, "A stock row writes 1, and no node applies it."),
    (
        0xCADD_A9DA,
        "Base 1. A stock row writes 1.2, and no node applies it.",
    ),
    (0xCB8D_FAA3, "Base 1. Tempest Strike writes 0."),
    (0xD67D_2043, "A stock row writes 1, and no node applies it."),
    (0xE2D3_AFB7, "A stock row writes 1, and no node applies it."),
    (
        0xE516_85C4,
        "Atomic Breach, Devour and Entropic Pull write 1.",
    ),
    (0xE77C_4A0F, "A stock row writes 1, and no node applies it."),
];

/// The recovered name of a script parameter, when there is one.
#[must_use]
pub fn parameter_name(hash: u32) -> Option<&'static ParameterName> {
    PARAMETER_NAMES.iter().find(|name| name.hash == hash)
}

/// What a script parameter's value means, so a control can show it as such.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    /// Off at 0 and on at 1: every stock row writes 0 or 1.
    Switch,
    /// A whole number of something, such as chains, seekers or Orbs.
    Count,
    /// A multiplier on a base value, 1 for none.
    Multiplier,
    /// A value whose meaning is known only from its stock writes.
    Number,
}

/// A parameter's kind, from its name and what the stock rows write. Most `enable_` and `_enabled`
/// names are switches the stock rows set to 1. A few are written other values and say so in their
/// meanings, so they are listed here. Unnamed parameters are plain numbers.
#[must_use]
pub fn parameter_kind(hash: u32) -> ParameterKind {
    let Some(parameter) = parameter_name(hash) else {
        return ParameterKind::Number;
    };
    match parameter.name {
        // Written 1 or 2 chains, one more Orb, 2 shots, 4 to 10 targets.
        "chain_enabled" | "extra_orb" | "enable_multishot" | "tether_max_target_count" => {
            ParameterKind::Count
        }
        // Written 2, 1.5, 0.5, 5 and lengths and distances whose units the packages do not say.
        "increase_duration"
        | "longer_lifetime"
        | "exotic_armor_extended_range"
        | "enable_extended_sigil"
        | "dot_duration"
        | "linger_time"
        | "tether_base_lifetime"
        | "tether_max_distance" => ParameterKind::Number,
        name if name.ends_with("_scalar") => ParameterKind::Multiplier,
        name if name.starts_with("additional_") => ParameterKind::Count,
        _ => ParameterKind::Switch,
    }
}

/// The label of a named script parameter, when there is one.
#[must_use]
pub fn parameter_label(hash: u32) -> Option<&'static str> {
    PARAMETER_NAMES
        .iter()
        .find(|parameter| parameter.hash == hash)
        .map(|parameter| parameter.label)
}

/// What a script parameter does: its name's meaning, or what the stock rows do with it.
#[must_use]
pub fn parameter_meaning(hash: u32) -> Option<&'static str> {
    parameter_name(hash).map(|name| name.meaning).or_else(|| {
        PARAMETER_NOTES
            .iter()
            .find(|(candidate, _)| *candidate == hash)
            .map(|(_, note)| *note)
    })
}
