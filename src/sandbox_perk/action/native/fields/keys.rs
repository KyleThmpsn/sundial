//! Event and mode keys named by the stock perks that use them.
//!
//! A 32-bit key carries no string. What the game does with it is recoverable: when every
//! stock perk that listens to a key reads "each time you pick up an Orb of Light", the key
//! is the Orb of Light pickup. A key is named only where the descriptions agree, and each
//! entry keeps the perks that establish it so a reader can check the claim.

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
// Ability Property (kind 7) keys, in the ability bank the slot byte selects.
const GRENADE_CHARGES: EventKey = EventKey {
    hash: 0xBDA0_ACD6,
    name: "Grenade Charges",
    evidence: "Written by And Another Thing (\"an additional grenade charge\"), Fusion Harness (\"extra Fusion Grenade\") and New Tricks.",
};

// The general predicate's key at +D4 is the engine state it checks, in the same namespace
// as the compiled comparison variables: is_arc, is_void, super_active, iron_sights,
// weapon_firing, charged_with_light_stacks, melee_energy and nearby_enemy_count all hash to
// stock values there with the predicate's own FNV-1 fold. The rest are named the way the
// event keys are, from the perks that check them.
const CHARGED_WITH_LIGHT_STATE: EventKey = EventKey {
    hash: 0x59E3_47ED,
    name: "Charged with Light",
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
    name: "Iron Sights (Hip Fire)",
    evidence: "The engine name iron_sights. Checked by Fan Fire (\"more effective in hip-fire\") and Archer's Gambit (\"hipfire precision hits\").",
};
const WEAPON_FIRING: EventKey = EventKey {
    hash: 0x59E4_FF8D,
    name: "Weapon Firing",
    evidence: "The engine name weapon_firing. Checked by Tap the Trigger (\"initial trigger pull\"), Opening Shot, Reservoir Burst, and inverted by Box Breathing (\"aiming without firing\").",
};
const NEARBY_ENEMIES_STATE: EventKey = EventKey {
    hash: 0x58A9_CB99,
    name: "Nearby Enemies (Surrounded)",
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
    name: "Aiming Down Sights",
    evidence: "Checked by Unstoppable Hand Cannon, Unstoppable Burst and their variant, all reading \"aiming down sights loads\". No engine name resolved.",
};
const WEAPON_HOLSTERED: EventKey = EventKey {
    hash: 0xD5B8_A34B,
    name: "Weapon Holstered",
    evidence: "Checked by Auto-Loading Holster, Serve the Colony, the Eriana's Vow Catalyst and the Witherhoard Catalyst, all reading \"holstered\" or \"unequipped\". No engine name resolved.",
};
const FULLY_DRAWN: EventKey = EventKey {
    hash: 0x7AA3_47DD,
    name: "Fully Drawn or Charged",
    evidence: "Checked by Overload Arrowheads and the Unstoppable bow perk (\"fully drawn arrows\") and Pyrogenesis (\"fully charging the laser\"), with Close the Gap and Archer's Gambit. No engine name resolved.",
};
const ALTERNATE_FIRE: EventKey = EventKey {
    hash: 0x748A_92CC,
    name: "Alternate Fire Mode",
    evidence: "Checked by Release the Wolves and the Cerberus+1 Catalyst, both reading \"swap to\" another mode, with Ravenous Beast and Noble Rounds, whose alternate modes are undescribed. No engine name resolved.",
};
const MAGAZINE_EMPTY: EventKey = EventKey {
    hash: 0x32CC_5402,
    name: "Magazine Empty",
    evidence: "Checked by Quick Access Sling (\"after emptying the magazine\") and Rapid-Fire Frame (\"fast reload when empty\"), with Swap Mag, Storm and Stress and Play with Your Prey. No engine name resolved.",
};
const WEAPON_ALTERNATE_STATE: EventKey = EventKey {
    hash: 0xA43A_8C2E,
    name: "Weapon Alternate State",
    evidence: "The key The Fundamentals reads to pick its element, checked here by The Fundamentals, Arc Conductor and Revolution.",
};

const WEAPON_EQUIPPED_STATE: EventKey = EventKey {
    hash: 0x6A1C_27EE,
    name: "Weapon Equipped",
    evidence: "The engine name equipped. Checked by Killing Tally (\"until it is stowed or reloaded\") and En Garde (\"immediately after swapping to this sword\").",
};
// Create Entity's first key names the entity's effect. The engine names resolve, and the
// perks that write them agree.
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
    FULLY_DRAWN,
    MAGAZINE_EMPTY,
    ALTERNATE_FIRE,
    WEAPON_ALTERNATE_STATE,
    SUPER_ACTIVE,
    SUBCLASS_ARC,
    SUBCLASS_VOID,
    NEARBY_ENEMIES_STATE,
    MELEE_ENERGY_STATE,
    MELEE_OVERCHARGE_STATE,
];

/// The named keys offered at one native key field, by class and offset.
pub const SITES: &[(u32, usize, &[EventKey])] = &[
    (0x8080_3DCE, 0xD4, STATE_KEYS),
    (0x8080_3DCC, 0xD4, STATE_KEYS),
    (
        0x8080_3E45,
        0x18,
        &[
            OVERSHIELD,
            INVISIBILITY,
            RETURN_ROUNDS,
            DAMAGE_REDUCTION,
            FIREFLY,
            HEALTH_REGEN,
        ],
    ),
    (0x8080_3E39, 0x4, &[SWORD_GUARD]),
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
            WEAPON_ALTERNATE_STATE,
        ],
    ),
    (0x8080_3E1D, 0x4, &[GRENADE_CHARGES]),
    (0x8080_3DEA, 0x8, &[ORB_EVENT, BARRIER_EVENT, RESURRECTION]),
    (0x8080_3DEA, 0xC, &[ORB_CONTEXT, BARRIER_CONTEXT]),
    (
        0x8080_3DEC,
        0x8,
        &[
            WARMIND_CELL,
            CHARGED_WITH_LIGHT,
            VEX_RELAY,
            TETHER_CHAIN,
            MOTES,
            NEAR_BANK,
            PICKUP_FLASH,
            INVISIBILITY,
        ],
    ),
    (
        0x8080_3DEB,
        0x8,
        &[
            WARMIND_CELL,
            CHARGED_WITH_LIGHT,
            VEX_RELAY,
            TETHER_CHAIN,
            MOTES,
            NEAR_BANK,
            PICKUP_FLASH,
            INVISIBILITY,
        ],
    ),
    (0x8080_3E1C, 0x4, &[FULL_AUTO]),
];

/// The named keys stock perks use at this key field. Empty where none is established.
#[must_use]
pub fn known(class: u32, offset: usize) -> &'static [EventKey] {
    SITES
        .iter()
        .find(|(candidate, at, _)| *candidate == class && *at == offset)
        .map_or(&[], |(_, _, keys)| *keys)
}

/// The name of a key, when a stock perk establishes it at any site.
#[must_use]
pub fn name(hash: u32) -> Option<&'static str> {
    SITES
        .iter()
        .flat_map(|(_, _, keys)| keys.iter())
        .find(|key| key.hash == hash)
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
