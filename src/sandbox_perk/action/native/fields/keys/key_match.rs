//! Event key match keys: the keys a condition starts or ends on as their effect is attached
//! or removed. Most signals are named in `keys.rs`. The weapon mod keys the undescribed mod
//! perks end on follow the lifetime families (`{weapon}_fast_ready`,
//! `{weapon}_improved_targeting`, `{weapon}_improved_reload` and `{weapon}_unflinching`) and
//! read in the engine's words, since stock perks already use the Bungie mod names for older
//! keys of the same weapons. Three states close the list.
use super::{
    CHARGED_WITH_LIGHT, CROSS_COUNTER_ACTIVE, EMPOWERED_ALLY, EventKey, HELIUM_SPIRALS_SIGNAL,
    INVISIBILITY, MOTES, NEAR_BANK, NOBLE_ROUNDS_SIGNAL, PICKUP_FLASH, TETHER_CHAIN,
    TOUCH_OF_VENOM_SIGNAL, VANISHING_SHADOW_SIGNAL, VEX_RELAY, WARMIND_CELL,
};

pub(super) const ALL: &[EventKey] = &[
    WARMIND_CELL,
    CHARGED_WITH_LIGHT,
    VEX_RELAY,
    TETHER_CHAIN,
    MOTES,
    NEAR_BANK,
    PICKUP_FLASH,
    INVISIBILITY,
    CROSS_COUNTER_ACTIVE,
    VANISHING_SHADOW_SIGNAL,
    TOUCH_OF_VENOM_SIGNAL,
    NOBLE_ROUNDS_SIGNAL,
    HELIUM_SPIRALS_SIGNAL,
    EMPOWERED_ALLY,
    EventKey {
        hash: 0x6EEA_2C2C,
        name: "Rocket Launcher Unflinching",
        evidence: "The engine name rocket_launcher_unflinching, one of the weapon mod keys that the 4 undescribed perks #2141 to #2144 end on.",
    },
    EventKey {
        hash: 0x280F_7C85,
        name: "Light Arms Fast Ready",
        evidence: "The engine name small_arms_fast_ready, one of the weapon mod keys that the 16 undescribed perks #2266 to #2313 end on.",
    },
    EventKey {
        hash: 0xF7D6_DCA9,
        name: "Large Arms Fast Ready",
        evidence: "The engine name large_arms_fast_ready, one of the weapon mod keys that the 20 undescribed perks #2274 to #2321 end on.",
    },
    EventKey {
        hash: 0x9A26_8560,
        name: "Bow Fast Ready",
        evidence: "The engine name bow_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2322 to #2325 end on.",
    },
    EventKey {
        hash: 0x2D69_865F,
        name: "Sidearm Fast Ready",
        evidence: "The engine name sidearm_cannon_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2322 to #2325 end on. The engine reuses the word cannon from hand_cannon_fast_ready, the third key of the same group.",
    },
    EventKey {
        hash: 0x3451_7EDD,
        name: "Submachine Gun Fast Ready",
        evidence: "The engine name smg_cannon_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2322 to #2325 end on. The engine reuses the word cannon from hand_cannon_fast_ready, the third key of the same group.",
    },
    EventKey {
        hash: 0x9421_0FEF,
        name: "Fusion Rifle Fast Ready",
        evidence: "The engine name fusion_rifle_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2326 to #2329 end on.",
    },
    EventKey {
        hash: 0x4AAD_BB77,
        name: "Linear Fusion Rifle Fast Ready",
        evidence: "The engine name linear_fusion_rifle_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2326 to #2329 end on.",
    },
    EventKey {
        hash: 0xB65B_7765,
        name: "Grenade Launcher Fast Ready",
        evidence: "The engine name grenade_launcher_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2330 to #2333 end on.",
    },
    EventKey {
        hash: 0x7D78_A971,
        name: "Sword Fast Ready",
        evidence: "The engine name sword_fast_ready, one of the weapon mod keys that the 4 undescribed perks #2330 to #2333 end on.",
    },
    EventKey {
        hash: 0x46A2_E7BB,
        name: "Scatter Weapon Improved Targeting",
        evidence: "The engine name scatter_weapon_improved_targeting, one of the weapon mod keys that the 24 undescribed perks #2334 to #2377 end on.",
    },
    EventKey {
        hash: 0xA399_0AF6,
        name: "Bow Improved Targeting",
        evidence: "The engine name bow_improved_targeting, one of the weapon mod keys that the 4 undescribed perks #2382 to #2385 end on.",
    },
    EventKey {
        hash: 0x1131_141D,
        name: "Linear Fusion Rifle Improved Targeting",
        evidence: "The engine name linear_fusion_rifle_improved_targeting, one of the weapon mod keys that the 4 undescribed perks #2382 to #2385 end on.",
    },
    EventKey {
        hash: 0x9C78_8855,
        name: "Fusion Rifle Improved Targeting",
        evidence: "The engine name fusion_rifle_improved_targeting, one of the weapon mod keys that the 4 undescribed perks #2386 to #2389 end on.",
    },
    EventKey {
        hash: 0x25DC_D340,
        name: "Rifle Improved Reload",
        evidence: "The engine name rifle_improved_reload, one of the weapon mod keys that the 24 undescribed perks #2390 to #2445 end on.",
    },
    EventKey {
        hash: 0xC5AF_AB91,
        name: "Light Arms Improved Reload",
        evidence: "The engine name small_arms_improved_reload, one of the weapon mod keys that the 16 undescribed perks #2394 to #2441 end on.",
    },
    EventKey {
        hash: 0x44F2_4445,
        name: "Large Arms Improved Reload",
        evidence: "The engine name large_arms_improved_reload, one of the weapon mod keys that the 16 undescribed perks #2402 to #2433 end on.",
    },
    EventKey {
        hash: 0xB751_DD46,
        name: "Bow Improved Reload",
        evidence: "The engine name bow_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2446 to #2449 end on.",
    },
    EventKey {
        hash: 0x6800_B037,
        name: "Hand Cannon Improved Reload",
        evidence: "The engine name hand_cannon_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2446 to #2449 end on.",
    },
    EventKey {
        hash: 0xEB4F_CC97,
        name: "Sidearm Improved Reload",
        evidence: "The engine name sidearm_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2446 to #2449 end on.",
    },
    EventKey {
        hash: 0x1165_6235,
        name: "Submachine Gun Improved Reload",
        evidence: "The engine name smg_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2446 to #2449 end on.",
    },
    EventKey {
        hash: 0x19A4_BB0E,
        name: "Auto Rifle Improved Reload",
        evidence: "The engine name auto_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0xACF3_50FF,
        name: "Fusion Rifle Improved Reload",
        evidence: "The engine name fusion_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0x0FF9_C817,
        name: "Linear Fusion Rifle Improved Reload",
        evidence: "The engine name linear_fusion_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0x6324_23CA,
        name: "Pulse Rifle Improved Reload",
        evidence: "The engine name pulse_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0x76C1_710F,
        name: "Scout Rifle Improved Reload",
        evidence: "The engine name scout_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0x60A2_87C6,
        name: "Sniper Rifle Improved Reload",
        evidence: "The engine name sniper_rifle_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2450 to #2453 end on.",
    },
    EventKey {
        hash: 0xC5FC_34F1,
        name: "Grenade Launcher Improved Reload",
        evidence: "The engine name grenade_launcher_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2454 to #2457 end on.",
    },
    EventKey {
        hash: 0x4334_4686,
        name: "Machine Gun Improved Reload",
        evidence: "The engine name hmg_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2454 to #2457 end on.",
    },
    EventKey {
        hash: 0xB0F2_DE51,
        name: "Rocket Launcher Improved Reload",
        evidence: "The engine name rocket_launcher_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2454 to #2457 end on.",
    },
    EventKey {
        hash: 0xF92E_8AA4,
        name: "Shotgun Improved Reload",
        evidence: "The engine name shotgun_improved_reload, one of the weapon mod keys that the 4 undescribed perks #2454 to #2457 end on.",
    },
    EventKey {
        hash: 0x74DC_7783,
        name: "Invading",
        evidence: "Named for what reads it: the Gambit invasion teleports interference_forced_teleport and interference_teleport_return, and invasion_kill_hopon. The undescribed Gambit perks #1252 to #1254 start on it, change health and shields as it starts and change outgoing damage for 30 seconds. No engine name matched.",
    },
    EventKey {
        hash: 0xF991_0AF8,
        name: "In the Well",
        evidence: "Named for what reads it, beside the Gambit armor pattern player_in_well (of bonus_in_well) and ultra_endcap_player_ramp_hopon. While it lasts, the undescribed Gambit perk #1249 adds 100 to Mobility, Recovery and a third player stat. No engine name matched.",
    },
    EventKey {
        hash: 0xB929_5D1C,
        name: "Reactive Reload",
        evidence: "The engine name reactive_reload. Heard only by Kill Clip (\"Reloading after a kill grants increased damage\"), which ends on it, on a holster or after 5 seconds.",
    },
];
