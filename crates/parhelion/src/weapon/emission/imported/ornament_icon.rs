//! Private ornament artwork uses Parhelion's existing checked PNG authoring.
use super::*;
use crate::tag_payload::{array_at, read_u16, write_u16};
pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    graph: &Value,
    package_index: usize,
    folder: &Path,
) -> AuthoringResult<()> {
    let Some(path) = graph["ornament_icon_png"].as_str() else {
        return Ok(());
    };
    if graph.get("ornament").is_none() {
        return Err(invalid("Ornament artwork requires a socket link"));
    }
    let target = graph["item_hash"]
        .as_u64()
        .ok_or_else(|| invalid("Ornament item hash"))? as u32;
    let ordinal = definition_ordinal(emission, target)?;
    let strings = &emission
        .host_new_tags
        .get(ordinal + 1)
        .ok_or_else(|| invalid("Ornament strings missing"))?
        .payload;
    let donor_index = read_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    let (count, _, rows, _) = array_at(&emission.item_icons, 8)?;
    if donor_index as usize >= count {
        return Err(invalid("Ornament donor icon outside table"));
    }
    let donor = TagHash(read_u32(
        &emission.item_icons,
        rows + donor_index as usize * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
    )?);
    let png = fs::read(folder.join(path)).map_err(|e| invalid(e.to_string()))?;
    let edit = crate::WeaponIconEdit {
        imported_image: Some(crate::icon_edit::ImportedIcon::from_bytes(&png).map_err(invalid)?),
        ..Default::default()
    };
    let package = &emission.asset_packages.packages[package_index];
    let package_id = package.id;
    let base = package.tags.len();
    let plan = crate::watermark::build_watermark_plan(
        manager,
        package_id,
        0,
        base,
        &[crate::watermark::WeaponIconRequest {
            donor_container_tag: donor,
            icon_edit: edit,
            rarity: crate::AuthoredWeaponRarity::Exotic,
        }],
    )?;
    let container = plan
        .container_for_request(0)
        .ok_or_else(|| invalid("Ornament icon container missing"))?;
    let added = (base..base + plan.new_tags.len())
        .map(|i| TagHash::new(package_id, i as u16))
        .collect::<Vec<_>>();
    emission.asset_packages.packages[package_index]
        .tags
        .extend(plan.new_tags);
    emission.asset_packages.packages[package_index]
        .references
        .extend(plan.reference_overrides);
    let (icons, icon_index) = append_authored_weapon_icon_row(
        std::mem::take(&mut emission.item_icons),
        donor_index,
        target,
        container,
    )?;
    emission.item_icons = icons;
    write_u16(
        &mut emission.host_new_tags[ordinal + 1].payload,
        ITEM_STRING_ICON_INDEX_OFFSET,
        icon_index,
    )?;
    let (count, _, rows, _) = array_at(&emission.item_table, 8)?;
    let item_index = (0..count)
        .find(|i| read_u32(&emission.item_table, rows + i * 24).ok() == Some(target))
        .ok_or_else(|| invalid("Ornament index missing"))?;
    let arrays = dense_item_presentation_arrays(&emission.dense)?;
    let selector = read_u32(
        &emission.dense,
        arrays[3].rows
            + item_index * ITEM_DENSE_PRESENTATION_ROW_SIZE
            + ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
    )? as usize;
    if selector >= arrays[2].count {
        return Err(invalid("Ornament dense selector invalid"));
    }
    let tag_index = read_u32(
        &emission.dense,
        arrays[2].rows + selector * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE,
    )? as usize;
    if tag_index >= arrays[0].count {
        return Err(invalid("Ornament dense icon invalid"));
    }
    let field = arrays[0].rows + tag_index * ITEM_DENSE_ICON_TAG_ROW_SIZE;
    if read_u32(&emission.dense, field)? != donor.0 {
        return Err(invalid("Ornament dense donor mismatch"));
    }
    // A private plug may still inherit a shared native selector. Append both
    // indirections and retarget only this authored item's presentation row.
    let selector_row = arrays[2].rows + selector * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE;
    let mut private_selector =
        emission.dense[selector_row..selector_row + ITEM_DENSE_ICON_SELECTOR_ROW_SIZE].to_vec();
    write_u32(
        &mut private_selector,
        0,
        u32::try_from(arrays[0].count).map_err(|_| invalid("Ornament icon index overflow"))?,
    )?;
    let private_selector_index =
        u32::try_from(arrays[2].count).map_err(|_| invalid("Ornament selector index overflow"))?;
    let mut dense = rebuild_dense_item_presentation_arrays(
        &emission.dense,
        [
            container.0.to_le_bytes().to_vec(),
            Vec::new(),
            private_selector,
            Vec::new(),
            Vec::new(),
        ],
    )?;
    let rebuilt = dense_item_presentation_arrays(&dense)?;
    let selector_field =
        item_index * ITEM_DENSE_PRESENTATION_ROW_SIZE + ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET;
    write_u32(
        &mut dense,
        rebuilt[3].rows + selector_field,
        private_selector_index,
    )?;
    // Read back every pre-existing array row. Only this item's selector may differ.
    for (i, before) in arrays.iter().enumerate() {
        let mut original_rows =
            dense[rebuilt[i].rows..rebuilt[i].rows + before.count * before.spec.row_size].to_vec();
        if i == 3 {
            write_u32(&mut original_rows, selector_field, selector as u32)?;
        }
        if original_rows != emission.dense[before.rows..before.rows_end] {
            return Err(invalid(
                "Ornament icon changed another dense presentation field",
            ));
        }
    }
    emission.dense = dense;
    let root = match &emission.runtime_dependencies {
        Some(root) => root.clone(),
        None => manager
            .read_tag(RUNTIME_DEPENDENCY_COMPANION)
            .map_err(|e| invalid(e.to_string()))?,
    };
    emission.runtime_dependencies = Some(crate::shared_tag_dependency_index::enroll_dependencies(
        &root,
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
        &added,
    )?);
    Ok(())
}
