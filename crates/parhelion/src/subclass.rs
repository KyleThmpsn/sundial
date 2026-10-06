//! Subclasses: a recipe's abilities and attunements, and how the build authors them.
//!
//! Every stock subclass's socket-entry list shares one layout. An entry's display hash, plug
//! source, group and prerequisites belong to its position, and only its pool, which grants the
//! ability, belongs to the subclass. So an ability is named by its slot and the stock ability it
//! is based on, and an attunement by its path and the stock subclass it comes from.
//!
//! An ability, and each node of an attunement path, can also be authored: a name, a
//! description, an icon and perks of its own ([`EntryEdits`]), and changes to the abilities of
//! its subclass while it is selected ([`AbilityModifier`]). A path may take its own name.

use serde::{Deserialize, Serialize};

pub mod art;
pub(crate) mod authoring;
pub(crate) mod compile;
mod edits;
pub(crate) mod grouping;
pub mod layout;
mod modifiers;
pub(crate) mod native;
pub mod palette;
pub(crate) mod tables;

pub use art::{ArtImage, ArtPart, ScreenArt};
pub use edits::{BankValue, EntryEdits, EntryIcon, SpawnSwap};
pub use modifiers::{
    AbilityModifier, MOST_CHARGES, ModifierEffect, ParameterValue, RECHARGE_RANGE, StockModifier,
    entry_place, holds_ability, place_entry,
};
pub use palette::{EffectGrade, PaletteEdit, TintEdit};

pub(crate) const EVERY_CLASS_TYPE_NAME: &str = "Guardian Subclass";

/// The type label a subclass defaults to: Guardian Subclass for every class, its chosen class's
/// for another class, or none to keep its base's.
pub(crate) const fn class_type_name(
    every_class: bool,
    class: Option<crate::ArmorClass>,
) -> Option<&'static str> {
    if every_class {
        return Some(EVERY_CLASS_TYPE_NAME);
    }
    match class {
        Some(crate::ArmorClass::Titan) => Some("Titan Subclass"),
        Some(crate::ArmorClass::Hunter) => Some("Hunter Subclass"),
        Some(crate::ArmorClass::Warlock) => Some("Warlock Subclass"),
        Some(crate::ArmorClass::Any) => Some(EVERY_CLASS_TYPE_NAME),
        None => None,
    }
}

/// Levels of spawned graphs below an ability that its edits reach and the build copies. Every
/// stock ability's graphs lie within it: the longest spawn route below a stock root is eight
/// levels, and no graph is more than six from its root (2026-10-05 census of the 49 roots).
pub const SPAWN_DEPTH: usize = 8;

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

    /// The slot's name for one of its entries: "Grenade 2", or "Super" for a slot of one.
    #[must_use]
    pub fn entry_label(self, entry: u8) -> String {
        let entries = self.entries();
        match entries.iter().position(|each| *each == entry) {
            Some(position) if entries.len() > 1 => format!("{} {}", self.label(), position + 1),
            _ => self.label().to_owned(),
        }
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

/// Where an authored entry sits: an ability slot's entry, or a node of an attunement path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Place {
    Ability(u8),
    Node(AttunementPath, u8),
}

impl Place {
    /// Every ability, then every node, in the order the game lists them.
    pub fn all() -> impl Iterator<Item = Self> {
        AbilitySlot::ALL
            .into_iter()
            .flat_map(|slot| slot.entries().iter().map(|&entry| Self::Ability(entry)))
            .chain(AttunementPath::ALL.into_iter().flat_map(|path| {
                (0..layout::PATH_NODES).map(move |position| Self::Node(path, position))
            }))
    }

    /// What names the entry's authored text and icon row: `ability-7`, or
    /// `attunement-top-node-2`.
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Ability(entry) => format!("ability-{entry}"),
            Self::Node(path, position) => {
                format!("attunement-{}-node-{}", path.key(), position + 1)
            }
        }
    }

    /// "Grenade 2", or "Top Path · Node 3".
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Ability(entry) => AbilitySlot::of_entry(entry)
                .map_or_else(|| format!("Entry {entry}"), |slot| slot.entry_label(entry)),
            Self::Node(path, position) => format!("{} Path · Node {}", path.label(), position + 1),
        }
    }
}

/// Abilities a subclass recipe bases on other stock abilities or authors, and its attunements.
/// Slots it leaves out keep the base subclass's own.
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

    /// The ability in `entry`: the recipe's own, or the base's.
    #[must_use]
    pub fn ability(&self, base: u32, entry: u8) -> SubclassChoice {
        self.choice(entry)
            .cloned()
            .unwrap_or_else(|| SubclassChoice::stock(entry, base, entry))
    }

    /// Puts `choice` in its entry, or restores the base's own ability when `choice` is just that.
    pub fn set_ability(&mut self, base: u32, choice: SubclassChoice) {
        let entry = choice.entry;
        self.choices.retain(|existing| existing.entry != entry);
        if choice != SubclassChoice::stock(entry, base, entry) {
            self.choices.push(choice);
            self.choices.sort_by_key(|choice| choice.entry);
        }
    }

    /// Restores the base's own ability in `entry`.
    pub fn reset_ability(&mut self, entry: u8) {
        self.choices.retain(|choice| choice.entry != entry);
    }

    #[must_use]
    pub fn attunement(&self, path: AttunementPath) -> Option<&SubclassAttunement> {
        self.attunements
            .iter()
            .find(|attunement| attunement.path == path)
    }

    /// The stock subclass and path that fill `path`.
    #[must_use]
    pub fn attunement_source(&self, base: u32, path: AttunementPath) -> (u32, AttunementPath) {
        self.attunement(path).map_or((base, path), |attunement| {
            (attunement.source, attunement.source_path)
        })
    }

    /// The node at `position` of `path`: the recipe's own, or its source path's.
    #[must_use]
    pub fn node(&self, base: u32, path: AttunementPath, position: u8) -> SubclassPathNode {
        let (source, source_path) = self.attunement_source(base, path);
        self.attunement(path)
            .and_then(|attunement| attunement.node(position))
            .cloned()
            .unwrap_or_else(|| SubclassPathNode::stock(position, source, source_path, position))
    }

    /// What the ability or node at `place` authors.
    #[must_use]
    pub fn edits(&self, base: u32, place: Place) -> EntryEdits {
        match place {
            Place::Ability(entry) => self.ability(base, entry).edits,
            Place::Node(path, position) => self.node(base, path, position).edits,
        }
    }

    /// Gives the ability or node at `place` these edits, keeping what it is based on.
    pub fn set_edits(&mut self, base: u32, place: Place, edits: EntryEdits) {
        match place {
            Place::Ability(entry) => {
                let choice = SubclassChoice {
                    edits,
                    ..self.ability(base, entry)
                };
                self.set_ability(base, choice);
            }
            Place::Node(path, position) => {
                let node = SubclassPathNode {
                    edits,
                    ..self.node(base, path, position)
                };
                self.set_path_node(path, base, node);
            }
        }
    }

    /// Restores the base's own attunement, nodes and name in `path`.
    pub fn reset_attunement(&mut self, path: AttunementPath) {
        self.attunements
            .retain(|attunement| attunement.path != path);
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

    /// Puts `node` at its position in `path`, or restores the path's own node there when `node`
    /// is just that.
    pub fn set_path_node(&mut self, path: AttunementPath, base: u32, node: SubclassPathNode) {
        let (source, source_path) = self.attunement_source(base, path);
        let position = node.position;
        let own = node == SubclassPathNode::stock(position, source, source_path, position);
        let attunement = self.attunement_entry(path, base);
        attunement.nodes.retain(|node| node.position != position);
        if !own {
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
            let context = slot.entry_label(choice.entry);
            if !slot.entries().contains(&choice.source_entry) {
                return Err(format!(
                    "{context} takes a {}, but source entry {} is not one",
                    slot.label().to_lowercase(),
                    choice.source_entry
                ));
            }
            if self.choices[..index]
                .iter()
                .any(|other| other.entry == choice.entry)
            {
                return Err(format!("{context} is set twice"));
            }
            choice
                .edits
                .validate(&context, Place::Ability(choice.entry))?;
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

/// One ability slot: the stock ability it is based on, and anything it authors over it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubclassChoice {
    /// The slot's entry in the authored subclass.
    pub entry: u8,
    /// The stock subclass whose ability it is based on.
    #[serde(with = "hex_hash")]
    pub source: u32,
    /// The ability's entry in that subclass, in the same slot.
    pub source_entry: u8,
    #[serde(flatten)]
    pub edits: EntryEdits,
}

impl SubclassChoice {
    /// The ability in `source`'s `source_entry`, taken whole into `entry`.
    #[must_use]
    pub fn stock(entry: u8, source: u32, source_entry: u8) -> Self {
        Self {
            entry,
            source,
            source_entry,
            edits: EntryEdits::default(),
        }
    }
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
            .is_some_and(|name| !edits::text_is_valid(name, edits::NAME_LIMIT))
        {
            return Err(format!(
                "The {path} attunement needs a name of 1 to {} characters",
                edits::NAME_LIMIT
            ));
        }
        let mut positions = std::collections::BTreeSet::new();
        for node in &self.nodes {
            let context = format!("Node {} of the {path} attunement", node.position + 1);
            if node.position >= layout::PATH_NODES || node.source_position >= layout::PATH_NODES {
                return Err(format!("{context} is outside its path"));
            }
            if !positions.insert(node.position) {
                return Err(format!("{context} is set twice"));
            }
            let lead = node.position == layout::LEAD_NODE;
            if lead != (node.source_position == layout::LEAD_NODE) {
                return Err(format!(
                    "{context} must come from {}",
                    if lead {
                        "the first node of a path"
                    } else {
                        "the second, third or fourth node of a path"
                    }
                ));
            }
            if lead && !node.source_path.fits(self.path) {
                return Err(format!(
                    "{context} cannot come from a {} path",
                    node.source_path.label().to_lowercase()
                ));
            }
            node.edits
                .validate(&context, Place::Node(self.path, node.position))?;
        }
        Ok(())
    }
}

/// One node of an attunement path, started from a stock node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubclassPathNode {
    /// Its position in the path, from 0. Position 0 leads the path.
    pub position: u8,
    /// The stock node it starts from: a subclass, one of its paths and a position in it.
    #[serde(with = "hex_hash")]
    pub source: u32,
    pub source_path: AttunementPath,
    pub source_position: u8,
    #[serde(flatten)]
    pub edits: EntryEdits,
}

impl SubclassPathNode {
    /// A node taken whole from `source`'s node at `source_position` of `source_path`.
    #[must_use]
    pub fn stock(
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
            edits: EntryEdits::default(),
        }
    }

    /// The stock entry it starts from, in its source subclass's list.
    #[must_use]
    pub fn source_entry(&self) -> u8 {
        self.source_path.entries()[usize::from(self.source_position)]
    }
}

/// A float as its bits, so a type holding it compares exactly, written as the number.
mod f32_bits {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bits: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        f32::from_bits(*bits).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        let value = f32::deserialize(deserializer)?;
        if value.is_finite() {
            Ok(value.to_bits())
        } else {
            Err(D::Error::custom("a value must be a finite number"))
        }
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

    /// As the module, for a hash that may be absent.
    pub mod optional {
        use serde::{Deserialize, Deserializer, Serializer};

        #[allow(clippy::ref_option)]
        pub fn serialize<S: Serializer>(
            value: &Option<u32>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                Some(value) => super::serialize(value, serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<u32>, D::Error> {
            #[derive(Deserialize)]
            struct Hash(#[serde(with = "super")] u32);
            Ok(Option::<Hash>::deserialize(deserializer)?.map(|Hash(value)| value))
        }
    }
}
