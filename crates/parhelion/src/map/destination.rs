//! Custom destination labels reuse blank rows that no stock activity references. The host's
//! destination capacity stays unchanged, and native destination identities remain intact.
use super::{MapRecipe, activity, plan::allocate};
use crate::{
    AuthoringResult, ReplacementSpec,
    error::invalid,
    tag_payload::{array_at, read_tag, read_u32, write_localized_reference},
};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

#[derive(Default)]
pub(super) struct Plan {
    pub rows: BTreeMap<String, u8>,
    pub strings: Vec<(u32, String)>,
    pub replacement: Option<ReplacementSpec>,
}

/// The display table and its rows a label may take, in row order.
pub(super) struct Rows {
    tag: TagHash,
    display: Vec<u8>,
    displays: usize,
    pub available: Vec<usize>,
}

pub(super) fn prepare(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    activities: &[u8],
    recipes: &[MapRecipe],
    floor: u32,
    taken: &mut BTreeSet<u32>,
) -> AuthoringResult<Plan> {
    let labels = recipes
        .iter()
        .filter_map(|r| r.destination.as_ref())
        .collect::<BTreeSet<_>>();
    if labels.is_empty() {
        return Ok(Plan::default());
    }
    let Rows {
        tag: display_tag,
        mut display,
        displays,
        available,
    } = rows(manager, root, globals, activities)?;
    if labels.len() > available.len() {
        return Err(invalid("No unused blank destination row is available"));
    }
    let mut plan = Plan::default();
    let bank = u32::try_from(crate::package_profile::LOCALIZATION_DONOR_TABLE_INDEX)
        .map_err(|_| invalid("Destination localization bank exceeds 32 bits"))?;
    for (label, index) in labels.into_iter().zip(available) {
        if !super::destination_label_valid(label) {
            return Err(invalid(
                "Destination label is empty, too long or contains a control character",
            ));
        }
        let hash = allocate("destination", label, |hash| {
            hash > floor && !taken.contains(&hash)
        })?;
        taken.insert(hash);
        write_localized_reference(&mut display, displays + index * 56 + 4, bank, hash)?;
        plan.rows.insert(label.clone(), index as u8);
        plan.strings.push((hash, label.clone()));
    }
    plan.replacement = Some(ReplacementSpec {
        tag: display_tag,
        payload: display,
    });
    Ok(plan)
}

/// The rows of the fixed 48 that no stock activity references and whose display title is empty.
pub(super) fn rows(
    manager: &PackageManager,
    root: &[u8],
    globals: &[u8],
    activities: &[u8],
) -> AuthoringResult<Rows> {
    let definition_tag = TagHash(read_u32(root, 8 + 25 * 16)?);
    let display_tag = TagHash(read_u32(globals, 16 + 21 * 16)?);
    for (tag, expected) in [(definition_tag, 0x8080_7768), (display_tag, 0x8080_5b27)] {
        if manager.get_entry(tag).map(|e| e.reference) != Some(expected) {
            return Err(invalid("Native destination table class changed"));
        }
    }
    let definition = read_tag(manager, definition_tag, "destination definitions")?;
    let display = read_tag(manager, display_tag, "destination display")?;
    let (count, _, definitions, class) = array_at(&definition, 8)?;
    let (display_count, _, displays, display_class) = array_at(&display, 8)?;
    if count != super::DESTINATION_ROWS
        || display_count != count
        || class != 0x8080_776c
        || display_class != 0x8080_5b2b
    {
        return Err(invalid(
            "Native destination rows differ from the supported host layout",
        ));
    }
    let used = activity::Rows::read(activities)?.destinations(activities)?;
    let mut available = Vec::new();
    for index in 0..count {
        let row = displays + index * 56;
        if read_u32(&definition, definitions + index * 56)? != read_u32(&display, row)? {
            return Err(invalid(
                "Destination definition and display identities disagree",
            ));
        }
        if !used.contains(&(index as u8))
            && read_u32(&display, row + 4)? == 0xffff
            && read_u32(&display, row + 8)? == 0x811c_9dc5
        {
            available.push(index);
        }
    }
    Ok(Rows {
        tag: display_tag,
        display,
        displays,
        available,
    })
}
