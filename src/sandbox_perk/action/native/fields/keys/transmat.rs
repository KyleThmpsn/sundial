//! Transmat effect keys, each named by the perk that sets it.
//!
//! Every stock Set Transmat Effect node belongs to a Transmat Effect item, one key per
//! effect, and the perk's description says what the effect adds. Two keys whose perks read
//! the same are listed under the same name. Which client code consumes the key is not
//! traced, so these name the effect a key stands for, not how the game applies it.
use super::EventKey;

pub(super) const ALL: &[EventKey] = &[
    EventKey {
        hash: 0x4AAC_A2AE,
        name: "Pink Rabbit Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a pink rabbit crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x7FA6_4FE4,
        name: "Purple Rabbit Crest",
        evidence: "Set only by the two transmat effect perks reading \"Adds a purple rabbit crest to your transmat effects\" and \"Adds the Jade Rabbit crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x64CC_D0B6,
        name: "Blue Crucible Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a blue Crucible crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x1432_FBD7,
        name: "Green Crucible Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a green Crucible crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x37E5_9BBB,
        name: "Pink Crucible Crest",
        evidence: "Set only by the two transmat effect perks reading \"Adds a pink Crucible crest to your transmat effects\" and \"Adds a Crucible crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x92D6_E3C5,
        name: "White Crucible Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a white Crucible crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x4FC9_30D6,
        name: "Gold Crucible Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a gold Crucible crest to your transmat effects\".",
    },
    EventKey {
        hash: 0xEF1A_7268,
        name: "Blue Guardian Crest",
        evidence: "Set only by the two transmat effect perks reading \"Adds a blue Guardian crest to your transmat effects\" and \"Adds a Guardian crest to your transmat effects\".",
    },
    EventKey {
        hash: 0xB8C1_54E5,
        name: "Green Guardian Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a green Guardian crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x1512_06F1,
        name: "Pink Guardian Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a pink Guardian crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x8CF3_069B,
        name: "White Guardian Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a white Guardian crest to your transmat effects\".",
    },
    EventKey {
        hash: 0x073A_D0DC,
        name: "Gold Guardian Crest",
        evidence: "Set only by the transmat effect perk reading \"Adds a gold Guardian crest to your transmat effects\".",
    },
    EventKey {
        hash: 0xCA07_6D95,
        name: "Arc Elements",
        evidence: "Set only by the transmat effect perk reading \"Adds Arc elements to your transmat effects\".",
    },
    EventKey {
        hash: 0x7BC0_EB76,
        name: "Solar Elements",
        evidence: "Set only by the transmat effect perk reading \"Adds Solar elements to your transmat effects\".",
    },
    EventKey {
        hash: 0xB462_D295,
        name: "Void Elements",
        evidence: "Set only by the transmat effect perk reading \"Adds Void elements to your transmat effects\".",
    },
    EventKey {
        hash: 0xA50C_1316,
        name: "Blue Ghost Ring",
        evidence: "Set only by the transmat effect perk reading \"Adds a blue Ghost ring to your transmat effects\".",
    },
    EventKey {
        hash: 0x7E7D_56B7,
        name: "Green Ghost Ring",
        evidence: "Set only by the two transmat effect perks reading \"Adds a green Ghost ring to your transmat effects\" and \"Adds a Ghost ring to your transmat effects\".",
    },
    EventKey {
        hash: 0xA567_059B,
        name: "Pink Ghost Ring",
        evidence: "Set only by the transmat effect perk reading \"Adds a pink Ghost ring to your transmat effects\".",
    },
    EventKey {
        hash: 0x18E6_C9A5,
        name: "White Ghost Ring",
        evidence: "Set only by the transmat effect perk reading \"Adds a white Ghost ring to your transmat effects\".",
    },
    EventKey {
        hash: 0x5697_2E36,
        name: "Gold Ghost Ring",
        evidence: "Set only by the transmat effect perk reading \"Adds a gold Ghost ring to your transmat effects\".",
    },
    EventKey {
        hash: 0xCCD2_284B,
        name: "Veteran's Flair",
        evidence: "Set only by Veteran's Flair, a transmat effect perk that carries no description.",
    },
    EventKey {
        hash: 0x7803_707B,
        name: "Cabal Drop Pod Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Cabal drop pod appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xCC15_60B6,
        name: "Fallen Servitor Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Fallen Servitor appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xAC84_79C6,
        name: "Hive Spawn Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Hive spawn appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x868F_BBFF,
        name: "Taken Rift Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Taken rift appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x054A_052F,
        name: "Vex Storm Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Vex storm appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xD0DA_ACFE,
        name: "Silver Beam",
        evidence: "Set only by the transmat effect perk reading \"Adds a silver beam to your transmat effects\".",
    },
    EventKey {
        hash: 0xFEEE_C64D,
        name: "Gold Beam",
        evidence: "Set only by the transmat effect perk reading \"Adds a gold beam to your transmat effects\".",
    },
    EventKey {
        hash: 0xC72B_8CF8,
        name: "Green Beam",
        evidence: "Set only by the transmat effect perk reading \"Adds a green beam to your transmat effects\".",
    },
    EventKey {
        hash: 0x3B21_9C29,
        name: "Purple Beam",
        evidence: "Set only by the two transmat effect perks reading \"Adds a purple beam to your transmat effects\" and \"Adds beam elements to your transmat effects\".",
    },
    EventKey {
        hash: 0xC346_0489,
        name: "Silver Spotlight",
        evidence: "Set only by the transmat effect perk reading \"Adds a silver spotlight to your transmat effects\".",
    },
    EventKey {
        hash: 0xB0AE_D202,
        name: "Yellow Spotlight",
        evidence: "Set only by the two transmat effect perks reading \"Adds a yellow spotlight to your transmat effects\" and \"Adds spotlights to your transmat effects\".",
    },
    EventKey {
        hash: 0xEFD9_5FD3,
        name: "Green Spotlight",
        evidence: "Set only by the transmat effect perk reading \"Adds a green spotlight to your transmat effects\".",
    },
    EventKey {
        hash: 0x964D_1FB4,
        name: "Purple Spotlight",
        evidence: "Set only by the transmat effect perk reading \"Adds a purple spotlight to your transmat effects\".",
    },
    EventKey {
        hash: 0x7D22_4499,
        name: "Pink Class Sigil",
        evidence: "Set only by the transmat effect perk reading \"Adds a pink class sigil to your transmat effects\".",
    },
    EventKey {
        hash: 0x2922_1935,
        name: "Yellow Class Sigil",
        evidence: "Set only by the transmat effect perk reading \"Adds a yellow class sigil to your transmat effects\".",
    },
    EventKey {
        hash: 0x8B4C_E0BE,
        name: "Green Class Sigil",
        evidence: "Set only by the two transmat effect perks reading \"Adds a green class sigil to your transmat effects\" and \"Adds a class sigil to your transmat effects\".",
    },
    EventKey {
        hash: 0xB885_AC33,
        name: "Purple Class Sigil",
        evidence: "Set only by the transmat effect perk reading \"Adds a purple class sigil to your transmat effects\".",
    },
    EventKey {
        hash: 0xFA4E_EC6A,
        name: "Osiris Theme",
        evidence: "Set only by the transmat effect perk reading \"Adds an Osiris theme to your transmat effects\".",
    },
    EventKey {
        hash: 0x15D6_FD82,
        name: "Stolen Light",
        evidence: "Set only by the transmat effect perk reading \"Adds stolen Light to your transmat effects\".",
    },
    EventKey {
        hash: 0x297E_1FAC,
        name: "Traveler Theme",
        evidence: "Set only by the transmat effect perk reading \"Adds a Traveler theme to your transmat effects\".",
    },
    EventKey {
        hash: 0xA079_4805,
        name: "Vex Effects",
        evidence: "Set only by the transmat effect perk reading \"Adds Vex effects to your transmat effects\".",
    },
    EventKey {
        hash: 0x9120_0434,
        name: "Frozen Sphere",
        evidence: "Set only by the transmat effect perk reading \"Adds a frozen sphere to your transmat effects\".",
    },
    EventKey {
        hash: 0xE85B_F9A4,
        name: "Silver Dawning Lantern",
        evidence: "Set only by the two transmat effect perks reading \"Adds a silver Dawning lantern to your transmat effects\" and \"Adds a lantern to your transmat effects\".",
    },
    EventKey {
        hash: 0x7D98_C32D,
        name: "Yellow Dawning Lantern",
        evidence: "Set only by the transmat effect perk reading \"Adds a yellow Dawning lantern to your transmat effects\".",
    },
    EventKey {
        hash: 0x927F_32D6,
        name: "Green Dawning Lantern",
        evidence: "Set only by the transmat effect perk reading \"Adds a green Dawning lantern to your transmat effects\".",
    },
    EventKey {
        hash: 0xC18B_DBDB,
        name: "Purple Dawning Lantern",
        evidence: "Set only by the transmat effect perk reading \"Adds a purple Dawning lantern to your transmat effects\".",
    },
    EventKey {
        hash: 0x8B6F_0371,
        name: "Vision of Xol",
        evidence: "Set only by the transmat effect perk reading \"Adds a vision of Xol to your transmat effects\".",
    },
    EventKey {
        hash: 0xE30B_2F8C,
        name: "Pixelation",
        evidence: "Set only by the transmat effect perk reading \"Adds pixelation to your transmat effects\".",
    },
    EventKey {
        hash: 0xB895_B210,
        name: "Warmind Flourish",
        evidence: "Set only by the transmat effect perk reading \"Adds a Warmind flourish to your transmat effects\".",
    },
    EventKey {
        hash: 0xA01B_5BF0,
        name: "Fireworks Burst",
        evidence: "Set only by the transmat effect perk reading \"Adds a fireworks burst to your transmat effects\".",
    },
    EventKey {
        hash: 0x9AE0_94EB,
        name: "Taegeuk",
        evidence: "Set only by the transmat effect perk reading \"Adds a Taegeuk to your transmat effects\".",
    },
    EventKey {
        hash: 0x4C99_619C,
        name: "Corrupted Ether Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a corrupted Ether appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x7CF1_BA66,
        name: "Awoken Teleportation Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds an Awoken teleportation appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x0E63_E9BF,
        name: "Awoken Pointillistic Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds an Awoken pointillistic appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xCA3B_089E,
        name: "Awoken Circle Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds an Awoken circle appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xE05F_03C6,
        name: "Ribbon of Spectral Energy",
        evidence: "Set only by the transmat effect perk reading \"Adds a ribbon of spectral energy to your transmat effects\".",
    },
    EventKey {
        hash: 0x3098_2C26,
        name: "Nest of Spiders",
        evidence: "Set only by the transmat effect perk reading \"Adds a nest of spiders to your transmat effects\".",
    },
    EventKey {
        hash: 0x19F7_9843,
        name: "Unearthed Tomb Visual",
        evidence: "Set only by the transmat effect perk reading \"Adds an unearthed tomb visual to your transmat effects\".",
    },
    EventKey {
        hash: 0x19F7_9842,
        name: "Guiding Star",
        evidence: "Set only by the transmat effect perk reading \"Adds a guiding star to your transmat effects\".",
    },
    EventKey {
        hash: 0x19F7_9841,
        name: "Black Armory Theme",
        evidence: "Set only by the transmat effect perk reading \"Adds a Black Armory theme to your transmat effects\".",
    },
    EventKey {
        hash: 0x19F7_9840,
        name: "Firing Synapse",
        evidence: "Set only by the transmat effect perk reading \"Adds a firing synapse to your transmat effects\".",
    },
    EventKey {
        hash: 0xF8EF_C9A8,
        name: "Explosion of Generosity",
        evidence: "Set only by the transmat effect perk reading \"Adds an explosion of generosity to your transmat effects\".",
    },
    EventKey {
        hash: 0xF8EF_C9A9,
        name: "Winter Storm",
        evidence: "Set only by the transmat effect perk reading \"Adds a winter storm to your transmat effects\".",
    },
    EventKey {
        hash: 0x7814_F145,
        name: "Dawning Flourish",
        evidence: "Set only by the transmat effect perk reading \"Adds a Dawning flourish to your transmat effects\".",
    },
    EventKey {
        hash: 0x7814_F144,
        name: "Dawning Flourish",
        evidence: "Set only by the transmat effect perk reading \"Adds a Dawning flourish to your transmat effects\".",
    },
    EventKey {
        hash: 0xE6FF_3ED0,
        name: "Unauthorized Modification",
        evidence: "Set only by the transmat effect perk reading \"Adds an unauthorized modification to your transmat effects\".",
    },
    EventKey {
        hash: 0xE6FF_3ED1,
        name: "Wave of Taken Energy",
        evidence: "Set only by the transmat effect perk reading \"Adds a wave of Taken energy to your transmat effects\".",
    },
    EventKey {
        hash: 0xE6FF_3ED2,
        name: "Illusion of a Gambit Coin",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a Gambit coin to your transmat effects\".",
    },
    EventKey {
        hash: 0xE6FF_3ED3,
        name: "Illusion of Gambit Snakes",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of Gambit snakes to your transmat effects\".",
    },
    EventKey {
        hash: 0xC034_5729,
        name: "Spring Breeze",
        evidence: "Set only by the transmat effect perk reading \"Adds a spring breeze to your transmat effects\".",
    },
    EventKey {
        hash: 0xC034_5728,
        name: "Geyser of Water",
        evidence: "Set only by the transmat effect perk reading \"Adds a geyser of water to your transmat effects\".",
    },
    EventKey {
        hash: 0xFFEF_64A7,
        name: "Hive Knight Miasma",
        evidence: "Set only by the transmat effect perk reading \"Adds Hive Knight miasma to your transmat effects\".",
    },
    EventKey {
        hash: 0xFFEF_64A6,
        name: "Eldritch Energy",
        evidence: "Set only by the transmat effect perk reading \"Adds eldritch energy to your transmat effects\".",
    },
    EventKey {
        hash: 0xFFEF_64A5,
        name: "Vex Minotaur Data",
        evidence: "Set only by the transmat effect perk reading \"Adds Vex Minotaur data to your transmat effects\".",
    },
    EventKey {
        hash: 0xFFEF_64A4,
        name: "Illusion of a Treasure Chest",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a treasure chest to your transmat effects\".",
    },
    EventKey {
        hash: 0xFFEF_64A3,
        name: "Illusion of a Tiger",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a tiger to your transmat effects\".",
    },
    EventKey {
        hash: 0x9B63_8F86,
        name: "Illusion of a Beach Ball",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a beach ball to your transmat effects\".",
    },
    EventKey {
        hash: 0x9B63_8F87,
        name: "Illusion of a Sandcastle",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a sandcastle to your transmat effects\".",
    },
    EventKey {
        hash: 0xDC81_3BEA,
        name: "Nightmarish Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a nightmarish appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xDC81_3BEB,
        name: "Hive Shrieker Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Hive Shrieker appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xDC81_3BE8,
        name: "Vex Harpy Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Vex Harpy appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xDC81_3BE9,
        name: "Mysterious Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a mysterious appearance to O Bearer Mine's transmat effects\".",
    },
    EventKey {
        hash: 0x9B2A_F5EF,
        name: "Avian Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds an avian appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xFD0A_23D9,
        name: "Moonlit Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a moonlit appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x9EDB_8060,
        name: "Black Garden Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Black Garden appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0x66FC_1D3B,
        name: "Vexsplosion",
        evidence: "Set only by the transmat effect perk reading \"Adds a Vexsplosion to your transmat effects\".",
    },
    EventKey {
        hash: 0xCDA0_053D,
        name: "Snowman",
        evidence: "Set only by the transmat effect perk reading \"Adds a snowman to your transmat effects\".",
    },
    EventKey {
        hash: 0xCDA0_053C,
        name: "Gingerbread House with a Peppermint Swirl",
        evidence: "Set only by the transmat effect perk reading \"Adds a gingerbread house with a peppermint swirl to your transmat effects\".",
    },
    EventKey {
        hash: 0x264D_5269,
        name: "Cabal Shield",
        evidence: "Set only by the transmat effect perk reading \"Adds a Cabal shield to your transmat effects\".",
    },
    EventKey {
        hash: 0x264D_5268,
        name: "Illusion of a Steamer Trunk",
        evidence: "Set only by the transmat effect perk reading \"Adds the illusion of a steamer trunk to your transmat effects\".",
    },
    EventKey {
        hash: 0x264D_526B,
        name: "Circular Vex Gate",
        evidence: "Set only by the transmat effect perk reading \"Adds a circular Vex Gate to your transmat effects\".",
    },
    EventKey {
        hash: 0x264D_526A,
        name: "Fire and Smoke",
        evidence: "Set only by the transmat effect perk reading \"Adds fire and smoke to your transmat effects\".",
    },
    EventKey {
        hash: 0xD430_735A,
        name: "Rasputin Respawn",
        evidence: "Set only by the transmat effect perk reading \"Adds a Rasputin respawn to your transmat effects\".",
    },
    EventKey {
        hash: 0x817A_E96C,
        name: "Crashing Warsat",
        evidence: "Set only by the transmat effect perk reading \"Adds a crashing Warsat to your transmat effects\".",
    },
    EventKey {
        hash: 0x817A_E96D,
        name: "Cabal Hologram Entrance",
        evidence: "Set only by the transmat effect perk reading \"Adds a Cabal hologram entrance to your transmat effects\".",
    },
    EventKey {
        hash: 0x817A_E96F,
        name: "SIVA Respawn",
        evidence: "Set only by the transmat effect perk reading \"Adds a SIVA respawn to your transmat effects\".",
    },
    EventKey {
        hash: 0xA642_BED3,
        name: "Cherry Blossom Petal Appearance",
        evidence: "Set only by the transmat effect perk reading \"Adds a cherry blossom petal appearance to your transmat effects\".",
    },
    EventKey {
        hash: 0xA642_BED2,
        name: "Bright Neon Class Logo",
        evidence: "Set only by the transmat effect perk reading \"Adds bright neon class logo to your transmat effects\".",
    },
    EventKey {
        hash: 0xFD96_FAA5,
        name: "Traveler",
        evidence: "Set only by the transmat effect perk reading \"Adds the Traveler to your transmat effects\".",
    },
    EventKey {
        hash: 0x3672_1113,
        name: "High-Tech Capsule",
        evidence: "Set only by the transmat effect perk reading \"Adds a high-tech capsule to your transmat effects\".",
    },
    EventKey {
        hash: 0x3672_1112,
        name: "Champion Wreath",
        evidence: "Set only by the transmat effect perk reading \"Adds a Champion wreath to your transmat effects\".",
    },
    EventKey {
        hash: 0xA21A_7C09,
        name: "Flash of Lightning",
        evidence: "Set only by the transmat effect perk reading \"Adds a flash of lightning to your transmat effects\".",
    },
    EventKey {
        hash: 0x8D22_4EEA,
        name: "Loose Wrappings",
        evidence: "Set only by the transmat effect perk reading \"Adds loose wrappings to your transmat effects\".",
    },
];
