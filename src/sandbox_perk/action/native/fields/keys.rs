//! Event and mode keys named by the stock perks that use them.
//!
//! A 32-bit key carries no string. What the game does with it is recoverable: when every
//! stock perk that listens to a key reads "each time you pick up an Orb of Light", the key
//! is the Orb of Light pickup. A key is named only where the descriptions agree, and each
//! entry keeps the perks that establish it so a reader can check the claim.

mod abilities;
mod key_match;
mod lifetimes;
mod transmat;
pub use abilities::for_slot as ability_properties;

/// One key the stock perks use at a site, with the perks that name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventKey {
    pub hash: u32,
    pub name: &'static str,
    pub evidence: &'static str,
}

const ORB_EVENT: EventKey = EventKey {
    hash: 0x6CEC_7A87,
    name: "Orb of Light Picked Up",
    evidence: "The event value all 19 stock perks reading \"each time you pick up an Orb of Light\" listen to: Recuperation, Better Already, Innervation, Invigoration, Insulation, Absolution and Supercharged Battery among them.",
};
const ORB_CONTEXT: EventKey = EventKey {
    hash: 0x18CC_BF24,
    name: "Orb of Light Picked Up",
    evidence: "The context key beside the Orb of Light event value in the same 19 stock perks.",
};
const BARRIER_EVENT: EventKey = EventKey {
    hash: 0x9431_F691,
    name: "Champion Barrier Pierced",
    evidence: "Shared by Breach Resonator (a fireteam member shuts down a Barrier Champion's ability) and Counter Charge (pierces a Champion's barrier), the one reading both descriptions share.",
};
const BARRIER_CONTEXT: EventKey = EventKey {
    hash: 0x1124_697D,
    name: "All Players",
    evidence: "The engine name all_players, recovered from the client's own strings. It sits beside the barrier event value in Breach Resonator and Counter Charge, which is why it once read as a second name for that event. It is the scope the event applies to, not the event.",
};
const WARMIND_CELL: EventKey = EventKey {
    hash: 0xAEF2_7365,
    name: "Warmind Cell Collected",
    evidence: "Listened to by all six stock perks reading \"collecting a Warmind Cell\": Blessing of Rasputin, Warmind's Light, Modular Lightning, Strength of Rasputin, Sheltering Energy and Chosen of the Warmind.",
};
const CHARGED_WITH_LIGHT: EventKey = EventKey {
    hash: 0x5AF2_4662,
    name: "Charged with Light Signal",
    evidence: "Resolves to the engine name charged_with_light_signal. Powerful Friends listens to it to spread Charged with Light to nearby allies.",
};
const VEX_RELAY: EventKey = EventKey {
    hash: 0xD642_1878,
    name: "Near an Active Vex Relay",
    evidence: "Started and ended on by Relay Defender and Enhanced Relay Defender, which read \"within 5 meters of an active Vex Relay\".",
};
const TETHER_CHAIN: EventKey = EventKey {
    hash: 0xCC86_F633,
    name: "In a Tether Chain",
    evidence: "Started and ended on by Resistant Tether and Enhanced Resistant Tether, which read \"while part of a tether chain\".",
};
const MOTES: EventKey = EventKey {
    hash: 0x7020_0731,
    name: "Motes Collected",
    evidence: "The engine name kill_tag_gathered. Listened to by Voltaic Mote Collector and its Enhanced form, which count collected Motes.",
};
const NEAR_BANK: EventKey = EventKey {
    hash: 0x4FAA_5194,
    name: "Near the Bank",
    evidence: "The engine name near_bank. Listened to by one undescribed Gambit perk.",
};
const PICKUP_FLASH: EventKey = EventKey {
    hash: 0x79A0_6A4E,
    name: "Pickup Flash",
    evidence: "The engine name pickup_flash. Listened to by one undescribed stock perk.",
};
const RESURRECTION: EventKey = EventKey {
    hash: 0xB331_1944,
    name: "Resurrection",
    evidence: "The engine name resurrection. Matched by one undescribed stock perk.",
};
const FULL_AUTO: EventKey = EventKey {
    hash: 0x75DD_B3C9,
    name: "Full Auto Fire",
    evidence: "Written by Full Auto Trigger System, Rapid-Fire Frame and Thunderer, the stock perks that fire a weapon at full auto or raise its rate of fire.",
};

// Named Property keys, read the same way. The three ammo find chances are split by ammo
// type rather than weapon slot: Hand Cannon Ammo Finder writes both the Primary and the
// Special key because Eriana's Vow is a Special ammo hand cannon, Bow Ammo Finder writes
// Primary and Heavy for Leviathan's Breath, and every other Finder mod fits the same reading.
const PRIMARY_FIND: EventKey = EventKey {
    hash: 0xDAAB_765C,
    name: "Primary Ammo Find Chance",
    evidence: "Written by Primary Ammo Finder and by the Auto Rifle, Pulse Rifle, Scout Rifle, Sidearm, Submachine Gun, Hand Cannon and Bow Ammo Finders, all reading \"chance of finding Primary ammo\".",
};
const SPECIAL_FIND: EventKey = EventKey {
    hash: 0xDC84_2CD7,
    name: "Special Ammo Find Chance",
    evidence: "Written by Special Ammo Finder and by the Fusion Rifle, Shotgun, Sniper Rifle, Grenade Launcher, Linear Fusion Rifle and Hand Cannon Ammo Finders, all reading \"chance of finding Special ammo\" or covering a Special ammo weapon.",
};
const HEAVY_FIND: EventKey = EventKey {
    hash: 0x4164_BE13,
    name: "Heavy Ammo Find Chance",
    evidence: "Written by Heavy Ammo Finder and by the Machine Gun, Rocket Launcher, Sword, Shotgun, Sniper Rifle, Fusion Rifle, Grenade Launcher, Linear Fusion Rifle and Bow Ammo Finders, all reading \"chance of finding Heavy ammo\" or covering a Heavy ammo weapon.",
};
const FINISHER_COST: EventKey = EventKey {
    hash: 0xA5E0_B02C,
    name: "Finisher Super Energy Cost",
    evidence: "Written by Bulwark Finisher, Empowered Finish, Explosive Finisher, Heavy Finisher, One-Two Finisher and Special Finisher, all reading \"requires one-Nth of your Super energy\".",
};
const CHARGED_STACKS: EventKey = EventKey {
    hash: 0x0427_C343,
    name: "Charged with Light Stack Limit",
    evidence: "Written by Charged Up (\"1 additional stack of Charged with Light\") and Supercharged (\"2 additional stacks, up to a maximum of 5\").",
};
const EXPLOSIVE_ROUNDS: EventKey = EventKey {
    hash: 0x5EE2_66FC,
    name: "Explosive Rounds",
    evidence: "Written by Timed Payload, Explosive Payload, Sunburn and Explosive Head, every one reading as projectiles that explode.",
};
const SHIELD_PIERCING: EventKey = EventKey {
    hash: 0xB120_D867,
    name: "Shield Piercing Rounds",
    evidence: "Written by Anti-Barrier Rounds and Looks Can Kill (\"shield-piercing ammunition\") and four undescribed Anti-Barrier variants.",
};
const ARMOR_PIERCING: EventKey = EventKey {
    hash: 0xCAF9_15D8,
    name: "Armor Piercing",
    evidence: "Written by Armor-Piercing Rounds, For the Empire (\"penetrates Phalanx shields\"), Dornröschen (\"laser overpenetrates\"), Shock Blast, Seraph Rounds and Anti-Barrier Rounds.",
};
const LIGHTNING_ROD: EventKey = EventKey {
    hash: 0x2995_F40E,
    name: "Lightning Rod Chain Lightning",
    evidence: "Written by Lightning Rod (\"the next shot chain-lightning capabilities\"), Split Electron and the Trinity Ghoul Catalyst.",
};
const PERSONAL_ASSISTANT: EventKey = EventKey {
    hash: 0x79E0_20E6,
    name: "Personal Assistant",
    evidence: "Written by Personal Assistant (\"shows critical information in scope\") and read by Target Acquired (\"when Personal Assistant is active\").",
};
const STICKY_GRENADES: EventKey = EventKey {
    hash: 0xA7C6_3FDC,
    name: "Sticky Grenades",
    evidence: "Written by Sticky Grenades (\"grenades attach on impact\") and Excavation (\"sticky flame grenades\").",
};
const WARMIND_CELL_CHANCE: EventKey = EventKey {
    hash: 0x7838_029C,
    name: "Warmind Cell Spawn Chance",
    evidence: "Added to by Blessing of Rasputin when a Warmind Cell is collected (\"increases the chances that your next final blow with a Seraph weapon will create a Warmind Cell\") and initialised by the five Seraph weapon perks that read it.",
};
// The general predicate's key at +D4 is the engine state it checks, in the same namespace
// as the compiled comparison variables: is_arc, is_void, super_active, iron_sights,
// weapon_firing, charged_with_light_stacks, melee_energy and nearby_enemy_count all hash to
// stock values there with the predicate's own FNV-1 fold. The rest are named the way the
// event keys are, from the perks that check them.
const CHARGED_WITH_LIGHT_STATE: EventKey = EventKey {
    hash: 0x59E3_47ED,
    name: "Charged with Light Stacks",
    evidence: "The engine name charged_with_light_stacks. Checked by 27 stock perks reading \"While Charged with Light\", from Striking Light and Heavy Handed to High-Energy Fire, and inverted by Charge Harvester (\"While you are not Charged with Light\").",
};
const SUBCLASS_ARC: EventKey = EventKey {
    hash: 0xE1E6_BB64,
    name: "Subclass Is Arc",
    evidence: "The engine name is_arc. Checked by Volatile Conduction (\"bonus Arc Super damage\") and the Arc grenade recharge perk.",
};
const SUBCLASS_VOID: EventKey = EventKey {
    hash: 0xEE5F_6482,
    name: "Subclass Is Void",
    evidence: "The engine name is_void. Checked by Touch of Venom and the perks reading \"bonus Void Super damage\" and \"activating Void class abilities\".",
};
const SUPER_ACTIVE: EventKey = EventKey {
    hash: 0xD16E_1FA3,
    name: "Super Active",
    evidence: "The engine name super_active. Checked by Resolute (\"casting Fists of Havoc\"), Vengeance and Cross Counter.",
};
const IRON_SIGHTS: EventKey = EventKey {
    hash: 0xD9C4_6BC4,
    name: "Iron Sights State",
    evidence: "The engine name iron_sights. Fan Fire and Archer's Gambit compare this state as part of their hip-fire requirements. The comparison determines which state qualifies, so the key itself must not be labeled Hip Fire.",
};
const WEAPON_FIRING: EventKey = EventKey {
    hash: 0x59E4_FF8D,
    name: "Weapon Firing",
    evidence: "The engine name weapon_firing. Checked by Tap the Trigger (\"initial trigger pull\"), Opening Shot, Reservoir Burst, and inverted by Box Breathing (\"aiming without firing\").",
};
const NEARBY_ENEMIES_STATE: EventKey = EventKey {
    hash: 0x58A9_CB99,
    name: "Nearby Enemy Count",
    evidence: "The engine name nearby_enemy_count, the same variable the compiled comparisons use. Checked by Distribution and Dynamo (\"near enemies\").",
};
const MELEE_ENERGY_STATE: EventKey = EventKey {
    hash: 0x2814_F006,
    name: "Melee Energy",
    evidence: "The engine name melee_energy, compared by Heavy Handed.",
};
const MELEE_OVERCHARGE_STATE: EventKey = EventKey {
    hash: 0xDFAD_6B26,
    name: "Melee Overcharge State",
    evidence: "The engine name melee_overcharge_state, compared by Heavy Handed.",
};
const AIMING_DOWN_SIGHTS: EventKey = EventKey {
    hash: 0x2471_5FE8,
    name: "Aiming Down Sights for a Moment",
    evidence: "Checked only by Unstoppable Hand Cannon and Unstoppable Burst (\"Aiming down sights loads a powerful explosive payload\"), each starting when it reads 1 and ending when it does not. The payload loads after aim is held briefly, so this is aim held long enough, not the weapon's own Aiming Down Sights state. No engine name resolved.",
};
const WEAPON_HOLSTERED: EventKey = EventKey {
    hash: 0xD5B8_A34B,
    name: "Weapon Holstered",
    evidence: "Checked by Auto-Loading Holster, Serve the Colony, the Eriana's Vow Catalyst and the Witherhoard Catalyst, all reading \"holstered\" or \"unequipped\". No engine name resolved.",
};
const CHARGE_FRACTION: EventKey = EventKey {
    hash: 0x7AA3_47DD,
    name: "Draw or Charge Fraction",
    evidence: "The engine name weapon_charge_fraction, a fraction rather than a switch. Overload Arrowheads and the Unstoppable bow perk (\"fully drawn arrows\") require 0.9 to 1, Pyrogenesis (\"fully charging the laser\") and Close the Gap require 1, and Archer's Gambit requires it above 0.",
};
const PERK_COUNTER: EventKey = EventKey {
    hash: 0x748A_92CC,
    name: "Perk Counter",
    evidence: "The engine name perk_counter. Release the Wolves, the Cerberus+1 Catalyst, Ravenous Beast and Noble Rounds compare this value. Its interpretation depends on the weapon's program, so it is not a universal alternate-fire switch.",
};
const ROUNDS_LOADED: EventKey = EventKey {
    hash: 0x32CC_5402,
    name: "Rounds Loaded",
    evidence: "The engine name rounds_loaded. Quick Access Sling and Rapid-Fire Frame compare it to detect an empty magazine. The comparison and threshold determine which ammunition counts qualify. Selecting this value alone does not require an empty magazine.",
};
const PERK_ACTIVE: EventKey = EventKey {
    hash: 0xA43A_8C2E,
    name: "Perk Active",
    evidence: "The engine name perk_active. The Fundamentals, Arc Conductor and Revolution use this shared property for different weapon states. Its values belong to the receiving program, not a universal element or alternate-fire enumeration.",
};

const WEAPON_EQUIPPED_STATE: EventKey = EventKey {
    hash: 0x6A1C_27EE,
    name: "Weapon Equipped",
    evidence: "The engine name equipped. Checked by Killing Tally (\"until it is stowed or reloaded\") and En Garde (\"immediately after swapping to this sword\").",
};
// More of the predicate's states, named the same way. These engine names were recovered from
// the running client's memory (see `CLIENT_NAMES`) and the stock perks that check them.
const SUBCLASS_SOLAR: EventKey = EventKey {
    hash: 0x42DE_FF49,
    name: "Subclass Is Solar",
    evidence: "The engine name is_thermal, its word for Solar beside is_arc and is_void. Checked by Sunfire Furnace, Solar Plexus and the Solar class ability mod (\"Activating Solar class abilities grants an overshield\").",
};
const EXOTIC_ARMOR_PERK_ACTIVE: EventKey = EventKey {
    hash: 0xD9E7_346F,
    name: "Exotic Armor Perk Active",
    evidence: "The engine name exotic_armor_perk_active. Checked by the exotic armor perks Auto-Loading Link, Helium Spirals, Vengeance and Overflowing Light, and written by 15, Horns of Doom and Sunfire Furnace among them.",
};
const GUARDING: EventKey = EventKey {
    hash: 0xD27B_4E9D,
    name: "Guarding",
    evidence: "The engine name is_guarding. Checked by Ursine Guard (\"Block damage with Sentinel Shield\").",
};
const GUARDING_WITH_SWORD: EventKey = EventKey {
    hash: 0x9202_856A,
    name: "Guarding with a Sword",
    evidence: "The engine name is_guarding_with_sword. Checked by Clenched Fist (\"Bonuses to guarding with Swords\").",
};
const EXOTIC_PERK_COUNTER: EventKey = EventKey {
    hash: 0x8C79_67C7,
    name: "Exotic Perk Counter",
    evidence: "The engine name exotic_perk_counter. Compared by Honed Edge and the Izanagi's Burden Catalyst, from 1 to 4, and written by two undescribed stock perks.",
};
const SERAPH_WEAPON_EQUIPPED: EventKey = EventKey {
    hash: 0xDE1D_8C04,
    name: "Seraph Weapon Equipped",
    evidence: "The engine name rasputin_weapon_equipped. Checked by the five Warmind Cell mods, the ones Blessing of Rasputin ties to \"a Seraph weapon\".",
};
const SOLAR_SPLASH_CELLS: EventKey = EventKey {
    hash: 0xA03D_DB0D,
    name: "Solar Splash Spawns Warmind Cells",
    evidence: "The engine name solar_splash_spawn_baubles. The engine calls Warmind Cells baubles, as in warmind_cells_increase_bauble_chance. Checked by the five Warmind Cell mods.",
};
const CELL_SPAWN_CHANCE: EventKey = EventKey {
    hash: 0x50C1_395D,
    name: "Warmind Cells Increase Cell Chance",
    evidence: "The engine name warmind_cells_increase_bauble_chance. Checked by the five Warmind Cell mods.",
};
const MAGAZINE_FRACTION: EventKey = EventKey {
    hash: 0x6F47_FA3A,
    name: "Magazine Fraction",
    evidence: "The engine name equipped_item_magazine_fraction, compared as a fraction by Strange Protractor (\"Sprinting reloads current weapon\") and one mod.",
};
const SUNSPOTS_GRANT_SUN_WARRIOR: EventKey = EventKey {
    hash: 0xA957_F024,
    name: "Sunspots Grant Sun Warrior",
    evidence: "The engine name enable_lingering_fire_apply_buff. Checked by Beacons of Empowerment (\"Allies who pass through your Sunspots are granted Sun Warrior\") and written by one undescribed stock perk.",
};
const RELOADING: EventKey = EventKey {
    hash: 0xECDE_609E,
    name: "Reloading",
    evidence: "The engine name reloading. Checked by Close the Gap and Revolution.",
};
const OVERCHARGE_STATE: EventKey = EventKey {
    hash: 0xE7A6_D9A5,
    name: "Overcharge State",
    evidence: "The engine name overcharge_state, checked by Scissor Fingers.",
};
const MELEE_OVERCHARGE: EventKey = EventKey {
    hash: 0xD1CC_38CC,
    name: "Melee Overcharge",
    evidence: "The engine name melee_overcharge, checked by Scissor Fingers.",
};
const RECENT_BLOCKED_DAMAGE: EventKey = EventKey {
    hash: 0x9D16_778B,
    name: "Recent Blocked Damage",
    evidence: "The engine name recent_blocked_damage, compared by one undescribed stock perk.",
};
const CURRENT_BLOCKED_DAMAGE: EventKey = EventKey {
    hash: 0xB636_2D8F,
    name: "Current Blocked Damage",
    evidence: "The engine name current_blocked_damage, compared by Ursine Guard (\"Block damage with Sentinel Shield for more Super energy\").",
};
const BONUS_DAMAGE_STACKS: EventKey = EventKey {
    hash: 0x2A98_BCB3,
    name: "Bonus Damage Stacks Remaining",
    evidence: "The engine name bonus_damage_stacks_remaining, compared by Surprise Attack.",
};
// These were matched by hashing names built from the checking perks' own words and the
// engine's recovered vocabulary, as rampage_level was: each hash matches exactly, and each
// name reads as its perk describes.
// A candidate that matched without reading that way (last_empty_revived_duration for Fusion
// Harness) was left out.
const TARGETING_FRACTION: EventKey = EventKey {
    hash: 0x1950_23EF,
    name: "Targeting Fraction",
    evidence: "The engine name targeting_fraction. Checked by Target Acquired (\"better target acquisition\").",
};
const OUTLAW_ACTIVE: EventKey = EventKey {
    hash: 0x3176_8F77,
    name: "Outlaw Active",
    evidence: "The engine name outlaw_applied. Checked by Desperado (\"Reloading while Outlaw is active\").",
};
const CHARGING_GRENADE: EventKey = EventKey {
    hash: 0x50E1_F7BE,
    name: "Charging Grenade",
    evidence: "The engine name is_charging_grenade. Checked by Chaotic Exchanger (\"Improved charging for Void grenades\").",
};
const SHOULDER_CHARGE_ENABLED: EventKey = EventKey {
    hash: 0x686F_5CBA,
    name: "Shoulder Charge Enabled",
    evidence: "The engine name shoulder_charge_enabled. Checked by Seriously, Watch Out.",
};
const SHOULDER_CHARGE_READY: EventKey = EventKey {
    hash: 0x706F_9D4C,
    name: "Shoulder Charge Ready",
    evidence: "The engine name shoulder_charge_ready. Checked by Peregrine Strike (\"Bonus damage on airborne shoulder charge\").",
};
const PERK_ACTIVE_COUNTER: EventKey = EventKey {
    hash: 0x7227_FCBB,
    name: "Perk Active Counter",
    evidence: "The engine name perk_active_counter. Compared by Revolution, from 1 to 20.",
};
const SLIDING: EventKey = EventKey {
    hash: 0xA1ED_574E,
    name: "Sliding",
    evidence: "The engine name is_sliding. Checked by one undescribed stock perk.",
};
const SPIN_FRACTION: EventKey = EventKey {
    hash: 0xBF74_EA1A,
    name: "Spin Fraction",
    evidence: "The engine name spin_fraction. Business Time (\"Sustained fire boosts range and rate of fire\") requires it at 1.",
};
const MIDA_MULTI_TOOL_EQUIPPED: EventKey = EventKey {
    hash: 0xDF62_9476,
    name: "MIDA Multi-Tool Equipped",
    evidence: "The engine name player_mida_equipped. Checked by MIDA Synergy (\"when MIDA Multi-Tool is also equipped\").",
};
const RAT_KING_COUNTER: EventKey = EventKey {
    hash: 0x7366_B825,
    name: "Rat King Counter",
    evidence: "The engine name rat_king_counter. Rat Pack (\"Nearby Rat Kings increase strength\") writes it and compares it from 1 to 6.",
};
const DESPERADO_CHECK: EventKey = EventKey {
    hash: 0x2A4E_DD93,
    name: "Desperado Check",
    evidence: "The engine name desperado_check. Checked by Desperado (\"Reloading while Outlaw is active increases your rate of fire\").",
};
const PERK_COUNTER_COMBO: EventKey = EventKey {
    hash: 0x3B29_169F,
    name: "Perk Counter Combo",
    evidence: "The engine name perk_counter_combo. Relentless Strikes requires it at 3 (\"Landing three light-attack hits within a short time\").",
};
const DRAWING_BOW: EventKey = EventKey {
    hash: 0x4184_2619,
    name: "Drawing a Bow",
    evidence: "The engine name charging_bow. Checked by Adamantine Brace (\"can be held indefinitely\").",
};
const SPLIT_ARROW_PRECISION: EventKey = EventKey {
    hash: 0x5950_DEE8,
    name: "Split Arrow Precision",
    evidence: "The engine name split_arrow_precision. Checked by Lightning Rod (\"Precision kills grant the next shot chain-lightning capabilities\") and Split Electron (\"Fires an arrow that splits\"), and written by Split Electron and the Trinity Ghoul Catalyst.",
};
const OVERFLOW_ROUNDS: EventKey = EventKey {
    hash: 0x69E6_D035,
    name: "Overflow Rounds",
    evidence: "The engine name overflow_rounds. Compared by Overflow (\"beyond normal capacity\") and Clown Cartridge (\"randomly overfills it from reserves\").",
};
const FULLY_CHARGED_FRACTION: EventKey = EventKey {
    hash: 0x8C2B_3422,
    name: "Fully Drawn or Charged Fraction",
    evidence: "The engine name weapon_fully_charged_fraction, beside weapon_charge_fraction. Poison Arrows (\"Fires poison arrows on full draw and quick release\") requires it above 0.",
};
const EXOTIC_PERFECT_GUARD: EventKey = EventKey {
    hash: 0xCDFE_45FF,
    name: "Exotic Perfect Guard",
    evidence: "The engine name exotic_perfect_guard. Clenched Fist (\"Bonuses to guarding with Swords\") requires it above 0.",
};
const BOW_DRAW_STATE: EventKey = EventKey {
    hash: 0x39E5_FF44,
    name: "Bow Draw State",
    evidence: "The engine name bow_charge_state. Queen's Wrath (\"When aiming down sights and fully drawn, gain Truesight\") requires it from 0.5 to 3.",
};
const CHARGE_FIRE_CHECK: EventKey = EventKey {
    hash: 0xE1AC_98B1,
    name: "Charge Fire Check",
    evidence: "The engine name charge_fire_check. Close the Gap (\"Charges to fire a high-powered laser\") ends when it is 0.",
};

// Keys no engine name resolved for, named by the only stock perks that write or check them.
// A property is named for its perk as the ones above are. A state that perk checks reads
// as that perk active, or as its value when it is compared over a range.
const RICOCHET_ROUNDS: EventKey = EventKey {
    hash: 0xAAF0_D5D6,
    name: "Ricochet Rounds",
    evidence: "Written by Ricochet Rounds, Seraph Rounds, Dornröschen (\"Laser overpenetrates and refracts\") and The Corruption Spreads.",
};
const MAXIMIZED_GUARD_EFFICIENCY: EventKey = EventKey {
    hash: 0x36BD_3333,
    name: "Maximized Guard Efficiency",
    evidence: "Written by Burst Guard and Enduring Guard, both reading \"Sword Guard has maximized efficiency\", and by Heavy Guard.",
};
const AEON_ENERGY: EventKey = EventKey {
    hash: 0x1418_00D8,
    name: "Aeon Energy",
    evidence: "Written by the three Aeon Energy perks.",
};
const UNSTOPPABLE_PAYLOAD: EventKey = EventKey {
    hash: 0x35F8_1390,
    name: "Unstoppable Payload",
    evidence: "Written by Unstoppable Hand Cannon and its pulse rifle and bow forms, all reading \"a powerful explosive payload that staggers unshielded combatants\".",
};
const OVERLOAD_ROUNDS: EventKey = EventKey {
    hash: 0xAD36_716D,
    name: "Overload Rounds",
    evidence: "Written by Overload Rounds and two variants reading \"cause disruption\".",
};
const SUPERCHARGED_ARC_SOUL: EventKey = EventKey {
    hash: 0x3AD2_5823,
    name: "Supercharged Arc Soul",
    evidence: "Written by Dynamic Duo (\"Convert your Arc Grenade into a supercharged Arc Soul\") and one undescribed perk.",
};
const FASTER_SUMMONING: EventKey = EventKey {
    hash: 0xA2DC_A8DC,
    name: "Faster Summoning",
    evidence: "Written by Dawning Parade (\"This vehicle takes less time to summon\") and Speed Demon.",
};
const RELOAD_WHILE_RIDING: EventKey = EventKey {
    hash: 0xE05D_B490,
    name: "Reload While Riding",
    evidence: "Written by Speed Demon and a Sparrow perk reading \"This vehicle reloads weapons while you ride\".",
};
const HIGH_CALIBER_ROUNDS: EventKey = EventKey {
    hash: 0x1789_1796,
    name: "High-Caliber Rounds",
    evidence: "Written by High-Caliber Rounds and Unstoppable Burst (\"staggers unshielded combatants\").",
};
const TESSERACT: EventKey = EventKey {
    hash: 0x5A5B_A104,
    name: "Tesseract",
    evidence: "Written and counted by Tesseract (\"launch a heavy Blink attack\") and by its catalyst (\"Reduces the activation time of Tesseract\").",
};
const AGGRESSIVE_FRAME: EventKey = EventKey {
    hash: 0xBE35_746F,
    name: "Aggressive Frame",
    evidence: "Written by Aggressive Frame (\"Increases rate of fire after kill\") and Shot Package.",
};
const HAWKEYE_HACK: EventKey = EventKey {
    hash: 0x0734_3A3C,
    name: "Hawkeye Hack",
    evidence: "Written only by Hawkeye Hack (\"Golden Gun fires one high-damage shot\").",
};
const SURVIVAL_WELL: EventKey = EventKey {
    hash: 0xAA42_90AB,
    name: "Survival Well",
    evidence: "Written only by Survival Well.",
};
const STANDARD_DRIVE: EventKey = EventKey {
    hash: 0x05A7_48C3,
    name: "Standard Drive",
    evidence: "Written only by the Sparrow perk Standard Drive.",
};
const TUNED_DRIVE: EventKey = EventKey {
    hash: 0xE148_A1C0,
    name: "Tuned Drive",
    evidence: "Written only by the Sparrow perk Tuned Drive.",
};
const CUSTOM_DRIVE: EventKey = EventKey {
    hash: 0x5EC5_FE22,
    name: "Custom Drive",
    evidence: "Written only by the Sparrow perk Custom Drive.",
};
const IMPROVED_ASSEMBLER: EventKey = EventKey {
    hash: 0x254E_F1D6,
    name: "Improved Assembler",
    evidence: "Written only by Improved Assembler (\"This vehicle has a shorter cooldown between summonings\").",
};
const AIRBORNE_AGILITY: EventKey = EventKey {
    hash: 0x12DA_FD65,
    name: "Airborne Agility",
    evidence: "Written only by a Sparrow perk reading \"This vehicle gains increased agility while airborne\".",
};
const VERNIER_THRUSTERS: EventKey = EventKey {
    hash: 0xD9EA_66FF,
    name: "Vernier Thrusters",
    evidence: "Written only by Vernier Thrusters (\"improved dodging from side to side\").",
};
const CLUSTER_BOMB: EventKey = EventKey {
    hash: 0xA026_F207,
    name: "Cluster Bomb",
    evidence: "Written only by Cluster Bomb (\"Rockets spawn cluster bombs upon detonation\").",
};
const TIMED_PAYLOAD: EventKey = EventKey {
    hash: 0x725F_0D88,
    name: "Timed Payload",
    evidence: "Written only by Timed Payload (\"Projectiles attached to enemies explode after a short delay\").",
};
const BLINDING_GRENADES: EventKey = EventKey {
    hash: 0xE2CA_4ED9,
    name: "Blinding Grenades",
    evidence: "Written only by Blinding Grenades (\"Detonation has a brief blinding effect\").",
};
const THERMOPLASTIC_GRENADES: EventKey = EventKey {
    hash: 0x1B1C_B96F,
    name: "Thermoplastic Grenades",
    evidence: "Written only by Thermoplastic Grenades (\"Grenades fired from this weapon bounce further\").",
};
const CONCUSSION_GRENADES: EventKey = EventKey {
    hash: 0x2FEF_7D87,
    name: "Concussion Grenades",
    evidence: "Written only by Concussion Grenades (\"Grenades emit a blast that staggers enemies\").",
};
const PROXIMITY_GRENADES: EventKey = EventKey {
    hash: 0x8DB1_9D54,
    name: "Proximity Grenades",
    evidence: "Written only by Proximity Grenades.",
};
const SPIKE_GRENADES: EventKey = EventKey {
    hash: 0xEFFA_FC47,
    name: "Spike Grenades",
    evidence: "Written only by Spike Grenades.",
};
const INFINITE_GUARD: EventKey = EventKey {
    hash: 0xC716_7371,
    name: "Infinite Guard",
    evidence: "Written only by Infinite Guard (\"Sword Guard has balanced defenses and maximized endurance\").",
};
const EXCAVATION: EventKey = EventKey {
    hash: 0x8CD4_5D94,
    name: "Excavation",
    evidence: "Written only by Excavation (\"Detonate multiple sticky flame grenades at once\").",
};
const ACCOMPLICE: EventKey = EventKey {
    hash: 0x059F_2F22,
    name: "Accomplice",
    evidence: "Written only by Accomplice.",
};
const SUPERCONDUCTOR: EventKey = EventKey {
    hash: 0x65D8_F47E,
    name: "Superconductor",
    evidence: "Written only by Superconductor (\"shots fired have the chance to become chain lightning\").",
};
const BLACK_HOLE: EventKey = EventKey {
    hash: 0x158B_5592,
    name: "Black Hole",
    evidence: "Written only by Black Hole (\"Second shot in burst does high damage\").",
};
const MIDA_MULTI_TOOL: EventKey = EventKey {
    hash: 0x1BEA_A89B,
    name: "MIDA Multi-Tool",
    evidence: "Written only by the MIDA Multi-Tool perk.",
};
const RAT_PACK: EventKey = EventKey {
    hash: 0x21CD_3B04,
    name: "Rat Pack",
    evidence: "Written only by Rat Pack.",
};
const VANISHING_SHADOW: EventKey = EventKey {
    hash: 0xAD25_5148,
    name: "Vanishing Shadow",
    evidence: "Written only by Vanishing Shadow (\"Improved invisibility\").",
};
const URSINE_GUARD: EventKey = EventKey {
    hash: 0x3924_BAAE,
    name: "Ursine Guard",
    evidence: "Written only by Ursine Guard (\"Block damage with Sentinel Shield for more Super energy\").",
};
const OVERFLOWING_LIGHT: EventKey = EventKey {
    hash: 0xC34A_D42E,
    name: "Overflowing Light",
    evidence: "Written only by Overflowing Light.",
};
const DESTABILIZERS: EventKey = EventKey {
    hash: 0xC45B_9C11,
    name: "Destabilizers",
    evidence: "Written only by Destabilizers (\"release roll stabilizers\").",
};
const UNFORESEEN_REPERCUSSIONS: EventKey = EventKey {
    hash: 0xB274_6C51,
    name: "Unforeseen Repercussions",
    evidence: "Written only by Unforeseen Repercussions (\"causing delayed explosions\").",
};
const SUPERCHARGED_BATTERY: EventKey = EventKey {
    hash: 0xEA62_1686,
    name: "Supercharged Battery",
    evidence: "Written only by Supercharged Battery (\"a short period of maximum power\").",
};
const SPLIT_ELECTRON: EventKey = EventKey {
    hash: 0x8B1C_AF28,
    name: "Split Electron",
    evidence: "Written only by Split Electron.",
};
const BROADHEAD: EventKey = EventKey {
    hash: 0xA37C_A9AE,
    name: "Broadhead",
    evidence: "Written only by Broadhead (\"One shot can overpenetrate multiple targets\").",
};
const QUEENS_WRATH: EventKey = EventKey {
    hash: 0xC137_AA1D,
    name: "Queen's Wrath",
    evidence: "Written only by Queen's Wrath (\"When aiming down sights and fully drawn, gain Truesight\").",
};
const MAGNIFICENT_HOWL: EventKey = EventKey {
    hash: 0x5B17_58A8,
    name: "Magnificent Howl",
    evidence: "Written only by Magnificent Howl (\"increases the next shot's damage and range\").",
};
const REIGN_HAVOC: EventKey = EventKey {
    hash: 0x3C80_64AA,
    name: "Reign Havoc",
    evidence: "Written only by Reign Havoc (\"Kills generate lightning strikes\").",
};
const POISON_ARROWS: EventKey = EventKey {
    hash: 0x6EEB_1CFA,
    name: "Poison Arrows",
    evidence: "Written only by Poison Arrows.",
};
const RAMPAGE_SPEC: EventKey = EventKey {
    hash: 0xB154_A405,
    name: "Rampage Spec",
    evidence: "Written only by Rampage Spec (\"Increases duration of Rampage\").",
};
const HAPPY_DAWNING: EventKey = EventKey {
    hash: 0x14A6_E541,
    name: "Happy Dawning",
    evidence: "Written only by the Sparrow perk Happy Dawning.",
};
const DAWNING_DARE: EventKey = EventKey {
    hash: 0x5D25_4E51,
    name: "Dawning Dare",
    evidence: "Written only by the Sparrow perk Dawning Dare.",
};
const FULL_COURT: EventKey = EventKey {
    hash: 0x1E08_3BC6,
    name: "Full Court",
    evidence: "Written only by Full Court (\"Increases detonation damage as the projectile travels further\").",
};
const GRENADES_AND_HORSESHOES: EventKey = EventKey {
    hash: 0x9FBC_42B6,
    name: "Grenades and Horseshoes",
    evidence: "Written only by Grenades and Horseshoes (\"Projectiles will detonate when they are within close proximity of their targets\").",
};
const EXPLOSIVE_LIGHT: EventKey = EventKey {
    hash: 0x44D3_2D54,
    name: "Explosive Light",
    evidence: "Written only by Explosive Light (\"increases the next grenade's blast radius and damage\").",
};
const UNSTOPPABLE_BURST: EventKey = EventKey {
    hash: 0x174C_8245,
    name: "Unstoppable Burst",
    evidence: "Written only by Unstoppable Burst.",
};
const REVOLUTION: EventKey = EventKey {
    hash: 0xE1D5_9F4A,
    name: "Revolution",
    evidence: "Written only by Revolution.",
};
const BLACK_TALON_CATALYST: EventKey = EventKey {
    hash: 0x4ACD_ED94,
    name: "Black Talon Catalyst",
    evidence: "Written only by the Black Talon Catalyst (\"increase the damage of Crow's Wings\").",
};
const BASTION_CATALYST: EventKey = EventKey {
    hash: 0x4519_F863,
    name: "Bastion Catalyst",
    evidence: "Written only by the Bastion Catalyst (\"Increases the maximum number of Dynamic Charge stacks\").",
};
const WAVE_FRAME: EventKey = EventKey {
    hash: 0xDCD8_142A,
    name: "Wave Frame",
    evidence: "Written only by Wave Frame (\"Projectiles release a wave of energy when they contact the ground\").",
};
const ROVING_ASSASSIN_ACTIVE: EventKey = EventKey {
    hash: 0x2863_3612,
    name: "Roving Assassin Active",
    evidence: "Written and checked only by Roving Assassin (\"Vanish after Spectral Blades kills\").",
};
const LINEAR_ACTUATORS_ACTIVE: EventKey = EventKey {
    hash: 0x7D9C_D621,
    name: "Linear Actuators Active",
    evidence: "Written and checked only by the two Linear Actuators perks.",
};
const FULLY_SPUN_UP: EventKey = EventKey {
    hash: 0xEC9F_5294,
    name: "Fully Spun Up",
    evidence: "Written by Business Time (\"Sustained fire boosts range and rate of fire\") and checked by the perk reading \"When this weapon is fully spun up, the flinch from incoming damage is greatly reduced\".",
};
const URSINE_GUARD_VALUE: EventKey = EventKey {
    hash: 0x290C_7A47,
    name: "Ursine Guard Value",
    evidence: "Written and compared only by Ursine Guard, from 0.01 to 100.",
};
const MULTIKILL_CLIP_ACTIVE: EventKey = EventKey {
    hash: 0x3F63_D768,
    name: "Multikill Clip Active",
    evidence: "Checked only by Multikill Clip, which starts while it holds.",
};
const LAST_STAND_VALUE: EventKey = EventKey {
    hash: 0x18EA_C8E2,
    name: "Last Stand Value",
    evidence: "Compared only by Last Stand, from 0.1 to 5.",
};
const RAPID_FIRE_FRAME_VALUE: EventKey = EventKey {
    hash: 0x0F37_AE29,
    name: "Rapid-Fire Frame Value",
    evidence: "Compared only by Rapid-Fire Frame (\"Fast reload when empty\"), from 0.8 to 1.",
};
const UNINTERRUPTED_FIRE: EventKey = EventKey {
    hash: 0x107F_957E,
    name: "Uninterrupted Fire",
    evidence: "Checked only by a perk reading \"Uninterrupted fire grants bullets that cause disruption\".",
};
const MAGAZINE_HIT_FRACTION: EventKey = EventKey {
    hash: 0x0786_9B5C,
    name: "Magazine Hit Fraction",
    evidence: "Compared from 0.5 to 1 by the one perk reading \"if you hit with at least half the rounds in the magazine\".",
};
const BACKUP_PLAN_ACTIVE: EventKey = EventKey {
    hash: 0x3373_F26C,
    name: "Backup Plan Active",
    evidence: "Checked only by Backup Plan (\"for a short time immediately after swapping to this weapon\"), which ends when it is 0.",
};
const SWORD_GUARD_RAISED: EventKey = EventKey {
    hash: 0x509E_50CB,
    name: "Sword Guard Raised",
    evidence: "Whirlwind Blade ends at 1 and is ready again at 0, and its text says \"Guarding also ends the effect\".",
};
const FUSION_HARNESS_VALUE: EventKey = EventKey {
    hash: 0x5885_28CA,
    name: "Fusion Harness Value",
    evidence: "Compared only by Fusion Harness, from 0.5 to 1.",
};
const CROSS_COUNTER_ACTIVE: EventKey = EventKey {
    hash: 0x5A3B_A996,
    name: "Cross Counter Active",
    evidence: "Checked by Cross Counter (\"Cross Counter regenerates health and deals extra damage\") as a state and as the signal that ends it.",
};
const STRANGE_PROTRACTOR_VALUE: EventKey = EventKey {
    hash: 0x65E1_D735,
    name: "Strange Protractor Value",
    evidence: "Compared only by Strange Protractor, from 1.",
};
const EN_GARDE_VALUE: EventKey = EventKey {
    hash: 0x6D26_7215,
    name: "En Garde Value",
    evidence: "Compared only by En Garde (\"Quick attacks immediately after swapping to this sword\"), from 3 to 5.",
};
const MASTER_OF_ARMS_VALUE: EventKey = EventKey {
    hash: 0x847B_6253,
    name: "Master of Arms Value",
    evidence: "Driven by the effect Master of Arms attaches and compared by Master of Arms (\"Kills with any weapon improve this weapon's damage\").",
};
const UNCANNY_ARROWS_VALUE: EventKey = EventKey {
    hash: 0x8136_C08E,
    name: "Uncanny Arrows Value",
    evidence: "Compared only by Uncanny Arrows (\"Grants Deadfall and Moebius Quiver energy\") and Actual Grandeur, at 0 and 2.",
};
const ARCHERS_GAMBIT_ACTIVE: EventKey = EventKey {
    hash: 0x8BB1_B35B,
    name: "Archer's Gambit Active",
    evidence: "Checked only by Archer's Gambit (\"Hipfire precision hits grant a massive draw-speed bonus\").",
};
const TESSERACT_ACTIVE: EventKey = EventKey {
    hash: 0x9993_B818,
    name: "Tesseract Active",
    evidence: "Checked only by Tesseract and its catalyst.",
};
const FIREFLY_ACTIVE: EventKey = EventKey {
    hash: 0xAE07_985B,
    name: "Firefly Active",
    evidence: "Checked only by Firefly, which starts while it does not hold.",
};
const INSIDE_WELL: EventKey = EventKey {
    hash: 0xC989_AF2E,
    name: "Inside Well of Radiance",
    evidence: "Checked only by Battle-Hearth (\"Gain Super energy for kills and assists inside Well of Radiance\").",
};
const COLOSSUS_ACTIVE: EventKey = EventKey {
    hash: 0xEE5E_0F34,
    name: "Armor of the Colossus Active",
    evidence: "Checked only by Armor of the Colossus (\"spinning up this weapon protects you with an Arc Shield\").",
};
const VANISHING_SHADOW_SIGNAL: EventKey = EventKey {
    hash: 0x6E45_0057,
    name: "Vanishing Shadow Signal",
    evidence: "Heard only by Vanishing Shadow.",
};
const TOUCH_OF_VENOM_SIGNAL: EventKey = EventKey {
    hash: 0xD4FD_8F9E,
    name: "Touch of Venom Signal",
    evidence: "Heard only by Touch of Venom, which also keeps its attached effect for as long as this lasts.",
};
const NOBLE_ROUNDS_SIGNAL: EventKey = EventKey {
    hash: 0x51BF_E9B3,
    name: "Noble Rounds Signal",
    evidence: "Heard only by Noble Rounds, which starts and ends on it.",
};
const HELIUM_SPIRALS_SIGNAL: EventKey = EventKey {
    hash: 0x4D34_C561,
    name: "Helium Spirals Signal",
    evidence: "Heard only by Helium Spirals (\"Solar grenades burn longer. Melee kills restore them\"), which extends its timers on it.",
};
const EMPOWERED_ALLY: EventKey = EventKey {
    hash: 0x3F98_9C3E,
    name: "Empowered Ally",
    evidence: "The engine name empowered_ally. Heard only by the undescribed perk #59, which also fires when a heal lands or an ally is revived.",
};
// Signals stock perks publish (kind 43) or hear as a named event (kind 40). Six of the seven
// published ones also sit in one native registry resource, 0x80B9E5BF.
const FOCUSED_FIRE_TRIGGERED: EventKey = EventKey {
    hash: 0x5913_75D1,
    name: "Focused Fire Triggered",
    evidence: "The engine name focused_fire_triggered. Published only by the undescribed perk #29 on a precision hit, as it attaches Precision Hit Buff.",
};
const SHADOWSHOT_TETHER: EventKey = EventKey {
    hash: 0xD402_EF01,
    name: "Shadowshot Tether",
    evidence: "The engine name void_bow_tether, where void bow is the Shadowshot label. Published only by the undescribed perk #97 after a void bow kill.",
};
const ARC_WARLOCK_CAPACITORS_SIGNAL: EventKey = EventKey {
    hash: 0xA569_730C,
    name: "Arc Warlock Capacitors Signal",
    evidence: "Published only by #92 when the Super starts, beside the capacitors_health_regen impulse of the Arc Warlock capacitors talent.",
};
const CATALYST_ORB_SIGNAL: EventKey = EventKey {
    hash: 0xCCF4_0AF1,
    name: "Catalyst Orb Signal",
    evidence: "Published only by Thorn Catalyst and Trinity Ghoul Catalyst, each time a kill makes them generate an Orb of Light.",
};
const BLESSING_OF_THE_SKY_SIGNAL: EventKey = EventKey {
    hash: 0x7595_58AC,
    name: "Blessing of the Sky Signal",
    evidence: "Published only by Blessing of the Sky (\"Using a Noble Round on an ally heals them and grants both you and them a weapon damage bonus\") each time its splash lands.",
};
const POWER_AMMO_REFILL_SIGNAL: EventKey = EventKey {
    hash: 0xF15B_2DEF,
    name: "Power Ammo Refill Signal",
    evidence: "Published only by the undescribed perk #71, which adds half the Power slot's reserves as it starts.",
};
const SWORD_HIT_SIGNAL: EventKey = EventKey {
    hash: 0xA23E_1885,
    name: "Sword Hit Signal",
    evidence: "Published only by the undescribed perk #746 when Sword damage lands, at most once a minute.",
};
const ARC_STAFF_BONUS_DAMAGE_SIGNAL: EventKey = EventKey {
    hash: 0x037E_57F5,
    name: "Arc Staff Bonus Damage Signal",
    evidence: "Heard only by the undescribed perk #18, which attaches Arc Staff Bonus Damage on it. No stock perk publishes it.",
};
const ALLY_REVIVED_EVENT: EventKey = EventKey {
    hash: 0x8A80_18BB,
    name: "Ally Revived",
    evidence: "Checked by Kindling the Flame (\"reviving a downed Guardian gives you a burst of healing\") and a mod reading \"Revive provides an overshield to you and nearby allies\".",
};
const ALLY_REVIVED_CONTEXT: EventKey = EventKey {
    hash: 0xC716_1F86,
    name: "Ally Revived",
    evidence: "The context the same revive perks check beside the revive event.",
};
const UNSTOPPABLE_STAGGER_EVENT: EventKey = EventKey {
    hash: 0x571D_3191,
    name: "Unstoppable Champion Staggered",
    evidence: "Checked by Counter Charge and by two mods reading \"Staggering Unstoppable Champions\" and \"staggers an Unstoppable Champion\".",
};
const OVERLOAD_DISRUPT_EVENT: EventKey = EventKey {
    hash: 0x5D19_601E,
    name: "Overload Champion Disrupted",
    evidence: "Checked only by Counter Charge (\"staggers or disrupts a Champion\") beside the stagger and barrier events, so it is the disruption.",
};
const PLAYER_CONTEXT: EventKey = EventKey {
    hash: 0x3FBE_3C2A,
    name: "Player",
    evidence: "The engine name player, the context the Unstoppable stagger mod checks beside its event.",
};
const HEAVY_GUARD: EventKey = EventKey {
    hash: 0xA8F9_A54E,
    name: "Heavy Guard",
    evidence: "Counted only by Heavy Guard (\"Sword Guard has high overall defenses\").",
};
const SWORDMASTERS_GUARD: EventKey = EventKey {
    hash: 0x530A_BDAB,
    name: "Swordmaster's Guard",
    evidence: "Counted only by Swordmaster's Guard (\"Sword Guard has low overall defenses, but increases charge rate\").",
};
const CHARGED_FIRE: EventKey = EventKey {
    hash: 0x9510_8C87,
    name: "Charged Fire",
    evidence: "Set only by Close the Gap (\"Charges to fire a high-powered laser\"), where Full Auto Fire is set by the full auto perks.",
};

// The frame keys Override a Pattern Key writes, the status a kill can require, and the name
// Vengeance remembers its target by.
const ADAPTIVE_FRAME_KEY: EventKey = EventKey {
    hash: 0x9420_6DC3,
    name: "Adaptive Frame",
    evidence: "The engine name adaptive, the pattern key Adaptive Frame writes.",
};
const HIGH_IMPACT_FRAME_KEY: EventKey = EventKey {
    hash: 0xA2A8_25E9,
    name: "High-Impact Frame",
    evidence: "Hashes from the engine words high impact, the pattern key a High-Impact Frame perk writes.",
};
const LIGHTWEIGHT_FRAME_KEY: EventKey = EventKey {
    hash: 0xCB61_3F5F,
    name: "Lightweight Frame",
    evidence: "The engine name lightweight, the pattern key Lightweight Frame writes.",
};
const PRECISION_FRAME_KEY: EventKey = EventKey {
    hash: 0x962E_A19B,
    name: "Precision Frame",
    evidence: "The engine name precision, the pattern key a Precision Frame perk writes.",
};
const RAPID_FIRE_FRAME_KEY: EventKey = EventKey {
    hash: 0x18F2_A437,
    name: "Rapid-Fire Frame",
    evidence: "Hashes from the engine words rapid fire, the pattern key a Rapid-Fire Frame perk writes.",
};
const OVERLOAD: EventKey = EventKey {
    hash: 0x6DC2_9533,
    name: "Overload",
    evidence: "The engine name overload, the key Overload Grenades and Overload Rounds attach their effects under and the key a disruption mod (\"Improves the effects of disruption\") requires of its kills.",
};
const VENGEANCE_TARGET: EventKey = EventKey {
    hash: 0x18C1_1019,
    name: "Vengeance Target",
    evidence: "Vengeance (\"Highlight and defeat those that harm you\") remembers its target under this name, ends when that target is far away or named by an event, and rewards the kill of it.",
};
// Found by hashing the engine's own words against the keys stock kills require, the way
// `overload` was. Four-word matches come up by chance, so this one is kept because it says
// what its only perk says: a Rasputin bauble is the engine's Warmind Cell.
const ENEMY_NEAR_WARMIND_CELL: EventKey = EventKey {
    hash: 0x9EC6_D5E9,
    name: "Enemy Near a Warmind Cell",
    evidence: "The engine name enemy_near_rasputin_bauble, in which a Rasputin bauble is a Warmind Cell. Light from Darkness (\"defeating multiple enemies near a Warmind Cell\") requires it of its kills.",
};
// Keys a filter-list entry of class 0x80804D75 holds, found the same way. Two-word matches
// almost never come up by chance over this vocabulary, and both say what their perk says.
const WEAKEN_TARGET: EventKey = EventKey {
    hash: 0xAC49_6004,
    name: "Weaken Target",
    evidence: "The engine name weaken_target. Oppressive Darkness (\"Causing damage with a Void grenade adds a weaken effect to combatants\") filters on it.",
};
const OVERLOAD_SUSTAIN_PLAYER: EventKey = EventKey {
    hash: 0x906E_3BE3,
    name: "Overload Sustain Player",
    evidence: "The engine name overload_sustain_player, overload being the engine's word for disruption. #1705 (\"Uninterrupted fire grants bullets that cause disruption\") filters on it.",
};
// Property keys written only by undescribed perks, found by hashing the engine's own words
// (package paths, catalog phrases and client names) against them. No item or plug in the
// investment tables grants these perks, so their names cannot come from a plug.
const SOLAR_GRENADES: EventKey = EventKey {
    hash: 0x063E_E118,
    name: "Solar Grenades",
    evidence: "The engine name enable_solar_grenades, set on the weapon while it is equipped by Excavation and #388. #389 and #390 set the Suppression and Void grenade flags the same way.",
};
const SUPPRESSION_GRENADES: EventKey = EventKey {
    hash: 0xC433_183E,
    name: "Suppression Grenades",
    evidence: "The engine name enable_suppression_grenades, set on the weapon while it is equipped by #389, between the Solar and Void grenade flags of #388 and #390.",
};
const VOID_GRENADES: EventKey = EventKey {
    hash: 0x6163_5FED,
    name: "Void Grenades",
    evidence: "The engine name enable_void_grenades, set on the weapon while it is equipped by #390.",
};
const VOID_SUPER_DAMAGE_BONUS: EventKey = EventKey {
    hash: 0x2A03_D3BD,
    name: "Void Super Damage Bonus",
    evidence: "The engine name void_super_damage_bonus, set on the Super by #1662.",
};
// Mamba is the engine's name for Gambit Prime: its strings, scoreboard and armor perk scripts
// all sit in mamba folders, and the perks beside these read "Equip multiple pieces from this set
// to unlock Gambit Prime set perks".
const GAMBIT_PRIME_GLOW: EventKey = EventKey {
    hash: 0xFDC8_E321,
    name: "Gambit Prime Glow",
    evidence: "The engine name mamba_glow, mamba being Gambit Prime. Set by #1264.",
};
const GAMBIT_PRIME_MOTE_CAPACITY: EventKey = EventKey {
    hash: 0x2E25_05DE,
    name: "Gambit Prime Mote Capacity",
    evidence: "The engine name mamba_mote_capacity, mamba being Gambit Prime. Set to 5 by #1244, beside the Gambit Prime armor set perks.",
};
const GAMBIT_PRIME_TAKEN_STACK: EventKey = EventKey {
    hash: 0xE93C_4F06,
    name: "Gambit Prime Taken Stack",
    evidence: "The engine name mamba_taken_stack, mamba being Gambit Prime. Raised by #1248, whose damage bonus requires the Taken label, and read by the Gambit Prime taken_buff_for_killing_mobs behavior.",
};
const EXTENDED_DAWNBLADE_STACKS: EventKey = EventKey {
    hash: 0x06A4_BB3E,
    name: "Extended Dawnblade Stacks",
    evidence: "Cleared by #1794 when the Super starts, beside the remove_all_stacks_of_extended_dawnblade script that also reads it, and checked at exactly 4 by #1795. Both sit in the orbs_grant_extended_dawnblade exotic with Embers of Light.",
};
const WEAKEN_MELEE_ACTIVE: EventKey = EventKey {
    hash: 0xC40D_49FE,
    name: "Weaken Melee Active",
    evidence: "The engine name weaken_melee_active. Checked by #109, which attaches its Weaken Target effect on physical melee hits while this holds.",
};
const STURM_AND_DRANG_STATE: EventKey = EventKey {
    hash: 0x66B2_06FE,
    name: "Sturm and Drang State",
    evidence: "Checked at 1 by #486, which sits between Storm and Stress and Together Forever and adds an overflowing round when its weapon kills while this holds. Which half of the pair it tracks is not established, and no engine name matched.",
};
const SIPHON_LIFE_ACTIVE: EventKey = EventKey {
    hash: 0xCC8F_C420,
    name: "Siphon Life Active",
    evidence: "The engine name siphon_life_active. Checked by #63, between #62 and #64, which start on the siphon melee label.",
};
// These three have no engine name any search matched, so each is named for what the
// structure shows.
const SENTINEL_SHIELD_FLAG: EventKey = EventKey {
    hash: 0xE668_EB4C,
    name: "Sentinel Shield Flag",
    evidence: "Set to 1 on the Super by #48, which has no trigger. The resources that read it hold Ursine Guard, the void shield label and the Unavailable Sentinel property beside it.",
};
const ARC_WARLOCK_CAPACITORS: EventKey = EventKey {
    hash: 0xCA53_214F,
    name: "Arc Warlock Capacitors",
    evidence: "Set to -1 on the Super by #92, whose other action applies capacitors_health_regen from the Arc Warlock capacitors talent.",
};
// Undescribed perks with no trigger or other action: named for what reads them or for the
// perks beside them.
const BARRIER_KILL_SWITCH: EventKey = EventKey {
    hash: 0x049A_C749,
    name: "Barrier Kill Switch",
    evidence: "Set to 1 by #39, an undescribed perk with no trigger and no other action. The player's variable table declares it, and the entities the undescribed perk #42 creates after a barrier kill read it.",
};
const NIGHTSTALKER_SWITCH: EventKey = EventKey {
    hash: 0xF85D_DFBD,
    name: "Nightstalker Switch",
    evidence: "Set to 1 by #103, an undescribed perk with no trigger and no other action. Its perk hash differs only in the last digit from those of #102 and #104, which act on void bow (Shadowshot) kills. The player's variable table declares it and about 50 entity resources read it.",
};
const THREE_TIER_LEVEL: EventKey = EventKey {
    hash: 0x9302_7C9B,
    name: "Three-Tier Level",
    evidence: "Set to 1, 2 and 3 by the undescribed perks #130, #131 and #132, one level each. The player's variable table declares it.",
};
const VOID_BLOOM_CATALYST: EventKey = EventKey {
    hash: 0x5A21_565F,
    name: "Void Bloom Catalyst",
    evidence: "The engine name void_bloom_catalyst, beside bloom_catalyst_active and bloom_catalyst_on_cooldown. One undescribed stock perk requires it of its kills and stores it as its weapon key.",
};

// These stock keys identify effect families, but Create Entity only tests whether its
// cleanup key is empty. Choosing a named key does not grant that gameplay behavior.
const OVERSHIELD: EventKey = EventKey {
    hash: 0x0D3C_3FB0,
    name: "Overshield",
    evidence: "The engine name overshield. Written by Voltaic Mote Collector and its Enhanced form (\"gain an overshield\") and Sheltering Energy (\"grants you an overshield\").",
};
const HEALTH_REGEN: EventKey = EventKey {
    hash: 0xBEB4_6779,
    name: "Health Regeneration",
    evidence: "The engine name health_regen, written by one undescribed stock perk.",
};
const INVISIBILITY: EventKey = EventKey {
    hash: 0x02B4_C507,
    name: "Invisibility",
    evidence: "Written by Vermin (\"a brief period of invisibility\") and Vanishing Execution (\"grant invisibility\"), and signalled on by Vanishing Shadow and Roving Assassin (\"vanish\").",
};
const RETURN_ROUNDS: EventKey = EventKey {
    hash: 0x2A2B_78D3,
    name: "Return Rounds to the Magazine",
    evidence: "Written by Triple Tap (\"return 1 round to the magazine\") and Fourth Time's the Charm (\"return two rounds\").",
};
const DAMAGE_REDUCTION: EventKey = EventKey {
    hash: 0x88DD_D26B,
    name: "Damage Reduction",
    evidence: "Written by the Fallen Barrier and Hive Barrier mods, both reading \"a 20% reduction in damage for 10 seconds\".",
};
const FIREFLY: EventKey = EventKey {
    hash: 0xDF54_42D8,
    name: "Firefly Explosion",
    evidence: "Written by Firefly (\"cause the target to explode\") and the Ace of Spades Catalyst (\"Firefly deals more damage\").",
};
const SWORD_GUARD: EventKey = EventKey {
    hash: 0xB0D2_C151,
    name: "Sword Guard",
    evidence: "Counted by Burst Guard, Enduring Guard and Heavy Guard, every one a Sword Guard perk.",
};

const STATE_KEYS: &[EventKey] = &[
    CHARGED_WITH_LIGHT_STATE,
    WEAPON_EQUIPPED_STATE,
    AIMING_DOWN_SIGHTS,
    IRON_SIGHTS,
    WEAPON_FIRING,
    WEAPON_HOLSTERED,
    CHARGE_FRACTION,
    ROUNDS_LOADED,
    PERK_COUNTER,
    PERK_ACTIVE,
    SUPER_ACTIVE,
    SUBCLASS_ARC,
    SUBCLASS_VOID,
    NEARBY_ENEMIES_STATE,
    MELEE_ENERGY_STATE,
    MELEE_OVERCHARGE_STATE,
    SUBCLASS_SOLAR,
    EXOTIC_ARMOR_PERK_ACTIVE,
    GUARDING,
    GUARDING_WITH_SWORD,
    EXOTIC_PERK_COUNTER,
    SERAPH_WEAPON_EQUIPPED,
    SOLAR_SPLASH_CELLS,
    CELL_SPAWN_CHANCE,
    MAGAZINE_FRACTION,
    SUNSPOTS_GRANT_SUN_WARRIOR,
    RELOADING,
    OVERCHARGE_STATE,
    MELEE_OVERCHARGE,
    RECENT_BLOCKED_DAMAGE,
    CURRENT_BLOCKED_DAMAGE,
    BONUS_DAMAGE_STACKS,
    TARGETING_FRACTION,
    OUTLAW_ACTIVE,
    CHARGING_GRENADE,
    SHOULDER_CHARGE_ENABLED,
    SHOULDER_CHARGE_READY,
    PERK_ACTIVE_COUNTER,
    SLIDING,
    SPIN_FRACTION,
    MIDA_MULTI_TOOL_EQUIPPED,
    RAT_KING_COUNTER,
    DESPERADO_CHECK,
    PERK_COUNTER_COMBO,
    DRAWING_BOW,
    SPLIT_ARROW_PRECISION,
    OVERFLOW_ROUNDS,
    FULLY_CHARGED_FRACTION,
    EXOTIC_PERFECT_GUARD,
    BOW_DRAW_STATE,
    CHARGE_FIRE_CHECK,
    // Named at other sites and also checked here: the Seraph weapon perks require the
    // Warmind Cell counter at 10 or more, Split Electron checks Lightning Rod's chain
    // lightning, and Unforeseen Repercussions ("causing delayed explosions") checks
    // Explosive Rounds.
    WARMIND_CELL_CHANCE,
    LIGHTNING_ROD,
    EXPLOSIVE_ROUNDS,
    ROVING_ASSASSIN_ACTIVE,
    LINEAR_ACTUATORS_ACTIVE,
    FULLY_SPUN_UP,
    URSINE_GUARD_VALUE,
    MULTIKILL_CLIP_ACTIVE,
    LAST_STAND_VALUE,
    RAPID_FIRE_FRAME_VALUE,
    UNINTERRUPTED_FIRE,
    MAGAZINE_HIT_FRACTION,
    BACKUP_PLAN_ACTIVE,
    SWORD_GUARD_RAISED,
    FUSION_HARNESS_VALUE,
    CROSS_COUNTER_ACTIVE,
    STRANGE_PROTRACTOR_VALUE,
    EN_GARDE_VALUE,
    MASTER_OF_ARMS_VALUE,
    UNCANNY_ARROWS_VALUE,
    ARCHERS_GAMBIT_ACTIVE,
    TESSERACT_ACTIVE,
    FIREFLY_ACTIVE,
    INSIDE_WELL,
    COLOSSUS_ACTIVE,
    EXTENDED_DAWNBLADE_STACKS,
    SIPHON_LIFE_ACTIVE,
    WEAKEN_MELEE_ACTIVE,
    STURM_AND_DRANG_STATE,
];

/// State keys whose value is a count or a level rather than a switch, so the range a
/// predicate gives them is the requirement itself. Every other state reads 1 while it holds.
pub const NUMERIC: &[u32] = &[
    0x59E3_47ED,
    0x32CC_5402,
    0x748A_92CC,
    0xA43A_8C2E,
    0xD9C4_6BC4,
    0x58A9_CB99,
    0x8C79_67C7,
    0x6F47_FA3A,
    0x9D16_778B,
    0xB636_2D8F,
    0x2A98_BCB3,
    0x1950_23EF,
    0x7227_FCBB,
    0xBF74_EA1A,
    0x7AA3_47DD,
    0x7366_B825,
    0x3B29_169F,
    0x69E6_D035,
    0x8C2B_3422,
    0xCDFE_45FF,
    0x39E5_FF44,
    0x7838_029C,
    0x290C_7A47,
    0x18EA_C8E2,
    0x0F37_AE29,
    0x0786_9B5C,
    0x5885_28CA,
    0x65E1_D735,
    0x6D26_7215,
    0x847B_6253,
    0x8136_C08E,
    0x06A4_BB3E,
];

/// The ability patterns the stock On a Specific Ability and Ends on a Specific Ability
/// conditions store at +0x10. Each is a hop-on entity under content/sandbox/abilities/player,
/// named by the ability the perk that starts or ends on it describes.
const SOLAR_GRENADE: EventKey = EventKey {
    hash: 0x80B8_0888,
    name: "Solar Grenade",
    evidence: "The thermal_flare grenade pattern. Helium Spirals, which reads \"Increases the duration of Solar Grenades\", ends on it.",
};
const SKIP_GRENADE: EventKey = EventKey {
    hash: 0x80B8_071E,
    name: "Skip Grenade",
    evidence: "The field_grenade pattern. New Tricks, which reads \"Improves Skip Grenade\", ends on it.",
};
const FUSION_GRENADE: EventKey = EventKey {
    hash: 0x80B8_0B2C,
    name: "Fusion Grenade",
    evidence: "The thermal_flux grenade pattern. Bring the Heat and Fusion Harness, which both read \"Fusion Grenades\", start and end on it.",
};
const BURNING_MAUL: EventKey = EventKey {
    hash: 0x80BC_8428,
    name: "Burning Maul",
    evidence: "The thermal_maul Super pattern. Sunfire Furnace, which reads \"while your Super is charged\", ends on it.",
};
const HAMMER_OF_SOL: EventKey = EventKey {
    hash: 0x80BA_A847,
    name: "Hammer of Sol",
    evidence: "The thermal_hammer Super pattern. Sunfire Furnace ends on it beside Burning Maul, the Sunbreaker's other Super.",
};
const GOLDEN_GUN: EventKey = EventKey {
    hash: 0x80BA_A30C,
    name: "Golden Gun",
    evidence: "The golden_gun_ability Super pattern. Scissor Fingers, which grants two knives per charge, ends on it.",
};
const BLADE_BARRAGE: EventKey = EventKey {
    hash: 0x80BC_83A5,
    name: "Blade Barrage",
    evidence: "The thermal_knives Super pattern. Scissor Fingers ends on it beside Golden Gun, the Gunslinger's other Super.",
};
const ABILITY_PATTERNS: &[EventKey] = &[
    SOLAR_GRENADE,
    SKIP_GRENADE,
    FUSION_GRENADE,
    BURNING_MAUL,
    HAMMER_OF_SOL,
    GOLDEN_GUN,
    BLADE_BARRAGE,
];

/// The named keys offered at one native key field, by class and offset.
pub const SITES: &[(u32, usize, &[EventKey])] = &[
    (0x8080_3DFF, 0x10, ABILITY_PATTERNS),
    (0x8080_3DFE, 0x10, ABILITY_PATTERNS),
    (0x8080_3DCE, 0xD4, STATE_KEYS),
    (0x8080_3DCC, 0xD4, STATE_KEYS),
    (0x8080_3E45, 0x18, lifetimes::ALL),
    (
        0x8080_3E39,
        0x4,
        &[SWORD_GUARD, TESSERACT, HEAVY_GUARD, SWORDMASTERS_GUARD],
    ),
    (
        0x8080_29ED,
        0x8,
        &[
            PRIMARY_FIND,
            SPECIAL_FIND,
            HEAVY_FIND,
            FINISHER_COST,
            CHARGED_STACKS,
            EXPLOSIVE_ROUNDS,
            SHIELD_PIERCING,
            ARMOR_PIERCING,
            LIGHTNING_ROD,
            PERSONAL_ASSISTANT,
            STICKY_GRENADES,
            WARMIND_CELL_CHANCE,
            PERK_ACTIVE,
            RICOCHET_ROUNDS,
            MAXIMIZED_GUARD_EFFICIENCY,
            AEON_ENERGY,
            UNSTOPPABLE_PAYLOAD,
            OVERLOAD_ROUNDS,
            SUPERCHARGED_ARC_SOUL,
            FASTER_SUMMONING,
            RELOAD_WHILE_RIDING,
            HIGH_CALIBER_ROUNDS,
            TESSERACT,
            AGGRESSIVE_FRAME,
            HAWKEYE_HACK,
            SURVIVAL_WELL,
            STANDARD_DRIVE,
            TUNED_DRIVE,
            CUSTOM_DRIVE,
            IMPROVED_ASSEMBLER,
            AIRBORNE_AGILITY,
            VERNIER_THRUSTERS,
            CLUSTER_BOMB,
            TIMED_PAYLOAD,
            BLINDING_GRENADES,
            THERMOPLASTIC_GRENADES,
            CONCUSSION_GRENADES,
            PROXIMITY_GRENADES,
            SPIKE_GRENADES,
            INFINITE_GUARD,
            EXCAVATION,
            ACCOMPLICE,
            SUPERCONDUCTOR,
            BLACK_HOLE,
            MIDA_MULTI_TOOL,
            RAT_PACK,
            VANISHING_SHADOW,
            URSINE_GUARD,
            OVERFLOWING_LIGHT,
            DESTABILIZERS,
            UNFORESEEN_REPERCUSSIONS,
            SUPERCHARGED_BATTERY,
            SPLIT_ELECTRON,
            BROADHEAD,
            QUEENS_WRATH,
            MAGNIFICENT_HOWL,
            REIGN_HAVOC,
            POISON_ARROWS,
            RAMPAGE_SPEC,
            HAPPY_DAWNING,
            DAWNING_DARE,
            FULL_COURT,
            GRENADES_AND_HORSESHOES,
            EXPLOSIVE_LIGHT,
            UNSTOPPABLE_BURST,
            REVOLUTION,
            BLACK_TALON_CATALYST,
            BASTION_CATALYST,
            WAVE_FRAME,
            ROVING_ASSASSIN_ACTIVE,
            LINEAR_ACTUATORS_ACTIVE,
            FULLY_SPUN_UP,
            URSINE_GUARD_VALUE,
            EXOTIC_ARMOR_PERK_ACTIVE,
            SUNSPOTS_GRANT_SUN_WARRIOR,
            RAT_KING_COUNTER,
            SPLIT_ARROW_PRECISION,
            EXOTIC_PERK_COUNTER,
            SOLAR_GRENADES,
            SUPPRESSION_GRENADES,
            VOID_GRENADES,
            VOID_SUPER_DAMAGE_BONUS,
            GAMBIT_PRIME_GLOW,
            GAMBIT_PRIME_MOTE_CAPACITY,
            GAMBIT_PRIME_TAKEN_STACK,
            EXTENDED_DAWNBLADE_STACKS,
            SENTINEL_SHIELD_FLAG,
            ARC_WARLOCK_CAPACITORS,
            BARRIER_KILL_SWITCH,
            NIGHTSTALKER_SWITCH,
            THREE_TIER_LEVEL,
        ],
    ),
    (0x8080_3E1D, 0x4, abilities::ALL),
    (
        0x8080_3DEA,
        0x8,
        &[
            ORB_EVENT,
            BARRIER_EVENT,
            RESURRECTION,
            ALLY_REVIVED_EVENT,
            UNSTOPPABLE_STAGGER_EVENT,
            OVERLOAD_DISRUPT_EVENT,
        ],
    ),
    (
        0x8080_3DEA,
        0xC,
        &[
            ORB_CONTEXT,
            BARRIER_CONTEXT,
            ALLY_REVIVED_CONTEXT,
            PLAYER_CONTEXT,
        ],
    ),
    (0x8080_3DEC, 0x8, key_match::ALL),
    (0x8080_3DEB, 0x8, key_match::ALL),
    (0x8080_3E1C, 0x4, &[FULL_AUTO, CHARGED_FIRE]),
    (
        0x8080_3E13,
        0x4,
        &[
            ADAPTIVE_FRAME_KEY,
            HIGH_IMPACT_FRAME_KEY,
            LIGHTWEIGHT_FRAME_KEY,
            PRECISION_FRAME_KEY,
            RAPID_FIRE_FRAME_KEY,
        ],
    ),
    (
        0x8080_3DE7,
        0x144,
        &[OVERLOAD, ENEMY_NEAR_WARMIND_CELL, VOID_BLOOM_CATALYST],
    ),
    (0x8080_4D75, 0x0, &[WEAKEN_TARGET, OVERLOAD_SUSTAIN_PLAYER]),
    (0x8080_3E2E, 0x4, transmat::ALL),
    (
        0x8080_3E1E,
        0x4,
        &[
            FOCUSED_FIRE_TRIGGERED,
            SHADOWSHOT_TETHER,
            ARC_WARLOCK_CAPACITORS_SIGNAL,
            CATALYST_ORB_SIGNAL,
            BLESSING_OF_THE_SKY_SIGNAL,
            POWER_AMMO_REFILL_SIGNAL,
            SWORD_HIT_SIGNAL,
        ],
    ),
    (0x8080_2D00, 0x8, &[ARC_STAFF_BONUS_DAMAGE_SIGNAL]),
    (0x8080_3DE7, 0x148, &[VENGEANCE_TARGET]),
    (0x8080_2D0D, 0x4, &[VENGEANCE_TARGET]),
    (0x8080_2D02, 0x8, &[VENGEANCE_TARGET]),
    (0x8080_2D01, 0x8, &[VENGEANCE_TARGET]),
];

/// The named keys stock perks use at this key field. Empty where none is established.
#[must_use]
pub fn known(class: u32, offset: usize) -> &'static [EventKey] {
    SITES
        .iter()
        .find(|(candidate, at, _)| *candidate == class && *at == offset)
        .map_or(&[], |(_, _, keys)| *keys)
}

/// Metadata for a mapped key, including keys unavailable in the current choice list.
#[must_use]
pub fn entry(hash: u32) -> Option<&'static EventKey> {
    SITES
        .iter()
        .flat_map(|(_, _, keys)| keys.iter())
        .find(|key| key.hash == hash)
}

/// The name of a key, when a stock perk establishes it at any site.
#[must_use]
pub fn name(hash: u32) -> Option<&'static str> {
    entry(hash)
        .map(|key| key.name)
        .or_else(|| client_name(hash))
}

/// The client's own identifier for a key, recovered 2026-09-17 by scanning the running
/// offline client's memory for identifier strings and matching their FNV-1 hashes against
/// the 851 key hashes the captured stock actions store. The on-disk binary is VMProtect
/// packed, so these strings exist only once the loader has unpacked them.
///
/// This is a different kind of evidence from `SITES` above. There the name is inferred from
/// what the perks using a key say they do. Here it is the identifier the engine itself uses,
/// so it is recorded verbatim in the engine's own snake case rather than reworded.
///
/// Four of these independently confirm names already recovered by client tracing and held in
/// `native::predicate::VARIABLES`: `nearby_enemy_count`, `perfect_guard`,
/// `sword_guard_recent_damage` and `melee_overcharge_state`.
///
/// At this corpus size, 963 target hashes against 22 million candidate strings, chance alone
/// predicts about five spurious 32-bit matches. One match was an obvious artefact, a 64 digit
/// hexadecimal blob, and is excluded. The short generic entries here carry the most risk of
/// being coincidence; the long compound identifiers essentially cannot be.
pub(crate) const CLIENT_NAMES: &[(u32, &str)] = &[
    // Recovered as a prefix or suffix of a longer run, because a name can sit directly
    // against other identifier bytes with no separator. The same pass independently returned
    // `kill_tag_gathered`, `pickup_flash` and `near_bank`, three names already established
    // above by client tracing, which is the control for this way of matching.
    (0x1124_697D, "all_players"),
    (0x15FB_2FCB, "mod3_stack"),
    (0x420E_2AC6, "m_filters"),
    (0x4BD6_4DBE, "label_filter"),
    (0x66C9_22E0, "disorient"),
    (0x7180_1682, "siphon_gun_overload"),
    (0xB3C8_4AFA, "m_target"),
    (0xD1CC_38CC, "melee_overcharge"),
    (0xD27B_4E9D, "is_guarding"),
    (0xE7A6_D9A5, "overcharge_state"),
    (0x0427_C343, "extended_charged_with_light"),
    (0x0D3C_3FB0, "overshield"),
    (0x16EF_9AA5, "renown_stack_count"),
    (0x2814_F006, "melee_energy"),
    (0x2837_E2C7, "siphon_gun_overload_no_cage"),
    (0x2A98_BCB3, "bonus_damage_stacks_remaining"),
    (0x32CC_5402, "rounds_loaded"),
    (0x42DE_FF49, "is_thermal"),
    (0x50C1_395D, "warmind_cells_increase_bauble_chance"),
    (0x58A9_CB99, "nearby_enemy_count"),
    (0x59E3_47ED, "charged_with_light_stacks"),
    (0x5AF2_4662, "charged_with_light_signal"),
    (0x5E85_33DB, "class"),
    (0x62DA_4AC3, "super_active_recent"),
    (0x6A1C_27EE, "equipped"),
    (0x6F47_FA3A, "equipped_item_magazine_fraction"),
    (0x748A_92CC, "perk_counter"),
    (0x7838_029C, "rasputin_bauble_drop_counter"),
    (0x7C74_8858, "sword_guard_melee_check_current"),
    (0x7E96_FCDD, "combat_role_overload"),
    (0x834B_CE26, "is_arc_recent"),
    (0x8A54_31AA, "perfect_guard"),
    (0x8C79_67C7, "exotic_perk_counter"),
    (0x9202_856A, "is_guarding_with_sword"),
    (0x9420_6DC3, "adaptive"),
    (0x9544_F264, "ammo_pickup_sorter"),
    (0x9D16_778B, "recent_blocked_damage"),
    (0xA03D_DB0D, "solar_splash_spawn_baubles"),
    (0xA43A_8C2E, "perk_active"),
    (0xA957_F024, "enable_lingering_fire_apply_buff"),
    (0xADB7_5801, "support_nearby_enemy"),
    (0xB331_1944, "resurrection"),
    (0xB636_2D8F, "current_blocked_damage"),
    (0xB762_7DF5, "nearby_ally_count"),
    (0xBB1F_E6D4, "sword_guard_recent_damage"),
    (0xBEB4_6779, "health_regen"),
    (0xC776_B6F8, "is_void_recent"),
    (0xCB61_3F5F, "lightweight"),
    (0xCC85_1A8D, "is_thermal_recent"),
    (0xCF40_9257, "combat_role_pierce"),
    (0xD16E_1FA3, "super_active"),
    (0xD7D9_DA64, "bloom_catalyst_on_cooldown"),
    (0xD9E7_346F, "exotic_armor_perk_active"),
    (0xDDB0_F48B, "shield_vitality"),
    (0xDE1B_9C59, "bloom_catalyst_active"),
    (0xDE1D_8C04, "rasputin_weapon_equipped"),
    (0xDE95_B972, "chaperone"),
    (0xDFAD_6B26, "melee_overcharge_state"),
    (0xE1E6_BB64, "is_arc"),
    (0xE93C_4F06, "mamba_taken_stack"),
    (0xECDE_609E, "reloading"),
    (0xEE5F_6482, "is_void"),
    (0xF799_4154, "combat_role_stagger"),
    (0xF2E2_CEF4, "thunderlord"),
];

/// The engine's own identifier for a key, when the client scan recovered one.
#[must_use]
pub fn client_name(hash: u32) -> Option<&'static str> {
    CLIENT_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == hash)
        .map(|(_, name)| *name)
}
