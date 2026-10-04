//! Ornament appearances: lay the ornament's gear-art row over its weapon's own row.
//!
//! An ornament row lists only the parts the ornament replaces. The game keeps the weapon's own
//! part in every slot the ornament leaves empty, so an ornament row used as the whole appearance
//! would drop those parts. The authored item gets a private row with the ornament's parts on top
//! of the appearance weapon's, matched by slot selector.
use super::*;
use crate::tag_payload::{read_u64, relative_target};

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<()> {
    for spec in weapons {
        if spec.overrides.art_arrangements.is_none() {
            continue;
        }
        let appearance = spec
            .presentation_donor
            .as_ref()
            .map_or(spec.donor_item_hash, |donor| donor.item_hash);
        overlay(emission, manager, spec.identity.item_hash, appearance).map_err(|error| {
            error.context(format!(
                "Weapon {:?}: laying its appearance override over 0x{appearance:08X}",
                spec.text.name
            ))
        })?;
    }
    Ok(())
}

fn overlay(
    emission: &mut PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    item: u32,
    appearance: u32,
) -> AuthoringResult<()> {
    let ordinal = definition_ordinal(emission, item)?;
    let authored = weapon_art_arrangements(&emission.host_new_tags[ordinal].payload)?;
    let base = weapon_art_arrangements(&reskin::stock_definition(emission, manager, appearance)?)?;
    let [top] = authored
        .iter()
        .map(|row| row.arrangement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()[..]
    else {
        return Ok(());
    };
    let [bottom] = base
        .iter()
        .map(|row| row.arrangement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()[..]
    else {
        return Ok(());
    };
    if top == bottom {
        return Ok(());
    }
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    if usize::from(top) >= count || usize::from(bottom) >= count {
        return Err(invalid("Gear-art row is outside the metadata table"));
    }
    // A row the item already owns was authored for it, such as an imported model.
    if read_u32(&emission.item_metadata, rows + usize::from(top) * 32)? == item {
        return Ok(());
    }
    let fills = fills(&emission.item_metadata, rows, top, bottom)?;
    if fills.is_empty() {
        return Ok(());
    }
    let arrangement = rewrite_row(emission, item, top, &fills)?;
    let updated = authored
        .iter()
        .map(|row| WeaponArtArrangementOverride {
            character_class: row.character_class,
            arrangement,
        })
        .collect::<Vec<_>>();
    set_weapon_art_arrangements(&mut emission.host_new_tags[ordinal].payload, &updated)
}

/// The keys the appearance weapon's row holds in every slot the ornament's row leaves empty.
fn fills(
    metadata: &[u8],
    rows: usize,
    top: u16,
    bottom: u16,
) -> AuthoringResult<BTreeMap<u64, Vec<u32>>> {
    let under = slots(metadata, rows + usize::from(bottom) * 32)?;
    let over = slots(metadata, rows + usize::from(top) * 32)?;
    Ok(over
        .iter()
        .filter(|(_, keys)| keys.is_empty())
        .filter_map(|(selector, _)| {
            under
                .iter()
                .find(|(candidate, keys)| candidate == selector && !keys.is_empty())
                .cloned()
        })
        .collect())
}

/// Give the item a private copy of the ornament's row with the fills laid in. Returns the
/// copy's arrangement index.
fn rewrite_row(
    emission: &mut PackageEmission,
    item: u32,
    top: u16,
    fills: &BTreeMap<u64, Vec<u32>>,
) -> AuthoringResult<u16> {
    let row_index = art::prepare_row_at(emission, item, usize::from(top))?;
    let (_, _, rows, _) = array(&emission.item_metadata, 8)?;
    art::rewrite(
        &mut emission.item_metadata,
        rows + row_index * 32,
        rows + usize::from(top) * 32,
        |_, slots| {
            for (selector, keys) in slots.iter_mut() {
                if keys.is_empty()
                    && let Some(fill) = fills.get(selector)
                {
                    keys.clone_from(fill);
                }
            }
            Ok(())
        },
    )?;
    u16::try_from(row_index).map_err(|_| invalid("Gear-art row index overflow"))
}

/// Every slot of a metadata row as `(selector, keys)`.
fn slots(data: &[u8], row: usize) -> AuthoringResult<Vec<(u64, Vec<u32>)>> {
    if read_u64(data, row + 16)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, entries, _) = array(data, row + 16)?;
    (0..count)
        .map(|slot| {
            let resource = relative_target(data, entries + slot * 8)?;
            let (n, _, keys, _) = array(data, resource + 8)?;
            Ok((
                read_u64(data, resource)?,
                (0..n)
                    .map(|j| read_u32(data, keys + j * 4))
                    .collect::<AuthoringResult<Vec<_>>>()?,
            ))
        })
        .collect()
}

fn array(data: &[u8], at: usize) -> AuthoringResult<(usize, usize, usize, u32)> {
    sundial::package_authoring::native_payload::native_array_at(data, at).map_err(invalid)
}
