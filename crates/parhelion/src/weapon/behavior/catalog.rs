//! The graftable behavior catalogue and the lookups that read it.
use super::*;

/// Graftable stock behavior records and firing graphs.
pub const CATALOG: &[Behavior] = &[
    Behavior {
        id: "hard-light",
        name: "Element Switch",
        source_name: "Hard Light",
        source_item_hash: 0xF5DE_4480,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_9461,
            content_group: 0x8F15_AF33,
            family_types: &[
                "Auto Rifle",
                "Scout Rifle",
                "Pulse Rifle",
                "Trace Rifle",
                "Hand Cannon",
                "Fusion Rifle",
                "Shotgun",
            ],
        },
        intrinsic_plug: Some(0xD738A749),
        trait_plug: Some(0x9C3304DA),
        effect: BehaviorEffect::ElementSwitch,
        caution: None,
        summary: "Hold Reload to cycle the damage type. Needs The Fundamentals.",
    },
    Behavior {
        id: "borealis",
        name: "Element Switch",
        source_name: "Borealis",
        source_item_hash: 0xBB46_CCD3,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_BC09,
            content_group: 0x21FD_5FEE,
            family_types: &["Sniper Rifle"],
        },
        intrinsic_plug: Some(0xB7A6F964),
        trait_plug: Some(0x1DE798BC),
        effect: BehaviorEffect::ElementSwitch,
        caution: None,
        summary: "Hold Reload to cycle the damage type. Needs The Fundamentals.",
    },
    Behavior {
        id: "graviton-lance",
        name: "Graviton Lance Behavior",
        source_name: "Graviton Lance",
        source_item_hash: 0xD84E_04AA,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_9461,
            content_group: 0xA158_5D8F,
            family_types: &[
                "Auto Rifle",
                "Scout Rifle",
                "Pulse Rifle",
                "Trace Rifle",
                "Hand Cannon",
                "Fusion Rifle",
                "Shotgun",
            ],
        },
        intrinsic_plug: Some(0xE8C9DED3),
        trait_plug: Some(0x131AF65A),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Copies the weapon's state record, not its firing or projectile graph. Gameplay transfer is untested.",
    },
    Behavior {
        id: "cerberus-plus-one",
        name: "Cerberus+1 Behavior",
        source_name: "Cerberus+1",
        source_item_hash: 0x5BDB_CC56,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_9461,
            content_group: 0xA046_841D,
            family_types: &[
                "Auto Rifle",
                "Scout Rifle",
                "Pulse Rifle",
                "Trace Rifle",
                "Hand Cannon",
                "Fusion Rifle",
                "Shotgun",
            ],
        },
        intrinsic_plug: Some(0xBF430319),
        trait_plug: Some(0x4CDDCE02),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Copies the weapon's state record, not its four-barrel firing graph. Gameplay transfer is untested.",
    },
    Behavior {
        id: "symmetry",
        name: "Symmetry Behavior",
        source_name: "Symmetry",
        source_item_hash: 0xEF7D_3366,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_9461,
            content_group: 0xFA9E_4076,
            family_types: &[
                "Auto Rifle",
                "Scout Rifle",
                "Pulse Rifle",
                "Trace Rifle",
                "Hand Cannon",
                "Fusion Rifle",
                "Shotgun",
            ],
        },
        intrinsic_plug: Some(0xF97737D0),
        trait_plug: Some(0x0796FC77),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Copies the weapon's state record, not its firing or projectile graph. Gameplay transfer is untested.",
    },
    Behavior {
        id: "lord-of-wolves",
        name: "Lord of Wolves Behavior",
        source_name: "Lord of Wolves",
        source_item_hash: 0xCB7B_5EDF,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_9461,
            content_group: 0xB39D_92A7,
            family_types: &[
                "Auto Rifle",
                "Scout Rifle",
                "Pulse Rifle",
                "Trace Rifle",
                "Hand Cannon",
                "Fusion Rifle",
                "Shotgun",
            ],
        },
        intrinsic_plug: Some(0x1CB0A51F),
        trait_plug: Some(0x11D68AF1),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Untested.",
    },
    Behavior {
        id: "izanagis-burden",
        name: "Izanagi's Burden Behavior",
        source_name: "Izanagi's Burden",
        source_item_hash: 0xBF70_4917,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_BC09,
            content_group: 0x1660_72BD,
            family_types: &["Sniper Rifle"],
        },
        intrinsic_plug: Some(0x3FC86EE4),
        trait_plug: Some(0xAADFDE43),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Copies the weapon's state record, not its firing or projectile graph. Gameplay transfer is untested.",
    },
    Behavior {
        id: "travelers-chosen",
        name: "Traveler's Chosen Behavior",
        source_name: "Traveler's Chosen",
        source_item_hash: 0x032B_2570,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_AEA5,
            content_group: 0x8DC3_3F1A,
            family_types: &["Sidearm"],
        },
        intrinsic_plug: None,
        trait_plug: None,
        effect: BehaviorEffect::NoObservedEffect,
        caution: None,
        summary: "Copies Traveler's Chosen's state record. Its gameplay effect on another weapon has not been verified.",
    },
    Behavior {
        id: "two-tailed-fox",
        name: "Two-Tailed Fox Behavior",
        source_name: "Two-Tailed Fox",
        source_item_hash: 0xA09B_F9B1,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_A37A,
            content_group: 0x0BCC_6D9A,
            family_types: &["Rocket Launcher"],
        },
        intrinsic_plug: Some(0xD985_E346),
        trait_plug: Some(0x2E59_CD3D),
        effect: BehaviorEffect::NoObservedEffect,
        caution: None,
        summary: "Copies Two-Tailed Fox's state record. This does not copy its two-rocket firing graph. Its gameplay effect on another weapon has not been verified.",
    },
    Behavior {
        id: "tarrabah",
        name: "Tarrabah Behavior",
        source_name: "Tarrabah",
        source_item_hash: 0xB969_7F3C,
        source: BehaviorSource::Record {
            owner_tag: 0x8152_C7C5,
            content_group: 0xC7A3_45AF,
            family_types: &["Submachine Gun", "Auto Rifle"],
        },
        intrinsic_plug: Some(0x976D_834D),
        trait_plug: Some(0xC7BC_3D87),
        effect: BehaviorEffect::NoObservedEffect,
        caution: None,
        summary: "Copies Tarrabah's state record. Ravenous Beast behavior on another weapon has not been verified.",
    },
    Behavior {
        id: "drang-state",
        name: "Drang State",
        source_name: "Drang",
        source_item_hash: 0x8CD0_74B0,
        source: BehaviorSource::State {
            owner_tag: 0x8152_AEA5,
            content_group: 0xA908_85A4,
        },
        intrinsic_plug: Some(0x4D21_471C),
        trait_plug: Some(0x9A65_D669),
        effect: BehaviorEffect::Untested,
        caution: Some(
            "Drang's state transfer is package-backed but has not been verified in game.",
        ),
        summary: "Copies Drang's distinct state array and can include Together Forever.",
    },
    Behavior {
        id: "ace-of-spades-graph",
        name: "Ace of Spades",
        source_name: "Ace of Spades",
        source_item_hash: 0x14B465B2,
        source: BehaviorSource::Graph { tag: 0x80EF2821 },
        intrinsic_plug: Some(0x2699DC63),
        trait_plug: Some(0x5D170526),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Ace of Spades's own firing and projectile graph.",
    },
    Behavior {
        id: "anarchy-graph",
        name: "Anarchy",
        source_name: "Anarchy",
        source_item_hash: 0x8DA63B0E,
        source: BehaviorSource::Graph { tag: 0x815281B0 },
        intrinsic_plug: Some(0x1733C5F9),
        trait_plug: Some(0x23153F37),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Anarchy's own firing and projectile graph.",
    },
    Behavior {
        id: "arbalest-graph",
        name: "Arbalest",
        source_name: "Arbalest",
        source_item_hash: 0x7EF63891,
        source: BehaviorSource::Graph { tag: 0x81526763 },
        intrinsic_plug: Some(0x98D60A62),
        trait_plug: Some(0x6456553B),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Arbalest's own firing and projectile graph.",
    },
    Behavior {
        id: "bastion-graph",
        name: "Bastion",
        source_name: "Bastion",
        source_item_hash: 0x8FF9DFD6,
        source: BehaviorSource::Graph { tag: 0x815266F9 },
        intrinsic_plug: Some(0x46B84272),
        trait_plug: Some(0xF9C82511),
        effect: BehaviorEffect::Untested,
        caution: Some(
            "Its frame expects its own ammo. A Grenade Launcher taking it was left with none.",
        ),
        summary: "Bastion's own firing and projectile graph.",
    },
    Behavior {
        id: "borealis-graph",
        name: "Borealis",
        source_name: "Borealis",
        source_item_hash: 0xBB46CCD3,
        source: BehaviorSource::Graph { tag: 0x80BC0674 },
        intrinsic_plug: Some(0xB7A6F964),
        trait_plug: Some(0x1DE798BC),
        effect: BehaviorEffect::Untested,
        caution: Some(
            "Choosing this sets the damage type to Variable and locks it, because Borealis switches damage as well as fires differently.",
        ),
        summary: "Borealis's own firing and projectile graph, plus its damage switching.",
    },
    Behavior {
        id: "cerberus-1-graph",
        name: "Cerberus+1",
        source_name: "Cerberus+1",
        source_item_hash: 0x5BDBCC56,
        source: BehaviorSource::Graph { tag: 0x80EF30B2 },
        intrinsic_plug: Some(0xBF430319),
        trait_plug: Some(0x4CDDCE02),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Cerberus+1's own firing and projectile graph.",
    },
    Behavior {
        id: "coldheart-graph",
        name: "Coldheart",
        source_name: "Coldheart",
        source_item_hash: 0x50384F33,
        source: BehaviorSource::Graph { tag: 0x80BBC8E0 },
        intrinsic_plug: Some(0x3DC436F0),
        trait_plug: Some(0x93F6B3E6),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Coldheart's own firing and projectile graph.",
    },
    Behavior {
        id: "crimson-graph",
        name: "Crimson",
        source_name: "Crimson",
        source_item_hash: 0xCCE7D927,
        source: BehaviorSource::Graph { tag: 0x80EF285E },
        intrinsic_plug: Some(0x3D73AC8D),
        trait_plug: Some(0x8E18595D),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Crimson's own firing and projectile graph.",
    },
    Behavior {
        id: "d-a-r-c-i-graph",
        name: "D.A.R.C.I.",
        source_name: "D.A.R.C.I.",
        source_item_hash: 0xBB46CCD2,
        source: BehaviorSource::Graph { tag: 0x80BC0674 },
        intrinsic_plug: Some(0x37F7FF54),
        trait_plug: Some(0x37FB7996),
        effect: BehaviorEffect::Untested,
        caution: Some(
            "Personal Assistant reads through D.A.R.C.I.'s scope, so it does nothing on a weapon without one.",
        ),
        summary: "D.A.R.C.I.'s own firing and projectile graph.",
    },
    Behavior {
        id: "deathbringer-graph",
        name: "Deathbringer",
        source_name: "Deathbringer",
        source_item_hash: 0x850C3A5B,
        source: BehaviorSource::Graph { tag: 0x8152A483 },
        intrinsic_plug: Some(0x188B8F9D),
        trait_plug: Some(0xC6B8B6B4),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Deathbringer's own firing and projectile graph.",
    },
    Behavior {
        id: "devil-s-ruin-graph",
        name: "Devil's Ruin",
        source_name: "Devil's Ruin",
        source_item_hash: 0xE3EF3A6E,
        source: BehaviorSource::Graph { tag: 0x8161F91B },
        intrinsic_plug: Some(0x13EF8C4A),
        trait_plug: Some(0x3F23A759),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Devil's Ruin's own firing and projectile graph.",
    },
    Behavior {
        id: "divinity-graph",
        name: "Divinity",
        source_name: "Divinity",
        source_item_hash: 0xF49521E2,
        source: BehaviorSource::Graph { tag: 0x81529533 },
        intrinsic_plug: Some(0x6B26D5A2),
        trait_plug: Some(0x46A8FFBF),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Divinity's own firing and projectile graph.",
    },
    Behavior {
        id: "eriana-s-vow-graph",
        name: "Eriana's Vow",
        source_name: "Eriana's Vow",
        source_item_hash: 0xD210C009,
        source: BehaviorSource::Graph { tag: 0x8152903D },
        intrinsic_plug: Some(0xBD33FC8B),
        trait_plug: Some(0x64929156),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Eriana's Vow's own firing and projectile graph.",
    },
    Behavior {
        id: "graviton-lance-graph",
        name: "Graviton Lance",
        source_name: "Graviton Lance",
        source_item_hash: 0xD84E04AA,
        source: BehaviorSource::Graph { tag: 0x80BC58C6 },
        intrinsic_plug: Some(0xE8C9DED3),
        trait_plug: Some(0x131AF65A),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Graviton Lance's own firing and projectile graph.",
    },
    Behavior {
        id: "hard-light-graph",
        name: "Hard Light",
        source_name: "Hard Light",
        source_item_hash: 0xF5DE4480,
        source: BehaviorSource::Graph { tag: 0x80BBC812 },
        intrinsic_plug: Some(0xD738A749),
        trait_plug: Some(0x9C3304DA),
        effect: BehaviorEffect::Untested,
        caution: Some(
            "Choosing this sets the damage type to Variable and locks it, because Hard Light switches damage as well as fires differently.",
        ),
        summary: "Hard Light's bouncing rounds, plus its damage switching.",
    },
    Behavior {
        id: "heir-apparent-graph",
        name: "Heir Apparent",
        source_name: "Heir Apparent",
        source_item_hash: 0x7C44B6B5,
        source: BehaviorSource::Graph { tag: 0x81A6AF7F },
        intrinsic_plug: Some(0x9B7AACF3),
        trait_plug: Some(0x46A5D616),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Heir Apparent's own firing and projectile graph.",
    },
    Behavior {
        id: "izanagi-s-burden-graph",
        name: "Izanagi's Burden",
        source_name: "Izanagi's Burden",
        source_item_hash: 0xBF704917,
        source: BehaviorSource::Graph { tag: 0x8152BC2B },
        intrinsic_plug: Some(0x3FC86EE4),
        trait_plug: Some(0xAADFDE43),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Izanagi's Burden's own firing and projectile graph.",
    },
    Behavior {
        id: "j-tunn-graph",
        name: "Jotunn",
        source_name: "Jotunn",
        source_item_hash: 0x18DD6E9C,
        source: BehaviorSource::Graph { tag: 0x8152659E },
        intrinsic_plug: Some(0x62C32A65),
        trait_plug: Some(0xC3409321),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Jotunn's own firing and projectile graph.",
    },
    Behavior {
        id: "le-monarque-graph",
        name: "Le Monarque",
        source_name: "Le Monarque",
        source_item_hash: 0xD5EACCB7,
        source: BehaviorSource::Graph { tag: 0x81525982 },
        intrinsic_plug: Some(0x8253D5D6),
        trait_plug: Some(0x39169B67),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Le Monarque's own firing and projectile graph.",
    },
    Behavior {
        id: "legend-of-acrius-graph",
        name: "Legend of Acrius",
        source_name: "Legend of Acrius",
        source_item_hash: 0x67F515B2,
        source: BehaviorSource::Graph { tag: 0x80BBD85B },
        intrinsic_plug: Some(0xDF991F04),
        trait_plug: None,
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Legend of Acrius's own firing and projectile graph.",
    },
    Behavior {
        id: "leviathan-s-breath-graph",
        name: "Leviathan's Breath",
        source_name: "Leviathan's Breath",
        source_item_hash: 0x9A7AEB9A,
        source: BehaviorSource::Graph { tag: 0x815259CC },
        intrinsic_plug: Some(0x654FBBD9),
        trait_plug: Some(0x8F07B14C),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Leviathan's Breath's own firing and projectile graph.",
    },
    Behavior {
        id: "lord-of-wolves-graph",
        name: "Lord of Wolves",
        source_name: "Lord of Wolves",
        source_item_hash: 0xCB7B5EDF,
        source: BehaviorSource::Graph { tag: 0x80BBD141 },
        intrinsic_plug: Some(0x1CB0A51F),
        trait_plug: Some(0x11D68AF1),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Lord of Wolves's own firing and projectile graph.",
    },
    Behavior {
        id: "malfeasance-graph",
        name: "Malfeasance",
        source_name: "Malfeasance",
        source_item_hash: 0x0C3630EB,
        source: BehaviorSource::Graph { tag: 0x80EF28E0 },
        intrinsic_plug: Some(0x6AC988C7),
        trait_plug: Some(0x7EF5DDB9),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Malfeasance's own firing and projectile graph.",
    },
    Behavior {
        id: "merciless-graph",
        name: "Merciless",
        source_name: "Merciless",
        source_item_hash: 0xF9C0B6B0,
        source: BehaviorSource::Graph { tag: 0x80BBAFAD },
        intrinsic_plug: Some(0x271CD3CE),
        trait_plug: Some(0x8B18058B),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Merciless's own firing and projectile graph.",
    },
    Behavior {
        id: "one-thousand-voices-graph",
        name: "One Thousand Voices",
        source_name: "One Thousand Voices",
        source_item_hash: 0x7B55DC8D,
        source: BehaviorSource::Graph { tag: 0x80EF3176 },
        intrinsic_plug: Some(0x62C4AE61),
        trait_plug: Some(0xD0DDD13F),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "One Thousand Voices's own firing and projectile graph.",
    },
    Behavior {
        id: "outbreak-perfected-graph",
        name: "Outbreak Perfected",
        source_name: "Outbreak Perfected",
        source_item_hash: 0x17D8FEAB,
        source: BehaviorSource::Graph { tag: 0x8153303A },
        intrinsic_plug: Some(0xFAD75D3E),
        trait_plug: Some(0x45A0BDD7),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Outbreak Perfected's own firing and projectile graph.",
    },
    Behavior {
        id: "prometheus-lens-graph",
        name: "Prometheus Lens",
        source_name: "Prometheus Lens",
        source_item_hash: 0x012248BA,
        source: BehaviorSource::Graph { tag: 0x80EF31F0 },
        intrinsic_plug: Some(0x220CDA80),
        trait_plug: Some(0xCEE0A2D2),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Prometheus Lens's own firing and projectile graph.",
    },
    Behavior {
        id: "riskrunner-graph",
        name: "Riskrunner",
        source_name: "Riskrunner",
        source_item_hash: 0xB824C63D,
        source: BehaviorSource::Graph { tag: 0x80BC10AF },
        intrinsic_plug: Some(0x95FF3C6B),
        trait_plug: Some(0x5B3E379D),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Riskrunner's own firing and projectile graph.",
    },
    Behavior {
        id: "ruinous-effigy-graph",
        name: "Ruinous Effigy",
        source_name: "Ruinous Effigy",
        source_item_hash: 0x5141601F,
        source: BehaviorSource::Graph { tag: 0x81A6ADDC },
        intrinsic_plug: Some(0x62C9F17F),
        trait_plug: Some(0x2F98742C),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Ruinous Effigy's own firing and projectile graph.",
    },
    Behavior {
        id: "skyburner-s-oath-graph",
        name: "Skyburner's Oath",
        source_name: "Skyburner's Oath",
        source_item_hash: 0xFDA23E68,
        source: BehaviorSource::Graph { tag: 0x80BBC7DE },
        intrinsic_plug: Some(0xA36F381C),
        trait_plug: Some(0x0A8B937E),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Skyburner's Oath's own firing and projectile graph.",
    },
    Behavior {
        id: "sleeper-simulant-graph",
        name: "Sleeper Simulant",
        source_name: "Sleeper Simulant",
        source_item_hash: 0xF0923C79,
        source: BehaviorSource::Graph { tag: 0x80BBAD79 },
        intrinsic_plug: Some(0xE783140A),
        trait_plug: Some(0x23153F37),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Sleeper Simulant's own firing and projectile graph.",
    },
    Behavior {
        id: "sturm-graph",
        name: "Sturm",
        source_name: "Sturm",
        source_item_hash: 0xAD4746D4,
        source: BehaviorSource::GraphState {
            tag: 0x80BB_C07C,
            owner_tag: 0x8152_905A,
            content_group: 0x6D7C_0BAC,
        },
        intrinsic_plug: Some(0x1E3A82EC),
        trait_plug: Some(0x8565B49A),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Copies Sturm's firing graph and distinct state array together. This combined transfer still needs an in-game test.",
    },
    Behavior {
        id: "sunshot-graph",
        name: "Sunshot",
        source_name: "Sunshot",
        source_item_hash: 0xAD4746D5,
        source: BehaviorSource::Graph { tag: 0x80BBC0ED },
        intrinsic_plug: Some(0xF1269C83),
        trait_plug: Some(0xF54FA31D),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Sunshot's own firing and projectile graph.",
    },
    Behavior {
        id: "sweet-business-graph",
        name: "Sweet Business",
        source_name: "Sweet Business",
        source_item_hash: 0x50384F32,
        source: BehaviorSource::Graph { tag: 0x80BBC8AB },
        intrinsic_plug: Some(0xDFD1D2A5),
        trait_plug: Some(0x6E75405C),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Sweet Business's own firing and projectile graph.",
    },
    Behavior {
        id: "symmetry-graph",
        name: "Symmetry",
        source_name: "Symmetry",
        source_item_hash: 0xEF7D3366,
        source: BehaviorSource::Graph { tag: 0x8161F5EC },
        intrinsic_plug: Some(0xF97737D0),
        trait_plug: Some(0x0796FC77),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Symmetry's own firing and projectile graph.",
    },
    Behavior {
        id: "telesto-graph",
        name: "Telesto",
        source_name: "Telesto",
        source_item_hash: 0x83A19696,
        source: BehaviorSource::Graph { tag: 0x80EF0CEB },
        intrinsic_plug: Some(0x72E9AA21),
        trait_plug: Some(0xECAB1CE7),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Telesto's own firing and projectile graph.",
    },
    Behavior {
        id: "the-colony-graph",
        name: "The Colony",
        source_name: "The Colony",
        source_item_hash: 0xE86A25CF,
        source: BehaviorSource::Graph { tag: 0x80EF1BA6 },
        intrinsic_plug: Some(0xE942B6D5),
        trait_plug: Some(0x786608F0),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Colony's own firing and projectile graph.",
    },
    Behavior {
        id: "the-huckleberry-graph",
        name: "The Huckleberry",
        source_name: "The Huckleberry",
        source_item_hash: 0x8843C72A,
        source: BehaviorSource::Graph { tag: 0x80BC1268 },
        intrinsic_plug: Some(0x2592127F),
        trait_plug: Some(0xD3ACF01C),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Huckleberry's own firing and projectile graph.",
    },
    Behavior {
        id: "the-jade-rabbit-graph",
        name: "The Jade Rabbit",
        source_name: "The Jade Rabbit",
        source_item_hash: 0xE5296126,
        source: BehaviorSource::Graph { tag: 0x815294C1 },
        intrinsic_plug: Some(0xDAAD2BD4),
        trait_plug: Some(0x8E4A757E),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Jade Rabbit's own firing and projectile graph.",
    },
    Behavior {
        id: "the-prospector-graph",
        name: "The Prospector",
        source_name: "The Prospector",
        source_item_hash: 0xD38BCABB,
        source: BehaviorSource::Graph { tag: 0x80BBB91F },
        intrinsic_plug: Some(0xB17C3C16),
        trait_plug: Some(0xFE63AC50),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Prospector's own firing and projectile graph.",
    },
    Behavior {
        id: "the-queenbreaker-graph",
        name: "The Queenbreaker",
        source_name: "The Queenbreaker",
        source_item_hash: 0x79DC9B1A,
        source: BehaviorSource::Graph { tag: 0x80BBB0FC },
        intrinsic_plug: Some(0x5B4321B6),
        trait_plug: Some(0x6F39A4F7),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Queenbreaker's own firing and projectile graph.",
    },
    Behavior {
        id: "the-wardcliff-coil-graph",
        name: "The Wardcliff Coil",
        source_name: "The Wardcliff Coil",
        source_item_hash: 0x59EFED62,
        source: BehaviorSource::Graph { tag: 0x80BC5CE1 },
        intrinsic_plug: Some(0x936D2A07),
        trait_plug: Some(0x46415613),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "The Wardcliff Coil's own firing and projectile graph.",
    },
    Behavior {
        id: "thorn-graph",
        name: "Thorn",
        source_name: "Thorn",
        source_item_hash: 0xECD240D4,
        source: BehaviorSource::Graph { tag: 0x80BBC0FE },
        intrinsic_plug: Some(0x6F108C16),
        trait_plug: Some(0xAE1C4EC2),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Thorn's own firing and projectile graph.",
    },
    Behavior {
        id: "thunderlord-graph",
        name: "Thunderlord",
        source_name: "Thunderlord",
        source_item_hash: 0xC6368B4E,
        source: BehaviorSource::Graph { tag: 0x81529CCB },
        intrinsic_plug: Some(0xF73FDF15),
        trait_plug: Some(0x54954949),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Thunderlord's own firing and projectile graph.",
    },
    Behavior {
        id: "tommy-s-matchbook-graph",
        name: "Tommy's Matchbook",
        source_name: "Tommy's Matchbook",
        source_item_hash: 0x2E43BDEE,
        source: BehaviorSource::Graph { tag: 0x81A6B4A6 },
        intrinsic_plug: Some(0x394F676E),
        trait_plug: Some(0xE0DB8E4B),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Tommy's Matchbook's own firing and projectile graph.",
    },
    Behavior {
        id: "tractor-cannon-graph",
        name: "Tractor Cannon",
        source_name: "Tractor Cannon",
        source_item_hash: 0xD5704485,
        source: BehaviorSource::Graph { tag: 0x80BBD712 },
        intrinsic_plug: Some(0x482B73DE),
        trait_plug: Some(0x01880A17),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Tractor Cannon's own firing and projectile graph.",
    },
    Behavior {
        id: "travelers-chosen-graph",
        name: "Traveler's Chosen",
        source_name: "Traveler's Chosen",
        // The exotic, which carries Gathering Light and Gift of the Traveler. The record entry
        // above names a second sidearm of the same name whose only intrinsic is Adaptive Frame.
        source_item_hash: 0x6E75_4BFC,
        source: BehaviorSource::Graph { tag: 0x80BB_DC29 },
        intrinsic_plug: Some(0x3A23_E13D),
        trait_plug: Some(0x017C_54BA),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Traveler's Chosen's own firing graph. Its state record is catalogued separately.",
    },
    Behavior {
        id: "trinity-ghoul-graph",
        name: "Trinity Ghoul",
        source_name: "Trinity Ghoul",
        source_item_hash: 0x3092080D,
        source: BehaviorSource::Graph { tag: 0x80C69DB6 },
        intrinsic_plug: Some(0x5DCFA024),
        trait_plug: Some(0x0BF86A01),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Trinity Ghoul's own firing and projectile graph.",
    },
    Behavior {
        id: "truth-graph",
        name: "Truth",
        source_name: "Truth",
        source_item_hash: 0x47A27ADF,
        source: BehaviorSource::Graph { tag: 0x8152A37E },
        intrinsic_plug: Some(0x94861F33),
        trait_plug: Some(0xAD875AEB),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Truth's own firing and projectile graph.",
    },
    Behavior {
        id: "vigilance-wing-graph",
        name: "Vigilance Wing",
        source_name: "Vigilance Wing",
        source_item_hash: 0xD84E_04AB,
        source: BehaviorSource::Graph { tag: 0x80BB_C8AF },
        intrinsic_plug: Some(0x8984_35DF),
        trait_plug: Some(0xF72A_1183),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Vigilance Wing's five-round burst, plus its Harsh Truths frame.",
    },
    Behavior {
        id: "wavesplitter-graph",
        name: "Wavesplitter",
        source_name: "Wavesplitter",
        source_item_hash: 0x6E7074F4,
        source: BehaviorSource::Graph { tag: 0x80EF31F5 },
        intrinsic_plug: Some(0x1B628488),
        trait_plug: Some(0x897980D4),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Wavesplitter's own firing and projectile graph.",
    },
    Behavior {
        id: "wish-ender-graph",
        name: "Wish-Ender",
        source_name: "Wish-Ender",
        source_item_hash: 0x3092080C,
        source: BehaviorSource::Graph { tag: 0x80EF017E },
        intrinsic_plug: Some(0x57A047A0),
        trait_plug: Some(0x885B87DA),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Wish-Ender's own firing and projectile graph.",
    },
    Behavior {
        id: "witherhoard-graph",
        name: "Witherhoard",
        source_name: "Witherhoard",
        source_item_hash: 0x8C8180D6,
        source: BehaviorSource::Graph { tag: 0x81A6AA30 },
        intrinsic_plug: Some(0xB6969154),
        trait_plug: Some(0x2C768973),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Witherhoard's own firing and projectile graph.",
    },
    Behavior {
        id: "xenophage-graph",
        name: "Xenophage",
        source_name: "Xenophage",
        source_item_hash: 0x532A003B,
        source: BehaviorSource::Graph { tag: 0x81529C54 },
        intrinsic_plug: Some(0x86CB9E20),
        trait_plug: Some(0xA9A8666A),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Xenophage's own firing and projectile graph.",
    },
    Behavior {
        id: "wardens-law-graph",
        name: "Warden's Law",
        source_name: "Warden's Law",
        source_item_hash: 0x0DE9_C46D,
        source: BehaviorSource::Graph { tag: 0x80EF_28E9 },
        intrinsic_plug: Some(0xE9DD_FAA0),
        trait_plug: None,
        effect: BehaviorEffect::Untested,
        caution: Some(
            "The graph and Double Fire intrinsic form one behavior. Their transfer has not been verified in game.",
        ),
        summary: "Copies Warden's Law's twin-fire graph and can include Double Fire, which attaches both firing assets while the weapon is drawn.",
    },
];

/// Virtual identifier meaning "whichever record drives The Fundamentals in this weapon's family".
/// Recipes use it so a saved weapon never names Hard Light or Borealis directly.
pub const ELEMENT_SWITCH: &str = "element-switch";

/// The catalogue entry a saved recipe refers to.
#[must_use]
pub fn behavior(id: &str) -> Option<&'static Behavior> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// The record that makes The Fundamentals cycle damage types inside one content owner.
#[must_use]
pub fn element_switch_for_owner(owner_tag: u32) -> Option<&'static Behavior> {
    CATALOG
        .iter()
        .find(|entry| entry.owner_tag() == Some(owner_tag) && entry.switches_element())
}

/// Every record a weapon in this owner can graft.
pub fn catalog_for_owner(owner_tag: u32) -> impl Iterator<Item = &'static Behavior> {
    CATALOG
        .iter()
        .filter(move |entry| entry.owner_tag() == Some(owner_tag))
}

/// Records a weapon of this type can graft, used to populate the picker before the packages are
/// read. The compiler resolves the real owner and refuses a graft the family cannot reach.
pub fn catalog_for_type(type_name: &str) -> impl Iterator<Item = &'static Behavior> + use<'_> {
    CATALOG
        .iter()
        .filter(move |entry| entry.reaches_type(type_name))
}

/// Whether a weapon of this type can switch elements.
///
/// Every weapon can. A family that holds no record of its own has one copied into its content
/// owner at build time, so this is no longer a question of which family the weapon belongs to.
#[must_use]
pub fn switches_element_for_type(_type_name: &str) -> bool {
    CATALOG.iter().any(Behavior::switches_element)
}

/// The behavior-carrying record for the same weapon as this firing graph.
///
/// Several exotics keep their special behavior in two halves: a firing and projectile graph at
/// the block, and a record inside the content owner that carries the behavior array. The picker
/// offers one choice per weapon and prefers the graph, so before this the record half was never
/// applied and a perk that needed it did nothing at all. Graviton Lance was reported that way,
/// with no detonation on a hand cannon, while Hard Light looked fine because element switching
/// lives entirely in its record and reaches the weapon through its own control.
pub(super) fn paired_record(entry: &Behavior) -> Option<&'static Behavior> {
    if !entry.has_graph() {
        return None;
    }
    if let Some((_, record)) = SHARED_RECORDS.iter().find(|(graph, _)| *graph == entry.id) {
        return behavior(record);
    }
    CATALOG.iter().find(|other| {
        other.source_item_hash == entry.source_item_hash
            && !other.has_graph()
            && matches!(other.source, BehaviorSource::Record { .. })
    })
}

/// The weapon whose behavior record travels with this choice, when one does.
///
/// A graph choice applies a record the author did not select and cannot decline, so the picker
/// has to say so rather than describe only the firing side.
#[must_use]
pub fn paired_record_source(entry: &Behavior) -> Option<&'static str> {
    paired_record(entry).map(|record| record.source_name)
}

/// Graphs whose weapon shares another weapon's behavior record, so the pair cannot be found by
/// matching item hashes.
///
/// Ten records serve fourteen exotics. `parhelion-behavior-graft-plan-2026-09-16` lists which
/// weapons share each one: record `0xD1C0` covers Graviton Lance, Vigilance Wing and Skyburner's
/// Oath, `0xD820` covers Cerberus+1 and Prometheus Lens, and `0xE720` covers Symmetry and
/// Divinity. All seven sit in content owner `0x81529461`, so the shared record extracts the same
/// bytes whichever of them names it. Without this the four sharers transferred a firing graph
/// with no behavior array, which is the half graft Graviton Lance was reported for.
pub(super) const SHARED_RECORDS: &[(&str, &str)] = &[
    ("vigilance-wing-graph", "graviton-lance"),
    ("skyburner-s-oath-graph", "graviton-lance"),
    ("prometheus-lens-graph", "cerberus-plus-one"),
    ("divinity-graph", "symmetry"),
];

/// Turns a recipe identifier into a catalogue entry this weapon's family can actually reach.
pub(super) fn resolve_request(
    id: &str,
    owner_tag: u32,
) -> AuthoringResult<Option<&'static Behavior>> {
    if id == ELEMENT_SWITCH {
        // A family without a record of its own gets one appended instead.
        return Ok(element_switch_for_owner(owner_tag));
    }
    let entry = behavior(id)
        .ok_or_else(|| invalid(format!("Unknown additional weapon behavior \"{id}\"")))?;
    Ok(Some(entry))
}

/// The catalogued sources whose graph actually launches something, in catalogue order.
///
/// Every graph carries the firing side, but only these expose a launch speed under the sentinel,
/// so only these have anything for the boost to raise. The rest were offering a speed control
/// that wrote nothing: the weapon looked tunable and no number moved. Measured against the clean
/// stock packages, which is what `every_launching_source_is_recorded` re-checks, so add a source
/// here only on the strength of that test rather than on what the weapon looks like in game.
pub(super) const LAUNCHING_SOURCES: &[&str] = &[
    "anarchy-graph",
    "arbalest-graph",
    "bastion-graph",
    "deathbringer-graph",
    "devil-s-ruin-graph",
    "j-tunn-graph",
    "le-monarque-graph",
    "legend-of-acrius-graph",
    "leviathan-s-breath-graph",
    "lord-of-wolves-graph",
    "merciless-graph",
    "skyburner-s-oath-graph",
    "sleeper-simulant-graph",
    "symmetry-graph",
    "telesto-graph",
    "the-colony-graph",
    "the-prospector-graph",
    "the-queenbreaker-graph",
    "the-wardcliff-coil-graph",
    "tractor-cannon-graph",
    // Measured, not assumed: its graph reads a launch speed under the hitscan sentinel, which is
    // why the sidearm belongs here alongside the obvious launchers.
    "travelers-chosen-graph",
    "trinity-ghoul-graph",
    "truth-graph",
    "wish-ender-graph",
    "witherhoard-graph",
];

/// The catalogued sources whose own intrinsic or trait plug changes the barrel's Rounds per
/// Burst, in catalogue order.
///
/// Only these can leave another weapon type firing the wrong burst, or none at all, so only these
/// offer a firing pattern. Measured against the clean stock packages, which is what
/// `every_burst_source_is_recorded` re-checks. The build reads the plugs themselves, so this list
/// decides only where the choice is shown.
pub(super) const BURST_SOURCES: &[&str] = &[
    "graviton-lance",
    "lord-of-wolves",
    "two-tailed-fox",
    "bastion-graph",
    "crimson-graph",
    "devil-s-ruin-graph",
    "graviton-lance-graph",
    "lord-of-wolves-graph",
    "vigilance-wing-graph",
];

/// Whether this behavior's source weapon also switches damage type.
///
/// Hard Light and Borealis each appear twice, once for the graph that carries their firing and
/// once for the record that carries the reload hold. Choosing either half should give both.
#[must_use]
pub fn source_switches_element(entry: &Behavior) -> bool {
    CATALOG
        .iter()
        .any(|other| other.switches_element() && other.source_item_hash == entry.source_item_hash)
}

/// The weapon whose element-switch record is copied into families that have none.
pub(super) fn element_switch_source() -> Option<(u32, u32)> {
    CATALOG
        .iter()
        .find(|entry| entry.switches_element())
        .and_then(|entry| match entry.source {
            BehaviorSource::Record {
                owner_tag,
                content_group,
                ..
            } => Some((owner_tag, content_group)),
            BehaviorSource::State { .. }
            | BehaviorSource::Graph { .. }
            | BehaviorSource::GraphState { .. } => None,
        })
}

/// Graph tags a recipe asks for, so the build can enrol their residency prerequisites.
#[must_use]
pub fn requested_graphs(ids: &[String]) -> Vec<u32> {
    ids.iter()
        .filter_map(|id| behavior(id))
        .filter_map(Behavior::graph_tag)
        .collect()
}
