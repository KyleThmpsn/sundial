use super::*;

mod cache;
mod dependencies;
mod plan;
pub(super) use plan::{Payloads, author};

/// Where imported runtime assets go: a group per weapon in the rolling asset packages, and
/// the tags placed there for the runtime dependency index.
pub(super) struct RuntimeAssets<'a> {
    pub packages: &'a mut crate::asset_packages::AssetPackages,
    pub placed: &'a mut Vec<TagHash>,
    pub impacts: &'a mut Vec<crate::ability::banks::MeleeImpact>,
    /// Each weapon's imported particle tags by node symbol, by weapon ordinal, for the private
    /// perk programs that name them.
    #[cfg(feature = "d2-model-importer")]
    pub particle_symbols: &'a mut BTreeMap<usize, BTreeMap<String, TagHash>>,
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
    mut assets: RuntimeAssets<'_>,
    entity_assignments: &mut Vec<u8>,
    weapon_runtime_new_tags: &mut Vec<NewTagSpec>,
    progress: &mut Progress<'_>,
) -> AuthoringResult<Vec<Option<u32>>> {
    let runtime_cache = cache::Session::open(manager, &stock);
    let stock_sandbox_patterns = stock.sandbox_patterns;
    let stock_entity_assignments = stock.entity_assignments;
    let mut authored_pattern_global_ids = Vec::with_capacity(resolved.len());
    let mut animation_cache = cache::AnimationCache::new();
    for donor in resolved {
        progress.start(&format!("Checking Runtime for {}", donor.weapon.text.name));
        // A cached runtime keeps no particle symbols, so a weapon whose private perks name its
        // imported particles is always authored here.
        let names_particles = donor
            .weapon
            .overrides
            .socket_plug_variants
            .iter()
            .flat_map(|variant| &variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_ref())
            .any(crate::item::custom_runtime::program_names_imported_particles);
        let pending = runtime_cache
            .as_ref()
            .filter(|_| !names_particles)
            .and_then(|cache| {
                cache.prepare(
                    donor,
                    weapon_runtime_tag_allocator,
                    weapon_runtime_new_tags,
                    &assets,
                    entity_assignments,
                    &animation_cache,
                )
            });
        if let Some(pattern) = pending.as_ref().and_then(|pending| {
            pending.restore(
                manager,
                weapon_runtime_new_tags,
                &mut assets,
                entity_assignments,
                &mut animation_cache,
            )
        }) {
            authored_pattern_global_ids.push(pattern);
            progress.finish(&format!("Reusing Runtime for {}", donor.weapon.text.name));
            continue;
        }
        let trace = pending.as_ref().and_then(|_| manager.trace_reads());
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
            let ImportedRuntime {
                animation: mut imported_animation,
                audio: imported_audio,
                extensions: imported_extensions,
                particles: imported_particles,
                equipment: equipment_animation,
                crosshair: imported_crosshair,
            } = ImportedRuntime::load(donor)?;
            #[cfg(not(feature = "d2-model-importer"))]
            let imported_animation: Option<()> = None;
            let vehicle_settings = donor.weapon.overrides.sparrow.as_ref();
            let has_runtime_edits = vehicle_settings.is_some_and(crate::vehicle::Sparrow::has_changes)
                || imported_animation.is_some()
                || {
                    #[cfg(feature = "d2-model-importer")]
                    { imported_audio.is_some() || equipment_animation.is_some()
                        || imported_particles.is_some() || !imported_extensions.is_empty()
                        || imported_crosshair.is_some() }
                    #[cfg(not(feature = "d2-model-importer"))]
                    { false }
                }
                || donor.appearance_rig_donor.is_some()
                || donor.animation_pattern_source.is_some()
                || !donor.animation_action_sources.is_empty()
                || donor.pinned_appearance.is_some()
                || donor.type_marker_pattern_source.is_some()
                || !component_donors.is_empty()
                || hud_key.is_some()
                || donor.weapon.overrides.ammo_type.is_some()
                || !donor.weapon.overrides.runtime_values.is_empty()
                || donor.weapon.overrides.sword_profile.is_some()
                || !donor.weapon.overrides.component_splices.is_empty()
                || !donor.weapon.overrides.runtime_resource_patches.is_empty()
                || !donor.weapon.overrides.additional_behaviors.is_empty()
                || runtime_entity_patches;
            if !has_runtime_edits {
                return reuse_stock_entity(
                    stock_entity_assignments,
                    entity_assignments,
                    authored_pattern_source.pattern_global_id_hash,
                    donor.weapon.identity.pattern_global_id_hash,
                )
                .map(Some);
            }
            let pattern_source = donor
                .runtime_pattern_source
                .as_ref()
                .ok_or_else(|| invalid("Runtime edits require an active item sandbox pattern"))?;
            let (pattern_entity_tag, pattern_entity) = resolve_runtime_weapon_entity(
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                pattern_source.item_hash,
                "weapon-pattern donor entity",
            )?;
            let (pattern_entity_tag, mut pattern_entity) = crate::vehicle::authoring::select(
                manager, vehicle_settings, pattern_entity_tag, pattern_entity,
            )?;
            // Before any component donor, so the appearance's rig is promoted onto exactly
            // the entity the resolver tested. A donor that then conflicts with it is reported
            // against that donor rather than silently dropping the appearance's animations.
            let mut rig_markers = Vec::new();
            if let Some(item_hash) = donor.appearance_rig_donor {
                let (_, appearance_entity) = resolve_runtime_weapon_entity(
                    manager,
                    stock_sandbox_patterns,
                    stock_entity_assignments,
                    item_hash,
                    "appearance runtime entity",
                )?;
                let base_entity = pattern_entity.clone();
                crate::weapon::rig::graft_presentation(&mut pattern_entity, &appearance_entity)
                    .map_err(|error| {
                        invalid(format!(
                            "Could not move the appearance's rig and animations onto this runtime: {error}"
                        ))
                    })?;
                rig_markers =
                    crate::weapon::rig::marker_set_appends(manager, &base_entity, &pattern_entity)?;
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
            let groups = crate::weapon::behavior::ContentGroups {
                selected: donor
                    .gear_art_pattern_source
                    .or(donor.runtime_pattern_source)
                    .map(|source| source.weapon_content_group_hash),
                own: donor
                    .runtime_pattern_source
                    .map(|source| source.weapon_content_group_hash),
                animations: donor
                    .animation_pattern_source
                    .map(|source| {
                        let (_, entity) = resolve_runtime_weapon_entity(
                            manager,
                            stock_sandbox_patterns,
                            stock_entity_assignments,
                            source.item_hash,
                            "animation donor entity",
                        )?;
                        crate::weapon::animations::profile(
                            manager,
                            &entity,
                            Some(source.weapon_content_group_hash),
                        )
                    })
                    .transpose()?,
                kind: donor
                    .type_marker_pattern_source
                    .map(|source| source.weapon_content_group_hash),
                hold: donor
                    .pinned_appearance
                    .map(|source| {
                        let (_, entity) = resolve_runtime_weapon_entity(
                            manager,
                            stock_sandbox_patterns,
                            stock_entity_assignments,
                            source.item_hash,
                            "pinned appearance entity",
                        )?;
                        crate::weapon::animations::hold(manager, &entity)
                    })
                    .transpose()?,
            };
            #[cfg(feature = "d2-model-importer")]
            crate::item::custom_runtime::extensions::author(
                manager,
                &imported_extensions,
                &mut pattern_entity,
            )?;
            let runtime_overrides = &donor.weapon.overrides;
            // The fired graph names imported particle systems by symbol, so they take their
            // own asset group before the graph is authored.
            #[cfg(feature = "d2-model-importer")]
            let particle_symbols = match &imported_particles {
                Some(particles) => {
                    let index = assets.packages.reserve_group(particles.asset_bounds())?;
                    let package = &mut assets.packages.packages[index];
                    let start = package.tags.len();
                    let symbols = particles.author(manager, package)?;
                    let allocator = AppendedTagAllocator::new(package.id, 0);
                    for index in start..package.tags.len() {
                        assets.placed.push(allocator.assigned_tag(
                            index,
                            "Imported particle asset",
                            "dependency",
                        )?);
                    }
                    assets.particle_symbols.insert(authored_pattern_global_ids.len(), symbols.clone());
                    symbols
                }
                None => BTreeMap::new(),
            };
            #[cfg(feature = "d2-model-importer")]
            let extended_overrides = {
                let mut overrides = runtime_overrides.clone();
                overrides.runtime_resource_patches.extend(
                    crate::item::custom_runtime::extensions::input_patches(
                        manager,
                        &imported_extensions,
                        &pattern_entity,
                    )?,
                );
                overrides.runtime_resource_patches.extend(
                    crate::item::custom_runtime::extensions::attachment_patches(
                        manager,
                        &imported_extensions,
                        &pattern_entity,
                    )?,
                );
                // The hip-fire crosshair follows the runtime content's keys, not the item strings.
                if let Some(crosshair) = &imported_crosshair
                    && crosshair.replaces(&crate::weapon::crosshair::type_keys(
                        manager,
                        &pattern_entity,
                    )?)?
                {
                    overrides.runtime_resource_patches.extend(
                        crate::weapon::crosshair::content_patches(
                            manager,
                            &pattern_entity,
                            crosshair.type_key,
                        )?,
                    );
                }
                if let Some(fired) = overrides.fired_graph.as_mut() {
                    let patches = crate::item::custom_runtime::particles::fired_graph_patches(
                        fired,
                        &particle_symbols,
                    )?;
                    fired.patches.extend(patches);
                }
                overrides
            };
            #[cfg(feature = "d2-model-importer")]
            let runtime_overrides = &extended_overrides;
            // A moved rig makes the pattern row name the appearance's weapon type, and the stat
            // translator converts by that type. The base weapon's conversion is put back, so the
            // appearance changes how the weapon looks and not its fire rate.
            let stat_patches = match (
                donor.appearance_rig_donor,
                donor.gear_art_pattern_source,
                donor.runtime_pattern_source,
            ) {
                (Some(_), Some(row), Some(own)) => crate::weapon::rig::stat_table_patches(
                    manager,
                    &pattern_entity,
                    row.weapon_translation_group_hash,
                    own.weapon_translation_group_hash,
                )?,
                _ => Vec::new(),
            };
            let with_stats;
            let runtime_overrides = if stat_patches.is_empty() {
                runtime_overrides
            } else {
                with_stats = {
                    let mut overrides = runtime_overrides.clone();
                    overrides.runtime_resource_patches.extend(stat_patches);
                    overrides
                };
                &with_stats
            };
            // Each spliced component's donor, read as the runtime entity its own row selects.
            let splices = runtime_overrides
                .component_splices
                .iter()
                .map(|&(binding, item_hash)| {
                    let (_, entity) = resolve_runtime_weapon_entity(
                        manager,
                        stock_sandbox_patterns,
                        stock_entity_assignments,
                        item_hash,
                        "component donor entity",
                    )?;
                    Ok((binding, entity))
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            // Each single action's donor, read as the animation profile its own row plays.
            let actions = donor
                .animation_action_sources
                .iter()
                .map(|(action, source)| {
                    let (_, entity) = resolve_runtime_weapon_entity(
                        manager,
                        stock_sandbox_patterns,
                        stock_entity_assignments,
                        source.item_hash,
                        "animation action donor entity",
                    )?;
                    Ok((
                        *action,
                        crate::weapon::animations::profile(
                            manager,
                            &entity,
                            Some(source.weapon_content_group_hash),
                        )?,
                    ))
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            author_runtime_edits(
                manager,
                &mut pattern_entity,
                runtime_overrides,
                hud_key,
                crate::item::custom_runtime::Sources {
                    groups,
                    splices: &splices,
                    actions: &actions,
                    markers: &rig_markers,
                },
                weapon_runtime_tag_allocator,
                weapon_runtime_new_tags,
            )?;
            #[cfg(feature = "d2-model-importer")]
            crate::item::custom_runtime::extensions::input_callbacks(
                manager,
                &imported_extensions,
                &mut pattern_entity,
                weapon_runtime_tag_allocator,
                weapon_runtime_new_tags,
            )?;
            // Clips and audio media grow with every imported weapon, so they take a group in
            // the asset packages. Only the owners that route them stay in the host package.
            #[cfg(feature = "d2-model-importer")]
            let group = imported_asset_group(assets.packages, imported_audio.as_ref(), imported_animation.as_ref(), equipment_animation.as_ref())?;
            #[cfg(feature = "d2-model-importer")]
            if let (Some(audio), Some(animation), Some((index, _))) =
                (&imported_audio, &mut imported_animation, group)
            {
                let (allocator, tags) = asset_target(assets.packages, index);
                crate::item::custom_runtime::audio::author_clip_events(
                    manager, audio, animation, allocator, tags,
                )?;
            }
            #[cfg(feature = "d2-model-importer")]
            let private_animation_owner = match (&imported_animation, group) {
                (Some(animation), Some((index, _))) => {
                    let (_, owner) = crate::item::custom_runtime::animation::author(
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
            if let (Some(animation), Some((index, _))) = (&equipment_animation, group) {
                crate::item::custom_runtime::animation::equipment::author(
                    manager, animation, pattern_entity_tag, &mut pattern_entity,
                    weapon_runtime_tag_allocator, weapon_runtime_new_tags,
                    asset_target(assets.packages, index), &mut animation_cache,
                )?;
            }
            #[cfg(feature = "d2-model-importer")]
            if let (Some(audio), Some((index, _))) = (&imported_audio, group) {
                let (allocator, tags) = asset_target(assets.packages, index);
                if let Some(impact) = crate::item::custom_runtime::audio::author_impacts(
                    manager, audio, allocator, tags,
                )? {
                    assets.impacts.push(impact);
                }
                crate::item::custom_runtime::audio::author(
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
            crate::vehicle::authoring::speed(
                manager, vehicle_settings, pattern_entity_tag, &mut pattern_entity,
                &weapon_runtime_tag_allocator, weapon_runtime_new_tags,
            )?;
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
        if let Some(reads) = trace.and_then(|trace| trace.finish())
            && let Some(pending) = pending
        {
            pending.save(
                reads,
                authored_pattern_global_id,
                weapon_runtime_new_tags,
                &assets,
                entity_assignments,
                &animation_cache,
            );
        }
        authored_pattern_global_ids.push(authored_pattern_global_id);
        progress.finish(&operation);
    }
    Ok(authored_pattern_global_ids)
}

/// Points `authored_pattern` at the stock runtime entity of `source_pattern`, for a weapon with
/// no runtime edits, and returns `authored_pattern`.
fn reuse_stock_entity(
    stock_entity_assignments: &[u8],
    entity_assignments: &mut Vec<u8>,
    source_pattern: u32,
    authored_pattern: u32,
) -> AuthoringResult<u32> {
    let stock_entity_tag = weapon_entity_assignment(stock_entity_assignments, source_pattern)
        .map_err(invalid)?
        .ok_or_else(|| {
            invalid(format!(
                "Selected item pattern 0x{source_pattern:08X} has no runtime entity assignment"
            ))
        })?;
    *entity_assignments = append_weapon_entity_assignment(
        std::mem::take(entity_assignments),
        authored_pattern,
        stock_entity_tag,
    )
    .map_err(invalid)?;
    Ok(authored_pattern)
}

/// What an imported model brings to its runtime, read from its asset graph.
#[cfg(feature = "d2-model-importer")]
struct ImportedRuntime {
    animation: Option<crate::item::custom_runtime::animation::ImportedAnimation>,
    audio: Option<crate::item::custom_runtime::audio::ImportedAudio>,
    extensions: Vec<crate::item::custom_runtime::extensions::Extension>,
    particles: Option<crate::item::custom_runtime::particles::ImportedParticles>,
    equipment: Option<crate::item::custom_runtime::animation::equipment::EquipmentAnimation>,
    crosshair: Option<crate::item::custom_runtime::crosshair::Crosshair>,
}

#[cfg(feature = "d2-model-importer")]
impl ImportedRuntime {
    /// Reads each part from `donor`'s imported graph, all empty for a weapon without one.
    fn load(donor: &resolve::ResolvedWeapon) -> AuthoringResult<Self> {
        use crate::item::custom_runtime::{animation, audio, crosshair, extensions, particles};
        let Some(graph) = donor.weapon.overrides.imported_graph.as_ref() else {
            return Ok(Self {
                animation: None,
                audio: None,
                extensions: Vec::new(),
                particles: None,
                equipment: None,
                crosshair: None,
            });
        };
        let mut audio = audio::load(graph)?;
        if let Some(audio) = audio.as_mut() {
            audio.set_item(donor.weapon.identity.item_hash);
        }
        Ok(Self {
            animation: animation::load(graph)?,
            audio,
            extensions: extensions::load(graph)?,
            particles: particles::load(graph)?,
            equipment: animation::equipment::load(graph)?,
            crosshair: crosshair::load(graph)?,
        })
    }
}

fn resolved_hud_key(
    manager: &PackageManager,
    patterns: &[u8],
    assignments: &[u8],
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<Option<u32>> {
    if !donor.weapon.kind.is_weapon() {
        return Ok(None);
    }
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
        donor.runtime_pattern_source == donor.gear_art_pattern_source
            && !content_graft
            && donor.pinned_appearance.is_none(),
        || {
            // A pinned model keeps the base's rig and row, yet the HUD shows the model it wears.
            let Some(appearance) = donor.pinned_appearance.or(donor.gear_art_pattern_source) else {
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

#[cfg(feature = "d2-model-importer")]
fn imported_asset_group(
    packages: &mut crate::asset_packages::AssetPackages,
    audio: Option<&crate::item::custom_runtime::audio::ImportedAudio>,
    animation: Option<&crate::item::custom_runtime::animation::ImportedAnimation>,
    equipment: Option<&crate::item::custom_runtime::animation::equipment::EquipmentAnimation>,
) -> AuthoringResult<Option<(usize, usize)>> {
    let clip_growth = match (audio, animation) {
        (Some(audio), Some(animation)) => {
            crate::item::custom_runtime::audio::clip_event_growth(audio, animation)?
        }
        _ => 0,
    };
    let bounds = animation
        .iter()
        .flat_map(|animation| animation.asset_bounds())
        .map(|size| size.saturating_add(clip_growth))
        .chain(audio.iter().flat_map(|audio| audio.asset_bounds()))
        .chain(
            equipment
                .iter()
                .flat_map(|animation| animation.asset_bounds()),
        )
        .collect::<Vec<_>>();
    Ok(if bounds.is_empty() {
        None
    } else {
        let index = packages.reserve_group(bounds)?;
        Some((index, packages.packages[index].tags.len()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires PARHELION_HUD_TEST_PACKAGES pointing to Shadowkeep packages"]
    fn hud_inheritance_follows_appearance_and_png_override_wins() {
        let packages = std::env::var_os("PARHELION_HUD_TEST_PACKAGES").unwrap();
        let sources = crate::item::sources::load_project_sources(Path::new(&packages)).unwrap();
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
                impacts: &mut vec![],
                #[cfg(feature = "d2-model-importer")]
                particle_symbols: &mut BTreeMap::new(),
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
        let properties = crate::weapon::ammo::property_offsets(&owner.payload, definition).unwrap();
        assert!(
            !properties.is_empty(),
            "the emitted owner has no HUD-bearing properties"
        );
        for offset in properties {
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
