//! Experimental slot/damage pairings: package evidence, not a gameplay claim.
use super::*;

#[test]
fn moving_slots_does_not_replace_a_legacy_damage_family_with_modern() {
    for target in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        let damage = WeaponDamageDescriptor::Elemental(ModernDamageType::Arc);
        let mut definition = synthetic_weapon_definition(WeaponInventorySlot::Energy, damage);
        let mut strings = synthetic_item_strings(damage);
        let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
        let (_, _, rows, _) =
            array_at(&definition, resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET).unwrap();
        write_u16(&mut definition, rows, LEGACY_ARC_DAMAGE_PERK_INDEX).unwrap();
        apply_weapon_slot_and_damage_overrides(
            &mut definition,
            &mut strings,
            &WeaponCloneOverrides {
                inventory_slot: Some(target),
                modern_damage_type: Some(ModernDamageType::Solar),
                ..Default::default()
            },
            Some(&ResolvedDamageCarrierSource {
                family: WeaponDamageCarrierFamily::ModernFixed,
                topology_definition: None,
            }),
            &synthetic_sandbox_perk_definition_template(),
            &synthetic_sandbox_perk_string_template(),
        )
        .unwrap();
        assert_eq!(
            weapon_sandbox_perks(&definition).unwrap(),
            [LEGACY_SOLAR_DAMAGE_PERK_INDEX]
        );
        assert_eq!(weapon_inventory_slot(&definition).unwrap(), target);
    }
}

#[test]
fn adding_a_fixed_carrier_uses_the_proven_source_family() {
    for family in [
        WeaponDamageCarrierFamily::LegacyFixed,
        WeaponDamageCarrierFamily::ModernFixed,
    ] {
        for damage_type in [
            ModernDamageType::Arc,
            ModernDamageType::Solar,
            ModernDamageType::Void,
        ] {
            let mut definition = synthetic_weapon_definition(
                WeaponInventorySlot::Kinetic,
                WeaponDamageDescriptor::Empty,
            );
            let mut strings = synthetic_item_strings(WeaponDamageDescriptor::Empty);
            apply_weapon_slot_and_damage_overrides(
                &mut definition,
                &mut strings,
                &WeaponCloneOverrides {
                    modern_damage_type: Some(damage_type),
                    ..Default::default()
                },
                Some(&ResolvedDamageCarrierSource {
                    family,
                    topology_definition: None,
                }),
                &synthetic_sandbox_perk_definition_template(),
                &synthetic_sandbox_perk_string_template(),
            )
            .unwrap();
            assert_eq!(
                weapon_sandbox_perks(&definition).unwrap(),
                [family
                    .base_sandbox_perk_index(damage_type.shared())
                    .unwrap()]
            );
            assert_eq!(
                weapon_damage_carrier(&definition).unwrap(),
                WeaponDamageCarrier::Fixed {
                    family,
                    damage_type
                }
            );
            assert_eq!(
                weapon_inventory_slot(&definition).unwrap(),
                WeaponInventorySlot::Kinetic
            );
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_damage_conversion_keeps_legacy_modern_and_plug_carriers_distinct() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog =
        sundial::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
            .unwrap();
    let cases = [
        (
            0x7CDE_3A31,
            WeaponInventorySlot::Energy,
            ModernDamageType::Solar,
            WeaponDamageCarrierFamily::ModernFixed,
        ),
        (
            0x5E73_BEF2,
            WeaponInventorySlot::Kinetic,
            ModernDamageType::Arc,
            WeaponDamageCarrierFamily::LegacyFixed,
        ),
        (
            0xA25B_8F8F,
            WeaponInventorySlot::Power,
            ModernDamageType::Void,
            WeaponDamageCarrierFamily::ModernFixed,
        ),
        (
            0xE042_B104,
            WeaponInventorySlot::Kinetic,
            ModernDamageType::Solar,
            WeaponDamageCarrierFamily::PlugDriven,
        ),
    ];
    let project = WeaponProjectSpec {
        weapons: cases
            .iter()
            .enumerate()
            .map(|(index, &(hash, slot, damage, _))| {
                let donor = catalog.weapon_donor(hash).unwrap();
                let mut spec = crate::WeaponRecipe::new_named_weapon_for_donor(
                    format!("Carrier regression {index}"),
                    hash,
                    &donor.summary.name,
                )
                .unwrap()
                .to_spec()
                .unwrap();
                spec.overrides.inventory_slot = Some(slot);
                spec.overrides.modern_damage_type = Some(damage);
                spec
            })
            .collect(),
    };
    let bundle = build_weapon_project_after_catalog_validation(&packages, &project).unwrap();
    let view = stage_trial_bundle(&packages, &bundle);
    let source = open_manager(&packages).unwrap();
    let authored = open_manager(&view.path().join("packages")).unwrap();
    for (plan, &(_, slot, damage, family)) in bundle.plan.weapons.iter().zip(&cases) {
        let before = source.read_tag(plan.template_definition_tag).unwrap();
        let after = authored.read_tag(plan.definition_tag).unwrap();
        let old_carrier = weapon_damage_carrier(&before).unwrap();
        let carrier = weapon_damage_carrier(&after).unwrap();
        assert_eq!(carrier.family(), Some(family));
        assert_eq!(
            carrier.descriptor(),
            WeaponDamageDescriptor::Elemental(damage)
        );
        if old_carrier.family().is_some() {
            assert_eq!(carrier.family(), old_carrier.family());
        }
        assert_eq!(weapon_inventory_slot(&after).unwrap(), slot);
        assert_eq!(weapon_equipment_slot(&after).unwrap(), slot);
        let non_damage_perks = |data: &[u8]| {
            weapon_sandbox_perks(data)
                .unwrap()
                .into_iter()
                .filter(|perk| fixed_damage_perk(*perk).is_none())
                .collect::<Vec<_>>()
        };
        assert_eq!(non_damage_perks(&after), non_damage_perks(&before));
        let mut expected_plugs = weapon_default_plug_indices(&before).unwrap();
        if let WeaponDamageCarrier::PlugDriven { lane, .. } = old_carrier {
            expected_plugs[lane] = family.default_plug_item_index(damage.shared()).unwrap();
        }
        assert_eq!(weapon_default_plug_indices(&after).unwrap(), expected_plugs);
        for (old_lane, new_lane) in weapon_damage_socket_lanes(&before)
            .unwrap()
            .iter()
            .zip(weapon_damage_socket_lanes(&after).unwrap())
        {
            assert_eq!(old_lane.0, new_lane.0);
        }
        assert_eq!(
            weapon_damage_socket_lanes(&before).unwrap().len(),
            weapon_damage_socket_lanes(&after).unwrap().len()
        );
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES; optionally exports trial recipes to PARHELION_DAMAGE_TRIAL_RECIPES"]
fn real_independent_damage_preserves_placement_appearance_and_ammo() {
    use crate::recipe::RecipeDamageType;
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let mut recipes = Vec::new();
    for (name, hash, donor, element) in [
        (
            "Arc in Kinetic",
            0x4CE3_CE93,
            "Breachlight",
            RecipeDamageType::Arc,
        ),
        (
            "Kinetic in Energy",
            0xA25B_8F8F,
            "Arc Logic",
            RecipeDamageType::Kinetic,
        ),
    ] {
        let mut recipe =
            crate::WeaponRecipe::new_named_weapon_for_donor(name, hash, donor).unwrap();
        recipe.flavor =
            "Slot/damage experiment. Placement, appearance and ammo are inherited.".to_owned();
        recipe.overrides.modern_damage_type = Some(element);
        assert!(recipe.overrides.inventory_slot.is_none());
        assert!(recipe.overrides.ammo_type.is_none());
        assert!(recipe.presentation_donor.is_none());
        recipes.push(recipe);
    }
    let project = WeaponProjectSpec {
        weapons: recipes
            .iter()
            .map(|recipe| recipe.to_spec().unwrap())
            .collect(),
    };
    let bundle = build_weapon_project_after_catalog_validation(&packages, &project).unwrap();
    let view = stage_trial_bundle(&packages, &bundle);
    let view_packages = view.path().join("packages");
    let stock = open_manager(&packages).unwrap();
    let authored = open_manager(&view_packages).unwrap();
    for (index, plan) in bundle.plan.weapons.iter().enumerate() {
        let source = stock.read_tag(plan.template_definition_tag).unwrap();
        let result = authored.read_tag(plan.definition_tag).unwrap();
        let source_strings = stock.read_tag(plan.template_string_tag).unwrap();
        let result_strings = authored.read_tag(plan.string_tag).unwrap();
        assert_eq!(
            weapon_inventory_slot(&result).unwrap(),
            weapon_inventory_slot(&source).unwrap()
        );
        assert_eq!(
            weapon_equipment_slot(&result).unwrap(),
            weapon_equipment_slot(&source).unwrap()
        );
        assert_eq!(
            weapon_art_arrangements(&result).unwrap(),
            weapon_art_arrangements(&source).unwrap()
        );
        assert_eq!(
            weapon_render_dye_rows(&result).unwrap(),
            weapon_render_dye_rows(&source).unwrap()
        );
        assert_eq!(
            weapon_default_plug_indices(&result).unwrap(),
            weapon_default_plug_indices(&source).unwrap()
        );
        assert_eq!(
            item_string_ammo_type(&result_strings).unwrap(),
            item_string_ammo_type(&source_strings).unwrap()
        );
        let expected = if index == 0 {
            WeaponDamageDescriptor::Elemental(ModernDamageType::Arc)
        } else {
            WeaponDamageDescriptor::Empty
        };
        assert_eq!(weapon_damage_descriptor(&result).unwrap(), expected);
        // Damage-only authoring must not graft runtime components or tune ammo properties.
        let source_runtime =
            sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager(
                &stock,
                project.weapons[index].donor_item_hash,
            )
            .unwrap();
        let result_runtime =
            sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager(
                &authored,
                plan.item_hash,
            )
            .unwrap();
        assert_eq!(source_runtime.payload, result_runtime.payload);
    }
    export_trial_recipes(&recipes);
}

fn stage_trial_bundle(packages: &Path, bundle: &NewWeaponProjectBundle) -> tempfile::TempDir {
    let view = tempfile::Builder::new()
        .prefix(".parhelion-independent-damage-")
        .tempdir_in(packages.parent().unwrap())
        .unwrap();
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).unwrap();
    for entry in fs::read_dir(packages).unwrap() {
        let entry = entry.unwrap();
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "pkg")
        {
            fs::hard_link(entry.path(), view_packages.join(entry.file_name())).unwrap();
        }
    }
    bundle.write_new(&view_packages).unwrap();
    view
}

fn export_trial_recipes(recipes: &[crate::WeaponRecipe]) {
    if let Some(output) = std::env::var_os("PARHELION_DAMAGE_TRIAL_RECIPES") {
        let output = PathBuf::from(output);
        fs::create_dir_all(&output).unwrap();
        for recipe in recipes {
            let path = output.join(format!("{}.parhelion.json", recipe.namespace));
            assert!(
                !path.exists(),
                "trial export must not overwrite an existing recipe"
            );
            recipe.save_json(&path).unwrap();
            println!("Trial recipe: {}", path.display());
        }
    }
}
