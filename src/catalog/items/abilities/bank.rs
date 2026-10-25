//! Catalog-only discovery of stock and privately allocated ability banks.
use super::*;
use crate::{
    ability::{
        AbilityTarget,
        bank::{SLOT_BANKS, UNPLACED_BANKS},
        modifier,
    },
    package_payload::{native_array_at, rows_fit, u64_at},
};
use std::collections::BTreeSet;

type Classes = (u32, u32);

/// Selection slots come from qualified stock identities. A class shared by more than one
/// slot stays ambiguous after private allocation unless its controller establishes the role.
pub(super) struct Banks {
    slots: BTreeMap<Classes, BTreeSet<Option<AbilityTarget>>>,
}

impl Banks {
    pub(super) fn new(manager: &PackageManager) -> Self {
        let mut slots = BTreeMap::<Classes, BTreeSet<Option<AbilityTarget>>>::new();
        let stock = SLOT_BANKS
            .iter()
            .flat_map(|(slot, banks)| banks.iter().map(move |&tag| (tag, Some(*slot))))
            .chain(UNPLACED_BANKS.iter().map(|&tag| (tag, None)));
        for (tag, slot) in stock {
            if let Ok(payload) = manager.read_tag(TagHash(tag))
                && let Ok(classes) = root(&payload, tag)
            {
                slots.entry(classes).or_default().insert(slot);
            }
        }
        Self { slots }
    }

    pub(super) fn entity(
        &self,
        manager: &PackageManager,
        payload: &[u8],
    ) -> Result<Option<(u32, Option<AbilityTarget>)>, String> {
        let (count, _, first, class) = native_array_at(payload, 16)?;
        if class != 0x8080_9C04 {
            return Err("Ability component table has an incompatible class".into());
        }
        rows_fit(payload, first, count, 12)?;
        let mut components = BTreeMap::new();
        for index in 0..count {
            let tag = u32_at(payload, first + index * 12)?;
            if components.contains_key(&tag)
                || !manager
                    .get_entry(TagHash(tag))
                    .is_some_and(|entry| entry.reference == 0x8080_9C36)
            {
                continue;
            }
            let owner = manager
                .read_tag(TagHash(tag))
                .map_err(|error| error.to_string())?;
            if let Ok(classes) = root(&owner, tag) {
                components.insert(tag, classes);
            }
        }
        let radiance = components
            .values()
            .any(|pair| *pair == (0x8080_4176, 0x8080_4177));
        let mut result = None;
        for (tag, classes) in components {
            if !modifier::is_bank(tag)
                && !(crate::runtime::native_type_inherits(classes.0, 0x8080_44FC)
                    && crate::runtime::native_type_inherits(classes.1, 0x8080_44FD))
            {
                continue;
            }
            let slot = modifier::bank_slot(tag).or_else(|| {
                if classes == (0x8080_417A, 0x8080_417B) && radiance {
                    return Some(AbilityTarget::Super);
                }
                let candidates = self.slots.get(&classes)?;
                (candidates.len() == 1)
                    .then(|| candidates.first().copied().flatten())
                    .flatten()
            });
            if result.replace((tag, slot)).is_some() {
                return Err("Ability entity has more than one qualified bank".into());
            }
        }
        Ok(result)
    }
}

/// Keep the original reciprocal pair distinct from the loader's optional sealed boundary.
fn root(payload: &[u8], tag: u32) -> Result<Classes, String> {
    let instance = relative_offset(16, 0, i64_at(payload, 16)?)?;
    let boundary = relative_offset(24, 0, i64_at(payload, 24)?)?;
    if instance < 4
        || instance
            .checked_add(16)
            .is_none_or(|end| end > payload.len())
    {
        return Err("Ability owner instance leaves its payload".into());
    }
    let definition =
        usize::try_from(u64_at(payload, instance + 8)?).map_err(|error| error.to_string())?;
    if u64_at(payload, 0)? != payload.len() as u64
        || instance < 4
        || instance >= definition
        || definition > boundary
        || boundary >= payload.len()
        || u32_at(payload, instance)? != tag
        || u32_at(payload, definition)? != tag
        || u32_at(payload, boundary)? != tag
        || u64_at(payload, definition + 8)? != instance as u64
        || u64_at(payload, boundary + 8)? != instance as u64
    {
        return Err("Ability owner root is not reciprocal".into());
    }
    let classes = (
        u32_at(payload, instance - 4)?,
        u32_at(payload, definition - 4)?,
    );
    if u32_at(payload, instance + 4)? != classes.1
        || u32_at(payload, definition + 4)? != classes.0
        || u32_at(payload, boundary - 4)? != classes.1
        || u32_at(payload, boundary + 4)? != classes.0
    {
        return Err("Ability owner root classes disagree".into());
    }
    Ok(classes)
}
