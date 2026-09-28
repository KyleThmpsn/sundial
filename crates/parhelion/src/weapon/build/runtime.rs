use super::*;

mod dependencies;
mod plan;
pub(super) use plan::{Payloads, author};

/// Where imported runtime assets go: a group per weapon in the rolling asset packages, and
/// the tags placed there for the runtime dependency index.
pub(super) struct RuntimeAssets<'a> {
    pub packages: &'a mut crate::asset_packages::AssetPackages,
    pub placed: &'a mut Vec<TagHash>,
}

/// The allocator and tag list of one reserved asset group's package.
#[cfg(feature = "d2-model-importer")]
fn asset_target(
    packages: &mut crate::asset_packages::AssetPackages,
    index: usize,
) -> (AppendedTagAllocator, &mut Vec<NewTagSpec>) {
    let package = &mut packages.packages[index];
    (AppendedTagAllocator::new(package.id, 0), &mut package.tags)
}

pub(super) struct EntitySources<'a> {
    pub sandbox_patterns: &'a [u8],
    pub entity_assignments: &'a [u8],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn author_entities(
    manager: &PackageManager,
    stock: EntitySources<'_>,
    resolved: &[resolve::ResolvedWeapon],
    weapon_runtime_tag_allocator: AppendedTagAllocator,
    assets: RuntimeAssets<'_>,
    entity_assignments: &mut Vec<u8>,
    weapon_runtime_new_tags: &mut Vec<NewTagSpec>,
    progress: &mut Progress<'_>,
) -> AuthoringResult<Vec<Option<u32>>> {
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = (assets.packages, assets.placed);
    let stock_sandbox_patterns = stock.sandbox_patterns;
    let stock_entity_assignments = stock.entity_assignments;
    let mut authored_pattern_global_ids = Vec::with_capacity(resolved.len());
    #[cfg(feature = "d2-model-importer")]
    let mut animation_cache = crate::weapon::custom_runtime::animation::AnimationTagCache::new();
    for donor in resolved {
        let operation = format!("Compiling Runtime for {}", donor.weapon.text.name);
        progress.start(&operation);
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
            #[cfg(feature = "d2-model-importer")]
            let imported_animation = donor
                .weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(crate::weapon::custom_runtime::animation::load)
                .transpose()?
                .flatten();
            #[cfg(feature = "d2-model-importer")]
            let imported_audio = donor
                .weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(crate::weapon::custom_runtime::audio::load)
                .transpose()?
                .flatten();
            #[cfg(not(feature = "d2-model-importer"))]
            let imported_animation: Option<()> = None;
            #[cfg(feature = "d2-model-importer")]
            let imported_extensions = donor
                .weapon
                .overrides
                .imported_graph
                .as_ref()
                .map(crate::weapon::custom_runtime::extensions::load)
                .transpose()?
                .unwrap_or_default();
            let has_runtime_edits = imported_animation.is_some()
                || {
                    #[cfg(feature = "d2-model-importer")]
                    { imported_audio.is_some() || !imported_extensions.is_empty() }
                    #[cfg(not(feature = "d2-model-importer"))]
                    { false }
                }
                || donor.appearance_rig_donor.is_some()
                || !component_donors.is_empty()
                || hud_key.is_some()
                || donor.weapon.overrides.ammo_type.is_some()
                || !donor.weapon.overrides.runtime_values.is_empty()
                || !donor.weapon.overrides.runtime_resource_patches.is_empty()
                || !donor.weapon.overrides.additional_behaviors.is_empty()
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
            // Before any component donor, so the appearance's rig is promoted onto exactly
            // the entity the resolver tested. A donor that then conflicts with it is reported
            // against that donor rather than silently dropping the appearance's animations.
            if let Some(item_hash) = donor.appearance_rig_donor {
                let (_, appearance_entity) = resolve_runtime_weapon_entity(
                    manager,
                    stock_sandbox_patterns,
                    stock_entity_assignments,
                    item_hash,
                    "appearance runtime entity",
                )?;
                crate::weapon::rig::graft_presentation(&mut pattern_entity, &appearance_entity)
                    .map_err(|error| {
                        invalid(format!(
                            "Could not move the appearance's rig and animations onto this runtime: {error}"
                        ))
                    })?;
            }
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
            graft_weapon_component_bindings_or_rewire(
                &mut pattern_entity,
                &component_grafts,
                &|tag| manager.read_tag(tag),
            )
            .map_err(invalid)?;
            // The pattern row selects the appearance's block when it supplies the row, and the
            // base weapon's own block keeps its behavior there.
            let groups = crate::weapon_behavior::ContentGroups {
                selected: donor
                    .gear_art_pattern_source
                    .or(donor.runtime_pattern_source)
                    .map(|source| source.weapon_content_group_hash),
                own: donor
                    .runtime_pattern_source
                    .map(|source| source.weapon_content_group_hash),
            };
            #[cfg(feature = "d2-model-importer")]
            crate::weapon::custom_runtime::extensions::author(
                manager,
                &imported_extensions,
                &mut pattern_entity,
            )?;
            let runtime_overrides = &donor.weapon.overrides;
            #[cfg(feature = "d2-model-importer")]
            let extended_overrides = {
                let mut overrides = runtime_overrides.clone();
                overrides.runtime_resource_patches.extend(
                    crate::weapon::custom_runtime::extensions::input_patches(
                        manager,
                        &imported_extensions,
                        &pattern_entity,
                    )?,
                );
                overrides
            };
            #[cfg(feature = "d2-model-importer")]
            let runtime_overrides = &extended_overrides;
            author_runtime_edits(
                manager,
                &mut pattern_entity,
                runtime_overrides,
                hud_key,
                groups,
                weapon_runtime_tag_allocator,
                weapon_runtime_new_tags,
            )?;
            #[cfg(feature = "d2-model-importer")]
            crate::weapon::custom_runtime::extensions::input_callbacks(
                manager,
                &imported_extensions,
                &mut pattern_entity,
                weapon_runtime_tag_allocator,
                weapon_runtime_new_tags,
            )?;
            // Clips and audio media grow with every imported weapon, so they take a group in
            // the asset packages. Only the owners that route them stay in the host package.
            #[cfg(feature = "d2-model-importer")]
            let group = {
                let bounds = imported_animation
                    .iter()
                    .flat_map(|animation| animation.asset_bounds())
                    .chain(imported_audio.iter().flat_map(|audio| audio.asset_bounds()))
                    .collect::<Vec<_>>();
                if bounds.is_empty() {
                    None
                } else {
                    let index = assets.packages.reserve_group(bounds)?;
                    Some((index, assets.packages.packages[index].tags.len()))
                }
            };
            #[cfg(feature = "d2-model-importer")]
            let private_animation_owner = match (&imported_animation, group) {
                (Some(animation), Some((index, _))) => {
                    let (_, owner) = crate::weapon::custom_runtime::animation::author(
                        manager,
                        animation,
                        pattern_entity_tag,
                        &mut pattern_entity,
                        weapon_runtime_tag_allocator,
                        weapon_runtime_new_tags,
                        asset_target(assets.packages, index),
                        &mut animation_cache,
                    )?;
                    Some(owner)
                }
                _ => None,
            };
            #[cfg(feature = "d2-model-importer")]
            if let (Some(audio), Some((index, _))) = (&imported_audio, group) {
                crate::weapon::custom_runtime::audio::author(
                    manager,
                    audio,
                    pattern_entity_tag,
                    &mut pattern_entity,
                    private_animation_owner,
                    weapon_runtime_tag_allocator,
                    weapon_runtime_new_tags,
                    asset_target(assets.packages, index),
                )?;
            }
            #[cfg(feature = "d2-model-importer")]
            if let Some((index, start)) = group {
                let package = &assets.packages.packages[index];
                let allocator = AppendedTagAllocator::new(package.id, 0);
                for ordinal in start..package.tags.len() {
                    assets.placed.push(allocator.assigned_tag(
                        ordinal,
                        "Imported runtime asset",
                        "dependency",
                    )?);
                }
            }
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
        .map_err(|error| donor.weapon.in_recipe(error))?;
        authored_pattern_global_ids.push(authored_pattern_global_id);
        progress.finish(&operation);
    }
    Ok(authored_pattern_global_ids)
}

fn resolved_hud_key(
    manager: &PackageManager,
    patterns: &[u8],
    assignments: &[u8],
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<Option<u32>> {
    let content_graft = donor.runtime_component_donors.iter().any(|component| {
        component.binding_hash == 0x5F0DD954
            && Some(component.pattern_item_hash)
                != donor.gear_art_pattern_source.map(|source| source.item_hash)
    });
    runtime_hud_key(
        manager,
        donor
            .weapon
            .overrides
            .hud_icon
            .as_ref()
            .map(|_| donor.weapon.identity.type_hash),
        donor.runtime_pattern_source == donor.gear_art_pattern_source && !content_graft,
        || {
            let Some(appearance) = donor.gear_art_pattern_source else {
                return Ok(None);
            };
            let (_, entity) = resolve_runtime_weapon_entity(
                manager,
                patterns,
                assignments,
                appearance.item_hash,
                "appearance donor HUD content",
            )?;
            Ok(Some((entity, appearance.weapon_content_group_hash)))
        },
    )
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
            EntitySources {
                sandbox_patterns: &sources.stock_sandbox_patterns,
                entity_assignments: &sources.stock_entity_assignments,
            },
            &donors,
            allocator,
            RuntimeAssets {
                packages: &mut crate::asset_packages::AssetPackages {
                    packages: Vec::new(),
                },
                placed: &mut vec![],
            },
            &mut assignments,
            &mut tags,
            &mut Progress::new(donors.len(), &mut |_, _, _, _| {}),
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
