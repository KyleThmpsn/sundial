//! Attach lifetime keys: the key an attach effect keeps its object under, shown on hover.
//!
//! An engine name comes from hashing the engine's own words against the key, and the mod
//! families follow one pattern each (`{weapon}_fast_reload`, `{weapon}_fast_ready`,
//! `{weapon}_unflinching`, `{weapon}_improved_targeting`, `improved_` and `better_` for
//! the Enhanced mods). Every other key is named for the perk that attaches under it or,
//! where that perk has no name, for its trigger and what its attached entity holds.
use super::{
    DAMAGE_REDUCTION, EventKey, FIREFLY, HEALTH_REGEN, INVISIBILITY, OVERLOAD,
    OVERLOAD_SUSTAIN_PLAYER, OVERSHIELD, RETURN_ROUNDS, TOUCH_OF_VENOM_SIGNAL, VOID_BLOOM_CATALYST,
    WEAKEN_TARGET,
};

pub(super) const ALL: &[EventKey] = &[
    DAMAGE_REDUCTION,
    FIREFLY,
    HEALTH_REGEN,
    INVISIBILITY,
    OVERLOAD,
    OVERLOAD_SUSTAIN_PLAYER,
    OVERSHIELD,
    RETURN_ROUNDS,
    TOUCH_OF_VENOM_SIGNAL,
    VOID_BLOOM_CATALYST,
    WEAKEN_TARGET,
    EventKey {
        hash: 0xCE29_AC9E,
        name: "Aeon Energy",
        evidence: "Used only by Aeon Energy (\"Melee grants energy to nearby Aeon Cultists\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x35DF_113B,
        name: "Arc Traps",
        evidence: "Used only by Arc Traps, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x6F25_4F09,
        name: "Ascending Amplitude",
        evidence: "Used only by Ascending Amplitude (\"Each enemy you defeat with Stormtrance increases the damage you deal with Stormtrance\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xC83B_FA78,
        name: "Auto Rifle Dexterity",
        evidence: "The engine name auto_rifle_fast_ready, the key Auto Rifle Dexterity (\"Faster ready and stow speed for Auto Rifles and Trace Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x0662_B418,
        name: "Auto Rifle Loader",
        evidence: "The engine name auto_rifle_fast_reload, the key Auto Rifle Loader (\"Increases reload speed of Auto Rifles and Trace Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x2494_634E,
        name: "Auto Rifle Targeting",
        evidence: "The engine name auto_rifle_improved_targeting, the key Auto Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Auto Rifles and Trace Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7058_C467,
        name: "Better Already",
        evidence: "Used by Better Already (\"Your health begins to regenerate immediately after picking up an Orb of Light\") and two unnamed perks, which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x51D6_F5E8,
        name: "Blessing of the Sky",
        evidence: "Used by Blessing of the Sky and Lumina Catalyst, which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x72AB_6923,
        name: "Bow Dexterity",
        evidence: "The engine name bows_fast_ready, the key Bow Dexterity (\"Faster ready and stow speed for Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xFF9E_662D,
        name: "Bow Reloader",
        evidence: "The engine name bows_fast_reload, the key Bow Reloader (\"Increases reload speed of Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x9C5C_F821,
        name: "Bow Targeting",
        evidence: "The engine name bows_improved_targeting, the key Bow Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xC14F_C3BD,
        name: "Bulwark Finisher",
        evidence: "The engine name finisher_overshield, the key Bulwark Finisher (\"Finisher final blows generate an overshield. Requires one-fourth of your Super energy\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x4113_6E32,
        name: "Healing",
        evidence: "Shared by 22 perks that heal or regenerate health, among them Survival Well, Transfusion Matrix, Cruel Remedy, Recuperation and Mask Upgrade. No engine name matched.",
    },
    EventKey {
        hash: 0x7C6C_F12F,
        name: "Chaotic Exchanger",
        evidence: "Used only by Chaotic Exchanger, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x6E3E_9065,
        name: "Close Enough",
        evidence: "Used only by Close Enough (\"Extended Chaos Reach. Sprinting can add Super energy\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x114B_44FF,
        name: "Conduction Tines",
        evidence: "The engine name arc_overcharge_hopon, the key Conduction Tines (\"Arc ability kills restore Arc abilities\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x46EB_367E,
        name: "Dearly Departed",
        evidence: "Used only by Dearly Departed (\"Grants Rift energy when critically wounded, creates healing rift on death\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x3663_BD2F,
        name: "Desperado",
        evidence: "Used only by Desperado (\"Reloading while Outlaw is active increases your rate of fire\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x5520_5190,
        name: "Dreaded Visage",
        evidence: "Used only by Dreaded Visage, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xBB44_1A7C,
        name: "En Garde",
        evidence: "Used only by En Garde (\"Quick attacks immediately after swapping to this sword do additional damage\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xF694_5560,
        name: "Energized",
        evidence: "Used only by Energized (\"While you're on the Leviathan, defeating normal enemies with an Energy weapon boosts Energy weapon damage by 15% for a short time.  This mod's abilities cannot stack across multiple copies\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x8CCC_879A,
        name: "Energy Dexterity",
        evidence: "The engine name energy_fast_ready, the key Energy Dexterity (\"Slightly faster ready and stow speed for Energy weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x8D59_E932,
        name: "Energy Weapon Loader",
        evidence: "The engine name energy_fast_reload, the key Energy Weapon Loader (\"Slightly increases reload speed of any equipped Energy weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x471C_BF40,
        name: "Energy Weapon Targeting",
        evidence: "The engine name energy_improved_targeting, the key Energy Weapon Targeting (\"Slightly improved target acquisition, accuracy, and aim-down-sights speed for Energy weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x7A3C_59AC,
        name: "Enhanced Bow Targeting",
        evidence: "The engine name better_bows_improved_targeting, the key Enhanced Bow Targeting (\"Greatly improved target acquisition, accuracy, and aim-down-sights speed for Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x3568_277E,
        name: "Enhanced Grenade Launcher Loader",
        evidence: "The engine name improved_grenade_launcher_fast_reload, the key Enhanced Grenade Launcher Loader (\"Greatly increases reload speed of Grenade Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x6BDC_D426,
        name: "Enhanced Hand Cannon Dexterity",
        evidence: "The engine name improved_hand_cannon_fast_ready, the key Enhanced Hand Cannon Dexterity (\"Greatly increased ready and stow speed for Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xDC0E_8E26,
        name: "Enhanced Hand Cannon Loader",
        evidence: "The engine name improved_hand_cannon_fast_reload, the key Enhanced Hand Cannon Loader (\"Greatly increases reload speed of Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7E96_0786,
        name: "Enhanced Hand Cannon Targeting",
        evidence: "The engine name better_hand_cannon_improved_targeting, the key Enhanced Hand Cannon Targeting (\"Greatly improved target acquisition, accuracy, and aim-down-sights speed for Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7CEB_7655,
        name: "Enhanced Linear Fusion Targeting",
        evidence: "The engine name better_linear_fusion_rifles_improved_targeting, the key Enhanced Linear Fusion Targeting (\"Greatly improved target acquisition, accuracy, and aim-down-sights speed for Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7D81_0558,
        name: "Enhanced Rocket Launcher Dexterity",
        evidence: "The engine name improved_rocket_launcher_fast_ready, the key Enhanced Rocket Launcher Dexterity (\"Greatly increased ready and stow speed for Rocket Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x3F99_FD78,
        name: "Enhanced Rocket Launcher Loader",
        evidence: "The engine name improved_rocket_launcher_fast_reload, the key Enhanced Rocket Launcher Loader (\"Greatly increases reload speed of Rocket Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x58CB_26F7,
        name: "Enhanced Shotgun Dexterity",
        evidence: "The engine name improved_shotgun_fast_ready, the key Enhanced Shotgun Dexterity (\"Greatly increased ready and stow speed for Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x8E54_4AE9,
        name: "Enhanced Shotgun Loader",
        evidence: "The engine name improved_shotgun_fast_reload, the key Enhanced Shotgun Loader (\"Greatly increases reload speed of Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x2001_54BF,
        name: "Enhanced Sniper Rifle Dexterity",
        evidence: "The engine name improved_sniper_rifle_fast_ready, the key Enhanced Sniper Rifle Dexterity (\"Greatly increased ready and stow speed for Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x67AB_932F,
        name: "Enhanced Sniper Rifle Targeting",
        evidence: "The engine name better_sniper_rifle_improved_targeting, the key Enhanced Sniper Rifle Targeting (\"Greatly improved target acquisition, accuracy, and aim-down-sights speed for Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x89E9_7DBF,
        name: "Enhanced Unflinching Bow Aim",
        evidence: "The engine name improved_bows_unflinching, the key Enhanced Unflinching Bow Aim (\"Greatly reduces flinching from incoming fire while aiming Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x26E3_C7D0,
        name: "Enhanced Unflinching Linear Fusion Aim",
        evidence: "The engine name improved_linear_fusion_rifles_unflinching, the key Enhanced Unflinching Linear Fusion Aim (\"Greatly reduces flinching from incoming fire while aiming Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xFCA7_EBB9,
        name: "Enhanced Unflinching Scout Rifle Aim",
        evidence: "The engine name improved_scout_rifle_unflinching, the key Enhanced Unflinching Scout Rifle Aim (\"Greatly reduces flinching from incoming fire while aiming Scout Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xCD27_A49A,
        name: "Enhanced Unflinching Sniper Aim",
        evidence: "The engine name improved_sniper_rifle_unflinching, the key Enhanced Unflinching Sniper Aim (\"Greatly reduces flinching from incoming fire while aiming Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x0A75_1904,
        name: "Fury Conductors",
        evidence: "Used by Fury Conductors (\"Melee kills store explosive defensive energy\"), which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x0A62_E0AB,
        name: "Fury Conductors 2",
        evidence: "Used only by Fury Conductors, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xBB5B_696E,
        name: "Fusion Rifle Dexterity",
        evidence: "The engine name fusion_rifles_fast_ready, the key Fusion Rifle Dexterity (\"Faster ready and stow speed for Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x037C_08EE,
        name: "Fusion Rifle Loader",
        evidence: "The engine name fusion_rifles_fast_reload, the key Fusion Rifle Loader (\"Increases reload speed of Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xFCDD_08DC,
        name: "Fusion Rifle Targeting",
        evidence: "The engine name fusion_rifles_improved_targeting, the key Fusion Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x6885_44DC,
        name: "Grenade Launcher Dexterity",
        evidence: "The engine name grenade_launchers_fast_ready, the key Grenade Launcher Dexterity (\"Faster ready and stow speed for Grenade Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xF27E_6664,
        name: "Grenade Launcher Loader",
        evidence: "The engine name grenade_launchers_fast_reload, the key Grenade Launcher Loader (\"Increases reload speed of Grenade Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x1098_8B40,
        name: "Guardian Angel",
        evidence: "Used only by Guardian Angel (\"Grants a chance to generate healing orbs for you on Scout Rifle, Sniper Rifle, Bow, and Linear Fusion Rifle precision final blows\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x0424_3717,
        name: "Hand Cannon Dexterity",
        evidence: "The engine name hand_cannon_fast_ready, the key Hand Cannon Dexterity (\"Faster ready and stow speed for Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x6E04_A889,
        name: "Hand Cannon Loader",
        evidence: "The engine name hand_cannon_fast_reload, the key Hand Cannon Loader (\"Increases reload speed of Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x12DC_6DBD,
        name: "Hand Cannon Targeting",
        evidence: "The engine name hand_cannon_improved_targeting, the key Hand Cannon Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xF90B_15F2,
        name: "Heal Thyself",
        evidence: "Used only by Heal Thyself (\"While you are Charged with Light, grenade final blows heal you and consume one stack of Charged with Light\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x9395_58FC,
        name: "Healthy Finisher",
        evidence: "Used only by Healthy Finisher (\"Finishers heal you. Requires one-tenth of your Super energy\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x6E1B_C3BE,
        name: "Heavy Handed",
        evidence: "Used only by Heavy Handed (\"While Charged with Light, regain half of your melee energy when you use a charged melee ability, consuming one stack of Charged with Light\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xCB11_93BD,
        name: "Heavy Hitter",
        evidence: "Used only by Heavy Hitter (\"While you're on the Leviathan, defeating challenging enemies with a Power weapon boosts Power weapon damage by 15% for a short time.  This mod's abilities cannot stack across multiple copies\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x157E_DCBF,
        name: "Helium Spirals",
        evidence: "Used only by Helium Spirals (\"Solar grenades burn longer. Melee kills restore them\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x7180_1682,
        name: "Judgment",
        evidence: "The engine name siphon_gun_overload, the key Judgment (\"Sustained damage with this weapon envelops the target in a field that weakens and  disrupts them. Strong against Overload Champions\") keep their attached effect under.",
    },
    EventKey {
        hash: 0x2E58_9BEF,
        name: "Kinetic Dexterity",
        evidence: "The engine name kinetic_fast_ready, the key Kinetic Dexterity (\"Slightly faster ready and stow speed for Kinetic weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x5422_19E1,
        name: "Kinetic Weapon Loader",
        evidence: "The engine name kinetic_fast_reload, the key Kinetic Weapon Loader (\"Slightly increases reload speed of any equipped Kinetic weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x2695_9455,
        name: "Kinetic Weapon Targeting",
        evidence: "The engine name kinetic_improved_targeting, the key Kinetic Weapon Targeting (\"Slightly improved target acquisition, accuracy, and aim-down-sights speed for Kinetic weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x1136_B95B,
        name: "Large Weapon Loader",
        evidence: "The engine name large_arms_fast_reload, the key Large Weapon Loader (\"Faster reload for Rocket Launchers, Grenade Launchers, Machine Guns, and Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x6434_6C08,
        name: "Light Arms Dexterity",
        evidence: "The engine name light_arms_fast_ready, the key Light Arms Dexterity (\"Faster ready and stow speed for Hand Cannons, Sidearms, and Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x2D12_049F,
        name: "Light Arms Loader",
        evidence: "The engine name small_arms_fast_reload, the key Light Arms Loader (\"Faster reload for Hand Cannons, Sidearms, Submachine Guns, and Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xD718_CF2A,
        name: "Static Charge",
        evidence: "The engine name static_charge, the key Linear Actuators keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7106_6166,
        name: "Linear Fusion Dexterity",
        evidence: "The engine name linear_fusion_rifles_fast_ready, the key Linear Fusion Dexterity (\"Faster ready and stow speed for Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x39FB_EE66,
        name: "Linear Fusion Rifle Loader",
        evidence: "The engine name linear_fusion_rifles_fast_reload, the key Linear Fusion Rifle Loader (\"Increases reload speed of Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x3FF4_50F4,
        name: "Linear Fusion Rifle Targeting",
        evidence: "The engine name linear_fusion_rifles_improved_targeting, the key Linear Fusion Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xB359_12A0,
        name: "Machine Gun Dexterity",
        evidence: "The engine name hmg_fast_ready, the key Machine Gun Dexterity (\"Faster ready and stow speed for Machine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x19C5_C97B,
        name: "Machine Gun Loader",
        evidence: "Used only by Machine Gun Loader (\"Increases the reload speed of Machine Guns\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xFF38_C736,
        name: "Machine Gun Targeting",
        evidence: "The engine name hmg_improved_targeting, the key Machine Gun Targeting (\"Improved target acquisition, accuracy, and aim down sights speed for Machine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xB24E_7DF1,
        name: "Master of Arms",
        evidence: "Used only by Master of Arms (\"Kills with any weapon improve this weapon's damage for a short time\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x83A6_1050,
        name: "Submachine Gun Ready Speed",
        evidence: "The engine name smg_ready_speed, the key Mecha Holster (\"Reloads stowed SMGs and allows instant ready\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xFF03_8001,
        name: "Mecha Holster",
        evidence: "Used only by Mecha Holster (\"Reloads stowed SMGs and allows instant ready\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x82E1_8B39,
        name: "Misdirection",
        evidence: "Used only by Misdirection (\"Dodging disorients and removes enemy radars\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x69A7_4439,
        name: "Multikill Clip",
        evidence: "The engine name multikill_clip, the key Multikill Clip (\"Reloading grants increased damage based on the number of rapid kills made beforehand\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x4F5B_0A60,
        name: "Overflow",
        evidence: "Used only by Overflow (\"Picking up Special or Heavy ammo reloads this weapon to beyond normal capacity\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xE875_0740,
        name: "Overflowing Light",
        evidence: "Used only by Overflowing Light (\"Use one ability to briefly improve your other abilities\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xB57E_77C5,
        name: "Overflowing Light 2",
        evidence: "Used only by Overflowing Light, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x725D_784D,
        name: "Oversize Weapon Dexterity",
        evidence: "The engine name oversized_fast_ready, the key Oversize Weapon Dexterity (\"Faster ready and stow speed for Rocket Launchers, Grenade Launchers, Shotguns, and Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7380_AB85,
        name: "Poison Arrows",
        evidence: "Used only by Poison Arrows, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x581A_5BEB,
        name: "Power Dexterity",
        evidence: "The engine name heavy_fast_ready, the key Power Dexterity (\"Slightly faster ready and stow speed for Power weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0xB33A_BE95,
        name: "Power Weapon Loader",
        evidence: "The engine name heavy_fast_reload, the key Power Weapon Loader (\"Slightly increases reload speed of any equipped Power weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0xA607_4F89,
        name: "Power Weapon Targeting",
        evidence: "The engine name heavy_improved_targeting, the key Power Weapon Targeting (\"Slightly improved target acquisition, accuracy, and aim-down-sights speed for Power weapons\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x9367_22B1,
        name: "Precision Weapon Targeting",
        evidence: "The engine name precision_weapon_improved_targeting, the key Precision Weapon Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Hand Cannons, Scout Rifles, Trace Rifles, Bows, Linear Fusion, Snipers, and Slug Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xEEB0_EE68,
        name: "Probability Matrix",
        evidence: "Used only by Probability Matrix (\"Improves Arc Bolt Grenade chains\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x3266_B134,
        name: "Pulse Rifle Dexterity",
        evidence: "The engine name pulse_rifle_fast_ready, the key Pulse Rifle Dexterity (\"Faster ready and stow speed for Pulse Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x5CE3_C8DC,
        name: "Pulse Rifle Loader",
        evidence: "The engine name pulse_rifle_fast_reload, the key Pulse Rifle Loader (\"Increases reload speed of Pulse Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x2F9C_56D2,
        name: "Pulse Rifle Targeting",
        evidence: "The engine name pulse_rifle_improved_targeting, the key Pulse Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Pulse Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x21DD_F3A9,
        name: "Rampage",
        evidence: "The engine name rampage_perk, the key Rampage (\"Kills with this weapon temporarily grant increased damage. Stacks 3x\") keep their attached effect under.",
    },
    EventKey {
        hash: 0x3528_B317,
        name: "Rampage Status",
        evidence: "The engine name rampage_status, the key Rampage (\"Kills with this weapon temporarily grant increased damage. Stacks 3x\") keep their attached effect under.",
    },
    EventKey {
        hash: 0x1474_1906,
        name: "Reflective Vents",
        evidence: "Used only by Reflective Vents (\"Sliding reflects projectiles and grants Super energy\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xB5E8_611C,
        name: "Reversal of Fortune",
        evidence: "Used only by Reversal of Fortune (\"Missing a shot returns the bullet to the magazine after a short duration\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xA628_8DD1,
        name: "Rifle Dexterity",
        evidence: "The engine name rifles_fast_ready, the key Rifle Dexterity (\"Faster ready and stow speed for all Rifle-class weapons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xA3EF_DE83,
        name: "Rifle Loader",
        evidence: "The engine name rifles_fast_reload, the key Rifle Loader (\"Increases reload speed for any Rifle-class weapon\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xA073_7945,
        name: "Rocket Launcher Dexterity",
        evidence: "The engine name rocket_launcher_fast_ready, the key Rocket Launcher Dexterity (\"Faster ready and stow speed for Rocket Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7277_545F,
        name: "Rocket Launcher Loader",
        evidence: "The engine name rocket_launcher_fast_reload, the key Rocket Launcher Loader (\"Increases reload speed of Rocket Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x13FF_ACF6,
        name: "Scatter Projectile Targeting",
        evidence: "Used only by Scatter Projectile Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Auto Rifles, Machine Guns, SMGs, Pulse Rifles, Sidearms, and Fusion Rifles\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xB3FD_079F,
        name: "Scout Rifle Dexterity",
        evidence: "The engine name scout_rifle_fast_ready, the key Scout Rifle Dexterity (\"Faster ready and stow speed for Scout Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x8697_35D1,
        name: "Scout Rifle Loader",
        evidence: "The engine name scout_rifle_fast_reload, the key Scout Rifle Loader (\"Increases reload speed of Scout Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7A61_75C5,
        name: "Scout Rifle Targeting",
        evidence: "The engine name scout_rifle_improved_targeting, the key Scout Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Scout Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x71F2_6BF8,
        name: "Shielding Hand",
        evidence: "Used only by Shielding Hand (\"While you're on the Leviathan, melee kills reduce incoming damage by 20%.  This mod's abilities cannot stack across multiple copies\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x779B_117A,
        name: "Shotgun Dexterity",
        evidence: "The engine name shotgun_fast_ready, the key Shotgun Dexterity (\"Faster ready and stow speed for Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x107C_F492,
        name: "Shotgun Loader",
        evidence: "The engine name shotgun_fast_reload, the key Shotgun Loader (\"Increases reload speed of Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x2CCE_93A0,
        name: "Shotgun Targeting",
        evidence: "The engine name shotgun_improved_targeting, the key Shotgun Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x3053_DCF7,
        name: "Sidearm Dexterity",
        evidence: "The engine name sidearm_fast_ready, the key Sidearm Dexterity (\"Faster ready and stow speed for Sidearms\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xDA8A_CCE9,
        name: "Sidearm Loader",
        evidence: "The engine name sidearm_fast_reload, the key Sidearm Loader (\"Increases reload speed of Sidearms\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x8389_E59D,
        name: "Sidearm Targeting",
        evidence: "The engine name sidearm_improved_targeting, the key Sidearm Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Sidearms\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xA323_08E0,
        name: "Sniper Rifle Dexterity",
        evidence: "The engine name sniper_rifle_fast_ready, the key Sniper Rifle Dexterity (\"Faster ready and stow speed for Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x289E_B4C0,
        name: "Sniper Rifle Loader",
        evidence: "The engine name sniper_rifle_fast_reload, the key Sniper Rifle Loader (\"Increases reload speed of Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xD472_7E76,
        name: "Sniper Rifle Targeting",
        evidence: "The engine name sniper_rifle_improved_targeting, the key Sniper Rifle Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xEA70_5587,
        name: "Soul Devourer",
        evidence: "Used only by Soul Devourer (\"Absorbing a Remnant strengthens Mark of the Devourer and partially refills the magazine\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xCD2A_8E7B,
        name: "Spheromatik Trigger",
        evidence: "Used only by Spheromatik Trigger, which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x477B_FDAB,
        name: "Sprint Grip",
        evidence: "Used only by Sprint Grip (\"Temporarily increases the weapon's ready speed and aim down sights speed after sprinting\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x10B4_4D5D,
        name: "Striking Hand",
        evidence: "Used only by Striking Hand (\"While you're on the Leviathan, melee kills increase all damage by 20%.  This mod's abilities cannot stack across multiple copies\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x0863_31F9,
        name: "Submachine Gun Dexterity",
        evidence: "The engine name smg_fast_ready, the key Submachine Gun Dexterity (\"Faster ready and stow speed for Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x9139_722B,
        name: "Submachine Gun Loader",
        evidence: "The engine name smg_fast_reload, the key Submachine Gun Loader (\"Increases reload speed of Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7C3D_A6FF,
        name: "Submachine Gun Targeting",
        evidence: "The engine name smg_improved_targeting, the key Submachine Gun Targeting (\"Improved target acquisition, accuracy, and aim-down-sights speed for Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x03C3_835B,
        name: "Synapse Junctions",
        evidence: "The engine name buff_arc_staff_charge, the key Synapse Junctions (\"Chained Arc Staff hits buff damage and duration\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x7D75_2796,
        name: "Taken Barrier",
        evidence: "Used only by Taken Barrier (\"Receiving Taken damage gives a 20% reduction in damage for 10 seconds\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x5E77_286B,
        name: "The Fourth Magic",
        evidence: "Used by The Fourth Magic and one unnamed perk, which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xB7DE_4307,
        name: "Tome of Dawn",
        evidence: "Used only by Tome of Dawn (\"Dawnblade holds you in midair when you're aiming\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0xB495_F523,
        name: "Unflinching Auto Rifle Aim",
        evidence: "The engine name auto_rifle_unflinching, the key Unflinching Auto Rifle Aim (\"Reduces flinching from incoming fire while aiming Auto Rifles and Trace Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x039D_F156,
        name: "Unflinching Bow Aim",
        evidence: "The engine name bows_unflinching, the key Unflinching Bow Aim (\"Reduces flinching from incoming fire while aiming Bows\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x4A23_B261,
        name: "Unflinching Energy Aim",
        evidence: "The engine name energy_unflinching, the key Unflinching Energy Aim (\"Slightly reduces flinching from incoming fire while aiming your Energy weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0x328B_D64D,
        name: "Unflinching Fusion Rifle Aim",
        evidence: "The engine name fusion_rifles_unflinching, the key Unflinching Fusion Rifle Aim (\"Reduces flinching from incoming fire while aiming Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x41F8_D4CF,
        name: "Unflinching Grenade Launcher Aim",
        evidence: "The engine name grenade_launchers_unflinching, the key Unflinching Grenade Launcher Aim (\"Reduces flinching from incoming fire while aiming Grenade Launchers\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xA18D_8EB2,
        name: "Unflinching Hand Cannon Aim",
        evidence: "The engine name hand_cannon_unflinching, the key Unflinching Hand Cannon Aim (\"Reduces flinching from incoming fire while aiming Hand Cannons\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x522F_D2AA,
        name: "Unflinching Kinetic Aim",
        evidence: "The engine name kinetic_unflinching, the key Unflinching Kinetic Aim (\"Slightly reduces flinching from incoming fire while aiming your Kinetic weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0xF1B3_D3D8,
        name: "Unflinching Large Arms",
        evidence: "The engine name large_arms_unflinching, the key Unflinching Large Arms (\"Slightly reduces flinching from incoming fire while aiming Rocket Launchers, Grenade Launchers, Machine Guns, and Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x9B42_616C,
        name: "Unflinching Light Arms Aim",
        evidence: "The engine name small_arms_unflinching, the key Unflinching Light Arms Aim (\"Reduces flinching from incoming fire while aiming Sidearms, Hand Cannons, Bows, and Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x5870_A265,
        name: "Unflinching Linear Fusion Aim",
        evidence: "The engine name linear_fusion_rifles_unflinching, the key Unflinching Linear Fusion Aim (\"Reduces flinching from incoming fire while aiming Linear Fusion Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xEB35_5B07,
        name: "Unflinching Machine Gun",
        evidence: "The engine name unflinching_hmg, the key Unflinching Machine Gun (\"Reduces hit flinch from incoming fire while aiming Machine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x8F12_E7FE,
        name: "Unflinching Power Aim",
        evidence: "The engine name heavy_unflinching, the key Unflinching Power Aim (\"Slightly reduces flinching from incoming fire while aiming a Power weapon\") and one unnamed perk keep their attached effect under.",
    },
    EventKey {
        hash: 0xA185_9207,
        name: "Unflinching Pulse Rifle Aim",
        evidence: "The engine name pulse_rifle_unflinching, the key Unflinching Pulse Rifle Aim (\"Reduces flinching from incoming fire while aiming Pulse Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x0797_4FE0,
        name: "Unflinching Rifle Aim",
        evidence: "The engine name rifles_unflinching, the key Unflinching Rifle Aim (\"Reduces flinching from incoming fire while aiming any Rifle-class weapon\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x4B43_4A3A,
        name: "Unflinching Scout Rifle Aim",
        evidence: "The engine name scout_rifle_unflinching, the key Unflinching Scout Rifle Aim (\"Reduces flinching from incoming fire while aiming Scout Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xB35D_4301,
        name: "Unflinching Shotgun Aim",
        evidence: "The engine name shotgun_unflinching, the key Unflinching Shotgun Aim (\"Reduces flinching from incoming fire while aiming Shotguns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x8090_09D2,
        name: "Unflinching Sidearm Aim",
        evidence: "The engine name sidearm_unflinching, the key Unflinching Sidearm Aim (\"Reduces flinching from incoming fire while aiming Sidearms\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xCD96_50EB,
        name: "Unflinching Sniper Aim",
        evidence: "The engine name sniper_rifle_unflinching, the key Unflinching Sniper Aim (\"Reduces flinching from incoming fire while aiming Sniper Rifles\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xB80C_BDD1,
        name: "Unflinching Submachine Gun Aim",
        evidence: "The engine name smg_fast_unflinching, the key Unflinching Submachine Gun Aim (\"Reduces flinching from incoming fire while aiming Submachine Guns\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xCFB6_A460,
        name: "Vampire's Caress",
        evidence: "Used by Vampire's Caress (\"Melee kills restore health for a short duration\") and one unnamed perk, which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x4DDC_6A7B,
        name: "Warlord's End",
        evidence: "Used only by Warlord's End (\"Powered melee kills create a burst of energy that weakens nearby enemies\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x93EE_9AC2,
        name: "Wire Rifle",
        evidence: "Used only by Wire Rifle (\"Fires a long-range blinding bolt\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x8C01_ABE9,
        name: "Wraithmetal Mail",
        evidence: "Used by Wraithmetal Mail (\"Dodge reloads guns and buffs speed and handling\") and one unnamed perk, which keep their attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x00EC_AD23,
        name: "Wraithmetal Mail 2",
        evidence: "Used only by Wraithmetal Mail (\"Dodge reloads guns and buffs speed and handling\"), which keeps its attached effect under it. No engine name matched.",
    },
    EventKey {
        hash: 0x66C9_22E0,
        name: "Disorient",
        evidence: "The engine name disorient, the key the undescribed perk #12 keeps its attached effect under.",
    },
    EventKey {
        hash: 0xFE7D_F1B5,
        name: "Palm Strike Bonus Damage",
        evidence: "Named for the pattern the undescribed perk #13 attaches under it after a palm strike kill, whose package path ends in palm_strike_bonus_damage. The pattern raises melee damage 60% against combatants and 22.7% against players. While it lasts, #14 turns on Arc Melee Damage and adds the disintegrate label. No engine name matched.",
    },
    EventKey {
        hash: 0xEED8_0502,
        name: "Arc Staff Bonus Damage",
        evidence: "Used only by the undescribed perk #18, which attaches it on a named event no stock perk publishes. Its entity triples Arc Staff damage and doubles it against bosses. No engine name matched.",
    },
    EventKey {
        hash: 0xFD81_BA18,
        name: "Palm Strike Healing",
        evidence: "Used only by the undescribed perk #19, which attaches it after a palm strike kill. Its entity has the same components as the ones Healing and Better Already attach. No engine name matched.",
    },
    EventKey {
        hash: 0x46A3_9552,
        name: "Reload Boost",
        evidence: "The engine name buff_reload_boost, the key the undescribed perk #22 keeps its attached effect under. #22 attaches it after a precision kill, and its entity adds 25 Reload Speed.",
    },
    EventKey {
        hash: 0x6F73_63A2,
        name: "Accuracy Boost",
        evidence: "The engine name buff_accuracy_boost, the key the undescribed perk #23 keeps its attached effect under. #23 attaches it after a precision hit or kill, and the perk before it attaches Reload Boost. Its entity adds 0.5 to a Super input.",
    },
    EventKey {
        hash: 0x5403_9733,
        name: "Golden Gun Aim Assist",
        evidence: "The engine name golden_gun_aim_assist, the key the undescribed perk #27 keeps its attached effect under. #27 attaches it to the player after a ghost gun kill, the label Hawkeye Hack (\"Golden Gun fires one high-damage shot\") filters on. Its entity widens the three Aim Assist Angles and adds to Effective Range.",
    },
    EventKey {
        hash: 0x787F_C58D,
        name: "Precision Hit Buff",
        evidence: "Used only by the undescribed perk #29, which attaches it to the player on a precision hit and publishes the signal focused_fire_triggered at the same time. Its entity multiplies a Super input by 0.7 and adds 0.45 to a Barrel input. No engine name matched.",
    },
    EventKey {
        hash: 0x32F3_D316,
        name: "Melee Energy Boost",
        evidence: "The engine name buff_melee_energy_boost, the key the undescribed perk #31 keeps its attached effect under. #31 attaches it after a throwing knife kill, and its entity adds 7 to melee recharge.",
    },
    EventKey {
        hash: 0xE04B_C60D,
        name: "Precision Kill Stability and Handling",
        evidence: "Used only by the undescribed perk #32, which attaches it after a precision kill. Its entity adds 30 Stability and 50 Handling, and kills raise a value of the same name up to 26. No engine name matched.",
    },
    EventKey {
        hash: 0x3F34_D979,
        name: "Slam and Slide Reload",
        evidence: "Used only by the undescribed perk #38, which reloads weapons from reserves after arc aerial slam damage or a pickup while sliding, and attaches it to the player for 0.52 seconds. No engine name matched.",
    },
    EventKey {
        hash: 0x7A24_2528,
        name: "Weapon Sigil",
        evidence: "The engine name weapon_sigil, the key the undescribed perk #53 keeps its attached effect under.",
    },
    EventKey {
        hash: 0x6735_DF1D,
        name: "Warlock Class Ability Effect",
        evidence: "Used only by the undescribed perk #56, which attaches it for an instant when the class ability is used outside the Super. The perks beside it set Fire During Lift and Glide and Super Glide. #20 reloads on the same event and shares Wraithmetal Mail's key (\"Dodge reloads guns\"), which identifies the event. No engine name matched.",
    },
    EventKey {
        hash: 0xC144_D2A1,
        name: "Ally Support Ability Recharge",
        evidence: "Used only by the undescribed perk #59, which attaches it to the player when a heal lands, an ally is empowered or an ally is revived. Its entity adds 5.1 grenade, 6.8 melee and 5.1 class ability recharge. No engine name matched.",
    },
    EventKey {
        hash: 0x1DE6_7D27,
        name: "Titan Solar Damage Buff",
        evidence: "The engine name titan_solar_damage_buff, the key the undescribed perk #91 keeps its attached effect under. #91 attaches it after any kill.",
    },
    EventKey {
        hash: 0x8F78_809D,
        name: "Shadowshot Hit Effect",
        evidence: "Used only by the undescribed perk #98, which attaches it to the target for 0.1 seconds when void bow damage lands, the Shadowshot label. Its entity carries particle and sound nodes and no modifiers. No engine name matched.",
    },
    EventKey {
        hash: 0x1B74_CAAE,
        name: "Shadowshot Kill Effect",
        evidence: "Used only by the undescribed perk #102, which attaches it after a void bow kill, the Shadowshot label. Its entity carries particle and sound nodes and no modifiers. No engine name matched.",
    },
    EventKey {
        hash: 0x6B25_3511,
        name: "Slow Effect",
        evidence: "Used only by the undescribed perk #107, which attaches it to a damaged target for 5 seconds beside an unkeyed entity that cuts the target's move speeds to 60%. No engine name matched.",
    },
    EventKey {
        hash: 0x6F5D_4FFE,
        name: "Truesight",
        evidence: "Used only by the undescribed perk #108, which attaches it with Invisibility for 0.5 seconds after a precision kill while crouching. Its entity has the Truesight components of Touch of Venom (\"Instant Smoke Bomb with Truesight\") and Queen's Wrath (\"gain Truesight\"). No engine name matched.",
    },
    EventKey {
        hash: 0x831A_F6B9,
        name: "Invisibility Melee Bonus",
        evidence: "Used only by the undescribed perk #108, which attaches it with Invisibility and Truesight after a precision kill while crouching. Its entity adds 0.4445 to a melee input no other stock effect sets. No engine name matched.",
    },
    EventKey {
        hash: 0xB5FC_5953,
        name: "Tether Mark",
        evidence: "The engine name tether_mark, the key the undescribed perk #112 keeps its attached effect under. #112 attaches it to targets that void bow damage hits. The void bow label is Shadowshot's, which Uncanny Arrows (\"Grants Deadfall and Moebius Quiver energy\") filters on.",
    },
    EventKey {
        hash: 0x3B2C_60FD,
        name: "Grenade Hit Effect",
        evidence: "Used only by the undescribed perk #128, which attaches it to targets that its grenade and chain grenade damage hits. Its entity carries particle and sound nodes and no modifiers. No engine name matched.",
    },
    EventKey {
        hash: 0x4514_3566,
        name: "Player Aura",
        evidence: "Used only by the undescribed perk #1205, which attaches it to the player 0.2 seconds after the perk starts. Its entity has the aura components of the crown aura perks (\"Applies a gold crown aura\"). No engine name matched.",
    },
    EventKey {
        hash: 0x8931_1B17,
        name: "Object Highlight",
        evidence: "Used only by the undescribed perk #1206, which attaches it to the event's first object 0.2 seconds after the perk starts. Its entity has the two components Cerebral Uplink (\"Marks priority targets\") and Vengeance (\"Highlight and defeat those that harm you\") attach. No engine name matched.",
    },
    EventKey {
        hash: 0x02DE_3763,
        name: "Grenade Recharge on Kill",
        evidence: "Used only by the undescribed perk #1259, which attaches it to the player for 7 seconds after a kill. Its entity adds 100 to grenade recharge. No engine name matched.",
    },
    EventKey {
        hash: 0x0EC2_7E0E,
        name: "Once More",
        evidence: "The engine name once_more, the key #1297 (\"Reloading increases this weapon's damage if you hit with at least half the rounds in the magazine, but fail to kill anything\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0x31D3_9C4F,
        name: "Once More Player Facing",
        evidence: "The engine name once_more_player_facing, the key #1297 (\"Reloading increases this weapon's damage if you hit with at least half the rounds in the magazine, but fail to kill anything\") keeps its attached effect under. #1297 attaches it to the player beside Once More, which holds the damage bonus.",
    },
    EventKey {
        hash: 0xB77A_7E58,
        name: "Resurrection Buff",
        evidence: "The engine name resurrection_buff, the key #2013 (\"Revive provides an overshield to you and nearby allies\") keeps its attached effect under.",
    },
    EventKey {
        hash: 0xB1CB_5774,
        name: "Bonus Recovery",
        evidence: "The engine name bonus_recovery, the key the unnamed perks #2202 and #2204 keep their attached effect under. Both attach it after an Orb of Light pickup, and its entity adds 20 Recovery.",
    },
];
