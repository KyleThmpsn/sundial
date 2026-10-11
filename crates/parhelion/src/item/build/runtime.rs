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

#[derive(Clone, Copy)]
pub(super) struct EntitySources<'a> {
    pub sandbox_patterns: &'a [u8],
    pub entity_assignments: &'a [u8],
}

/// The build's store of compiled runtime fragments, or why it is unavailable. Weapons keep their
/// runtime entities in it, and abilities their private copies.
pub(super) struct Cache(Result<cache::Session, cache::Reason>);

impl Cache {
    /// Opens the store for one build and reports whether it is available.
    pub(super) fn open(
        manager: &PackageManager,
        stock: &EntitySources<'_>,
        progress: &mut Progress<'_>,
    ) -> Self {
        let initialization = std::time::Instant::now();
        let session = cache::Session::open(manager, stock);
        progress.diagnostic(format!(
            "Runtime cache: {}. Initialization {:.3}s.",
            match &session {
                Ok(_) => "available".to_owned(),
                Err(reason) => format!("unavailable {reason}"),
            },
            initialization.elapsed().as_secs_f64()
        ));
        Self(session)
    }

    /// The private copy of an ability entity that `author` writes into `tags` and the asset
    /// packages, or the one it wrote in an earlier build, replayed where they place it now, while
    /// `inputs`, everything the copy is made from besides native payloads, and every native
    /// payload it read are unchanged. `slot` names the authored ability's fragment, and `name`
    /// the ability in build progress.
    pub(super) fn ability_copy(
        &self,
        manager: &PackageManager,
        (slot, inputs, name): (&str, &str, &str),
        (allocator, tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
        (packages, placed): (&mut crate::asset_packages::AssetPackages, &mut Vec<TagHash>),
        progress: &mut Progress<'_>,
        author: impl FnOnce(
            &mut Vec<NewTagSpec>,
            (&mut crate::asset_packages::AssetPackages, &mut Vec<TagHash>),
            &mut Progress<'_>,
        ) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<TagHash> {
        // A copy has no assignment row, imported inputs, shared animation or impacts of its own.
        let mut impacts = Vec::new();
        #[cfg(feature = "d2-model-importer")]
        let mut particle_symbols = BTreeMap::new();
        let mut assets = RuntimeAssets {
            packages,
            placed,
            impacts: &mut impacts,
            #[cfg(feature = "d2-model-importer")]
            particle_symbols: &mut particle_symbols,
        };
        let (mut assignments, mut animation) = (Vec::new(), cache::AnimationCache::new());
        let pending = self
            .0
            .as_ref()
            .ok()
            .map(|session| session.prepare_copy((slot, inputs), (allocator, tags), &assets));
        if let Some(pending) = &pending {
            match pending.restore(manager, tags, &mut assets, &mut assignments, &mut animation) {
                Ok(restored) => {
                    let copy = restored
                        .result
                        .ok_or_else(|| invalid(format!("The reused copy of {name} is lost")))?;
                    progress.diagnostic(format!(
                        "Runtime cache for {name}: hit. {}.",
                        pending.timings()
                    ));
                    progress.start(&format!("Reusing Runtime for {name}"));
                    return Ok(copy);
                }
                Err(reason) => progress.diagnostic(format!(
                    "Runtime cache for {name}: miss {reason}. {}.",
                    pending.timings()
                )),
            }
        } else if let Err(reason) = &self.0 {
            progress.diagnostic(format!("Runtime cache for {name}: bypass {reason}."));
        }
        let trace = pending.as_ref().and_then(|_| manager.trace_reads());
        let compiling = std::time::Instant::now();
        let copy = author(tags, (&mut *assets.packages, &mut *assets.placed), progress)?;
        let compile_time = compiling.elapsed();
        let writing = std::time::Instant::now();
        let saved = pending.as_ref().map(|pending| {
            let reads = trace
                .and_then(|trace| trace.finish())
                .ok_or(cache::Reason::UntrackedReads)?;
            pending.save(
                manager,
                reads,
                (None, Some(copy)),
                tags,
                &assets,
                &assignments,
                &animation,
                #[cfg(feature = "d2-model-importer")]
                Some(custom_runtime::imports::Fingerprints::new()),
            )
        });
        progress.diagnostic(format!(
            "Runtime timings for {name}: compilation {:.3}s, cache save {:.3}s. Save {}.",
            compile_time.as_secs_f64(),
            writing.elapsed().as_secs_f64(),
            match saved {
                Some(Ok(())) => "saved".to_owned(),
                Some(Err(reason)) => format!("skipped {reason}"),
                None => "not requested".to_owned(),
            }
        ));
        Ok(copy)
    }
}

/// What authoring one weapon's runtime entity reads and extends: the stock tables, the tag
/// allocator, and the lists its authored tags, assets and assignments go into.
struct EntityAuthoring<'s, 'r, 'a> {
    manager: &'s PackageManager,
    stock_sandbox_patterns: &'s [u8],
    stock_entity_assignments: &'s [u8],
    weapon_runtime_tag_allocator: AppendedTagAllocator,
    assets: &'a mut RuntimeAssets<'r>,
    entity_assignments: &'a mut Vec<u8>,
    weapon_runtime_new_tags: &'a mut Vec<NewTagSpec>,
    animation_cache: &'a mut cache::AnimationCache,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn author_entities(
    manager: &PackageManager,
    stock: EntitySources<'_>,
    Cache(runtime_cache): &Cache,
    resolved: &[resolve::ResolvedWeapon],
    weapon_runtime_tag_allocator: AppendedTagAllocator,
    mut assets: RuntimeAssets<'_>,
    entity_assignments: &mut Vec<u8>,
    weapon_runtime_new_tags: &mut Vec<NewTagSpec>,
    progress: &mut Progress<'_>,
) -> AuthoringResult<Vec<Option<u32>>> {
    let (mut hits, mut misses, mut bypassed, mut saved, mut skipped) = (0, 0, 0, 0, 0);
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
            .ok()
            .filter(|_| !names_particles)
            .map(|cache| {
                cache.prepare(
                    donor,
                    weapon_runtime_tag_allocator,
                    weapon_runtime_new_tags,
                    &assets,
                    entity_assignments,
                    &animation_cache,
                )
            });
        if let Some(pending) = &pending {
            match pending.restore(
                manager,
                weapon_runtime_new_tags,
                &mut assets,
                entity_assignments,
                &mut animation_cache,
            ) {
                Ok(restored) => {
                    hits += 1;
                    progress.diagnostic(format!(
                        "Runtime cache for {}: hit. {}.",
                        donor.weapon.text.name,
                        pending.timings()
                    ));
                    authored_pattern_global_ids.push(restored.pattern);
                    progress.finish(&format!("Reusing Runtime for {}", donor.weapon.text.name));
                    continue;
                }
                Err(reason) => {
                    misses += 1;
                    progress.diagnostic(format!(
                        "Runtime cache for {}: miss {reason}. {}.",
                        donor.weapon.text.name,
                        pending.timings()
                    ));
                }
            }
        } else {
            bypassed += 1;
            let reason = runtime_cache.as_ref().err().map_or_else(
                || cache::Reason::ParticleSymbols.to_string(),
                ToString::to_string,
            );
            progress.diagnostic(format!(
                "Runtime cache for {}: bypass {reason}.",
                donor.weapon.text.name
            ));
        }
        let trace = pending.as_ref().and_then(|_| manager.trace_reads());
        #[cfg(feature = "d2-model-importer")]
        let mut imported_reads = Some(custom_runtime::imports::Fingerprints::new());
        let operation = format!("Compiling Runtime for {}", donor.weapon.text.name);
        progress.start(&operation);
        let compiling = std::time::Instant::now();
        let authored_pattern_global_id = author_entity(
            EntityAuthoring {
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                weapon_runtime_tag_allocator,
                assets: &mut assets,
                entity_assignments: &mut *entity_assignments,
                weapon_runtime_new_tags: &mut *weapon_runtime_new_tags,
                animation_cache: &mut animation_cache,
            },
            donor,
            authored_pattern_global_ids.len(),
            #[cfg(feature = "d2-model-importer")]
            &mut imported_reads,
        )
        .map_err(|error| donor.weapon.in_recipe(error))?;
        let compile_time = compiling.elapsed();
        let writing = std::time::Instant::now();
        let write_result = pending.as_ref().map(|pending| {
            let reads = trace
                .and_then(|trace| trace.finish())
                .ok_or(cache::Reason::UntrackedReads)?;
            pending.save(
                manager,
                reads,
                (authored_pattern_global_id, None),
                weapon_runtime_new_tags,
                &assets,
                entity_assignments,
                &animation_cache,
                #[cfg(feature = "d2-model-importer")]
                imported_reads,
            )
        });
        let write_time = writing.elapsed();
        let write_status = match write_result {
            Some(Ok(())) => {
                saved += 1;
                "saved".to_owned()
            }
            Some(Err(reason)) => {
                skipped += 1;
                format!("skipped {reason}")
            }
            None => "not requested".to_owned(),
        };
        progress.diagnostic(format!(
            "Runtime timings for {}: compilation {:.3}s, cache save {:.3}s. Save {write_status}.",
            donor.weapon.text.name,
            compile_time.as_secs_f64(),
            write_time.as_secs_f64()
        ));
        authored_pattern_global_ids.push(authored_pattern_global_id);
        progress.finish(&operation);
    }
    progress.diagnostic(format!("Runtime cache summary: {hits} hits, {misses} misses, {bypassed} bypassed, {saved} saved, {skipped} saves skipped."));
    Ok(authored_pattern_global_ids)
}

/// Whether the recipe asks for anything on the runtime entity beyond the donor's own.
fn recipe_runtime_edits(donor: &resolve::ResolvedWeapon) -> bool {
    let overrides = &donor.weapon.overrides;
    overrides
        .sparrow
        .as_ref()
        .is_some_and(crate::vehicle::Sparrow::has_changes)
        || donor.appearance_rig_donor.is_some()
        || donor.animation_pattern_source.is_some()
        || !donor.animation_action_sources.is_empty()
        || donor.pinned_appearance.is_some()
        || donor.type_marker_pattern_source.is_some()
        || overrides.ammo_type.is_some()
        || !overrides.runtime_values.is_empty()
        || overrides.projectile.is_some()
        || overrides.barrel.is_some()
        || overrides.sword_profile.is_some()
        || !donor.component_splice_sources.is_empty()
        || !overrides.runtime_resource_patches.is_empty()
        || !overrides.additional_behaviors.is_empty()
        || overrides
            .raw_payload_patches
            .iter()
            .any(|patch| patch.target == WeaponRawPayloadTarget::RuntimeWeaponEntity)
}

/// The authored runtime entity for one weapon, or `None` when its donor has no pattern row.
/// Runtime edits are compiled onto the pattern donor's entity, and the entity is appended as a
/// new tag with its assignment row.
// The importer adds its own runtime parts to this one orchestration.
#[cfg_attr(feature = "d2-model-importer", allow(clippy::cognitive_complexity))]
fn author_entity(
    authoring: EntityAuthoring<'_, '_, '_>,
    donor: &resolve::ResolvedWeapon,
    weapon_ordinal: usize,
    #[cfg(feature = "d2-model-importer")] imported_reads: &mut Option<
        custom_runtime::imports::Fingerprints,
    >,
) -> AuthoringResult<Option<u32>> {
    let EntityAuthoring {
        manager,
        stock_sandbox_patterns,
        stock_entity_assignments,
        weapon_runtime_tag_allocator,
        assets,
        entity_assignments,
        weapon_runtime_new_tags,
        animation_cache,
    } = authoring;
    // These inputs are needed only when imported runtime attachments are enabled.
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = (weapon_ordinal, assets, animation_cache);
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
    let hud_key = resolved_hud_key(
        manager,
        stock_sandbox_patterns,
        stock_entity_assignments,
        donor,
    )?;
    #[cfg(feature = "d2-model-importer")]
    let imported = ImportedRuntime::load(donor, imported_reads)?;
    #[cfg(feature = "d2-model-importer")]
    let imported_edits = imported.any();
    #[cfg(not(feature = "d2-model-importer"))]
    let imported_edits = false;
    #[cfg(feature = "d2-model-importer")]
    let ImportedRuntime {
        animation: mut imported_animation,
        audio: imported_audio,
        extensions: imported_extensions,
        particles: mut imported_particles,
        equipment: equipment_animation,
        crosshair: imported_crosshair,
    } = imported;
    let vehicle_settings = donor.weapon.overrides.sparrow.as_ref();
    let has_runtime_edits = recipe_runtime_edits(donor)
        || imported_edits
        || !component_donors.is_empty()
        || hud_key.is_some();
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
        manager,
        vehicle_settings,
        pattern_entity_tag,
        pattern_entity,
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
        crate::weapon::rig::graft_presentation(&mut pattern_entity, &appearance_entity).map_err(
            |error| {
                invalid(format!(
                    "Could not move the appearance's rig and animations onto this runtime: {error}"
                ))
            },
        )?;
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
    graft_weapon_component_bindings_or_rewire(&mut pattern_entity, &component_grafts, &|tag| {
        manager.read_tag(tag)
    })
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
    let particle_symbols = match &mut imported_particles {
        Some(particles) => {
            particles.prepare_runtime(
                manager,
                runtime_overrides
                    .fired_graph
                    .as_ref()
                    .and_then(|fired| fired.imported.as_deref()),
                &pattern_entity,
                groups.own.or(groups.selected).unwrap_or(0),
                runtime_overrides
                    .behavior_projectile_speed
                    .unwrap_or(crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST),
            )?;
            let (symbols, eager) = particles.author(manager, assets.packages)?;
            assets.placed.extend(eager);
            assets
                .particle_symbols
                .insert(weapon_ordinal, symbols.clone());
            symbols
        }
        None => BTreeMap::new(),
    };
    #[cfg(feature = "d2-model-importer")]
    if let Some(assets) = &imported_particles {
        assets.apply_presentation(
            manager,
            pattern_entity_tag,
            &mut pattern_entity,
            &particle_symbols,
        )?;
    }
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
            overrides
                .runtime_resource_patches
                .extend(crate::weapon::crosshair::content_patches(
                    manager,
                    &pattern_entity,
                    crosshair.type_key,
                )?);
        }
        if let Some(fired) = overrides.fired_graph.as_mut() {
            fired.validate()?;
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
    #[cfg(feature = "d2-model-importer")]
    let imported_projectile = runtime_overrides
        .fired_graph
        .as_ref()
        .and_then(|fired| fired.imported.as_deref())
        .map(|symbol| {
            imported_particles
                .as_ref()
                .ok_or_else(|| invalid("Imported projectile has no asset group"))?
                .projectile(symbol)?;
            particle_symbols
                .get(symbol)
                .copied()
                .ok_or_else(|| invalid("Imported projectile root has not been allocated"))
        })
        .transpose()?;
    #[cfg(not(feature = "d2-model-importer"))]
    let imported_projectile = None;
    // A moved rig makes the pattern row name the appearance's weapon type, and the stat
    // translator converts by that type. The base weapon's conversion is put back, so the
    // appearance changes how the weapon looks and not its fire rate.
    let mut stat_patches = match (
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
    // Bullets per Shot is the translator column every Barrel bullet input copies, in the gameplay
    // pattern's table, whose arrays a moved rig's row is pointed at too.
    if let Some(bullets) = donor
        .weapon
        .overrides
        .barrel
        .as_ref()
        .and_then(|barrel| barrel.bullets_per_shot)
    {
        stat_patches.extend(crate::weapon::burst::patches(
            manager,
            &pattern_entity,
            pattern_source.weapon_translation_group_hash,
            bullets,
        )?);
    }
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
    // A donor item's selector can name a shared pattern with a different item identity.
    let splices = donor
        .component_splice_sources
        .iter()
        .map(|(binding, source)| {
            let (_, entity) = resolve_runtime_weapon_entity(
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                source.item_hash,
                "component donor entity",
            )?;
            Ok((*binding, entity))
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
            projectile: imported_projectile,
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
    let group = imported_asset_group(
        assets.packages,
        imported_audio.as_ref(),
        imported_animation.as_ref(),
        equipment_animation.as_ref(),
    )?;
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
                animation_cache,
            )?;
            Some(owner)
        }
        _ => None,
    };
    #[cfg(feature = "d2-model-importer")]
    if let (Some(animation), Some((index, _))) = (&equipment_animation, group) {
        crate::item::custom_runtime::animation::equipment::author(
            manager,
            animation,
            pattern_entity_tag,
            &mut pattern_entity,
            weapon_runtime_tag_allocator,
            weapon_runtime_new_tags,
            asset_target(assets.packages, index),
            animation_cache,
        )?;
    }
    #[cfg(feature = "d2-model-importer")]
    if let (Some(audio), Some((index, _))) = (&imported_audio, group) {
        let (allocator, tags) = asset_target(assets.packages, index);
        if let Some(impact) =
            crate::item::custom_runtime::audio::author_impacts(manager, audio, allocator, tags)?
        {
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
    crate::vehicle::authoring::apply(
        manager,
        vehicle_settings,
        pattern_entity_tag,
        &mut pattern_entity,
        &weapon_runtime_tag_allocator,
        weapon_runtime_new_tags,
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
    /// Whether the recipe imports anything onto the runtime entity.
    fn any(&self) -> bool {
        self.animation.is_some()
            || self.audio.is_some()
            || self.equipment.is_some()
            || self.particles.is_some()
            || !self.extensions.is_empty()
            || self.crosshair.is_some()
    }

    /// Reads each part from `donor`'s imported graph, all empty for a weapon without one.
    fn load(
        donor: &resolve::ResolvedWeapon,
        reads: &mut Option<custom_runtime::imports::Fingerprints>,
    ) -> AuthoringResult<Self> {
        use crate::item::custom_runtime::{animation, audio, crosshair, extensions, particles};
        let mut loaded = Self {
            animation: None,
            audio: None,
            extensions: Vec::new(),
            particles: None,
            equipment: None,
            crosshair: None,
        };
        let mut directories = BTreeSet::new();
        if let Some(reference) = donor.weapon.overrides.imported_graph.as_ref() {
            let inputs = custom_runtime::imports::Inputs::open(reference)?;
            let graph = &inputs;
            loaded = Self {
                animation: animation::load(graph)?,
                audio: audio::load(graph)?,
                extensions: extensions::load(graph)?,
                particles: particles::load(graph)?,
                equipment: animation::equipment::load(graph)?,
                crosshair: crosshair::load(graph)?,
            };
            if let Some(audio) = loaded.audio.as_mut() {
                audio.set_item(donor.weapon.identity.item_hash);
            }
            directories.insert(
                fs::canonicalize(&reference.directory)
                    .map_err(|error| invalid(error.to_string()))?,
            );
            *reads = inputs.fingerprints();
        }
        for imported in donor
            .weapon
            .overrides
            .socket_plug_variants
            .iter()
            .flat_map(|variant| &variant.sandbox_perks)
            .filter_map(|perk| perk.program.as_ref())
            .flat_map(|program| &program.imported_assets)
        {
            let directory = fs::canonicalize(&imported.directory).map_err(|error| {
                invalid(format!("Imported attachment {}: {error}", imported.symbol))
            })?;
            let check = || -> AuthoringResult<()> {
                let actual = parhelion_import::d2_mot::native::attachment::fingerprint(&directory)
                    .map_err(|error| {
                        invalid(format!("Imported attachment {}: {error}", imported.symbol))
                    })?;
                if actual != imported.sha256 {
                    return Err(invalid(
                        "Imported attachment assets changed after preparation. Import the perk again.",
                    ));
                }
                Ok(())
            };
            check()?;
            if directories.insert(directory.clone()) {
                let inputs = custom_runtime::imports::Inputs::open_directory(&directory)?;
                let group = particles::load(&inputs)?
                    .ok_or_else(|| invalid("Imported attachment group has no assets"))?;
                group.attachment(&imported.symbol)?;
                if let Some(assets) = loaded.particles.as_mut() {
                    assets.merge(group)?;
                } else {
                    loaded.particles = Some(group);
                }
            }
            loaded
                .particles
                .as_ref()
                .ok_or_else(|| invalid("Imported attachment has no asset group"))?
                .attachment(&imported.symbol)?;
            check()?;
            // These additional directories participate in the live symbol allocation.
            // Programs naming them bypass persistent runtime reuse above.
            *reads = None;
        }
        Ok(loaded)
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
    #[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
    fn hud_inheritance_follows_appearance_and_png_override_wins() {
        let packages = crate::test_support::stock_packages();
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
        let stock = EntitySources {
            sandbox_patterns: &sources.stock_sandbox_patterns,
            entity_assignments: &sources.stock_entity_assignments,
        };
        let mut report = |_: Event<'_>| {};
        let mut progress = Progress::new(donors.len(), &mut report);
        let cache = Cache::open(&sources.manager, &stock, &mut progress);
        author_entities(
            &sources.manager,
            stock,
            &cache,
            &donors,
            allocator,
            RuntimeAssets {
                packages: &mut crate::asset_packages::AssetPackages::default(),
                placed: &mut vec![],
                impacts: &mut vec![],
                #[cfg(feature = "d2-model-importer")]
                particle_symbols: &mut BTreeMap::new(),
            },
            &mut assignments,
            &mut tags,
            &mut progress,
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
