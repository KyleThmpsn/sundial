use super::*;
use std::borrow::Cow;

#[cfg(feature = "d2-model-importer")]
mod attachments;
mod cache;
mod composition;
mod dependencies;
#[cfg(feature = "d2-model-importer")]
mod imported;
mod plan;
mod report;
pub(super) use plan::{DependencyTimings, Payloads, author};

/// Where imported runtime assets go: a group per item in the rolling asset packages, and
/// the tags placed there for the runtime dependency index.
pub(super) struct RuntimeAssets<'a> {
    pub packages: &'a mut crate::asset_packages::AssetPackages,
    pub placed: &'a mut Vec<TagHash>,
    pub impacts: &'a mut Vec<crate::ability::banks::MeleeImpact>,
    /// Each item's imported particle tags by node symbol, by item ordinal, for the private
    /// perk programs that name them.
    #[cfg(feature = "d2-model-importer")]
    pub particle_symbols: &'a mut BTreeMap<usize, BTreeMap<String, TagHash>>,
    /// The HUD statuses bound on the imported groups weapon runtimes allocated, whose images
    /// are authored with the attachment groups.
    #[cfg(feature = "d2-model-importer")]
    pub hud_statuses: &'a mut Vec<sundial::package_authoring::sandbox_perk::program::HudStatus>,
}

/// What one item's runtime work extends in [`RuntimeAssets`], recorded before it starts so a
/// failed item can leave them as it found them while the other items still compile. The lists
/// only grow, so their lengths are recorded rather than their payloads, and only the small map
/// of shared clips is copied.
struct Restore {
    #[cfg(feature = "d2-model-importer")]
    ordinal: usize,
    packages: Vec<(usize, usize)>,
    reservations: usize,
    shared_clips: BTreeMap<
        (
            Option<sundial::package_authoring::mod_content::Importer>,
            [u8; 32],
        ),
        TagHash,
    >,
    placed: usize,
    impacts: usize,
    #[cfg(feature = "d2-model-importer")]
    statuses: usize,
    #[cfg(feature = "d2-model-importer")]
    symbols: Option<BTreeMap<String, TagHash>>,
}

impl Restore {
    fn take(assets: &RuntimeAssets<'_>, ordinal: usize) -> Self {
        #[cfg(not(feature = "d2-model-importer"))]
        let _ = ordinal;
        Self {
            #[cfg(feature = "d2-model-importer")]
            ordinal,
            packages: assets
                .packages
                .packages
                .iter()
                .map(|package| (package.tags.len(), package.references.len()))
                .collect(),
            reservations: assets.packages.reservations.len(),
            shared_clips: assets.packages.shared_clips.clone(),
            placed: assets.placed.len(),
            impacts: assets.impacts.len(),
            #[cfg(feature = "d2-model-importer")]
            statuses: assets.hud_statuses.len(),
            #[cfg(feature = "d2-model-importer")]
            symbols: assets.particle_symbols.get(&ordinal).cloned(),
        }
    }

    fn apply(self, assets: &mut RuntimeAssets<'_>) {
        let packages = &mut assets.packages;
        packages.packages.truncate(self.packages.len());
        for (package, (tags, references)) in packages.packages.iter_mut().zip(self.packages) {
            package.tags.truncate(tags);
            package.references.truncate(references);
        }
        packages.reservations.truncate(self.reservations);
        packages.shared_clips = self.shared_clips;
        assets.placed.truncate(self.placed);
        assets.impacts.truncate(self.impacts);
        #[cfg(feature = "d2-model-importer")]
        {
            assets.hud_statuses.truncate(self.statuses);
            match self.symbols {
                Some(symbols) => {
                    assets.particle_symbols.insert(self.ordinal, symbols);
                }
                None => {
                    assets.particle_symbols.remove(&self.ordinal);
                }
            }
        }
    }
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
        if session.is_err() {
            progress.diagnostic(
                "Saved item resources are unavailable. The build will compile them again.".into(),
            );
        }
        progress.runtime_detail(format!(
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
        let (mut particle_symbols, mut hud_statuses) = (BTreeMap::new(), Vec::new());
        let mut assets = RuntimeAssets {
            packages,
            placed,
            impacts: &mut impacts,
            #[cfg(feature = "d2-model-importer")]
            particle_symbols: &mut particle_symbols,
            #[cfg(feature = "d2-model-importer")]
            hud_statuses: &mut hud_statuses,
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
                    progress.runtime_detail(format!(
                        "Runtime cache for {name}: hit. {}.",
                        pending.timings()
                    ));
                    progress.start(&format!("Reusing Cached Resources for {name}"));
                    return Ok(copy);
                }
                Err(reason) => {
                    progress.diagnostic(report::cache_miss(name, &reason));
                    progress.runtime_detail(format!(
                        "Runtime cache for {name}: miss {reason}. {}.",
                        pending.timings()
                    ));
                }
            }
        } else if let Err(reason) = &self.0 {
            progress.runtime_detail(format!("Runtime cache for {name}: bypass {reason}."));
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
        let write_time = writing.elapsed();
        let cache_saved = matches!(&saved, Some(Ok(())));
        if let Some(Err(reason)) = &saved {
            progress.diagnostic(report::cache_not_saved(name, reason));
        }
        progress.diagnostic(report::compiled(
            name,
            compile_time,
            cache_saved.then_some(write_time),
        ));
        progress.runtime_detail(format!(
            "Runtime timings for {name}: compilation {:.3}s, cache save {:.3}s. {}.",
            compile_time.as_secs_f64(),
            write_time.as_secs_f64(),
            match saved {
                Some(Ok(())) => "Cache saved".to_owned(),
                Some(Err(reason)) => format!("Cache not saved: {reason}"),
                None => "No cache save requested".to_owned(),
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
    stages: &'a mut Stages,
}

/// Where one weapon's runtime compile spent its time, by stage in authoring order, for the build
/// log. A stage the weapon never reached is left out.
struct Stages {
    last: std::time::Instant,
    laps: Vec<(&'static str, std::time::Duration)>,
}

impl Stages {
    fn start() -> Self {
        Self {
            last: std::time::Instant::now(),
            laps: Vec::new(),
        }
    }

    /// Ends `stage` now, counting the time since the previous stage ended.
    fn lap(&mut self, stage: &'static str) {
        let now = std::time::Instant::now();
        self.laps.push((stage, now - self.last));
        self.last = now;
    }
}

impl std::fmt::Display for Stages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut laps = self.laps.iter();
        let Some((stage, time)) = laps.next() else {
            return Ok(());
        };
        write!(f, " ({stage} {:.3}s", time.as_secs_f64())?;
        for (stage, time) in laps {
            write!(f, ", {stage} {:.3}s", time.as_secs_f64())?;
        }
        write!(f, ")")
    }
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
    failed: &mut BTreeMap<usize, crate::AuthoringError>,
    progress: &mut Progress<'_>,
) -> AuthoringResult<Vec<Option<u32>>> {
    let (mut hits, mut misses, mut bypassed, mut saved, mut skipped) = (0, 0, 0, 0, 0);
    let stock_sandbox_patterns = stock.sandbox_patterns;
    let stock_entity_assignments = stock.entity_assignments;
    let mut authored_pattern_global_ids = Vec::with_capacity(resolved.len());
    let mut animation_cache = cache::AnimationCache::new();
    for donor in resolved {
        progress.start(&format!(
            "Checking Runtime Cache for {}",
            donor.weapon.text.name
        ));
        let placed_from = (weapon_runtime_new_tags.len(), assets.placed.len());
        // A fragment keeps the item's imported particle symbols and HUD statuses, so private
        // perks that name them compile the same after a replay.
        let pending = runtime_cache.as_ref().ok().map(|cache| {
            cache.prepare(
                (donor, authored_pattern_global_ids.len()),
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
                    progress.runtime_detail(format!(
                        "Runtime cache for {}: hit. {}.",
                        donor.weapon.text.name,
                        pending.timings()
                    ));
                    authored_pattern_global_ids.push(restored.pattern);
                    progress.runtime_detail(report::placement(
                        &donor.weapon.text.name,
                        weapon_runtime_tag_allocator,
                        placed_from.0..weapon_runtime_new_tags.len(),
                        &assets.placed[placed_from.1..],
                    ));
                    progress.finish(&format!(
                        "Reusing Cached Resources for {}",
                        donor.weapon.text.name
                    ));
                    continue;
                }
                Err(reason) => {
                    misses += 1;
                    progress.diagnostic(report::cache_miss(&donor.weapon.text.name, &reason));
                    progress.runtime_detail(format!(
                        "Runtime cache for {}: miss {reason}. {}.",
                        donor.weapon.text.name,
                        pending.timings()
                    ));
                }
            }
        } else if let Err(reason) = runtime_cache {
            bypassed += 1;
            progress.runtime_detail(format!(
                "Runtime cache for {}: bypass {reason}.",
                donor.weapon.text.name
            ));
        }
        let trace = pending.as_ref().and_then(|_| manager.trace_reads());
        #[cfg(feature = "d2-model-importer")]
        let mut imported_reads = Some(custom_runtime::imports::Fingerprints::new());
        let operation = format!("Building Item Resources for {}", donor.weapon.text.name);
        progress.start(&operation);
        let compiling = std::time::Instant::now();
        let mut stages = Stages::start();
        // A failed item leaves everything it extended as it was, and the other items compile.
        let ordinal = authored_pattern_global_ids.len();
        let restore = Restore::take(&assets, ordinal);
        let host_start = weapon_runtime_new_tags.len();
        let (assignments_before, animation_before) =
            (entity_assignments.clone(), animation_cache.clone());
        let allocation = crate::appended_tags::checkpoint();
        let authored = author_entity(
            EntityAuthoring {
                manager,
                stock_sandbox_patterns,
                stock_entity_assignments,
                weapon_runtime_tag_allocator,
                assets: &mut assets,
                entity_assignments: &mut *entity_assignments,
                weapon_runtime_new_tags: &mut *weapon_runtime_new_tags,
                animation_cache: &mut animation_cache,
                stages: &mut stages,
            },
            donor,
            ordinal,
            #[cfg(feature = "d2-model-importer")]
            &mut imported_reads,
        );
        let authored_pattern_global_id = match authored {
            Ok(id) => {
                allocation.commit();
                id
            }
            Err(error) => {
                drop(allocation);
                restore.apply(&mut assets);
                weapon_runtime_new_tags.truncate(host_start);
                *entity_assignments = assignments_before;
                animation_cache = animation_before;
                progress.diagnostic(format!(
                    "Runtime for {} failed. Compiling the other items before reporting it.",
                    donor.weapon.text.name
                ));
                failed.insert(ordinal, donor.weapon.in_recipe(error));
                authored_pattern_global_ids.push(None);
                progress.finish(&operation);
                continue;
            }
        };
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
        let cache_saved = matches!(&write_result, Some(Ok(())));
        let write_status = match write_result {
            Some(Ok(())) => {
                saved += 1;
                "Cache saved".to_owned()
            }
            Some(Err(reason)) => {
                skipped += 1;
                progress.diagnostic(report::cache_not_saved(&donor.weapon.text.name, &reason));
                format!("Cache not saved: {reason}")
            }
            None => "No cache save requested".to_owned(),
        };
        progress.diagnostic(report::compiled(
            &donor.weapon.text.name,
            compile_time,
            cache_saved.then_some(write_time),
        ));
        progress.runtime_detail(format!(
            "Runtime timings for {}: compilation {:.3}s{stages}, cache save {:.3}s. {write_status}.",
            donor.weapon.text.name,
            compile_time.as_secs_f64(),
            write_time.as_secs_f64()
        ));
        authored_pattern_global_ids.push(authored_pattern_global_id);
        progress.runtime_detail(report::placement(
            &donor.weapon.text.name,
            weapon_runtime_tag_allocator,
            placed_from.0..weapon_runtime_new_tags.len(),
            &assets.placed[placed_from.1..],
        ));
        progress.finish(&operation);
    }
    progress.runtime_detail(format!("Runtime cache summary: {hits} hits, {misses} misses, {bypassed} bypassed, {saved} saved, {skipped} saves skipped."));
    Ok(authored_pattern_global_ids)
}

/// Whether the recipe asks for anything on the runtime entity beyond the donor's own.
fn recipe_runtime_edits(donor: &resolve::ResolvedWeapon) -> bool {
    donor
        .weapon
        .overrides
        .sparrow
        .as_ref()
        .is_some_and(crate::vehicle::Sparrow::has_changes)
        || weapon_runtime_edits(donor)
}

fn weapon_runtime_edits(donor: &resolve::ResolvedWeapon) -> bool {
    let overrides = &donor.weapon.overrides;
    let donors = &donor.donors;
    donors.appearance_rig.is_some()
        || donors.animation.is_some()
        || !donors.animation_actions.is_empty()
        || donors.pinned_appearance.is_some()
        || donors.type_marker.is_some()
        || donors.behavior.is_some()
        || overrides.content_variant.is_some()
        || overrides.ammo_type.is_some()
        || !overrides.runtime_values.is_empty()
        || overrides.fired_graph.is_some()
        || overrides.projectile.is_some()
        || overrides.barrel.is_some()
        || overrides.sword_profile.is_some()
        || !donors.component_splices.is_empty()
        || !overrides.runtime_resource_patches.is_empty()
        || !overrides.additional_behaviors.is_empty()
        || overrides
            .raw_payload_patches
            .iter()
            .any(|patch| patch.target == WeaponRawPayloadTarget::RuntimeWeaponEntity)
}

/// The authored runtime entity for one weapon, or `None` when its donor has no pattern row.
/// Runtime edits are compiled onto the pattern donor's entity, and the entity is appended as a
/// new tag with its assignment row. A weapon without edits points at its donor's stock entity,
/// and a complete imported vehicle keeps the entity its graph prepared.
///
/// Each stage reads what the stages before it wrote. Native donors compose the entity, imported
/// particles take their asset group before the fired graph names them, the overrides gain the
/// imported and stat translator patches, and the runtime edits are compiled before the imported
/// callbacks, media and vehicle settings are authored onto the edited entity.
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
        weapon_runtime_tag_allocator: allocator,
        assets,
        entity_assignments,
        weapon_runtime_new_tags: tags,
        animation_cache,
        stages,
    } = authoring;
    // This input is needed only when imported runtime attachments are enabled.
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = weapon_ordinal;
    let Some(row) = donor
        .runtime_pattern_source
        .or(donor.gear_art_pattern_source)
    else {
        return Ok(None);
    };
    let stock = composition::Stock {
        manager,
        patterns: stock_sandbox_patterns,
        assignments: stock_entity_assignments,
    };
    let component_donors = composition::component_donors(donor);
    let hud_key = resolved_hud_key(
        manager,
        stock_sandbox_patterns,
        stock_entity_assignments,
        donor,
    )?;
    stages.lap("HUD key");
    #[cfg(feature = "d2-model-importer")]
    let mut imported = imported::Runtime::load(donor, imported_reads)?;
    #[cfg(feature = "d2-model-importer")]
    stages.lap("import loading");
    #[cfg(feature = "d2-model-importer")]
    let imported_edits = imported.any();
    #[cfg(not(feature = "d2-model-importer"))]
    let imported_edits = false;
    if !(recipe_runtime_edits(donor)
        || imported_edits
        || !component_donors.is_empty()
        || hud_key.is_some())
    {
        return reuse_stock_entity(
            stock_entity_assignments,
            entity_assignments,
            row.pattern_global_id_hash,
            donor.weapon.identity.pattern_global_id_hash,
        )
        .map(Some);
    }
    let pattern_source = donor
        .runtime_pattern_source
        .ok_or_else(|| invalid("Runtime edits require an active item sandbox pattern"))?;

    // Native donors compose the entity the edits are compiled onto.
    let (tag, mut entity) = composition::pattern_entity(&stock, donor, pattern_source)?;
    let markers = composition::graft_rig(&stock, donor, &mut entity)?;
    composition::graft_components(&stock, &component_donors, &mut entity)?;
    let groups = composition::content_groups(&stock, donor, &entity)?;
    stages.lap("composition");

    // Imported particles take their asset group, and a complete imported vehicle ends here.
    #[cfg(feature = "d2-model-importer")]
    let symbols = imported.prepare(
        manager,
        donor,
        &mut entity,
        groups,
        assets,
        &mut *animation_cache,
        weapon_ordinal,
    )?;
    #[cfg(feature = "d2-model-importer")]
    stages.lap("imported particles");
    #[cfg(feature = "d2-model-importer")]
    let vehicle = imported.vehicle(
        manager,
        donor,
        (tag, &mut entity),
        &symbols,
        !component_donors.is_empty(),
    )?;
    #[cfg(not(feature = "d2-model-importer"))]
    let vehicle = None;
    if let Some(prepared) = vehicle {
        return complete_vehicle(
            donor.weapon.identity.pattern_global_id_hash,
            (prepared, entity.as_slice()),
            hud_key,
            assets.packages,
            entity_assignments,
        )
        .map(Some);
    }

    // The recipe's overrides gain the imported graph's patches before the projectile roots are
    // chosen from them, and the stat translator patches last.
    let overrides = Cow::Borrowed(&donor.weapon.overrides);
    #[cfg(feature = "d2-model-importer")]
    let overrides = Cow::Owned(imported.overrides(
        manager,
        &overrides,
        &entity,
        pattern_source.weapon_content_group_hash,
        groups,
        &symbols,
    )?);
    #[cfg(feature = "d2-model-importer")]
    let projectile = imported.projectile(&overrides, &symbols)?;
    #[cfg(not(feature = "d2-model-importer"))]
    let projectile = ImportedProjectile::default();
    let bundled = bundled_projectile(
        manager,
        &overrides,
        &entity,
        groups,
        (assets, &mut *animation_cache),
    )?;
    let stat_patches = composition::stat_patches(manager, &entity, donor, pattern_source)?;
    #[cfg(feature = "d2-model-importer")]
    let stat_patches = {
        let mut stat_patches = stat_patches;
        stat_patches.extend(imported.stat_patches(
            manager,
            &entity,
            pattern_source.weapon_translation_group_hash,
        )?);
        stat_patches
    };
    let overrides = with_stat_patches(overrides, stat_patches);
    stages.lap("projectile and stats");

    // The runtime edits, with the donors the recipe's splices and single actions name.
    let splices = composition::splices(&stock, donor)?;
    let actions = composition::actions(&stock, donor)?;
    author_runtime_edits(
        manager,
        &mut entity,
        &overrides,
        hud_key,
        crate::item::custom_runtime::Sources {
            groups,
            splices: &splices,
            actions: &actions,
            markers: &markers,
            projectile: projectile.root.or(bundled),
            trajectories: projectile.trajectories,
            source_trajectory: projectile.source_trajectory,
        },
        allocator,
        tags,
    )?;
    stages.lap("runtime edits");

    // What is authored onto the edited entity before it is appended.
    #[cfg(feature = "d2-model-importer")]
    imported.callbacks(manager, &mut entity, allocator, tags)?;
    #[cfg(feature = "d2-model-importer")]
    imported.author_media(
        manager,
        (tag, &mut entity),
        (allocator, &mut *tags),
        assets,
        animation_cache,
        &symbols,
    )?;
    #[cfg(feature = "d2-model-importer")]
    stages.lap("callbacks and media");
    crate::vehicle::authoring::apply(
        manager,
        donor.weapon.overrides.sparrow.as_ref(),
        tag,
        &mut entity,
        &allocator,
        tags,
    )?;
    let appended = append_entity(
        donor.weapon.identity.pattern_global_id_hash,
        (tag, entity),
        (allocator, tags),
        entity_assignments,
    )
    .map(Some);
    stages.lap("appending");
    appended
}

/// The imported projectile root the fired graph names, with what its layout tells the runtime
/// edits. Without the model importer there is none.
#[derive(Default)]
struct ImportedProjectile {
    root: Option<TagHash>,
    /// Rows in the root's trajectory pool, when its layout is known.
    trajectories: Option<usize>,
    source_trajectory: bool,
}

/// The bundled projectile preset the fired graph names, prepared for `entity` and authored into
/// the asset packages, sharing entries other groups already hold. Returns its root.
fn bundled_projectile(
    manager: &PackageManager,
    overrides: &WeaponCloneOverrides,
    entity: &[u8],
    groups: crate::weapon::behavior::ContentGroups,
    (assets, shared): (&mut RuntimeAssets<'_>, &mut cache::AnimationCache),
) -> AuthoringResult<Option<TagHash>> {
    overrides
        .fired_graph
        .as_ref()
        .and_then(|fired| fired.bundled)
        .map(|preset| {
            let mut projectile =
                crate::item::custom_runtime::bundled::Projectile::load(manager, preset)?;
            projectile.prepare(
                manager,
                entity,
                groups.own.or(groups.selected).unwrap_or(0),
                overrides
                    .behavior_projectile_speed
                    .unwrap_or(crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST),
            )?;
            let (root, eager) = projectile.author(manager, assets.packages, shared)?;
            assets.placed.extend(eager);
            Ok::<_, crate::AuthoringError>(root)
        })
        .transpose()
}

/// `overrides` with the stat translator `patches` appended, cloned only when there are any.
fn with_stat_patches(
    mut overrides: Cow<'_, WeaponCloneOverrides>,
    patches: Vec<WeaponRuntimeResourcePatch>,
) -> Cow<'_, WeaponCloneOverrides> {
    if !patches.is_empty() {
        overrides.to_mut().runtime_resource_patches.extend(patches);
    }
    overrides
}

/// Points `pattern` at a complete imported vehicle's prepared entity `tag`, after giving it the
/// recipe's HUD key, and returns `pattern`. The prepared root already owns every component and
/// its replication record. Weapon composition cannot read those pending asset owners.
fn complete_vehicle(
    pattern: u32,
    (tag, entity): (TagHash, &[u8]),
    hud_key: Option<u32>,
    packages: &mut crate::asset_packages::AssetPackages,
    entity_assignments: &mut Vec<u8>,
) -> AuthoringResult<u32> {
    if let Some(key) = hud_key {
        crate::hud_icon::runtime::write_prepared_key(packages, entity, key)?;
    }
    retain_imported_runtime(packages, tag, entity)?;
    *entity_assignments =
        append_weapon_entity_assignment(std::mem::take(entity_assignments), pattern, tag.0)
            .map_err(invalid)?;
    Ok(pattern)
}

/// Appends the authored `entity` as a new tag that inherits the `template` entity's storage, and
/// points `pattern` at it. Returns `pattern`.
fn append_entity(
    pattern: u32,
    (template, entity): (TagHash, Vec<u8>),
    (allocator, tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    entity_assignments: &mut Vec<u8>,
) -> AuthoringResult<u32> {
    let authored_entity_tag = allocator.assigned_tag(
        tags.len(),
        "Authored runtime weapon entity",
        "runtime weapon entity",
    )?;
    tags.push(NewTagSpec {
        template_tag: template,
        payload: entity,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    *entity_assignments = append_weapon_entity_assignment(
        std::mem::take(entity_assignments),
        pattern,
        authored_entity_tag.0,
    )
    .map_err(invalid)?;
    Ok(pattern)
}

fn retain_imported_runtime(
    packages: &crate::asset_packages::AssetPackages,
    tag: TagHash,
    entity: &[u8],
) -> AuthoringResult<()> {
    let placed = packages
        .packages
        .iter()
        .find(|package| package.id == tag.pkg_id())
        .and_then(|package| package.tags.get(tag.entry_index() as usize))
        .ok_or_else(|| invalid("Imported vehicle runtime was not placed"))?;
    if placed.payload != entity {
        return Err(invalid(
            "Runtime edits changed an imported vehicle's prepared component graph. Reimport the vehicle with those changes.",
        ));
    }
    // Keep the identity owned by its private replication record. Copying this
    // payload to a second entity would leave that back-reference behind.
    Ok(())
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

fn resolved_hud_key(
    manager: &PackageManager,
    patterns: &[u8],
    assignments: &[u8],
    donor: &resolve::ResolvedWeapon,
) -> AuthoringResult<Option<u32>> {
    if donor.weapon.kind == ItemKind::Sparrow {
        return Ok(donor
            .weapon
            .overrides
            .hud_icon
            .as_ref()
            .map(|_| donor.weapon.identity.type_hash));
    }
    if !donor.weapon.kind.is_weapon() {
        return Ok(None);
    }
    let content_graft = donor.donors.runtime_components.iter().any(|component| {
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
            && donor.donors.pinned_appearance.is_none(),
        || {
            // A pinned model keeps the base's rig and row, yet the HUD shows the model it wears.
            let Some(appearance) = donor
                .donors
                .pinned_appearance
                .or(donor.gear_art_pattern_source)
            else {
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
        let mut failed = BTreeMap::new();
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
                #[cfg(feature = "d2-model-importer")]
                hud_statuses: &mut vec![],
            },
            &mut assignments,
            &mut tags,
            &mut failed,
            &mut progress,
        )
        .unwrap();
        assert!(failed.is_empty(), "{failed:?}");
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
