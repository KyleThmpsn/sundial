//! Stable recipe destinations and the shared native presentation-node budget.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NODE_CAPACITY: usize = 1024;
pub const BASE_NODE_COUNT: usize = crate::progression::STOCK_PRESENTATION_NODE_COUNT + 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ammo {
    Primary,
    Special,
    Heavy,
}

impl Ammo {
    pub const ALL: [Self; 3] = [Self::Primary, Self::Special, Self::Heavy];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Special => "Special",
            Self::Heavy => "Heavy",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    AutoRifles,
    Bows,
    FusionRifles,
    Glaives,
    GrenadeLaunchers,
    HandCannons,
    LinearFusionRifles,
    MachineGuns,
    PulseRifles,
    RocketLaunchers,
    ScoutRifles,
    Shotguns,
    Sidearms,
    SniperRifles,
    SubmachineGuns,
    Swords,
}
impl Family {
    pub const ALL: [Self; 16] = [
        Self::AutoRifles,
        Self::Bows,
        Self::FusionRifles,
        Self::Glaives,
        Self::GrenadeLaunchers,
        Self::HandCannons,
        Self::LinearFusionRifles,
        Self::MachineGuns,
        Self::PulseRifles,
        Self::RocketLaunchers,
        Self::ScoutRifles,
        Self::Shotguns,
        Self::Sidearms,
        Self::SniperRifles,
        Self::SubmachineGuns,
        Self::Swords,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoRifles => "Auto Rifles",
            Self::Bows => "Bows",
            Self::FusionRifles => "Fusion Rifles",
            Self::Glaives => "Glaives",
            Self::GrenadeLaunchers => "Grenade Launchers",
            Self::HandCannons => "Hand Cannons",
            Self::LinearFusionRifles => "Linear Fusion Rifles",
            Self::MachineGuns => "Machine Guns",
            Self::PulseRifles => "Pulse Rifles",
            Self::RocketLaunchers => "Rocket Launchers",
            Self::ScoutRifles => "Scout Rifles",
            Self::Shotguns => "Shotguns",
            Self::Sidearms => "Sidearms",
            Self::SniperRifles => "Sniper Rifles",
            Self::SubmachineGuns => "Submachine Guns",
            Self::Swords => "Swords",
        }
    }
    pub(crate) fn from_type_name(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|family| {
            let label = family.label().to_ascii_lowercase();
            normalized == label || normalized == label.trim_end_matches('s')
        })
    }
    pub const fn template(self) -> Destination {
        if matches!(self, Self::Glaives) {
            // Only the native page structure is reused. The new family keeps
            // its own identity and localized label.
            return Self::FusionRifles.template();
        }
        let ammo = match self {
            Self::FusionRifles | Self::GrenadeLaunchers | Self::Shotguns | Self::SniperRifles => {
                Ammo::Special
            }
            Self::LinearFusionRifles | Self::MachineGuns | Self::RocketLaunchers | Self::Swords => {
                Ammo::Heavy
            }
            _ => Ammo::Primary,
        };
        Destination { ammo, family: self }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub ammo: Ammo,
    pub family: Family,
}

impl Destination {
    pub(crate) fn name_hash(self) -> Option<u32> {
        (self.family.template().family != self.family).then(|| self.hash("name"))
    }

    pub fn label(self) -> String {
        format!("Weapons / {} / {}", self.ammo.label(), self.family.label())
    }
    /// Audited stock items anchor semantic destinations without persisting table indices.
    pub const fn stock_exemplar(self) -> Option<u32> {
        use Ammo::*;
        use Family::*;
        Some(match (self.ammo, self.family) {
            (Primary, AutoRifles) => 0x61FF_EE96,
            (Primary, Bows) => 0x2AEF_B232,
            (Primary, HandCannons) => 0x3E7B_47F8,
            (Primary, PulseRifles) => 0x1437_3AFC,
            (Primary, ScoutRifles) => 0x74D6_8F77,
            (Primary, Sidearms) => 0x5B67_57E0,
            (Primary, SubmachineGuns) => 0x478C_DF8F,
            (Special, FusionRifles) => 0xCD5D_35CD,
            (Special, GrenadeLaunchers) => 0x7637_40D0,
            (Special, Shotguns) => 0x2B94_6BA9,
            (Special, SniperRifles) => 0xC56A_395D,
            (Heavy, GrenadeLaunchers) => 0x8140_7A43,
            (Heavy, LinearFusionRifles) => 0x9527_F0F7,
            (Heavy, MachineGuns) => 0x23F4_BF01,
            (Heavy, RocketLaunchers) => 0x0837_DFF1,
            (Heavy, Swords) => 0xF8D1_86CA,
            _ => return None,
        })
    }
    pub(crate) fn hash(self, field: &str) -> u32 {
        crate::presentation::text_hash(
            &format!("collection/{:?}/{:?}", self.ammo, self.family),
            field,
        )
    }
}

/// Supported Guardian classes, in native Titan, Hunter, Warlock order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Classes(u8);
impl Classes {
    pub(crate) const ALL: Self = Self(7);
    pub(crate) const fn one(class: u8) -> Self {
        Self(1 << class)
    }
    pub(crate) const fn supports(self, class: u8) -> bool {
        self.0 & (1 << class) != 0
    }
    pub(crate) fn iter(self) -> impl Iterator<Item = u8> {
        (0..3).filter(move |&class| self.supports(class))
    }
}

/// The Collections leaves for one collectible, including up to three supported classes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollectionPages([u16; 4]);
impl CollectionPages {
    pub(crate) const fn one(page: u16) -> Self {
        Self([page, u16::MAX, u16::MAX, u16::MAX])
    }
    pub(crate) fn new(pages: impl IntoIterator<Item = u16>) -> crate::AuthoringResult<Self> {
        let mut result = Self([u16::MAX; 4]);
        let mut count = 0;
        for page in pages {
            if page == u16::MAX {
                return Err(crate::error::invalid("Collections page is unavailable"));
            }
            if result.0.contains(&page) {
                continue;
            }
            if count == result.0.len() {
                return Err(crate::error::invalid("Too many Collections destinations"));
            }
            result.0[count] = page;
            count += 1;
        }
        if count == 0 {
            return Err(crate::error::invalid("Item has no Collections destination"));
        }
        Ok(result)
    }
    pub(crate) fn iter(self) -> impl Iterator<Item = u16> {
        self.0.into_iter().filter(|&page| page != u16::MAX)
    }
    pub(crate) fn contains(self, page: u16) -> bool {
        self.iter().any(|value| value == page)
    }
}

/// A runtime-branded gear page, armor category or numbered armor set.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct GearPage {
    kind: crate::ItemKind,
    class: Option<u8>,
    set: Option<usize>,
}
impl GearPage {
    #[must_use]
    pub const fn for_kind(kind: crate::ItemKind) -> Option<Self> {
        match kind {
            crate::ItemKind::Sparrow
            | crate::ItemKind::Ship
            | crate::ItemKind::GhostShell
            | crate::ItemKind::Emblem
            | crate::ItemKind::Shader => Some(Self {
                kind,
                class: None,
                set: None,
            }),
            _ => None,
        }
    }
    pub(crate) const fn armor(class: u8) -> Self {
        Self {
            kind: crate::ItemKind::Armor,
            class: Some(class),
            set: None,
        }
    }
    pub(crate) const fn armor_set(class: u8, set: usize) -> Self {
        Self {
            kind: crate::ItemKind::Armor,
            class: Some(class),
            set: Some(set),
        }
    }
    pub(crate) const fn set(self) -> Option<usize> {
        self.set
    }
    pub(crate) fn set_name(self, brand: &str) -> Option<String> {
        self.set.map(|set| {
            let mut number = set + 1;
            let mut roman = String::new();
            for (value, token) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while number >= value {
                    roman.push_str(token);
                    number -= value;
                }
            }
            format!("{brand} Armor Set {roman}")
        })
    }
    #[must_use]
    pub const fn kind(self) -> crate::ItemKind {
        self.kind
    }
    pub(crate) const fn class(self) -> Option<u8> {
        self.class
    }
    pub(crate) fn hash(self, field: &str) -> u32 {
        let key = if let Some(class) = self.class {
            format!("collection/gear/Armor/{class}")
        } else {
            format!("collection/gear/{:?}", self.kind)
        };
        let key = if let Some(set) = self.set {
            format!("{key}/set/{set}")
        } else {
            key
        };
        crate::presentation::text_hash(&key, field)
    }
}

/// Each native armor-set row holds at most five collectible tiles.
pub(crate) const ARMOR_SET_SIZE: usize = 5;

pub(crate) fn armor_pages(counts: [usize; 3]) -> impl Iterator<Item = GearPage> {
    counts.into_iter().enumerate().flat_map(|(class, count)| {
        (count != 0)
            .then_some(GearPage::armor(class as u8))
            .into_iter()
            .chain(
                (0..count.div_ceil(ARMOR_SET_SIZE))
                    .map(move |set| GearPage::armor_set(class as u8, set)),
            )
    })
}

/// Node identities of the gear pages a build adds.
pub(crate) fn gear_page_node_hashes(pages: impl IntoIterator<Item = GearPage>) -> BTreeSet<u64> {
    pages
        .into_iter()
        .map(|page| u64::from(page.hash("node")))
        .collect()
}

pub struct NodeBudget {
    pub badges: usize,
    pub pages: usize,
    pub gear_pages: usize,
}
impl NodeBudget {
    pub fn new<'a>(
        entries: impl IntoIterator<Item = (Option<&'a str>, Option<Destination>)>,
    ) -> Self {
        let (badges, pages) = custom_members(entries);
        Self {
            badges: badges.len(),
            pages: pages.len(),
            gear_pages: 0,
        }
    }
    /// Counts the gear pages a build adds beside its weapon pages.
    #[must_use]
    pub const fn with_gear_pages(mut self, gear_pages: usize) -> Self {
        self.gear_pages = gear_pages;
        self
    }
    pub const fn used(&self) -> usize {
        BASE_NODE_COUNT + self.badges * 4 + self.pages + self.gear_pages
    }
    pub(crate) fn validate(&self) -> crate::AuthoringResult<()> {
        if self.used() > NODE_CAPACITY {
            return Err(crate::error::invalid(format!(
                "Collections needs {} of {NODE_CAPACITY} nodes. Remove a custom badge or an added page from this build.",
                self.used()
            )));
        }
        Ok(())
    }
}

pub(crate) fn custom_node_hashes<'a>(
    entries: impl IntoIterator<Item = (Option<&'a str>, Option<Destination>)>,
) -> BTreeSet<u64> {
    let (badges, pages) = custom_members(entries);
    badges
        .into_iter()
        .flat_map(|badge| {
            (0..4).map(move |index| u64::from(crate::presentation::badge_node_hash(badge, index)))
        })
        .chain(
            pages
                .into_iter()
                .map(|destination| u64::from(destination.hash("node"))),
        )
        .collect()
}

fn custom_members<'a>(
    entries: impl IntoIterator<Item = (Option<&'a str>, Option<Destination>)>,
) -> (BTreeSet<&'a str>, BTreeSet<Destination>) {
    let mut badges = BTreeSet::new();
    let mut pages = BTreeSet::new();
    for (badge, destination) in entries {
        badges.extend(badge);
        pages.extend(destination.filter(|page| page.stock_exemplar().is_none()));
    }
    (badges, pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_node_identities_share_badges_and_pages() {
        let page = Destination {
            ammo: Ammo::Special,
            family: Family::Sidearms,
        };
        let entries = [
            (Some("Travelers"), Some(page)),
            (Some("Travelers"), Some(page)),
        ];
        let hashes = custom_node_hashes(entries);
        assert_eq!(hashes.len(), 5);
        assert_eq!(hashes, custom_node_hashes([entries[0]]));
        assert_eq!(
            NodeBudget::new(entries).used() - BASE_NODE_COUNT,
            hashes.len()
        );
    }

    #[test]
    fn stock_collection_pages_do_not_allocate_custom_nodes() {
        let destination = Destination {
            ammo: Ammo::Primary,
            family: Family::AutoRifles,
        };
        assert!(custom_node_hashes([(None, Some(destination))]).is_empty());
    }
}
