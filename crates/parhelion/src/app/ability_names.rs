//! The game's names for the stock abilities' entities and banks, registered with the entity
//! catalog and the bank knowledge once a catalog loads, so the pickers, the Engine Catalog and
//! the property modal read Vortex Grenade where the engine says Solar Flare. The perks the
//! abilities grant are remembered by their ability too, since most have no name of their own
//! and the assets they attach are named after the perk that attaches them.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

use sundial::investment::{AbilityRowSummary, SubclassSummary};
use sundial::package_authoring::{
    ability_bank::{self, AbilityTarget},
    sandbox_perk::entity,
};

/// The abilities that share an entity or bank, with their classes, and the slots they sit in.
type Named = (BTreeSet<(String, u8)>, BTreeSet<AbilityTarget>);

/// The ability that grants each stock perk, by finished perk index.
static PERK_OWNERS: RwLock<BTreeMap<u16, String>> = RwLock::new(BTreeMap::new());

/// The ability that grants the stock perk `index`, once a catalog registered them: the name
/// an asset the perk attaches is read by when the perk has no name of its own.
pub(in crate::app) fn perk_owner(index: u16) -> Option<String> {
    PERK_OWNERS.read().ok()?.get(&index).cloned()
}

/// The class an entry belongs to, as the Subclass summaries number them.
fn class_name(class: u8) -> Option<&'static str> {
    match class {
        0 => Some("Titan"),
        1 => Some("Hunter"),
        2 => Some("Warlock"),
        _ => None,
    }
}

/// One name for the abilities that share an entity or bank: the ability when there is one,
/// the class and the word the abilities end with when they end alike (Titan Barricade, Hunter
/// Dodge, Warlock Glide), the class and slot otherwise (Titan Melee).
fn shared_name((names, slots): &Named) -> Option<String> {
    let distinct = names
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    if let (1, Some(name)) = (distinct.len(), distinct.first()) {
        return Some((*name).to_owned());
    }
    let classes = names
        .iter()
        .map(|(_, class)| *class)
        .collect::<BTreeSet<_>>();
    let class = (classes.len() == 1)
        .then(|| classes.first().copied())
        .flatten()
        .and_then(class_name);
    let last = distinct
        .iter()
        .map(|name| name.rsplit(' ').next().unwrap_or(name))
        .collect::<BTreeSet<_>>();
    let word = match (last.len(), last.first()) {
        (1, Some(word)) if distinct.iter().all(|name| name.contains(' ')) => (*word).to_owned(),
        _ => {
            let slot = (slots.len() == 1)
                .then(|| slots.first().copied())
                .flatten()?;
            ability_bank::slot_name(slot)?.to_owned()
        }
    };
    Some(match class {
        Some(class) => format!("{class} {word}"),
        None => word,
    })
}

/// The name a granted perk is read by: its entry when one grants it, the subclass when several
/// of one subclass's entries do, the first entry otherwise.
fn owner_name(owners: &BTreeSet<(String, String)>) -> String {
    let entries = owners
        .iter()
        .map(|(entry, _)| entry.as_str())
        .collect::<BTreeSet<_>>();
    let subclasses = owners
        .iter()
        .map(|(_, subclass)| subclass.as_str())
        .collect::<BTreeSet<_>>();
    let chosen = if entries.len() == 1 {
        entries.first()
    } else if subclasses.len() == 1 {
        subclasses.first()
    } else {
        entries.first()
    };
    chosen.map_or_else(String::new, |name| (*name).to_owned())
}

/// Registers every stock ability's entity and bank under the game's name, and which abilities
/// list each script parameter, for the pickers and the property modal to read.
pub(in crate::app) fn remember(subclasses: &[SubclassSummary], rows: &[AbilityRowSummary]) {
    let mut entities = BTreeMap::<u32, Named>::new();
    let mut banks = BTreeMap::<u32, Named>::new();
    let mut perks = BTreeMap::<u16, BTreeSet<(String, String)>>::new();
    let mut listings = BTreeMap::<(AbilityTarget, u32), BTreeSet<String>>::new();
    for subclass in subclasses {
        for (entry, name) in &subclass.entry_names {
            let row = subclass
                .entry_rows
                .get(entry)
                .and_then(|row| rows.iter().find(|summary| summary.row == *row));
            let slot = row.and_then(|row| row.slot);
            let note = |named: &mut Named| {
                named.0.insert((name.clone(), subclass.class_type));
                named.1.extend(slot);
            };
            if let Some(entity) = subclass.entry_entities.get(entry) {
                note(entities.entry(*entity).or_default());
            }
            let Some(row) = row else {
                continue;
            };
            if let Some(bank) = row.bank {
                note(banks.entry(bank).or_default());
            }
            if let Some(slot) = slot {
                for parameter in &row.parameters {
                    listings
                        .entry((slot, parameter.name))
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        // A perk is owned by the entry that grants it, or by the subclass when the entry has
        // no name of its own, as its passive nodes do.
        for (entry, granted) in &subclass.entry_perks {
            let owner = subclass.entry_names.get(entry).unwrap_or(&subclass.name);
            for perk in granted {
                perks
                    .entry(*perk)
                    .or_default()
                    .insert((owner.clone(), subclass.name.clone()));
            }
        }
    }
    if let Ok(mut owners) = PERK_OWNERS.write() {
        *owners = perks
            .iter()
            .map(|(index, owners)| (*index, owner_name(owners)))
            .collect();
    }
    // Registered before the entity names, whose registration bumps the generation the label
    // caches follow.
    entity::catalog::register_game_names(
        entities
            .iter()
            .filter_map(|(tag, named)| Some((*tag, shared_name(named)?)))
            .collect(),
    );
    ability_bank::register_bank_names(
        banks
            .iter()
            .filter_map(|(tag, named)| Some((*tag, format!("{} Bank", shared_name(named)?))))
            .collect(),
        listings
            .into_iter()
            .map(|(key, names)| (key, names.into_iter().collect()))
            .collect(),
    );
}
