//! Stock behaviors offered as one choice. Each adds exactly the actions the stock perks carry,
//! so it reads and builds like the perks it follows.
use std::collections::BTreeMap;
use sundial::investment::discovery::behaviors::Catalog;
use sundial::package_authoring::sandbox_perk::program::NativeNode;

/// Set a Weapon Firing Mode's key for Full Auto Fire.
const FULL_AUTO_FIRE: u32 = 0x75DD_B3C9;

/// The full auto recipe's title, which Suggested also lists by.
pub(super) const FULL_AUTO: &str = "Fire at Full Auto";

/// Change Weapon Properties' row title, which Suggested also lists by.
pub(super) const WEAPON_PROPERTIES: &str = "Change Weapon Properties";
/// The attachment Change Weapon Properties starts from: the entity Crimson's Banned Weapon
/// (perk 662) attaches to the weapon itself from the start: one modifier component holding 21
/// records over the Weapon Controller, the Magazine and the Barrel, and nothing else. The row
/// attaches it the same way, with
/// every record neutral, so the rows an author sets are its whole effect.
pub(in crate::app::custom_perks::workbench) const WEAPON_PROPERTIES_GRAPH: u32 = 0x80EF_AE52;

/// A behavior the stock perks build from one or more actions.
#[derive(Clone)]
pub(super) struct Recipe {
    pub title: &'static str,
    pub detail: &'static str,
    /// The node kind the picker files it under.
    pub kind: u8,
    /// Its actions, in the order they are added.
    pub nodes: Vec<NativeNode>,
}

/// Every recipe whose actions build.
pub(super) fn all() -> Vec<Recipe> {
    [full_auto(), track_targets(), hold_to_charge()]
        .into_iter()
        .flatten()
        .collect()
}

/// All ten full auto perks set Hold to Fire and the three weapon values to 0.35. Full Auto
/// Trigger System, Rapid-Fire Frame and Thunderer also set the firing mode to Full Auto Fire.
fn full_auto() -> Option<Recipe> {
    let mut trigger = NativeNode::effect(28)?;
    trigger.bytes.get_mut(2..4)?.fill(0);
    let mut values = NativeNode::effect(29)?;
    for at in [4_usize, 8, 12] {
        values
            .bytes
            .get_mut(at..at + 4)?
            .copy_from_slice(&0.35_f32.to_le_bytes());
    }
    // Target 0, interface 14 and the weapon rather than the player, as those three perks store.
    let mut mode = NativeNode::effect(35)?;
    *mode.bytes.get_mut(2)? = 0;
    *mode.bytes.get_mut(3)? = 14;
    mode.bytes
        .get_mut(4..8)?
        .copy_from_slice(&FULL_AUTO_FIRE.to_le_bytes());
    *mode.bytes.get_mut(8)? = 0;
    Some(Recipe {
        title: FULL_AUTO,
        detail: "Hold to Fire, Full Auto Fire and the values the full auto perks set.",
        kind: 29,
        nodes: vec![trigger, values, mode],
    })
}

/// Tracking Module, Prototype Trueseeker and Precision Frame ("adds tracking capability to
/// rockets") add one weapon count while aiming, and nothing else.
fn track_targets() -> Option<Recipe> {
    let mut count = NativeNode::effect(30)?;
    *count.bytes.get_mut(2)? = 1;
    Some(Recipe {
        title: "Track Targets",
        detail: "The weapon count the tracking perks add while aiming, as Tracking Module does.",
        kind: 30,
        nodes: vec![count],
    })
}

/// Charge Shot and Ahamkara's Eye set both of how the trigger fires to Hold to Charge.
fn hold_to_charge() -> Option<Recipe> {
    let mut trigger = NativeNode::effect(28)?;
    trigger.bytes.get_mut(2..4)?.fill(1);
    Some(Recipe {
        title: "Hold to Charge",
        detail: "How the trigger fires set to Hold to Charge, as Charge Shot does.",
        kind: 28,
        nodes: vec![trigger],
    })
}

/// Stock perks whose one attached buff is the behavior, adopted by the perk's name from the
/// installed catalog: the perk, the recipe's title and its detail.
const ADOPTED: [(&str, &str, &str); 2] = [
    (
        "Rampage",
        "Rampage's Stacking Damage",
        "The buff Rampage attaches on a kill: more damage, stacking three times.",
    ),
    (
        "Outlaw",
        "Outlaw's Faster Reload",
        "The buff Outlaw attaches on a precision kill: a much faster reload.",
    ),
];

/// The adopted recipes this installation has, each the attach action its perk carries most.
pub(super) fn adopted(catalog: &Catalog, names: &BTreeMap<u16, String>) -> Vec<Recipe> {
    ADOPTED
        .iter()
        .filter_map(|&(perk, title, detail)| {
            let effect = catalog
                .effects
                .iter()
                .filter(|effect| effect.kind == 1)
                .filter(|effect| {
                    effect
                        .sources
                        .iter()
                        .any(|source| names.get(&source.perk).is_some_and(|name| name == perk))
                })
                .max_by_key(|effect| effect.sources.len())?;
            Some(Recipe {
                title,
                detail,
                kind: 1,
                nodes: vec![NativeNode {
                    kind: effect.kind,
                    bytes: effect.bytes.clone(),
                }],
            })
        })
        .collect()
}
