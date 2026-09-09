use super::*;

mod plan;
pub(super) use plan::{Payloads, author};

pub(super) fn author_entities(
    manager: &PackageManager,
    stock_sandbox_patterns: &[u8],
    stock_entity_assignments: &[u8],
    resolved: &[resolve::ResolvedWeapon],
    weapon_runtime_tag_allocator: AppendedTagAllocator,
    entity_assignments: &mut Vec<u8>,
    weapon_runtime_new_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<Vec<Option<u32>>> {
    let mut authored_pattern_global_ids = Vec::with_capacity(resolved.len());
    for donor in resolved {
        let authored_pattern_global_id = (|| -> AuthoringResult<Option<u32>> {
            let authored_pattern_source = donor
                .runtime_pattern_source
                .as_ref()
                .or(donor.gear_art_pattern_source.as_ref());
            let Some(authored_pattern_source) = authored_pattern_source else {
                return Ok(None);
            };
            let pattern_source_hash = donor
                .runtime_pattern_source
                .as_ref()
                .map(|source| source.item_hash);
            let mut component_donors = donor
                .runtime_component_donors
                .iter()
                .filter(|component| Some(component.pattern_item_hash) != pattern_source_hash)
                .collect::<Vec<_>>();
            component_donors.sort_by_key(|component| component.binding_hash);
            let runtime_entity_patches = donor
                .weapon
                .overrides
                .raw_payload_patches
                .iter()
                .any(|patch| patch.target == WeaponRawPayloadTarget::RuntimeWeaponEntity);
            let hud_key = resolved_hud_key(
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                donor,
            )?;
            let has_runtime_edits = !component_donors.is_empty()
                || hud_key.is_some()
                || donor.weapon.overrides.ammo_type.is_some()
                || !donor.weapon.overrides.runtime_values.is_empty()
                || !donor.weapon.overrides.runtime_resource_patches.is_empty()
                || runtime_entity_patches;
            if !has_runtime_edits {
                let stock_entity_tag = weapon_entity_assignment(
                    stock_entity_assignments,
                    authored_pattern_source.pattern_global_id_hash,
                )
                .map_err(invalid)?
                .ok_or_else(|| {
                    invalid(format!(
                        "Selected weapon pattern 0x{:08X} has no runtime weapon-entity assignment",
                        authored_pattern_source.pattern_global_id_hash
                    ))
                })?;
                *entity_assignments = append_weapon_entity_assignment(
                    std::mem::take(entity_assignments),
                    donor.weapon.identity.pattern_global_id_hash,
                    stock_entity_tag,
                )
                .map_err(invalid)?;
                return Ok(Some(donor.weapon.identity.pattern_global_id_hash));
            }
            let pattern_source = donor
                .runtime_pattern_source
                .as_ref()
                .ok_or_else(|| invalid("Runtime edits require an active weapon sandbox pattern"))?;
            let (pattern_entity_tag, mut pattern_entity) = resolve_runtime_weapon_entity(
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                pattern_source.item_hash,
                "weapon-pattern donor entity",
            )?;
            let mut component_entities = Vec::with_capacity(component_donors.len());
            for component in component_donors {
                let (_, component_entity) = resolve_runtime_weapon_entity(
                    manager,
                    stock_sandbox_patterns,
                    stock_entity_assignments,
                    component.pattern_item_hash,
                    "runtime-component donor weapon entity",
                )?;
                component_entities.push((component.binding_hash, component_entity));
            }
            let component_grafts = component_entities
                .iter()
                .map(|(binding_hash, entity)| (*binding_hash, entity.as_slice()))
                .collect::<Vec<_>>();
            graft_weapon_component_bindings(&mut pattern_entity, &component_grafts)
                .map_err(invalid)?;
            let mut runtime_resource_patches =
                donor.weapon.overrides.runtime_resource_patches.clone();
            if let Some(key) = hud_key {
                runtime_resource_patches.extend(crate::hud_icon::runtime::patches(
                    manager,
                    &pattern_entity,
                    key,
                )?);
            }
            if let Some(ammo) = donor.weapon.overrides.ammo_type {
                runtime_resource_patches.extend(crate::weapon_ammo::patches(
                    manager,
                    &pattern_entity,
                    ammo,
                )?);
            }
            append_patched_runtime_resource_owners(
                manager,
                &mut pattern_entity,
                &donor.weapon.overrides.runtime_values,
                &runtime_resource_patches,
                weapon_runtime_tag_allocator,
                weapon_runtime_new_tags,
            )?;
            apply_raw_payload_target(
                &mut pattern_entity,
                WeaponRawPayloadTarget::RuntimeWeaponEntity,
                &donor.weapon.overrides.raw_payload_patches,
            )?;
            validate_raw_payload_target(
                &pattern_entity,
                WeaponRawPayloadTarget::RuntimeWeaponEntity,
                &donor.weapon.overrides.raw_payload_patches,
            )?;
            validate_weapon_entity(&pattern_entity).map_err(invalid)?;
            let authored_entity_tag = weapon_runtime_tag_allocator.assigned_tag(
                weapon_runtime_new_tags.len(),
                "Authored runtime weapon entity",
                "runtime weapon entity",
            )?;
            *entity_assignments = append_weapon_entity_assignment(
                std::mem::take(entity_assignments),
                donor.weapon.identity.pattern_global_id_hash,
                authored_entity_tag.0,
            )
            .map_err(invalid)?;
            weapon_runtime_new_tags.push(NewTagSpec {
                template_tag: pattern_entity_tag,
                payload: pattern_entity,
                storage: crate::NewTagStorageMode::InheritTemplate,
            });
            Ok(Some(donor.weapon.identity.pattern_global_id_hash))
        })()
        .map_err(|error| {
            error.context(format!(
                "Weapon {:?} ({})",
                donor.weapon.text.name, donor.weapon.namespace
            ))
        })?;
        authored_pattern_global_ids.push(authored_pattern_global_id);
    }
    Ok(authored_pattern_global_ids)
}

fn resolved_hud_key(
    manager: &PackageManager,
    patterns: &[u8],
    assignments: &[u8],
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<Option<u32>> {
    if donor.weapon.overrides.hud_icon.is_some() {
        return Ok(Some(donor.weapon.identity.type_hash));
    }
    let Some(appearance) = donor.gear_art_pattern_source else {
        return Ok(None);
    };
    let content_graft = donor.runtime_component_donors.iter().any(|component| {
        component.binding_hash == 0x5F0DD954 && component.pattern_item_hash != appearance.item_hash
    });
    if donor.runtime_pattern_source == Some(appearance) && !content_graft {
        // The unchanged content component already selects the appearance's native HUD key.
        return Ok(None);
    }
    let (_, entity) = resolve_runtime_weapon_entity(
        manager,
        patterns,
        assignments,
        appearance.item_hash,
        "appearance donor HUD content",
    )?;
    crate::hud_icon::runtime::inherited_key(manager, &entity, appearance.weapon_content_group_hash)
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires PARHELION_HUD_TEST_PACKAGES pointing to Shadowkeep packages"]
    fn hud_inheritance_follows_appearance_and_png_override_wins() {
        let packages = std::env::var_os("PARHELION_HUD_TEST_PACKAGES").unwrap();
        let sources = crate::weapon::sources::load_project_sources(Path::new(&packages)).unwrap();
        let mut spec = crate::WeaponRecipe::new_weapon_for_donor(
            "parhelion.hud-inheritance-test",
            0x02222CBF,
            "Temptation's Hook",
        )
        .unwrap()
        .to_spec()
        .unwrap();
        spec.overrides = WeaponCloneOverrides::default();
        spec.presentation_donor = Some(WeaponPresentationDonorReference {
            item_hash: 0xE0794C51,
            expected_name: Some("Black Talon".into()),
        });
        spec.icon_donor = Some(WeaponIconDonorReference {
            item_hash: 0x02222CBF,
            expected_name: Some("Temptation's Hook".into()),
        });
        let mut donors = resolve::resolve_project_weapons(&sources, &[spec]).unwrap();
        let donor = &mut donors[0];
        let key = |d: &resolve::ResolvedWeapon| {
            resolved_hud_key(
                &sources.manager,
                &sources.stock_sandbox_patterns,
                &sources.stock_entity_assignments,
                d,
            )
            .unwrap()
        };
        assert_eq!(
            key(donor),
            Some(0x08491234),
            "appearance overrides gameplay and separate inventory icon"
        );
        let original_appearance = donor.gear_art_pattern_source;
        donor.gear_art_pattern_source = donor.runtime_pattern_source;
        assert_eq!(
            key(donor),
            None,
            "unchanged native HUD needs no runtime clone"
        );
        donor.gear_art_pattern_source = original_appearance;

        let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, HOST_EXPECTED_ENTRY_COUNT + 256);
        let mut assignments = sources.stock_entity_assignments.clone();
        let mut tags = vec![];
        author_entities(
            &sources.manager,
            &sources.stock_sandbox_patterns,
            &sources.stock_entity_assignments,
            &donors,
            allocator,
            &mut assignments,
            &mut tags,
        )
        .unwrap();
        let entity = &tags.last().unwrap().payload;
        let binding = weapon_component_bindings(entity, 0x5F0DD954).unwrap()[0];
        let owner = tags
            .iter()
            .enumerate()
            .find(|(i, _)| {
                allocator.assigned_tag(*i, "test", "HUD owner").unwrap().0 == binding.owner_tag
            })
            .unwrap()
            .1;
        let definition =
            read_u64(&owner.payload, binding.resource_offset as usize + 8).unwrap() as usize;
        for offset in crate::weapon_ammo::property_offsets(&owner.payload, definition).unwrap() {
            assert_eq!(read_u32(&owner.payload, offset + 0xE0).unwrap(), 0x08491234);
        }
        let mut png = std::io::Cursor::new(vec![]);
        image::RgbaImage::new(137, 76)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        donors[0].weapon.overrides.hud_icon =
            Some(crate::hud_icon::HudImage::from_png(&png.into_inner()).unwrap());
        assert_eq!(key(&donors[0]), Some(donors[0].weapon.identity.type_hash));
        donors[0].weapon.overrides.hud_icon = None;
        assert_eq!(key(&donors[0]), Some(0x08491234));

        let mut spec = crate::WeaponRecipe::new_weapon_for_donor(
            "parhelion.hud-empty-override-test",
            0x63344D56,
            "Threat Level",
        )
        .unwrap()
        .to_spec()
        .unwrap();
        spec.overrides = WeaponCloneOverrides::default();
        spec.presentation_donor = Some(WeaponPresentationDonorReference {
            item_hash: 0xCA44FDCB,
            expected_name: Some("Perfect Paradox".into()),
        });
        let donors = resolve::resolve_project_weapons(&sources, &[spec]).unwrap();
        assert_eq!(
            key(&donors[0]),
            Some(0x811C9DC5),
            "native empty override is inherited without requiring a texture row"
        );
    }
}
