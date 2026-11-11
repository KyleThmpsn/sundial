//! Cosmetic collectibles for an imported weapon's own ornaments, which acquire with the weapon. An
//! Exotic's join the native ornament's weapon-type page under Exotic and Weapon Ornaments, and an
//! ordinary weapon's its weapon type's page under Weapons and Weapon Ornaments, as stock ones do.
use super::*;
use crate::progression::*;
use crate::tag_payload::{array_at, read_u16, write_localized_reference, write_u16};

/// The Exotic native ornament's one weapon-type page and the count pools it is counted in.
fn exotic_page(
    emission: &PackageEmission,
    donor: usize,
    source_unlock: usize,
) -> AuthoringResult<(u16, Vec<u16>, SunriseAcquiredPoolSelection)> {
    let parents = template_presentation_parents(&emission.nodes, &emission.collectibles, donor)?;
    let [leaf] = parents.as_slice() else {
        return Err(invalid("Native ornament must have one weapon-type leaf"));
    };
    let leaf = *leaf;
    let (_, _, node_rows, _) = array_at(&emission.nodes, 8)?;
    let parent = |index: u16| -> AuthoringResult<u16> {
        let (count, _, rows, _) = array_at(
            &emission.nodes,
            node_rows + index as usize * PRESENTATION_NODE_ROW_SIZE + 0x18,
        )?;
        if count != 1 {
            return Err(invalid("Ambiguous ornament collection ancestry"));
        }
        read_u16(&emission.nodes, rows)
    };
    let ornaments = parent(leaf)?;
    let exotics = parent(ornaments)?;
    if read_u32(
        &emission.nodes,
        node_rows + ornaments as usize * PRESENTATION_NODE_ROW_SIZE + 0x28,
    )? != 0x47D9E328
        || read_u32(
            &emission.nodes,
            node_rows + exotics as usize * PRESENTATION_NODE_ROW_SIZE + 0x28,
        )? != 0x3FB0E331
    {
        return Err(invalid(
            "Ornament donor is not in Exotics / Weapon Ornaments / weapon type",
        ));
    }
    let selection = classify_sunrise_count_pools(
        &emission.pools,
        &emission.nodes,
        &emission.collectibles,
        &emission.objectives,
        &parents,
        leaf,
        source_unlock as u16,
    )?;
    Ok((leaf, parents, selection))
}

/// The child row naming `collectible` among stock node `node`'s collectibles. An importer's
/// entries placed first take the rows of stock entries (`collection::front`), so an earlier one may
/// hold this one's place, and the row then comes from the stock table.
fn child_row(
    manager: &sundial::package_authoring::PackageManager,
    emission: &PackageEmission,
    node: usize,
    collectible: usize,
) -> AuthoringResult<crate::collection::front::Row> {
    crate::progression::presentation::collectible_child_row(&emission.nodes, node, collectible)
        .or_else(|_| {
            let stock = crate::tag_payload::read_tag(
                manager,
                emission.table_tags.presentation_node_table_tag,
                "Collections presentation nodes",
            )?;
            crate::progression::presentation::collectible_child_row(&stock, node, collectible)
        })
}

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    graph: &Value,
) -> AuthoringResult<()> {
    let Some(link) = graph.get("ornament") else {
        return Ok(());
    };
    // Only a weapon's own ornaments join Collections here, and only their links name the weapon as
    // `target_weapon`. An armor ornament's link names just its armor as `target_item`, which
    // binding gear art also adds to a Rise of Iron weapon ornament's link, so that field cannot
    // tell them apart.
    if link.get("target_weapon").is_none() {
        return Ok(());
    }
    let value = |v: &Value| {
        v.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("Ornament collection identity"))
    };
    let target = value(&graph["item_hash"])?;
    let donor_hash = value(&link["native_plug"])?;
    let owner_hash = value(&link["target_weapon"])?;
    let owner = emission
        .plans
        .iter()
        .find(|p| p.item_hash == owner_hash)
        .ok_or_else(|| invalid("Ornament owner plan"))?;
    // An imported cosmetic choice is acquired with its authored weapon, through the same flag or
    // unconditionally as the weapon is.
    let acquisition = owner
        .collection
        .as_ref()
        .ok_or_else(|| invalid("Ornament owner has no Collections entry"))?
        .unlock
        .map_or(Acquisition::Always, |unlock| {
            Acquisition::Flag(unlock.definition_index)
        });
    let (n, _, rows, _) = array_at(&emission.item_table, 8)?;
    let index = |hash| -> AuthoringResult<usize> {
        let found = (0..n)
            .filter(|i| read_u32(&emission.item_table, rows + i * 24).ok() == Some(hash))
            .collect::<Vec<_>>();
        if found.len() != 1 {
            return Err(invalid("Ornament collection item missing or ambiguous"));
        }
        Ok(found[0])
    };
    let donor_index = index(donor_hash)?;
    let item_index =
        u16::try_from(index(target)?).map_err(|_| invalid("Ornament item index overflow"))?;
    let (count, _, rows, _) = array_at(&emission.collectibles, 8)?;
    let donors = (0..count)
        .filter(|i| {
            read_u16(
                &emission.collectibles,
                rows + i * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
            )
            .ok()
                == Some(donor_index as u16)
        })
        .collect::<Vec<_>>();
    let [donor] = donors.as_slice() else {
        return Err(invalid("Native ornament must have one collectible"));
    };
    let donor = *donor;
    let donor_row = rows + donor * COLLECTIBLE_ROW_SIZE;
    let source_unlock = collection_unlock_index(&emission.collectibles, donor_row)?;
    // Weapon emission has already appended count programs after the parallel
    // lookup. Restore the compiler's terminal-array contract without changing
    // those programs or their pointers before extending cosmetic counts. An entry acquired
    // unconditionally extends no count, so the pool stays as it is.
    if acquisition.flag().is_some() {
        validate_shared_expression_table(&emission.pools, false)?;
        let (parallel_count, parallel_header, parallel_rows, _) = array_at(&emission.pools, 0x18)?;
        let parallel = emission.pools[parallel_header..parallel_rows + parallel_count * 2].to_vec();
        while !emission.pools.len().is_multiple_of(16) {
            emission.pools.push(0);
        }
        let new_parallel = emission.pools.len();
        emission.pools.extend_from_slice(&parallel);
        crate::tag_payload::write_relative_pointer(&mut emission.pools, 0x20, new_parallel)?;
    }
    // An ordinary weapon's planned page counts it directly. An Exotic's counts as the native
    // ornament's pools count that ornament.
    let weapon_page = emission.ornament_targets.get(&owner_hash).copied();
    let (leaf, parents, selection) = match weapon_page {
        Some(page) => (
            page.page,
            vec![page.page],
            SunriseAcquiredPoolSelection::default(),
        ),
        None => exotic_page(emission, donor, source_unlock)?,
    };
    let mut identity =
        WeaponCloneIdentity::from_namespace(&format!("parhelion.imported-ornament-{target:08x}"))?;
    if (0..count).any(|i| {
        read_u32(
            &emission.collectibles,
            rows + i * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_HASH_OFFSET,
        )
        .ok()
            == Some(identity.collectible_hash)
    }) {
        return Err(invalid("Imported ornament collectible identity collision"));
    }
    let material = read_u16(
        &emission.collectibles,
        donor_row + COLLECTIBLE_MATERIAL_SET_OFFSET,
    )?;
    let acquisition_mode = emission.collectibles[donor_row];
    let reacquisition = read_u16(
        &emission.collectibles,
        donor_row + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
    )?;
    emission.collectibles = append_collectible(
        std::mem::take(&mut emission.collectibles),
        donor,
        AuthoredCollectibleSpec {
            collectible_hash: identity.collectible_hash,
            item_index,
            unlock: CollectibleUnlockClone {
                source_index: source_unlock,
                acquisition,
            },
            material_set_index: material,
            presentation_parents: &parents,
            require_donor_parent_subset: weapon_page.is_none(),
        },
    )?;
    let (_, _, new_rows, _) = array_at(&emission.collectibles, 8)?;
    let new_row = new_rows + count * COLLECTIBLE_ROW_SIZE;
    // Preserve cosmetic acquisition/reacquisition semantics, not weapon costs.
    emission.collectibles[new_row] = acquisition_mode;
    write_u16(
        &mut emission.collectibles,
        new_row + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
        reacquisition,
    )?;
    let ordinal = definition_ordinal(emission, target)?;
    let strings = &emission.host_new_tags[ordinal + 1].payload;
    identity.name_hash = read_u32(strings, ITEM_NAME_REFERENCE_OFFSET + 4)?;
    identity.flavor_hash = read_u32(strings, ITEM_DESCRIPTION_REFERENCE_OFFSET + 4)?;
    let table = read_u32(strings, ITEM_NAME_REFERENCE_OFFSET)?;
    let description_table = read_u32(strings, ITEM_DESCRIPTION_REFERENCE_OFFSET)?;
    let icon = read_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    emission.collectible_displays = append_collectible_display(
        std::mem::take(&mut emission.collectible_displays),
        donor,
        identity,
        icon,
        table,
    )?;
    let (_, _, display_rows, _) = array_at(&emission.collectible_displays, 8)?;
    let display = display_rows + count * COLLECTIBLE_DISPLAY_ROW_SIZE;
    write_localized_reference(
        &mut emission.collectible_displays,
        display + COLLECTIBLE_DISPLAY_DESCRIPTION_REFERENCE_OFFSET,
        description_table,
        identity.flavor_hash,
    )?;
    write_localized_reference(
        &mut emission.collectible_displays,
        display + COLLECTIBLE_DISPLAY_SOURCE_REFERENCE_OFFSET,
        0xFFFF,
        0x811C9DC5,
    )?;
    // The entry's row copies a stock child: on a Weapon Ornaments page, the one the page was
    // cloned from, which a page the build added has none of its own, and beside an Exotic, the
    // native ornament's.
    let template = match weapon_page {
        Some(page) => child_row(
            manager,
            emission,
            usize::from(page.child_page),
            page.child_donor,
        )?,
        None => child_row(manager, emission, usize::from(leaf), donor)?,
    };
    let first = crate::collection::leads(
        None,
        usize::from(leaf),
        PRESENTATION_NODE_COLLECTIBLES_OFFSET,
    );
    // Every member's ornaments go before the stock entries of a stock page, as its weapons do,
    // each member's in rows of its own (`collection::front`).
    if first && usize::from(leaf) < crate::progression::STOCK_PRESENTATION_NODE_COUNT {
        let mut row = template;
        write_u16(
            &mut row,
            0,
            u16::try_from(count).map_err(|_| invalid("Ornament collectible index overflow"))?,
        )?;
        crate::collection::front::place_first(&mut emission.nodes, usize::from(leaf), &[row])?;
    } else {
        crate::progression::presentation::add_templated_collectible_children(
            &mut emission.nodes,
            usize::from(leaf),
            &[count],
            template,
            first,
        )?;
    }
    // An entry acquired unconditionally has no flag, and counts in its page below instead.
    if acquisition.flag().is_some() {
        emission.objectives = patch_project_collection_objectives(
            std::mem::take(&mut emission.objectives),
            &emission.nodes,
            &BTreeMap::from([(leaf, 1)]),
            1,
        )?;
    }
    let row = ProjectAuthoredRow {
        donor_collectible_index: donor,
        authored_collectible_index: count,
        pages: crate::collection::CollectionPages::one(leaf),
        classes: crate::collection::Classes::ALL,
        source_acquired_flag: source_unlock as u16,
        acquisition,
        count_selection: selection,
    };
    if acquisition.flag().is_some() {
        if weapon_page.is_some() {
            add_direct_acquired_counts(
                &mut emission.objectives,
                &emission.nodes,
                std::slice::from_ref(&row),
                &emission.pools,
            )?;
        } else {
            emission.pools = patch_project_acquired_count_programs(
                std::mem::take(&mut emission.pools),
                std::slice::from_ref(&row),
            )?;
        }
    } else {
        // An entry acquired unconditionally counts in its page's total and acquired count, as
        // the build's items do.
        let objective_table = emission.table_tags.objective_table_tag;
        crate::item::build::count_constant_entries(
            (&mut emission.objectives, &emission.nodes, &emission.pools),
            std::slice::from_ref(&row),
            objective_table,
            false,
        )?;
    }
    for data in [
        &mut emission.collectibles,
        &mut emission.collectible_displays,
        &mut emission.nodes,
        &mut emission.objectives,
        &mut emission.pools,
    ] {
        let len = data.len() as u64;
        crate::tag_payload::write_u64(data, 0, len)?;
    }
    eprintln!(
        "Imported ornament {target:08X} collectible {} on Collections page {leaf}; acquired by {acquisition:?}",
        identity.collectible_hash
    );
    Ok(())
}
