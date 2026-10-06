//! Property rows for the ability banks that lack them, so a private perk's Ability Property
//! actions work on every class.
//!
//! Two kinds of row are authored. A charge row gives the melee and class ability banks the
//! +1 charge that Bungie put under `Extra Melee Charge` and `Extra Class Ability Charge` in
//! one bank each, so those stock keys reach every class. A tuning row is a perk's own: the
//! program defines a script parameter and value under a key of its own
//! (`Program::ability_tunings`), and every bank of the slot that lists the parameter gets the
//! row. Which keys, slots, banks and parameters exist is stock knowledge kept with the bank
//! editor in `sundial::package_authoring::ability_bank`; this stage only asks which of them a
//! project's private perks apply, and replaces the stock banks still without the row in the
//! sandbox packages. A bank whose rows show no handler slot for the modifier is left alone,
//! since a guessed slot faults the client at world load. The edit moves the bank's instance
//! region, and the ability entities name the bank's blocks by absolute offset, so every tag
//! the reference index lists for an edited bank ships with those offsets moved. A row is
//! inert until a perk applies its key, so stock play does not change.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

use sundial::package_authoring::{
    PackageManager,
    ability_bank::{
        AbilityTarget, CHARGE_ROWS, Modifier, bank_owner, handler_slot, parameters, property_rows,
        retarget_references, slot_banks, with_property_row,
    },
    referrers,
    sandbox_perk::program::{AbilityInput, AbilityTuning, Program, ability_properties_in},
};
use tiger_pkg::TagHash;

use crate::{AuthoringResult, ReplacementSpec, error::invalid};

/// A private perk as this stage sees it: its authored program, if it has one, and the stock
/// action it clones otherwise.
pub(crate) struct Perk<'a> {
    pub program: Option<&'a Program>,
    pub stock_action: &'a [u8],
}

/// Private damage profiles selected only by an equipped item's property action, with the
/// item's own per-surface impact responses when it has them.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct MeleeImpact {
    pub key: u32,
    pub damage: u32,
    #[serde(default)]
    pub responses: Option<u32>,
}

/// The banks a project replaces, and the rows it adds to each.
pub(crate) struct Plan {
    /// The replaced banks and their referrers, by the sandbox package each lives in.
    pub replacements: BTreeMap<u16, Vec<ReplacementSpec>>,
    /// Each edited stock bank's added rows, in the order they were added, so a private copy of
    /// the bank can take them too.
    pub rows: BTreeMap<u32, Vec<(u32, Modifier)>>,
}

/// The replaced banks a project needs, by the sandbox package each lives in: for each charge
/// key its private perks apply, every stock bank of that slot still without the row, and for
/// each tuning they apply, every bank of the slot that lists the parameter, each with the row
/// added.
/// `rows` adds explicit property rows, each to one bank: a Subclass ability modifier's charges
/// or parameter value under a key of its own.
pub(crate) fn plan<'a>(
    manager: &PackageManager,
    perks: impl IntoIterator<Item = Perk<'a>>,
    impacts: &[MeleeImpact],
    rows: &[(u32, u32, Modifier)],
) -> AuthoringResult<Plan> {
    let mut applied = BTreeSet::new();
    let mut tunings: BTreeMap<u32, AbilityTuning> = BTreeMap::new();
    let mut inputs: BTreeMap<u32, AbilityInput> = BTreeMap::new();
    for perk in perks {
        applied.extend(keys_applied(&perk)?);
        if let Some(program) = perk.program {
            for input in &program.ability_inputs {
                input.validate().map_err(invalid)?;
                if inputs
                    .insert(input.key, input.clone())
                    .is_some_and(|old| old != *input)
                {
                    return Err(invalid("Private ability input keys conflict"));
                }
            }
            for tuning in &program.ability_tunings {
                // A key is a 32-bit hash of its tuning, so two different tunings can share one.
                // Keeping the first would quietly give every perk that applies the key its value.
                if tunings
                    .insert(tuning.key, tuning.clone())
                    .is_some_and(|old| old != *tuning)
                {
                    return Err(invalid(format!(
                        "Two private ability tunings share key {:08X} with different values. Change one of their values slightly",
                        tuning.key
                    )));
                }
            }
        }
    }
    let mut wanted: Vec<(u32, Vec<u32>, Modifier)> = CHARGE_ROWS
        .iter()
        .filter(|row| applied.contains(&(row.slot, row.key)))
        .map(|row| (row.key, row.banks.to_vec(), Modifier::Charges(1)))
        .collect();
    for tuning in tunings.values() {
        if !applied.contains(&(tuning.slot, tuning.key)) {
            continue;
        }
        wanted.push((
            tuning.key,
            slot_banks(tuning.slot).to_vec(),
            Modifier::Parameter {
                name: tuning.parameter,
                applied: f32::from_bits(tuning.value_bits),
                add: tuning.add,
            },
        ));
    }
    let mut impact_keys = BTreeMap::new();
    for input in inputs.values() {
        if !applied.contains(&(input.slot, input.key)) || tunings.contains_key(&input.key) {
            return Err(invalid(
                "Private ability input has no unique activating property",
            ));
        }
        wanted.push((
            input.key,
            slot_banks(input.slot).to_vec(),
            Modifier::Scalar {
                input: input.input,
                value: f32::from_bits(input.value_bits),
                multiply: input.multiply,
            },
        ));
    }
    for impact in impacts {
        if !applied.contains(&(
            sundial::package_authoring::ability_bank::MELEE_SLOT,
            impact.key,
        )) {
            return Err(invalid(
                "Imported melee feedback has no activating private perk",
            ));
        }
        if impact_keys.insert(impact.key, impact.damage).is_some() {
            return Err(invalid(
                "Imported melee feedback repeats a private property key",
            ));
        }
        wanted.push((
            impact.key,
            slot_banks(sundial::package_authoring::ability_bank::MELEE_SLOT).to_vec(),
            Modifier::Melee {
                damage: impact.damage,
                responses: impact.responses,
            },
        ));
    }
    // A Subclass ability modifier's row goes to the one bank its ability reads.
    for &(bank, key, modifier) in rows {
        wanted.push((key, vec![bank], modifier));
    }
    // A bank may take several rows, so each edit starts from the bank's latest payload.
    let mut edited: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut added: BTreeMap<u32, Vec<(u32, Modifier)>> = BTreeMap::new();
    let mut filed = BTreeSet::new();
    for (key, banks, modifier) in wanted {
        for &bank in &banks {
            // A key names what its row does, so a second request for it is the same row.
            if !filed.insert((bank, key)) {
                continue;
            }
            if let Some(next) = with_row(manager, &edited, bank, (key, modifier))? {
                edited.insert(bank, next);
                added.entry(bank).or_default().push((key, modifier));
            }
        }
    }
    // Every tag that references an edited bank follows the move of its instance region. An
    // edited bank that references another ships the payload with both changes, once.
    let retargeted = retarget_referrers(manager, &edited)?;
    edited.extend(retargeted);
    let mut replacements: BTreeMap<u16, Vec<ReplacementSpec>> = BTreeMap::new();
    for (bank, payload) in edited {
        let tag = TagHash(bank);
        replacements
            .entry(tag.pkg_id())
            .or_default()
            .push(ReplacementSpec { tag, payload });
    }
    Ok(Plan {
        replacements,
        rows: added,
    })
}

/// Gives each private copy of an ability bank among `tags` the rows `rows` added to its stock
/// bank, in the same order, as the stock bank took them. A copy names its blocks by its own tag,
/// as do the copies of ability entities that bind it, so every other tag in `tags` and `others`
/// that names the copy's blocks by offset follows the move of its instance region. Without the
/// rows, a key a perk or node applies to the ability would find nothing in the copy.
pub(crate) fn sync_private_banks(
    rows: &BTreeMap<u32, Vec<(u32, Modifier)>>,
    tags: &mut [crate::NewTagSpec],
    others: &mut [crate::NewTagSpec],
) -> AuthoringResult<()> {
    for index in 0..tags.len() {
        let stock = tags[index].template_tag;
        let Some(added) = rows.get(&stock.0) else {
            continue;
        };
        let context =
            |error: String| invalid(format!("Private copy of ability bank {stock}: {error}"));
        let before = tags[index].payload.clone();
        let owner = bank_owner(&before).map_err(context)?;
        let mut after = before.clone();
        for &(key, modifier) in added {
            if property_rows(&after)
                .map_err(context)?
                .iter()
                .any(|row| row.key == key)
            {
                continue;
            }
            after = with_property_row(&after, key, modifier).map_err(context)?;
        }
        if after == before {
            continue;
        }
        // The copy's own block headers already moved with the rows, so it is left out here.
        for (other, tag) in tags.iter_mut().enumerate() {
            if other == index {
                continue;
            }
            if let Some(next) =
                retarget_references(&tag.payload, owner, &before, &after).map_err(context)?
            {
                tag.payload = next;
            }
        }
        for tag in others.iter_mut() {
            if let Some(next) =
                retarget_references(&tag.payload, owner, &before, &after).map_err(context)?
            {
                tag.payload = next;
            }
        }
        tags[index].payload = after;
    }
    Ok(())
}

/// `bank`'s latest payload with a row for `key` added, or `None` when the bank has the row
/// already, does not list the parameter, or shows no handler slot for the modifier.
fn with_row(
    manager: &PackageManager,
    edited: &BTreeMap<u32, Vec<u8>>,
    bank: u32,
    (key, modifier): (u32, Modifier),
) -> AuthoringResult<Option<Vec<u8>>> {
    let tag = TagHash(bank);
    if crate::package_profile::canonical_package(tag.pkg_id()).is_none() {
        return Err(invalid(format!(
            "Ability bank {tag} is outside the packages the build overlays"
        )));
    }
    let payload = match edited.get(&bank) {
        Some(payload) => payload.clone(),
        None => manager
            .read_tag(tag)
            .map_err(|error| invalid(format!("Could not read ability bank {tag}: {error}")))?,
    };
    let context = |error: String| invalid(format!("Ability bank {tag}: {error}"));
    if let Some(existing) = property_rows(&payload)
        .map_err(context)?
        .into_iter()
        .find(|row| row.key == key)
    {
        // A row already under the key is only the same row when it does the same thing. A key
        // shared with an unrelated row would bind the perk to that row's behavior instead.
        let same = match modifier {
            Modifier::Charges(count) => existing.charge == Some(count),
            Modifier::Parameter { name, applied, add } => {
                existing.parameters.iter().any(|parameter| {
                    parameter.name == name
                        && parameter.applied.to_bits() == applied.to_bits()
                        && parameter.add == add
                })
            }
            Modifier::Melee { .. } | Modifier::Scalar { .. } => {
                return Err(invalid(format!(
                    "Ability bank {tag} already defines private melee key {key:08X}"
                )));
            }
        };
        if !same {
            return Err(invalid(format!(
                "Ability bank {tag} already uses key {key:08X} for a different property"
            )));
        }
        return Ok(None);
    }
    if let Modifier::Parameter { name, .. } = modifier
        && !parameters(&payload)
            .map_err(context)?
            .iter()
            .any(|parameter| parameter.name == name)
    {
        // Only a listed parameter is known to be read by this bank's script.
        return Ok(None);
    }
    if handler_slot(&payload, modifier).map_err(context)?.is_none() {
        // No stock row shows which slot takes the modifier.
        return Ok(None);
    }
    with_property_row(&payload, key, modifier)
        .map(Some)
        .map_err(context)
}

/// Every tag the reference index lists for an edited bank, with the offsets it names inside
/// that bank moved to the bank's edited layout. A referrer that is an edited bank itself starts
/// from its edited payload.
fn retarget_referrers(
    manager: &PackageManager,
    edited: &BTreeMap<u32, Vec<u8>>,
) -> AuthoringResult<BTreeMap<u32, Vec<u8>>> {
    let mut retargeted: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    if edited.is_empty() {
        return Ok(retargeted);
    }
    let index = referrers::read(manager, &AtomicBool::new(false), |_, _| {})
        .map_err(|error| invalid(format!("Could not read the reference index: {error}")))?;
    for (&bank, after) in edited {
        let tag = TagHash(bank);
        let before = manager
            .read_tag(tag)
            .map_err(|error| invalid(format!("Could not read ability bank {tag}: {error}")))?;
        for &referrer in index.of(bank) {
            if referrer == bank {
                continue;
            }
            let referrer_tag = TagHash(referrer);
            let payload = match retargeted.get(&referrer).or_else(|| edited.get(&referrer)) {
                Some(payload) => payload.clone(),
                None => manager.read_tag(referrer_tag).map_err(|error| {
                    invalid(format!(
                        "Could not read {referrer_tag}, which references ability bank {tag}: {error}"
                    ))
                })?,
            };
            let Some(next) = retarget_references(&payload, bank, &before, after)
                .map_err(|error| invalid(format!("Ability bank {tag}: {error}")))?
            else {
                continue;
            };
            if crate::package_profile::canonical_package(referrer_tag.pkg_id()).is_none() {
                return Err(invalid(format!(
                    "{referrer_tag} names blocks of ability bank {tag} by offset and is outside the packages the build overlays"
                )));
            }
            retargeted.insert(referrer, next);
        }
    }
    Ok(retargeted)
}

/// The slot and key of every Ability Property action a private perk applies, read from the
/// program it compiles to, or from the stock action it clones unchanged.
fn keys_applied(perk: &Perk<'_>) -> AuthoringResult<BTreeSet<(AbilityTarget, u32)>> {
    match perk.program {
        Some(program) => program.ability_properties(),
        None => ability_properties_in(perk.stock_action),
    }
    .map_err(invalid)
}

/// Moves the offsets a new tag names inside an edited bank, as the reference index moves the
/// stock referrers'. A private copy of an ability entity names its bank's blocks by offset too,
/// and the index does not know it.
pub(crate) fn retarget_new_tags(
    manager: &PackageManager,
    replaced: &BTreeMap<u16, Vec<ReplacementSpec>>,
    tags: &mut [crate::NewTagSpec],
) -> AuthoringResult<()> {
    for spec in replaced.values().flatten() {
        let bank = spec.tag.0;
        if !sundial::package_authoring::ability_modifier::is_bank(bank) {
            continue;
        }
        let before = manager.read_tag(spec.tag).map_err(|error| {
            invalid(format!("Could not read ability bank {}: {error}", spec.tag))
        })?;
        for tag in tags.iter_mut() {
            if let Some(next) = retarget_references(&tag.payload, bank, &before, &spec.payload)
                .map_err(|error| invalid(format!("Ability bank {}: {error}", spec.tag)))?
            {
                tag.payload = next;
            }
        }
    }
    Ok(())
}
