//! Grafts an exotic behavior record between weapons that share a content component owner.
//!
//! A weapon's behavior lives in its content variant block (class `0x80803ACE`), selected inside a
//! shared per-family owner by the pattern row's content group hash. Each block holds one or more
//! triples of relative pointers: a label array, a state array, and a behavior array. Pointing a
//! target block's state and behavior slots at another block's records transplants the behavior.
//! Confirmed in game across auto rifles, scout rifles, pulse rifles and sniper rifles.
use std::collections::BTreeSet;

use crate::tag_payload::{read_u32 as u32_at, read_u64 as u64_at};
use crate::{AuthoringResult, error::invalid, weapon::WeaponRuntimeResourcePatch};
use sundial::package_authoring::weapon_entity::weapon_component_bindings;
use tiger_pkg::{PackageManager, TagHash};

const BINDING: u32 = 0x5F0D_D954;
const ARRAY_HEADER_CLASS: u32 = 0x8080_9FBD;
const LABEL_ARRAY_CLASS: u32 = 0x8080_94B3;
const STATE_ARRAY_CLASS: u32 = 0x8080_94B0;
const BEHAVIOR_ARRAY_CLASS: u32 = 0x8080_3AD9;
/// The class of a weapon's content variant block.
const VARIANT_BLOCK_CLASS: u32 = 0x8080_3ACE;
/// A triple occupies three consecutive slots of sixteen bytes: label, state, behavior.
const SLOT_STRIDE: usize = 0x10;

/// What a grafted record was observed to do in game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BehaviorEffect {
    /// Hold Reload cycles the damage type, when a trait socket carries The Fundamentals.
    ElementSwitch,
    /// The record grafts cleanly but produced no observed effect on its own.
    NoObservedEffect,
    /// Not yet taken into a session.
    Untested,
}

/// Where a behavior lives, which decides how far it can be grafted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BehaviorSource {
    /// A record inside the content owner, reached by a relative pointer. Only weapons in the same
    /// owner can point at it.
    Record {
        owner_tag: u32,
        /// Locates the source weapon's block inside that owner.
        content_group: u32,
        /// Weapon types the owner covers, used to offer the record only where it can apply.
        family_types: &'static [&'static str],
    },
    /// The weapon's firing and projectile graph, named by tag at block `+0xF0`. A tag reference is
    /// absolute, so any weapon can point at it.
    Graph { tag: u32 },
}

/// One graftable behavior, identified by the weapon whose block carries it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Behavior {
    pub id: &'static str,
    pub name: &'static str,
    pub source_name: &'static str,
    pub source_item_hash: u32,
    pub source: BehaviorSource,
    /// The source weapon's own intrinsic plug, which some behaviors need in a socket to work.
    pub intrinsic_plug: Option<u32>,
    /// The source weapon's exotic trait plug, which usually carries the perk half.
    pub trait_plug: Option<u32>,
    pub effect: BehaviorEffect,
    /// A known limit of this source, shown before the weapon is built.
    pub caution: Option<&'static str>,
    pub summary: &'static str,
}

impl Behavior {
    /// Whether this record drives The Fundamentals.
    #[must_use]
    pub const fn switches_element(&self) -> bool {
        matches!(self.effect, BehaviorEffect::ElementSwitch)
    }

    /// The content owner a record belongs to. Graphs are not owner bound.
    #[must_use]
    pub const fn owner_tag(&self) -> Option<u32> {
        match self.source {
            BehaviorSource::Record { owner_tag, .. } => Some(owner_tag),
            BehaviorSource::Graph { .. } => None,
        }
    }

    /// Whether this source brings projectiles whose launch speed a graft can raise.
    ///
    /// Two things have to hold. Only a graph carries the firing side at all, and only a graph
    /// from a weapon that launches something reads a speed below the hitscan sentinel for the
    /// boost to raise. A graph from a weapon that fires instantly answers no, because raising a
    /// multiplier it does not have writes nothing.
    #[must_use]
    pub fn launches_projectiles(&self) -> bool {
        matches!(self.source, BehaviorSource::Graph { .. }) && LAUNCHING_SOURCES.contains(&self.id)
    }

    /// Whether a weapon of this type can graft this behavior. Graphs reach every weapon.
    #[must_use]
    pub fn reaches_type(&self, type_name: &str) -> bool {
        match self.source {
            BehaviorSource::Record { family_types, .. } => family_types.contains(&type_name),
            BehaviorSource::Graph { .. } => true,
        }
    }
}

/// Every distinct behavior record found in the stock packages.
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
        summary: "Shared with Vigilance Wing and Skyburner's Oath. Untested.",
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
        summary: "Shared with Prometheus Lens. Untested.",
    },
    Behavior {
        id: "symmetry",
        name: "Symmetry Behavior",
        source_name: "Symmetry",
        source_item_hash: 0x9C0A_F31B,
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
        summary: "Shared with Divinity. Untested.",
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
        summary: "Shared with Twilight Oath. Untested.",
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
        summary: "No element switch on a sidearm. Other effects untested.",
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
        intrinsic_plug: None,
        trait_plug: None,
        effect: BehaviorEffect::NoObservedEffect,
        caution: None,
        summary: "No element switch on a rocket launcher. Other effects untested.",
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
        intrinsic_plug: None,
        trait_plug: None,
        effect: BehaviorEffect::NoObservedEffect,
        caution: None,
        summary: "No element switch on a submachine gun. Other effects untested.",
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
        source_item_hash: 0x9BCF6E60,
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
        source_item_hash: 0x7FF347D1,
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
        source: BehaviorSource::Graph { tag: 0x80BBC07C },
        intrinsic_plug: Some(0x1E3A82EC),
        trait_plug: Some(0x8565B49A),
        effect: BehaviorEffect::Untested,
        caution: None,
        summary: "Sturm's own firing and projectile graph.",
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
        source_item_hash: 0x9C0AF31B,
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
        source_item_hash: 0xC07AC8FB,
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
        source_item_hash: 0xC13DCD47,
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
        source_item_hash: 0x965F2337,
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

/// Turns a recipe identifier into a catalogue entry this weapon's family can actually reach.
fn resolve_request(id: &str, owner_tag: u32) -> AuthoringResult<Option<&'static Behavior>> {
    if id == ELEMENT_SWITCH {
        // A family without a record of its own gets one appended instead.
        return Ok(element_switch_for_owner(owner_tag));
    }
    let entry = behavior(id)
        .ok_or_else(|| invalid(format!("Unknown additional weapon behavior \"{id}\"")))?;
    Ok(Some(entry))
}

/// Launch-speed boost applied when a graft gives a weapon projectiles it does not normally fire.
///
/// The private Micro-Missile captures behind `guided::profiles` established that this field is a
/// multiplier on the weapon's own launch speed rather than an absolute velocity, and that the
/// instance and definition copies must agree. The follow-up captures in
/// `docs/private-perk-runtime-loading.md` measured both halves of that product. A Mountaintop
/// launch supplied a launch input of 78.75 against a multiplier of one, and a weapon that fires
/// instantly supplied 1.26 against the same multiplier. Restoring a launching frame's speed on a
/// host that fires instantly therefore takes a multiplier near 62.5, and a grafted round that
/// does not get one crawls.
///
/// Kyle arrived at this figure from in-game testing on 2026-09-17, against sources whose own
/// multiplier reads one, so it is a working default rather than a measured constant, which is why
/// it stays editable per weapon. It is also the most a graft is raised to. A source already
/// reading more than this came from a frame that supplies no launch speed either, so it needs no
/// help, and scaling it as well is what drove every source above 149 into the sentinel.
pub const DEFAULT_PROJECTILE_SPEED_BOOST: f32 = 67.0;

/// The firing graph a weapon's own content block names, if it names one.
fn host_graph(content: &Content, block: usize) -> Option<u32> {
    u32_at(&content.owner, block + GRAPH_OFFSET)
        .ok()
        .filter(|tag| *tag != 0 && *tag != u32::MAX)
}

/// Weapons that fire instantly carry this sentinel where a launch speed would be. Every stock
/// hitscan weapon's graph reads exactly this, and no weapon that launches anything does.
const HITSCAN_SPEED: f32 = 9999.0;

/// The catalogued sources whose graph actually launches something, in catalogue order.
///
/// Every graph carries the firing side, but only these expose a launch speed under the sentinel,
/// so only these have anything for the boost to raise. The rest were offering a speed control
/// that wrote nothing: the weapon looked tunable and no number moved. Measured against the clean
/// stock packages, which is what `every_launching_source_is_recorded` re-checks, so add a source
/// here only on the strength of that test rather than on what the weapon looks like in game.
const LAUNCHING_SOURCES: &[&str] = &[
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
    "trinity-ghoul-graph",
    "truth-graph",
    "wish-ender-graph",
    "witherhoard-graph",
];

/// Values to apply inside a grafted graph's private clone, if it needs any.
///
/// A behavior graph is itself a weapon entity, so both sides are read the same way. The speed a
/// grafted round leaves at is the graph's own multiplier times the launch input its frame
/// supplies, and a weapon that fires instantly supplies almost none, which is why the round
/// crawls. Raising the multiplier in the private clone fixes that without touching the source
/// weapon or the host.
///
/// How much to raise it depends on the graph. Each stock multiplier is paired with the frame it
/// shipped on, so one from a launching frame reads at or below one and one from a frame that
/// fires instantly reads in the hundreds or thousands. Only the first sort loses anything to a
/// graft. So the boost is capped at itself: a graph reading one lands on the boost, a slower
/// graph lands proportionally below it, and a graph already above it keeps the number its own
/// weapon uses.
fn graph_values(
    manager: &PackageManager,
    host_graph: Option<u32>,
    graph: u32,
    boost: f32,
) -> AuthoringResult<Vec<sundial::package_authoring::weapon_runtime::WeaponRuntimeValueOverride>> {
    // One is the neutral multiplier, so anything at or below it asks for no change. A hand-edited
    // recipe carrying NaN stops here too, rather than reaching the clamp, where `f32::min` would
    // quietly turn it into the largest value this can write.
    if boost.is_nan() || boost <= 1.0 {
        return Ok(Vec::new());
    }
    // A weapon that already launches something of its own supplies a real speed, so leave it be.
    if host_graph.is_some_and(|tag| launches_its_own(manager, tag)) {
        return Ok(Vec::new());
    }
    let Ok(payload) = manager.read_tag(TagHash(graph)) else {
        return Ok(Vec::new());
    };
    // Clamp before anything is measured against it, so a hand-typed figure above the sentinel can
    // neither reach hitscan nor lower a graph that already reads more than the clamp allows.
    let boost = boost.min(HITSCAN_SPEED - 1.0);
    let mut draft = Vec::new();
    for parameter in projectile_speeds(manager, graph, &payload)? {
        let base = parameter.original();
        // Scale by the boost, stop at the boost, and never end below what the graph already has.
        // Stopping at the boost is what spares a graph that came from a frame supplying no launch
        // speed of its own, the sentinel included. Ending at the base keeps this sound on a base
        // large enough to overflow the product.
        let raised = (base * boost).min(boost).max(base);
        // Nothing to write when the speed does not move, and no private clone is appended for it.
        if raised <= base {
            continue;
        }
        parameter.set(&mut draft, raised).map_err(invalid)?;
    }
    Ok(draft)
}

/// Whether a weapon's own firing graph launches something rather than hitting instantly.
fn launches_its_own(manager: &PackageManager, graph: u32) -> bool {
    let Ok(payload) = manager.read_tag(TagHash(graph)) else {
        return false;
    };
    projectile_speeds(manager, graph, &payload).is_ok_and(|speeds| {
        speeds
            .iter()
            .any(|parameter| parameter.original() < HITSCAN_SPEED)
    })
}

/// The projectile launch-speed parameters an entity's runtime graph exposes, if any.
fn projectile_speeds(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> AuthoringResult<Vec<sundial::package_authoring::sandbox_perk::projectile::parameters::Parameter>>
{
    use sundial::package_authoring::sandbox_perk::projectile::parameters::{Kind, discover};
    let loaded = sundial::package_authoring::weapon_runtime::load_weapon_runtime_graph_for_entity(
        manager, 0, 0, entity_tag, entity,
    )
    .map_err(invalid)?;
    Ok(discover(&loaded)
        .into_iter()
        .filter(|parameter| parameter.kind == Kind::Speed)
        .collect())
}

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
fn element_switch_source() -> Option<(u32, u32)> {
    CATALOG
        .iter()
        .find(|entry| entry.switches_element())
        .and_then(|entry| match entry.source {
            BehaviorSource::Record {
                owner_tag,
                content_group,
                ..
            } => Some((owner_tag, content_group)),
            BehaviorSource::Graph { .. } => None,
        })
}

/// The variant block a content group selects, found by scanning an owner payload directly.
///
/// Used for a source owner, where there is no entity to resolve a binding through.
fn block_in_owner(owner: &[u8], group: u32) -> Option<usize> {
    let mut at = 0;
    while at + 0x20 <= owner.len() {
        if u32_at(owner, at + 4).ok()? == VARIANT_BLOCK_CLASS
            && u32_at(owner, at + 0x10).ok()? == group
        {
            return Some(at);
        }
        at += 4;
    }
    None
}

/// One weapon's state array and behavior record, copied out of its owner.
///
/// Every reference inside is a class word, an inline value or an absolute tag, so the run is
/// position independent and can be appended to another owner. The run ends at the next array
/// marker, which is how its length is known without a schema for the row type.
pub(crate) struct Record {
    pub(crate) bytes: Vec<u8>,
    pub(crate) state_offset: usize,
    pub(crate) behavior_offset: usize,
}

fn extract_record(owner: &[u8], group: u32) -> AuthoringResult<Record> {
    let block =
        block_in_owner(owner, group).ok_or_else(|| invalid("Behavior source block is missing"))?;
    let triple = first_triple(owner, block)?;
    let state = slot_target(owner, triple + SLOT_STRIDE)
        .ok_or_else(|| invalid("Behavior source has no state array"))?;
    let behavior = slot_target(owner, triple + SLOT_STRIDE * 2)
        .ok_or_else(|| invalid("Behavior source has no behavior record"))?;
    if u32_at(owner, behavior + 8)? != BEHAVIOR_ARRAY_CLASS {
        return Err(invalid("Behavior source record has an unexpected class"));
    }
    let start = state
        .checked_sub(4)
        .ok_or_else(|| invalid("Behavior source array marker is missing"))?;
    let rows = behavior
        .checked_add(16)
        .ok_or_else(|| invalid("Behavior source record overflows"))?;
    let end = (rows..owner.len().saturating_sub(4))
        .step_by(4)
        .find(|at| u32_at(owner, *at).is_ok_and(|word| word == ARRAY_HEADER_CLASS))
        .ok_or_else(|| invalid("Behavior source record has no end marker"))?;
    if end <= behavior || start >= state {
        return Err(invalid("Behavior source record has an unexpected layout"));
    }
    Ok(Record {
        bytes: owner[start..end].to_vec(),
        state_offset: state - start,
        behavior_offset: behavior - start,
    })
}

/// What a set of behavior grafts compiles into.
#[derive(Debug, Default)]
pub(crate) struct Grafted {
    pub(crate) patches: Vec<WeaponRuntimeResourcePatch>,
    pub(crate) appends: Vec<crate::weapon::WeaponRuntimeResourceAppend>,
}

/// The resolved content component of one weapon entity.
pub(crate) struct Content {
    pub(crate) owner_tag: u32,
    pub(crate) owner: Vec<u8>,
    /// Offset of the resource inside the owner payload; patch offsets are relative to it.
    pub(crate) resource: usize,
    /// Every variant block in the owner, in payload order.
    pub(crate) blocks: Vec<usize>,
}

/// Resolves the shared content owner and its variant blocks for one weapon entity.
pub(crate) fn content(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Content> {
    let bindings = weapon_component_bindings(entity, BINDING).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "Behavior grafting requires one weapon-content component",
        ));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Weapon-content resource offset overflow"))?;
    let definition = usize::try_from(u64_at(&owner, resource + 8)?)
        .map_err(|_| invalid("Weapon-content definition offset overflow"))?;
    let blocks = crate::weapon_ammo::property_offsets(&owner, definition)?;
    Ok(Content {
        owner_tag: binding.owner_tag,
        owner,
        resource,
        blocks,
    })
}

/// The variant block a content group selects, falling back to the first block.
pub(crate) fn block_for_group(content: &Content, group: u32) -> AuthoringResult<usize> {
    let first = *content
        .blocks
        .first()
        .ok_or_else(|| invalid("Weapon content owner has no variant block"))?;
    for &block in &content.blocks {
        if u32_at(&content.owner, block + 0x10)? == group {
            return Ok(block);
        }
    }
    Ok(first)
}

/// Follows one slot's relative pointer and returns the class of the array it reaches.
fn slot_class(owner: &[u8], slot: usize) -> Option<u32> {
    let target = slot_target(owner, slot)?;
    if u32_at(owner, target.checked_sub(4)?).ok()? != ARRAY_HEADER_CLASS {
        return None;
    }
    u32_at(owner, target + 8).ok()
}

/// Absolute offset a slot's self-relative pointer reaches.
fn slot_target(owner: &[u8], slot: usize) -> Option<usize> {
    let relative = i64::from_le_bytes(owner.get(slot..slot + 8)?.try_into().ok()?);
    if relative == 0 {
        return None;
    }
    slot.checked_add_signed(isize::try_from(relative).ok()?)
}

/// Offset of the label slot of the first label, state and behavior triple in one block.
///
/// Block layout is not fixed. A block is a run of sub-blocks and larger weapons repeat the triple,
/// so the slots are found by following pointers rather than by a hard-coded offset.
pub(crate) fn first_triple(owner: &[u8], block: usize) -> AuthoringResult<usize> {
    let size = usize::try_from(u32_at(owner, block + 8)?)
        .map_err(|_| invalid("Weapon variant block size overflow"))?;
    let end = block
        .checked_add(size)
        .filter(|end| *end <= owner.len())
        .ok_or_else(|| invalid("Weapon variant block is truncated"))?;
    let mut slot = block + 0x20;
    while slot + SLOT_STRIDE * 3 <= end {
        if slot_class(owner, slot) == Some(LABEL_ARRAY_CLASS)
            && slot_class(owner, slot + SLOT_STRIDE) == Some(STATE_ARRAY_CLASS)
        {
            return Ok(slot);
        }
        slot += 8;
    }
    Err(invalid(
        "Weapon variant block has no behavior slots to graft onto",
    ))
}

/// The source records a behavior graft copies.
pub(crate) struct Graft {
    pub(crate) state_target: usize,
    pub(crate) behavior_target: usize,
}

/// Locates the state and behavior records the source weapon's block points at.
pub(crate) fn resolve(content: &Content, source_group: u32) -> AuthoringResult<Graft> {
    let block = block_for_group(content, source_group)?;
    if u32_at(&content.owner, block + 0x10)? != source_group {
        return Err(invalid(
            "The behavior source is not present in this weapon's content owner",
        ));
    }
    let triple = first_triple(&content.owner, block)?;
    let state_target = slot_target(&content.owner, triple + SLOT_STRIDE)
        .ok_or_else(|| invalid("Behavior source has no state array"))?;
    let behavior_target = slot_target(&content.owner, triple + SLOT_STRIDE * 2)
        .ok_or_else(|| invalid("Behavior source has no behavior record"))?;
    if u32_at(&content.owner, behavior_target + 8)? != BEHAVIOR_ARRAY_CLASS {
        return Err(invalid("Behavior source record has an unexpected class"));
    }
    Ok(Graft {
        state_target,
        behavior_target,
    })
}

/// The block field naming the weapon's firing and projectile graph.
const GRAPH_OFFSET: usize = 0xF0;

/// Turns an owner offset into the resource-relative offset a runtime patch carries.
fn slot_offset(slot: usize, resource: usize) -> AuthoringResult<u32> {
    slot.checked_sub(resource)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| invalid("Behavior graft offset overflow"))
}

/// Graph tags a recipe asks for, so the build can enrol their residency prerequisites.
#[must_use]
pub fn requested_graphs(ids: &[String]) -> Vec<u32> {
    ids.iter()
        .filter_map(|id| behavior(id))
        .filter_map(|entry| match entry.source {
            BehaviorSource::Graph { tag } => Some(tag),
            BehaviorSource::Record { .. } => None,
        })
        .collect()
}

/// Eight bytes of self-relative pointer followed by eight bytes of count.
fn slot_bytes(target: usize, slot: usize, count: i64) -> AuthoringResult<Vec<u8>> {
    let target = i64::try_from(target).map_err(|_| invalid("Behavior graft pointer overflow"))?;
    let slot = i64::try_from(slot).map_err(|_| invalid("Behavior graft pointer overflow"))?;
    let mut bytes = (target - slot).to_le_bytes().to_vec();
    bytes.extend_from_slice(&count.to_le_bytes());
    Ok(bytes)
}

/// Builds the runtime patches that graft each behavior onto the weapon's own variant block.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    target_group: Option<u32>,
    requested: &[String],
    speed_boost: f32,
) -> AuthoringResult<Grafted> {
    if requested.is_empty() {
        return Ok(Grafted::default());
    }
    let content = content(manager, entity)?;
    let behaviors = requested
        .iter()
        .map(|id| resolve_request(id, content.owner_tag))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let mut appends = Vec::new();
    let block = match target_group {
        Some(group) => block_for_group(&content, group)?,
        None => *content
            .blocks
            .first()
            .ok_or_else(|| invalid("Weapon content owner has no variant block"))?,
    };
    let mut patches = Vec::with_capacity(behaviors.len() * 2);
    for entry in behaviors {
        // A record this family does not hold is copied in from the weapon that does.
        let foreign = match entry {
            None => element_switch_source(),
            Some(entry) => match entry.source {
                BehaviorSource::Record {
                    owner_tag,
                    content_group,
                    ..
                } if owner_tag != content.owner_tag => Some((owner_tag, content_group)),
                _ => None,
            },
        };
        if let Some((owner_tag, group)) = foreign {
            let source = manager
                .read_tag(TagHash(owner_tag))
                .map_err(|error| invalid(error.to_string()))?;
            let record = extract_record(&source, group)?;
            let triple = first_triple(&content.owner, block)?;
            appends.push(crate::weapon::WeaponRuntimeResourceAppend {
                binding_hash: BINDING,
                resource_index: 0,
                bytes: record.bytes,
                slots: vec![
                    (
                        slot_offset(triple + SLOT_STRIDE, content.resource)?,
                        record.state_offset,
                        1,
                    ),
                    (
                        slot_offset(triple + SLOT_STRIDE * 2, content.resource)?,
                        record.behavior_offset,
                        0,
                    ),
                ],
            });
            continue;
        }
        let Some(entry) = entry else {
            return Err(invalid("No element-switch record is available to graft."));
        };
        match entry.source {
            BehaviorSource::Graph { tag } => {
                // The graph is named by tag at block +0xF0, so the patch is the tag itself.
                patches.push(WeaponRuntimeResourcePatch {
                    binding_hash: BINDING,
                    resource_index: 0,
                    offset: slot_offset(block + GRAPH_OFFSET, content.resource)?,
                    bytes: tag.to_le_bytes().to_vec(),
                    graph_values: graph_values(
                        manager,
                        host_graph(&content, block),
                        tag,
                        speed_boost,
                    )?,
                });
            }
            BehaviorSource::Record { content_group, .. } => {
                let triple = first_triple(&content.owner, block)?;
                let graft = resolve(&content, content_group)?;
                for (slot, target, count) in [
                    (triple + SLOT_STRIDE, graft.state_target, 1),
                    (triple + SLOT_STRIDE * 2, graft.behavior_target, 0),
                ] {
                    patches.push(WeaponRuntimeResourcePatch {
                        binding_hash: BINDING,
                        resource_index: 0,
                        offset: slot_offset(slot, content.resource)?,
                        bytes: slot_bytes(target, slot, count)?,
                        graph_values: Vec::new(),
                    });
                }
            }
        }
    }
    Ok(Grafted { patches, appends })
}

/// Socket type of a weapon's intrinsic frame.
pub(crate) const INTRINSIC_SOCKET_TYPE: u16 = 176;
/// Socket type of a weapon's trait columns.
pub(crate) const TRAIT_SOCKET_TYPE: u16 = 92;

/// The role every lane actually has, which is not always the role the donor shipped.
///
/// An author can turn one of the donor's sockets into a trait column, and can append columns the
/// donor never had. The socket list draws both as trait sockets, so anything choosing a lane has
/// to read them the same way or it lands somewhere the author was never shown.
pub(crate) fn effective_socket_types(
    authored_roles: &[Option<u16>],
    donor_socket_types: &[u16],
) -> Vec<u16> {
    (0..donor_socket_types.len().max(authored_roles.len()))
        .map(|lane| {
            authored_roles
                .get(lane)
                .copied()
                .flatten()
                .or_else(|| donor_socket_types.get(lane).copied())
                .unwrap_or(u16::MAX)
        })
        .collect()
}

/// Every plug the chosen behaviors own, whether or not a pin was needed to place it.
///
/// A lane is only cleared of a perk no behavior wants any more, so this has to name the perks
/// that are still wanted even when they already sit where the author put them.
pub(crate) fn claimed_plugs<'a>(
    behaviors: impl IntoIterator<Item = &'a str>,
    skip_behavior_perks: bool,
) -> BTreeSet<u32> {
    if skip_behavior_perks {
        return BTreeSet::new();
    }
    behaviors
        .into_iter()
        .filter_map(behavior)
        .flat_map(|entry| [entry.intrinsic_plug, entry.trait_plug])
        .flatten()
        .collect()
}

/// The socket lanes each grafted behavior claims, and the plug it puts first in them.
///
/// The editor writes these as soon as a behavior is chosen and the build writes them again, so
/// both read the lanes from here rather than each deciding for itself which socket a behavior
/// takes. A behavior whose lane the weapon does not have is skipped.
///
/// `socket_types` is every lane's effective role, so a column the author turned into a trait
/// socket counts as one of them. `placed` is what each lane already holds: a perk the author put
/// in a socket of their own already satisfies the behavior, and pinning a second copy into the
/// donor's own trait lane would show the same perk twice and reshuffle a list they arranged.
pub(crate) fn socket_pins<'a>(
    behaviors: impl IntoIterator<Item = &'a str>,
    skip_behavior_perks: bool,
    socket_types: &[u16],
    placed: &[Vec<u32>],
) -> Vec<(usize, u32)> {
    if skip_behavior_perks {
        return Vec::new();
    }
    let lanes = |kind: u16| {
        socket_types
            .iter()
            .enumerate()
            .filter(move |(_, socket_type)| **socket_type == kind)
            .map(|(lane, _)| lane)
    };
    let holds = |lane: usize, plug: u32| {
        placed
            .get(lane)
            .is_some_and(|choices| choices.contains(&plug))
    };
    // Each behavior takes a fresh trait lane, so two of them do not land on one socket. A lane
    // already holding one of their perks counts as spoken for.
    let mut taken = BTreeSet::new();
    let mut pins = Vec::new();
    for entry in behaviors.into_iter().filter_map(behavior) {
        for (plug, kind) in [
            (entry.intrinsic_plug, INTRINSIC_SOCKET_TYPE),
            (entry.trait_plug, TRAIT_SOCKET_TYPE),
        ] {
            let Some(plug) = plug else { continue };
            if let Some(lane) = lanes(kind).find(|lane| holds(*lane, plug)) {
                taken.insert(lane);
                continue;
            }
            // The intrinsic column is single, so behaviors share it as they always have.
            let free = if kind == INTRINSIC_SOCKET_TYPE {
                lanes(kind).next()
            } else {
                lanes(kind).find(|lane| !taken.contains(lane))
            };
            let Some(lane) = free else { continue };
            taken.insert(lane);
            pins.push((lane, plug));
        }
    }
    pins
}

/// The role the recipe gives each lane, empty where it leaves the donor's own.
pub(crate) fn authored_socket_roles(
    overrides: &crate::weapon::WeaponCloneOverrides,
) -> Vec<Option<u16>> {
    overrides
        .socket_columns
        .iter()
        .map(|column| column.as_ref().and_then(|column| column.socket_type))
        .collect()
}

/// The plugs the recipe puts in each lane, empty where it inherits the donor's own.
pub(crate) fn authored_socket_choices(
    overrides: &crate::weapon::WeaponCloneOverrides,
) -> Vec<Vec<u32>> {
    overrides
        .socket_columns
        .iter()
        .map(|column| {
            column
                .as_ref()
                .map(|column| column.choices.clone())
                .unwrap_or_default()
        })
        .collect()
}

/// Pins each grafted behavior's own plugs into the weapon's intrinsic and trait sockets.
///
/// A graph moves the firing and projectile side, but several exotics keep half of the behavior in
/// their perk, so the plugs travel with the graft unless the author turned them off. The plug
/// leads the socket rather than emptying it, so an author's own choices stay behind it.
pub(crate) fn expand_socket_columns(
    overrides: &crate::weapon::WeaponCloneOverrides,
    socket_types: &[u16],
) -> crate::AuthoringResult<crate::weapon::WeaponCloneOverrides> {
    use crate::weapon::WeaponSocketColumnOverride;
    let pins = socket_pins(
        overrides.additional_behaviors.iter().map(String::as_str),
        overrides.skip_behavior_perks,
        &effective_socket_types(&authored_socket_roles(overrides), socket_types),
        &authored_socket_choices(overrides),
    );
    if pins.is_empty() {
        return Ok(overrides.clone());
    }
    let mut expanded = overrides.clone();
    if expanded.socket_columns.is_empty() {
        expanded.socket_columns = vec![None; socket_types.len()];
    }
    if expanded.socket_columns.len() < socket_types.len() {
        return Err(invalid(format!(
            "The recipe lists {} socket columns but the base weapon has {} sockets.",
            expanded.socket_columns.len(),
            socket_types.len()
        )));
    }
    for (lane, plug) in pins {
        let column =
            expanded.socket_columns[lane].get_or_insert_with(|| WeaponSocketColumnOverride {
                choices: Vec::new(),
                socket_type: None,
                choice_weight_bits: Vec::new(),
                choice_conditions: Vec::new(),
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
                randomized_selection_program: Vec::new(),
            });
        column.choices.retain(|choice| *choice != plug);
        column.choices.insert(0, plug);
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a synthetic owner holding one block with a label, state and behavior triple.
    fn owner_with_triple(behavior_record: bool) -> (Vec<u8>, usize) {
        let mut owner = vec![0_u8; 0x400];
        let block = 0x40;
        owner[block + 8..block + 12].copy_from_slice(&0x1C0_u32.to_le_bytes());
        owner[block + 0x10..block + 0x14].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
        for (at, class) in [
            (0x200_usize, LABEL_ARRAY_CLASS),
            (0x240, STATE_ARRAY_CLASS),
            (0x280, BEHAVIOR_ARRAY_CLASS),
        ] {
            owner[at - 4..at].copy_from_slice(&ARRAY_HEADER_CLASS.to_le_bytes());
            owner[at + 8..at + 12].copy_from_slice(&class.to_le_bytes());
        }
        let triple = block + 0x170;
        for (slot, target, present) in [
            (triple, 0x200_usize, true),
            (triple + SLOT_STRIDE, 0x240, true),
            (triple + SLOT_STRIDE * 2, 0x280, behavior_record),
        ] {
            if present {
                let relative = target as i64 - slot as i64;
                owner[slot..slot + 8].copy_from_slice(&relative.to_le_bytes());
            }
        }
        (owner, block)
    }

    fn content_for(owner: Vec<u8>, block: usize) -> Content {
        Content {
            owner_tag: 0x8152_9461,
            owner,
            resource: 0,
            blocks: vec![block],
        }
    }

    #[test]
    fn triple_is_found_by_following_pointers_not_by_offset() {
        let (owner, block) = owner_with_triple(true);
        assert_eq!(first_triple(&owner, block).unwrap(), block + 0x170);
    }

    #[test]
    fn a_block_without_a_state_array_is_rejected() {
        let mut owner = vec![0_u8; 0x400];
        owner[8..12].copy_from_slice(&0x1C0_u32.to_le_bytes());
        assert!(first_triple(&owner, 0).is_err());
    }

    #[test]
    fn slot_bytes_carry_a_self_relative_pointer_and_count() {
        let bytes = slot_bytes(0x280, 0x1B0, 1).unwrap();
        assert_eq!(i64::from_le_bytes(bytes[..8].try_into().unwrap()), 0xD0);
        assert_eq!(i64::from_le_bytes(bytes[8..].try_into().unwrap()), 1);
    }

    #[test]
    fn resolve_reads_both_source_records() {
        let (owner, block) = owner_with_triple(true);
        let graft = resolve(&content_for(owner, block), 0xAABB_CCDD).unwrap();
        assert_eq!(graft.state_target, 0x240);
        assert_eq!(graft.behavior_target, 0x280);
    }

    #[test]
    fn resolve_rejects_a_source_without_a_behavior_record() {
        let (owner, block) = owner_with_triple(false);
        assert!(resolve(&content_for(owner, block), 0xAABB_CCDD).is_err());
    }

    #[test]
    fn resolve_rejects_a_content_group_the_owner_does_not_hold() {
        let (owner, block) = owner_with_triple(true);
        assert!(resolve(&content_for(owner, block), 0x1234_5678).is_err());
    }

    /// Pins the offsets confirmed in game, so a change to the reader cannot silently move them.
    /// Hard Light and SUROS Regime share the auto rifle content owner.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn the_element_switch_graft_matches_the_offsets_verified_in_game() {
        use sundial::package_authoring::open_shadowkeep_package_manager;
        const AUTO_RIFLE_OWNER: u32 = 0x8152_9461;
        const SUROS_GROUP: u32 = 0xA581_883B;
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&path).unwrap();
        let owner = manager.read_tag(TagHash(AUTO_RIFLE_OWNER)).unwrap();
        let resource = 0x80_usize;
        let definition = usize::try_from(u64_at(&owner, resource + 8).unwrap()).unwrap();
        let blocks = crate::weapon_ammo::property_offsets(&owner, definition).unwrap();
        let content = Content {
            owner_tag: AUTO_RIFLE_OWNER,
            owner,
            resource,
            blocks,
        };

        // Hard Light's block holds the records at the offsets the in-game trials used.
        let source = CATALOG[0];
        assert_eq!(source.id, "hard-light");
        let BehaviorSource::Record { content_group, .. } = source.source else {
            panic!("Hard Light's element switch is a record");
        };
        let graft = resolve(&content, content_group).unwrap();
        assert_eq!(graft.state_target, 0xC520);
        assert_eq!(graft.behavior_target, 0xC540);

        // SUROS Regime's block exposes its triple where the verified patches wrote.
        let target = block_for_group(&content, SUROS_GROUP).unwrap();
        assert_eq!(target, 0x1620);
        let triple = first_triple(&content.owner, target).unwrap();
        assert_eq!(triple, target + 0x170);
        assert_eq!(triple + SLOT_STRIDE - content.resource, 0x1720);
        assert_eq!(triple + SLOT_STRIDE * 2 - content.resource, 0x1730);
        assert_eq!(
            slot_bytes(graft.state_target, triple + SLOT_STRIDE, 1).unwrap(),
            hex_bytes("80AD0000000000000100000000000000")
        );
        assert_eq!(
            slot_bytes(graft.behavior_target, triple + SLOT_STRIDE * 2, 0).unwrap(),
            hex_bytes("90AD0000000000000000000000000000")
        );
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn a_graph_behavior_writes_the_tag_at_the_graph_slot() {
        let (owner, block) = owner_with_triple(true);
        let content = content_for(owner, block);
        let entry = behavior("malfeasance-graph").expect("Malfeasance carries a graph");
        let BehaviorSource::Graph { tag } = entry.source else {
            panic!("Malfeasance's half is a graph");
        };
        assert_eq!(tag, 0x80EF_28E0);
        assert_eq!(
            slot_offset(block + GRAPH_OFFSET, content.resource).unwrap(),
            u32::try_from(block + GRAPH_OFFSET).unwrap()
        );
    }

    #[test]
    fn graph_behaviors_reach_every_weapon_and_records_do_not() {
        let graphs = CATALOG
            .iter()
            .filter(|entry| matches!(entry.source, BehaviorSource::Graph { .. }))
            .count();
        assert!(
            graphs >= 50,
            "the catalogue should carry every exotic graph"
        );
        // A graph is a tag reference, so no owner bounds it.
        assert!(
            CATALOG
                .iter()
                .filter(|entry| matches!(entry.source, BehaviorSource::Graph { .. }))
                .all(|entry| entry.owner_tag().is_none() && entry.reaches_type("Sword"))
        );
        // Records stay inside their family.
        assert!(
            CATALOG
                .iter()
                .filter(|entry| matches!(entry.source, BehaviorSource::Record { .. }))
                .all(|entry| entry.owner_tag().is_some())
        );
    }

    #[test]
    fn requested_graphs_lists_only_graph_behaviors() {
        let ids = [
            "malfeasance-graph".to_owned(),
            ELEMENT_SWITCH.to_owned(),
            "hard-light".to_owned(),
            "nope".to_owned(),
        ];
        assert_eq!(requested_graphs(&ids), vec![0x80EF_28E0]);
    }

    fn overrides_for(id: &str) -> crate::weapon::WeaponCloneOverrides {
        crate::weapon::WeaponCloneOverrides {
            additional_behaviors: vec![id.to_owned()],
            ..Default::default()
        }
    }

    #[test]
    fn a_graft_pins_the_source_weapons_own_intrinsic_and_trait() {
        let types = [
            INTRINSIC_SOCKET_TYPE,
            65,
            177,
            TRAIT_SOCKET_TYPE,
            TRAIT_SOCKET_TYPE,
        ];
        let expanded = expand_socket_columns(&overrides_for("malfeasance-graph"), &types).unwrap();
        let entry = behavior("malfeasance-graph").unwrap();
        assert_eq!(
            expanded.socket_columns[0]
                .as_ref()
                .map(|c| c.choices.clone()),
            Some(vec![entry.intrinsic_plug.unwrap()])
        );
        assert_eq!(
            expanded.socket_columns[3]
                .as_ref()
                .map(|c| c.choices.clone()),
            Some(vec![entry.trait_plug.unwrap()])
        );
        // The second trait column is left for the author.
        assert!(expanded.socket_columns[4].is_none());
    }

    #[test]
    fn the_author_can_keep_the_graft_without_its_perks() {
        let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
        let mut overrides = overrides_for("malfeasance-graph");
        overrides.skip_behavior_perks = true;
        let expanded = expand_socket_columns(&overrides, &types).unwrap();
        assert!(expanded.socket_columns.is_empty());
    }

    #[test]
    fn a_behavior_leads_a_socket_the_author_already_chose_for() {
        let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
        let mut overrides = overrides_for("malfeasance-graph");
        overrides.socket_columns = vec![
            Some(crate::weapon::WeaponSocketColumnOverride {
                choices: vec![0x1234_5678],
                socket_type: None,
                choice_weight_bits: Vec::new(),
                choice_conditions: Vec::new(),
                reusable_plug_set_index: None,
                randomized_plug_set_index: None,
                randomized_selection_program: Vec::new(),
            }),
            None,
        ];
        // The author's own choice is kept, behind the perk the behavior needs first.
        let expanded = expand_socket_columns(&overrides, &types).unwrap();
        let intrinsic = behavior("malfeasance-graph")
            .and_then(|entry| entry.intrinsic_plug)
            .expect("the graft names an intrinsic plug");
        assert_eq!(
            expanded.socket_columns[0]
                .as_ref()
                .map(|column| column.choices.as_slice()),
            Some([intrinsic, 0x1234_5678].as_slice())
        );
    }

    fn column(
        choices: Vec<u32>,
        socket_type: Option<u16>,
    ) -> crate::weapon::WeaponSocketColumnOverride {
        crate::weapon::WeaponSocketColumnOverride {
            choices,
            socket_type,
            choice_weight_bits: Vec::new(),
            choice_conditions: Vec::new(),
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
            randomized_selection_program: Vec::new(),
        }
    }

    /// An author who gives the behavior's perk a socket of their own has already satisfied it.
    /// Leading the donor's own trait column with a second copy showed the same perk twice and
    /// pushed the choices they had arranged down a place.
    #[test]
    fn a_perk_the_author_already_placed_is_not_pinned_a_second_time() {
        let types = [INTRINSIC_SOCKET_TYPE, TRAIT_SOCKET_TYPE];
        let entry = behavior("malfeasance-graph").unwrap();
        let mut overrides = overrides_for("malfeasance-graph");
        overrides.socket_columns = vec![
            None,
            Some(column(vec![0x1234_5678], None)),
            Some(column(
                vec![entry.trait_plug.unwrap()],
                Some(TRAIT_SOCKET_TYPE),
            )),
        ];
        let expanded = expand_socket_columns(&overrides, &types).unwrap();
        assert_eq!(expanded.socket_columns[1], overrides.socket_columns[1]);
        assert_eq!(expanded.socket_columns[2], overrides.socket_columns[2]);
    }

    /// The socket list draws a column the author gave the trait role as a trait socket, so the
    /// build has to pin into it too. Reading only the donor's own types skipped it entirely.
    #[test]
    fn a_socket_the_author_turned_into_a_trait_column_is_where_the_perk_lands() {
        let types = [INTRINSIC_SOCKET_TYPE, u16::MAX];
        let entry = behavior("malfeasance-graph").unwrap();
        let mut overrides = overrides_for("malfeasance-graph");
        overrides.socket_columns = vec![
            None,
            Some(column(vec![0x1234_5678], Some(TRAIT_SOCKET_TYPE))),
        ];
        let expanded = expand_socket_columns(&overrides, &types).unwrap();
        assert_eq!(
            expanded.socket_columns[1]
                .as_ref()
                .map(|column| column.choices.as_slice()),
            Some([entry.trait_plug.unwrap(), 0x1234_5678].as_slice())
        );
    }

    #[test]
    fn every_graph_behavior_names_the_plugs_its_perk_half_needs() {
        let missing = CATALOG
            .iter()
            .filter(|entry| matches!(entry.source, BehaviorSource::Graph { .. }))
            .filter(|entry| entry.intrinsic_plug.is_none())
            .map(|entry| entry.source_name)
            .collect::<Vec<_>>();
        assert!(missing.is_empty(), "no intrinsic recorded for {missing:?}");
    }

    #[test]
    fn a_family_without_a_record_falls_back_to_the_weapon_that_has_one() {
        // No hand cannon owner holds an element switch, so the request resolves to nothing and the
        // compiler copies Hard Light's record in instead.
        assert!(
            resolve_request(ELEMENT_SWITCH, 0x8152_905A)
                .unwrap()
                .is_none()
        );
        assert_eq!(element_switch_source(), Some((0x8152_9461, 0x8F15_AF33)));
        // A record named directly is no longer refused outside its family either.
        assert_eq!(
            resolve_request("tarrabah", 0x8152_905A)
                .unwrap()
                .map(|entry| entry.id),
            Some("tarrabah")
        );
    }

    /// Every record is copied out of its own owner, so the extents have to be readable.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn every_behavior_record_extracts_from_its_owner() {
        use sundial::package_authoring::open_shadowkeep_package_manager;
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&path).unwrap();
        let mut checked = 0;
        for entry in CATALOG {
            let BehaviorSource::Record {
                owner_tag,
                content_group,
                ..
            } = entry.source
            else {
                continue;
            };
            let owner = manager.read_tag(TagHash(owner_tag)).unwrap();
            let record = extract_record(&owner, content_group)
                .unwrap_or_else(|error| panic!("{}: {error:?}", entry.source_name));
            assert_eq!(record.state_offset, 4, "{}", entry.source_name);
            assert_eq!(record.behavior_offset, 0x24, "{}", entry.source_name);
            assert!(
                (96..=112).contains(&record.bytes.len()),
                "{} record is {} bytes",
                entry.source_name,
                record.bytes.len()
            );
            // The copy names its own arrays, so it stands alone in another owner.
            assert_eq!(
                u32_at(&record.bytes, record.state_offset + 8).unwrap(),
                STATE_ARRAY_CLASS
            );
            assert_eq!(
                u32_at(&record.bytes, record.behavior_offset + 8).unwrap(),
                BEHAVIOR_ARRAY_CLASS
            );
            checked += 1;
        }
        assert_eq!(checked, 10, "every record in the catalogue should extract");
    }

    /// Only a graph source can hand a weapon projectiles, and the recorded list has to name
    /// sources that exist.
    #[test]
    fn only_graph_sources_launch_projectiles() {
        for entry in CATALOG {
            assert!(
                !entry.launches_projectiles()
                    || matches!(entry.source, BehaviorSource::Graph { .. }),
                "{} is not a graph source",
                entry.id
            );
        }
        for id in LAUNCHING_SOURCES {
            let entry = behavior(id).unwrap_or_else(|| panic!("{id} is not in the catalogue"));
            assert!(entry.launches_projectiles(), "{id}");
        }
    }

    /// The recorded list is a measurement, not a judgement about how a weapon looks in game, so
    /// it is checked against the packages it was taken from. Hard Light fires visible bouncing
    /// rounds and still exposes no launch speed to raise, which is exactly the sort of guess this
    /// catches.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn every_launching_source_is_recorded() {
        use sundial::package_authoring::open_shadowkeep_package_manager;
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&path).unwrap();
        let measured = CATALOG
            .iter()
            .filter(|entry| match entry.source {
                BehaviorSource::Graph { tag } => launches_its_own(&manager, tag),
                BehaviorSource::Record { .. } => false,
            })
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(measured, LAUNCHING_SOURCES);
    }

    /// The boost only fires when the graft launches something the host cannot speed up itself.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn the_speed_boost_scales_a_launching_graft_and_leaves_every_other_case_alone() {
        use sundial::package_authoring::{
            open_shadowkeep_package_manager,
            weapon_runtime::load_weapon_runtime_entity_at_pattern_index_with_manager,
        };
        let path =
            std::path::PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let manager = open_shadowkeep_package_manager(&path).unwrap();
        let launching = behavior("anarchy-graph").expect("Anarchy launches grenades");
        let BehaviorSource::Graph { tag: launching } = launching.source else {
            panic!("Anarchy's half is a graph");
        };
        let instant = behavior("malfeasance-graph").expect("Malfeasance hits instantly");
        let BehaviorSource::Graph { tag: instant } = instant.source else {
            panic!("Malfeasance's half is a graph");
        };

        // Find one host of each sort by reading the graph each weapon's block names.
        let mut hitscan = None;
        let mut launcher = None;
        for index in 0..3000_u16 {
            if hitscan.is_some() && launcher.is_some() {
                break;
            }
            let Ok(source) =
                load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, index)
            else {
                continue;
            };
            let Ok(content) = content(&manager, &source.payload) else {
                continue;
            };
            let Ok(block) = block_for_group(&content, source.weapon_content_group_hash) else {
                continue;
            };
            let Some(graph) = host_graph(&content, block) else {
                continue;
            };
            if launches_its_own(&manager, graph) {
                launcher.get_or_insert(graph);
            } else {
                hitscan.get_or_insert(graph);
            }
        }
        let hitscan = hitscan.expect("some weapon fires instantly");
        let launcher = launcher.expect("some weapon launches its own");

        // A launching graft on a weapon that supplies no speed of its own is raised. Each
        // parameter writes two lanes, the instance and the definition.
        let values = graph_values(&manager, Some(hitscan), launching, 4.0).unwrap();
        assert_eq!(values.len(), 2);
        // A weapon with no firing graph at all is treated the same way.
        assert_eq!(
            graph_values(&manager, None, launching, 4.0).unwrap().len(),
            2
        );
        // A boost of one changes nothing, so no private clone is made.
        assert!(
            graph_values(&manager, Some(hitscan), launching, 1.0)
                .unwrap()
                .is_empty()
        );
        // A host that already launches its own supplies a real speed.
        assert!(
            graph_values(&manager, Some(launcher), launching, 4.0)
                .unwrap()
                .is_empty()
        );
        // A graft that also fires instantly has no launch speed worth raising.
        assert!(
            graph_values(&manager, Some(hitscan), instant, 4.0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn catalogue_ids_are_unique_and_the_element_switch_resolves_per_owner() {
        let mut ids = CATALOG.iter().map(|entry| entry.id).collect::<Vec<_>>();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert_eq!(
            element_switch_for_owner(0x8152_9461).map(|entry| entry.id),
            Some("hard-light")
        );
        assert_eq!(
            element_switch_for_owner(0x8152_BC09).map(|entry| entry.id),
            Some("borealis")
        );
        assert!(element_switch_for_owner(0x8152_905A).is_none());
        assert_eq!(catalog_for_owner(0x8152_C7C5).count(), 1);
        assert_eq!(
            resolve_request(ELEMENT_SWITCH, 0x8152_BC09)
                .unwrap()
                .unwrap()
                .id,
            "borealis"
        );
        assert!(
            resolve_request(ELEMENT_SWITCH, 0x8152_905A)
                .unwrap()
                .is_none()
        );
        // A record from another family is copied in rather than refused.
        assert!(
            resolve_request("hard-light", 0x8152_BC09)
                .unwrap()
                .is_some()
        );
        assert!(resolve_request("nope", 0x8152_9461).is_err());
    }

    /// Every family reaches the catalogue, whether or not it holds a record of its own.
    #[test]
    fn every_weapon_family_can_reach_the_catalogue_and_switch_damage() {
        // A family without a record of its own is given one, so all of them switch.
        assert!(switches_element_for_type("Scout Rifle"));
        assert!(switches_element_for_type("Sniper Rifle"));
        assert!(switches_element_for_type("Sword"));
        // A sword's family holds no record, but graphs are tag references and reach every weapon.
        assert!(catalog_for_type("Sword").all(|entry| entry.owner_tag().is_none()));
        assert!(catalog_for_type("Sword").count() > 0);
        assert!(
            catalog_for_type("Scout Rifle").any(|entry| entry.owner_tag() == Some(0x8152_9461))
        );
    }
}
