use super::*;

#[test]
fn native_slot_check_rejects_mismatched_or_corrupt_equipment_and_ignores_plugs() {
    use crate::tag_payload::{write_u16, write_u32, write_u64};
    let mut item = vec![0; 256];
    write_u64(
        &mut item,
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET,
        0xD0 - ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET as u64,
    )
    .unwrap();
    write_u32(&mut item, 0xCC, ITEM_EQUIPMENT_BLOCK_CLASS).unwrap();
    write_u16(
        &mut item,
        0xD0 + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET,
        u16::MAX,
    )
    .unwrap();
    for bucket in 0..3 {
        item[ITEM_INVENTORY_SLOT_OFFSET] = bucket;
        write_u16(
            &mut item,
            0xD0 + ITEM_EQUIPMENT_SLOT_OFFSET,
            u16::from(bucket) + 7,
        )
        .unwrap();
        assert_eq!(native_weapon_slot(&item).unwrap(), Some(bucket));
        write_u16(
            &mut item,
            0xD0 + ITEM_EQUIPMENT_SLOT_OFFSET,
            u16::from((bucket + 1) % 3) + 7,
        )
        .unwrap();
        assert!(native_weapon_slot(&item).is_err());
    }
    item[ITEM_INVENTORY_SLOT_OFFSET] = 255;
    assert_eq!(native_weapon_slot(&item).unwrap(), None);
    assert!(native_weapon_slot(&item[..ITEM_INVENTORY_SLOT_OFFSET]).is_err());
}

#[test]
#[ignore = "builds isolated native generations, requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_SLOT_REPLACEMENT_ROOT"]
fn native_slot_replacement_compares_compiled_generations_in_both_directions() {
    use crate::recipe::RecipeInventorySlot;
    use crate::{
        BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, build_and_stage_snapshot_with_progress,
    };
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_SLOT_REPLACEMENT_ROOT").unwrap());
    let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
        "Slot Replacement Check",
        0xA25B_8F8F,
        "Arc Logic",
    )
    .unwrap();
    let mut stages = vec![];
    for slot in [RecipeInventorySlot::Kinetic, RecipeInventorySlot::Energy] {
        recipe.overrides.inventory_slot = Some(slot);
        let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
            package_directory: packages.clone(),
            staging_root: output.clone(),
            ignore_installed_authored_overlays: true,
            recipes: vec![recipe.clone()],
        })
        .unwrap();
        stages.push(
            build_and_stage_snapshot_with_progress(&snapshot, |_| {})
                .unwrap()
                .run_directory,
        );
    }
    for (old, new, previous, incoming) in [
        (&stages[0], &stages[1], 0, 1),
        (&stages[1], &stages[0], 1, 0),
    ] {
        let (hashes, _) = generation_identities(&packages, old).unwrap();
        let replacement = with_generation(&packages, old, |installed| {
            slot_replacement(installed, new, &hashes)
        })
        .unwrap()
        .unwrap();
        assert_eq!(replacement.changes.len(), 1);
        assert_eq!(replacement.changes[0].previous_bucket, previous);
        assert_eq!(replacement.changes[0].incoming_bucket, incoming);
        assert_eq!(
            replacement.incoming_buckets[&replacement.changes[0].definition_hash],
            incoming
        );
        assert!(replacement.incoming_buckets.len() > 1000);
        assert!(
            replacement
                .weapon_capacities
                .iter()
                .all(|capacity| *capacity > 1)
        );
        assert!(
            with_generation(&packages, old, |installed| slot_replacement(
                installed, old, &hashes
            ))
            .unwrap()
            .is_none()
        );
    }
}
