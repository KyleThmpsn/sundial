//! A subclass recipe's abilities: which stock subclass supplies each ability slot and attunement.
//!
//! Every stock subclass's socket-entry list shares one layout. An entry's display hash, plug
//! source, group and prerequisites belong to its position, and only its pool, which grants the
//! ability, belongs to the subclass. So an ability is named by its slot and the stock subclass
//! whose pool fills it, and an attunement by its path and the stock subclass it comes from.
//!
//! An attunement path can also be authored node by node. Each node starts from a stock node, and
//! may take a name, a description and sandbox perks of its own. The path may take its own name.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Positions in a stock subclass's socket-entry list.
pub mod layout {
    /// Entries every stock list holds.
    pub const ENTRY_COUNT: usize = 24;
    /// The base melee target that each attunement's melee links to. It differs by class.
    pub const CLASS_BASE: u8 = 0;
    pub const CLASS_ABILITIES: [u8; 2] = [2, 3];
    pub const MOVEMENT: [u8; 3] = [4, 5, 6];
    pub const GRENADES: [u8; 3] = [7, 8, 9];
    /// The super every attunement uses unless the middle one brings its own.
    pub const SUPER: u8 = 10;
    /// Attunement entries: top, bottom and middle. Top and bottom lead with their melee, and the
    /// middle attunement leads with its own super.
    pub const ATTUNEMENTS: [[u8; 4]; 3] = [[11, 12, 13, 14], [15, 16, 17, 18], [20, 21, 22, 23]];
    /// Entries the character's selection names first: class ability, movement, grenade, super
    /// and melee. Sunrise and Sundial start every subclass here.
    pub const DEFAULT_SELECTION: [u8; 5] = [2, 4, 7, 10, 11];
    /// Nodes in an attunement path.
    pub const PATH_NODES: u8 = 4;
    /// The node that leads a path: its melee in the top and bottom paths, its super in the
    /// middle one. It takes only another path's lead node.
    pub const LEAD_NODE: u8 = 0;
}

/// Longest authored path or node name, and node description.
const NAME_LIMIT: usize = 64;
const DESCRIPTION_LIMIT: usize = 1_024;

fn text_is_valid(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit && !text.contains('\0')
}

/// An ability slot a subclass recipe can fill from another subclass.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AbilitySlot {
    ClassAbility,
    Movement,
    Grenade,
    Super,
}

impl AbilitySlot {
    pub const ALL: [Self; 4] = [
        Self::ClassAbility,
        Self::Movement,
        Self::Grenade,
        Self::Super,
    ];

    #[must_use]
    pub const fn entries(self) -> &'static [u8] {
        match self {
            Self::ClassAbility => &layout::CLASS_ABILITIES,
            Self::Movement => &layout::MOVEMENT,
            Self::Grenade => &layout::GRENADES,
            Self::Super => &[layout::SUPER],
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClassAbility => "Class Ability",
            Self::Movement => "Movement",
            Self::Grenade => "Grenade",
            Self::Super => "Super",
        }
    }

    #[must_use]
    pub fn of_entry(entry: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|slot| slot.entries().contains(&entry))
    }
}

/// An attunement path. Top and bottom share a shape, so either can fill the other's place. The
/// middle path leads with its own super and fits only the middle place.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttunementPath {
    Top,
    Bottom,
    Middle,
}

impl AttunementPath {
    pub const ALL: [Self; 3] = [Self::Top, Self::Bottom, Self::Middle];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Bottom => 1,
            Self::Middle => 2,
        }
    }

    #[must_use]
    pub const fn entries(self) -> [u8; 4] {
        layout::ATTUNEMENTS[self.index()]
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Middle => "Middle",
        }
    }

    /// The path's name in authored text hashes.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Middle => "middle",
        }
    }

    /// Whether an attunement from `self` can fill `place`.
    #[must_use]
    pub const fn fits(self, place: Self) -> bool {
        matches!(self, Self::Middle) == matches!(place, Self::Middle)
    }
}

/// Abilities a subclass recipe takes from other stock subclasses. Slots it leaves out keep the
/// base subclass's own.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SubclassAbilities {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<SubclassChoice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attunements: Vec<SubclassAttunement>,
}

impl SubclassAbilities {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.choices.is_empty() && self.attunements.is_empty()
    }

    #[must_use]
    pub fn choice(&self, entry: u8) -> Option<&SubclassChoice> {
        self.choices.iter().find(|choice| choice.entry == entry)
    }

    #[must_use]
    pub fn attunement(&self, path: AttunementPath) -> Option<&SubclassAttunement> {
        self.attunements
            .iter()
            .find(|attunement| attunement.path == path)
    }

    /// Fills `entry` from `source`, or restores the base's own ability when `source` is `None`.
    pub fn set_choice(&mut self, entry: u8, source: Option<(u32, u8)>) {
        self.choices.retain(|choice| choice.entry != entry);
        if let Some((source, source_entry)) = source {
            self.choices.push(SubclassChoice {
                entry,
                source,
                source_entry,
            });
            self.choices.sort_by_key(|choice| choice.entry);
        }
    }

    /// Fills `path` from `source`, keeping any name and nodes of its own, or restores the base's
    /// own attunement, nodes and name when `source` is `None`.
    pub fn set_attunement(&mut self, path: AttunementPath, source: Option<(u32, AttunementPath)>) {
        let Some((source, source_path)) = source else {
            self.attunements
                .retain(|attunement| attunement.path != path);
            return;
        };
        let attunement = self.attunement_entry(path, source);
        attunement.source = source;
        attunement.source_path = source_path;
    }

    /// Fills `path` from `source`'s path, keeping any name and nodes of its own. `base` is the
    /// base subclass, whose own path with nothing of its own needs no entry.
    pub fn set_path_source(
        &mut self,
        path: AttunementPath,
        base: u32,
        (source, source_path): (u32, AttunementPath),
    ) {
        let attunement = self.attunement_entry(path, base);
        attunement.source = source;
        attunement.source_path = source_path;
        self.prune(base);
    }

    /// Names `path`, or restores its source's name when `name` is `None`. `base` fills the path
    /// from the base subclass when the recipe does not set it yet.
    pub fn set_path_name(&mut self, path: AttunementPath, base: u32, name: Option<String>) {
        self.attunement_entry(path, base).name = name;
        self.prune(base);
    }

    /// Puts `node` at its position in `path`, or restores that position's node from the path's
    /// source when `node` is `None`.
    pub fn set_path_node(
        &mut self,
        path: AttunementPath,
        base: u32,
        position: u8,
        node: Option<SubclassPathNode>,
    ) {
        let attunement = self.attunement_entry(path, base);
        attunement.nodes.retain(|node| node.position != position);
        if let Some(node) = node {
            attunement.nodes.push(node);
            attunement.nodes.sort_by_key(|node| node.position);
        }
        self.prune(base);
    }

    /// The recipe's attunement for `path`, first set to the base's own path when it has none.
    fn attunement_entry(&mut self, path: AttunementPath, base: u32) -> &mut SubclassAttunement {
        let index = match self
            .attunements
            .iter()
            .position(|attunement| attunement.path == path)
        {
            Some(index) => index,
            None => {
                self.attunements.push(SubclassAttunement {
                    path,
                    source: base,
                    source_path: path,
                    name: None,
                    nodes: Vec::new(),
                });
                self.attunements.sort_by_key(|attunement| attunement.path);
                self.attunements
                    .iter()
                    .position(|attunement| attunement.path == path)
                    .unwrap_or_default()
            }
        };
        &mut self.attunements[index]
    }

    /// Drops attunements that are the base's own path with nothing of their own.
    fn prune(&mut self, base: u32) {
        self.attunements.retain(|attunement| {
            attunement.source != base
                || attunement.source_path != attunement.path
                || attunement.name.is_some()
                || !attunement.nodes.is_empty()
        });
    }

    /// Checks the shape of every choice. Sources and classes are checked against the installed
    /// subclasses when the recipe builds.
    pub fn validate(&self) -> Result<(), String> {
        for (index, choice) in self.choices.iter().enumerate() {
            let slot = AbilitySlot::of_entry(choice.entry)
                .ok_or_else(|| format!("Subclass entry {} is not an ability slot", choice.entry))?;
            if !slot.entries().contains(&choice.source_entry) {
                return Err(format!(
                    "Subclass entry {} takes a {}, but source entry {} is not one",
                    choice.entry,
                    slot.label().to_lowercase(),
                    choice.source_entry
                ));
            }
            if self.choices[..index]
                .iter()
                .any(|other| other.entry == choice.entry)
            {
                return Err(format!("Subclass entry {} is set twice", choice.entry));
            }
        }
        for (index, attunement) in self.attunements.iter().enumerate() {
            if !attunement.source_path.fits(attunement.path) {
                return Err(format!(
                    "A {} attunement cannot fill the {} place",
                    attunement.source_path.label().to_lowercase(),
                    attunement.path.label().to_lowercase()
                ));
            }
            if self.attunements[..index]
                .iter()
                .any(|other| other.path == attunement.path)
            {
                return Err(format!(
                    "The {} attunement is set twice",
                    attunement.path.label().to_lowercase()
                ));
            }
            attunement.validate()?;
        }
        Ok(())
    }
}

/// One ability slot filled from another stock subclass.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubclassChoice {
    /// The slot's entry in the authored subclass.
    pub entry: u8,
    /// The stock subclass that supplies the ability.
    #[serde(with = "hex_hash")]
    pub source: u32,
    /// The ability's entry in that subclass, in the same slot.
    pub source_entry: u8,
}

/// One attunement: a stock subclass's path in this place, with any name and nodes of its own.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubclassAttunement {
    pub path: AttunementPath,
    #[serde(with = "hex_hash")]
    pub source: u32,
    pub source_path: AttunementPath,
    /// The path's own name, in place of its source's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Nodes of the path's own, by position. The rest come from the source path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SubclassPathNode>,
}

impl SubclassAttunement {
    #[must_use]
    pub fn node(&self, position: u8) -> Option<&SubclassPathNode> {
        self.nodes.iter().find(|node| node.position == position)
    }

    fn validate(&self) -> Result<(), String> {
        let path = self.path.label().to_lowercase();
        if self
            .name
            .as_deref()
            .is_some_and(|name| !text_is_valid(name, NAME_LIMIT))
        {
            return Err(format!(
                "The {path} attunement needs a name of 1 to {NAME_LIMIT} characters"
            ));
        }
        let mut positions = BTreeSet::new();
        for node in &self.nodes {
            let number = node.position + 1;
            if node.position >= layout::PATH_NODES || node.source_position >= layout::PATH_NODES {
                return Err(format!(
                    "Node {number} of the {path} attunement is outside its path"
                ));
            }
            if !positions.insert(node.position) {
                return Err(format!(
                    "Node {number} of the {path} attunement is set twice"
                ));
            }
            let lead = node.position == layout::LEAD_NODE;
            if lead != (node.source_position == layout::LEAD_NODE) {
                return Err(format!(
                    "Node {number} of the {path} attunement must come from {}",
                    if lead {
                        "the first node of a path"
                    } else {
                        "the second, third or fourth node of a path"
                    }
                ));
            }
            if lead && !node.source_path.fits(self.path) {
                return Err(format!(
                    "Node 1 of the {path} attunement cannot come from a {} path",
                    node.source_path.label().to_lowercase()
                ));
            }
            if node
                .name
                .as_deref()
                .is_some_and(|name| !text_is_valid(name, NAME_LIMIT))
                || node
                    .description
                    .as_deref()
                    .is_some_and(|text| !text_is_valid(text, DESCRIPTION_LIMIT))
            {
                return Err(format!(
                    "Node {number} of the {path} attunement has an empty or overlong name or description"
                ));
            }
            let added = node.added_perks.iter().collect::<BTreeSet<_>>();
            let removed = node.removed_perks.iter().collect::<BTreeSet<_>>();
            if added.len() != node.added_perks.len()
                || removed.len() != node.removed_perks.len()
                || !added.is_disjoint(&removed)
                || added.contains(&u16::MAX)
            {
                return Err(format!(
                    "Node {number} of the {path} attunement repeats a perk"
                ));
            }
        }
        Ok(())
    }
}

/// One node of an attunement path, started from a stock node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubclassPathNode {
    /// Its position in the path, from 0. Position 0 leads the path.
    pub position: u8,
    /// The stock node it starts from: a subclass, one of its paths and a position in it.
    #[serde(with = "hex_hash")]
    pub source: u32,
    pub source_path: AttunementPath,
    pub source_position: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Sandbox perks it grants beyond its source's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_perks: Vec<u16>,
    /// Its source's sandbox perks it leaves out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_perks: Vec<u16>,
}

impl SubclassPathNode {
    /// A node taken whole from `source`'s node at `source_position` of `source_path`.
    #[must_use]
    pub const fn stock(
        position: u8,
        source: u32,
        source_path: AttunementPath,
        source_position: u8,
    ) -> Self {
        Self {
            position,
            source,
            source_path,
            source_position,
            name: None,
            description: None,
            added_perks: Vec::new(),
            removed_perks: Vec::new(),
        }
    }

    /// Whether the node changes its source's text or perks, and so needs records of its own.
    #[must_use]
    pub fn is_authored(&self) -> bool {
        self.name.is_some()
            || self.description.is_some()
            || !self.added_perks.is_empty()
            || !self.removed_perks.is_empty()
    }
}

/// Recipes write item hashes as `0x` and eight hex digits.
mod hex_hash {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("0x{value:08X}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text
            .strip_prefix("0x")
            .filter(|digits| digits.len() == 8)
            .ok_or_else(|| D::Error::custom(format!("{text:?} is not a 0x-prefixed hash")))?;
        u32::from_str_radix(digits, 16).map_err(D::Error::custom)
    }
}
