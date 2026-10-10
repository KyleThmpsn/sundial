//! Catalog of the native perk action nodes registered by the supported client.
//!
//! Build 86657.20.08.23. The counts are occurrences across the 1,632 action resources
//! recovered from the installed packages, not a count of distinct perks. A name here
//! records a traced operation. It is not a claim that every field of a node is mapped
//! or that a given combination works in game.

mod conditions;
pub use conditions::CONDITIONS;
mod effects;
pub use effects::EFFECTS;

/// Client build used for the recovered catalog, not a gameplay test result.
pub const CLIENT_BUILD: &str = "86657.20.08.23";

/// How far Parhelion supports one native node kind today.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// The action compiler can emit this node from an authored program.
    Authorable,
    /// The decoder reads this node's mapped fields, but the compiler cannot emit it.
    Readable,
    /// The node's class and size are known. Its individual fields are not mapped.
    Structural,
    /// The client registers this kind, but no surveyed action uses it.
    Unobserved,
}

impl Support {
    /// Short label for a UI badge.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Authorable => "Authorable",
            Self::Readable => "Readable",
            Self::Structural => "Structure Only",
            Self::Unobserved => "Not Observed",
        }
    }

    /// Sentence-case explanation of what the level means for authoring.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Authorable => {
                "Parhelion can build a supported configuration of this node into a custom effect."
            }
            Self::Readable => {
                "Parhelion can read mapped fields of this node, but cannot author it yet."
            }
            Self::Structural => "Only the node type and size are known. Its fields are not mapped.",
            Self::Unobserved => {
                "No surveyed action uses this registered kind. Its native layout has not been recovered."
            }
        }
    }
}

/// One registered condition or effect kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeKind {
    /// Dispatch index used by the node header byte.
    pub kind: u8,
    /// Native structure class, or zero when the kind was never observed.
    pub class: u32,
    /// Native structure size in bytes, or zero when the kind was never observed.
    pub struct_size: u32,
    /// Occurrences across the surveyed action resources.
    pub occurrences: u32,
    /// Title-case display name for the traced operation.
    pub name: &'static str,
    /// Sentence-case explanation for a workbench reader.
    pub summary: &'static str,
    /// Traced role and its recorded limits.
    pub evidence: &'static str,
    /// Authoring support level.
    pub support: Support,
}

impl NodeKind {
    /// Compiler coverage and understanding of runtime behavior are separate evidence axes.
    pub const fn semantics(&self) -> &'static str {
        if self.occurrences == 0 {
            "Unverified Native Layout"
        } else {
            "Traced Operation with Field Limits"
        }
    }
    /// Whether the surveyed packages contain this kind at all.
    #[must_use]
    pub const fn observed(&self) -> bool {
        self.occurrences != 0
    }
}

/// One action execution policy selected by the action root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionPolicy {
    /// Policy index stored in the action root.
    pub kind: u8,
    /// Title-case display name.
    pub name: &'static str,
    /// Sentence-case explanation for a workbench reader.
    pub summary: &'static str,
    /// Typed configuration class, or zero when the policy carries no configuration.
    pub configuration_class: u32,
    /// Surveyed action resources that select this policy.
    pub actions: u32,
}

/// Every action execution policy the client registers, indexed by kind.
pub const POLICIES: [ActionPolicy; 6] = [
    ActionPolicy {
        kind: 0,
        name: "Standard",
        summary: "The action runs from its conditions and effects alone.",
        configuration_class: 0x00000000,
        actions: 1569,
    },
    ActionPolicy {
        kind: 1,
        name: "Entity Variable Binding",
        summary: "The action reads a named numeric variable from its host entity. A host without that variable supplies zero.",
        configuration_class: 0x80803E07,
        actions: 61,
    },
    ActionPolicy {
        kind: 2,
        name: "Unobserved Policy 2",
        summary: "Registered by the client. No surveyed action uses it.",
        configuration_class: 0x00000000,
        actions: 0,
    },
    ActionPolicy {
        kind: 3,
        name: "Protected Policy 3",
        summary: "One surveyed action uses it. Its event handler enters protected code, so its contract is not established.",
        configuration_class: 0x80803E09,
        actions: 1,
    },
    ActionPolicy {
        kind: 4,
        name: "Unobserved Policy 4",
        summary: "Registered by the client. No surveyed action uses it.",
        configuration_class: 0x00000000,
        actions: 0,
    },
    ActionPolicy {
        kind: 5,
        name: "Protected Policy 5",
        summary: "One surveyed action uses it. Its event handler enters protected code, so its contract is not established.",
        configuration_class: 0x808029EA,
        actions: 1,
    },
];

/// Looks up a condition kind.
#[must_use]
pub fn condition(kind: u8) -> Option<&'static NodeKind> {
    CONDITIONS.get(kind as usize)
}

/// Looks up an effect kind.
#[must_use]
pub fn effect(kind: u8) -> Option<&'static NodeKind> {
    EFFECTS.get(kind as usize)
}

/// Looks up an action policy.
#[must_use]
pub fn policy(kind: u8) -> Option<&'static ActionPolicy> {
    POLICIES.get(kind as usize)
}

/// Title-case display name for a condition kind, falling back to its number.
#[must_use]
pub fn condition_name(kind: u8) -> String {
    condition(kind).map_or_else(|| format!("Condition {kind}"), |node| node.name.to_owned())
}

/// Title-case display name for an effect kind, falling back to its number.
#[must_use]
pub fn effect_name(kind: u8) -> String {
    effect(kind).map_or_else(|| format!("Effect {kind}"), |node| node.name.to_owned())
}

/// The name a condition kind carries wherever the workbench offers or shows it: the plain
/// title where there is one, otherwise the engine's traced name.
#[must_use]
pub fn condition_title(kind: u8) -> &'static str {
    plain_condition_title(kind)
        .unwrap_or_else(|| condition(kind).map_or("Condition", |node| node.name))
}

/// The name an effect kind carries wherever the workbench offers or shows it, on the same
/// terms as `condition_title`.
#[must_use]
pub fn effect_title(kind: u8) -> &'static str {
    plain_effect_title(kind).unwrap_or_else(|| effect(kind).map_or("Action", |node| node.name))
}

/// Articles, conjunctions and prepositions, which stay lowercase inside a title.
const SMALL_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "nor", "but", "of", "to", "from", "by", "with", "at", "in",
    "on", "for", "as", "while",
];

/// One word of a title built from an engine identifier, at `index` of `count` words: capitalized,
/// unless it is a small word inside the title, so `apply_tiered_charge_of_light` reads "Apply
/// Tiered Charge of Light". The first and last words always capitalize.
#[must_use]
pub fn title_word(word: &str, index: usize, count: usize) -> String {
    if index > 0 && index + 1 < count && SMALL_WORDS.contains(&word) {
        return word.to_owned();
    }
    let mut characters = word.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// A name in the words a player uses, for the effect kinds whose traced behavior says
/// plainly what happens in game.
///
/// A kind is listed here only when its recorded evidence above leaves the behavior resolved.
/// Kinds whose evidence ends in "remains unresolved" stay off this list even when their
/// fields are editable, because the workbench would be putting words to something it has
/// not established. Everything not listed keeps the engine's own traced name.
#[must_use]
pub fn plain_effect_title(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 => "Attach an Effect",
        2 => "Attach an Effect with a Dynamic Value",
        3 => "Spawn an Object or Effect",
        4 => "Apply an Effect to a Chosen Target",
        5 => "Generate Orbs of Light",
        6 => "Change Damage Type",
        7 => "Change an Ability Property",
        8 => "Change Ability Energy",
        10 => "Change a Weapon or Ability Stat",
        11 => "Change Ammo Drop Chance",
        13 => "Drop Ammo by Weighted Chance",
        14 => "Adjust Ammo",
        15 => "Adjust Ammo by Capacity",
        16 => "Reload from Reserves",
        18 => "Set Radar Detection Range",
        20 => "Improve Radar Detail",
        25 => "Override a Pattern Key",
        26 => "Change Fired Projectile",
        // Kind 28: its two bytes are how the trigger fires, read off the stock perks that set
        // them (see `fields::values`).
        28 => "Change How the Trigger Fires",
        29 => "Replace Three Weapon Values",
        30 => "Hold a Weapon Count",
        33 => "Change Incoming Damage",
        32 => "Extend Timers",
        35 => "Set a Weapon Firing Mode",
        37 => "Label the Event When the Damage Source Matches",
        40 => "Change Outgoing Damage",
        41 => "Hold a Named Count",
        42 => "Set the Effect's Counter",
        43 => "Send a Game Signal",
        47 => "Set Transmat Effect",
        49 => "Remember a Target by Name",
        48 => "Run a Game Script",
        52 => "Add to a Named Player Value",
        53 => "Adjust Several Named Values",
        54 => "Label the Event When the Target Matches",
        _ => return None,
    })
}

/// A name in the words a player uses for a condition kind, on the same terms as
/// `plain_effect_title`. The weapon events share the names of the triggers they start.
#[must_use]
pub fn plain_condition_title(kind: u8) -> Option<&'static str> {
    Some(match kind {
        0 => "Always",
        1 => "After a Delay",
        2 => "On a Kill",
        4 => "On Dealing Damage",
        5 => "On Taking Damage",
        6 => "On Picking Up Ammo",
        8 => "On Using an Ability",
        9 => "On Activating an Ability",
        10 => "On a Specific Ability",
        11 => "Ends on a Specific Ability",
        13 => "On Releasing the Trigger",
        12 => "On a Game Event",
        14 => "On Equip",
        15 => "On Unequip",
        16 => "On Draw",
        17 => "On Holster",
        18 => "On Weapon Swap",
        19 => "On Reloading",
        22 => "On Crouching",
        23 => "On Aiming Down Sights",
        24 => "On Sliding",
        25 => "On Sprinting",
        26 => "When the Effect's Counter Is Reached",
        27 => "On Firing This Weapon",
        29 => "On a Game Signal",
        30 => "Ends on a Game Signal",
        31 => "When All Requirements Are Met",
        // Kind 35 runs the general predicate, a state check, and then its nested condition.
        35 => "State Check with a Condition",
        38 => "When a Remembered Target Is Far Away",
        42 => "On a Finisher",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_node(node: &NodeKind, expected_kind: usize) {
        assert_eq!(usize::from(node.kind), expected_kind);
        for text in [node.name, node.summary, node.evidence] {
            assert!(!text.contains(';'), "{text}");
            assert!(!text.contains('\u{2014}'), "{text}");
            assert!(!text.is_empty());
        }
        if node.support == Support::Unobserved {
            assert_eq!(node.occurrences, 0);
            assert_eq!(node.class, 0);
        } else {
            assert_ne!(node.class, 0);
            assert_ne!(node.struct_size, 0);
        }
    }

    #[test]
    fn every_kind_is_indexed_by_its_number_and_carries_usable_prose() {
        for (index, node) in CONDITIONS.iter().enumerate() {
            check_node(node, index);
        }
        for (index, node) in EFFECTS.iter().enumerate() {
            check_node(node, index);
        }
        for (index, entry) in POLICIES.iter().enumerate() {
            assert_eq!(usize::from(entry.kind), index);
            assert!(!entry.name.is_empty());
            assert!(!entry.summary.contains(';'));
        }
    }
}
