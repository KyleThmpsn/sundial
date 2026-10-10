//! Author runtime payloads without interleaving host and private-perk tag allocation.
use super::*;

pub(in crate::item::build) struct Payloads {
    pub entity_assignments: Vec<u8>,
    pub finished_sandbox_perks: Vec<u8>,
    pub sandbox_perk_indices: Vec<u8>,
    pub pattern_global_ids: Vec<Option<u32>>,
    pub weapon_tags: Vec<NewTagSpec>,
    /// The badge artwork followed by each weapon's imported runtime assets. Linked graphs
    /// reserve their groups after these during emission.
    pub asset_packages: crate::asset_packages::AssetPackages,
    pub private_perk_tags: Vec<NewTagSpec>,
    pub private_perk_append_start: usize,
    /// Each authored subclass entry's custom perks as finished sandbox perks, by item ordinal
    /// and entry.
    pub subclass_entries: Vec<Vec<crate::subclass::authoring::CompiledEntry>>,
    /// Behavior graphs grafted into component owners, whose prerequisites still need enrolling.
    pub grafted_graphs: Vec<u32>,
    pub impacts: Vec<crate::ability::banks::MeleeImpact>,
    weapon_allocator: AppendedTagAllocator,
    /// Every imported runtime asset placed in the asset packages, for the dependency index.
    runtime_asset_tags: Vec<TagHash>,
    /// The badge artwork's entries in the primary asset package, which the HUD icons end.
    badge_tag_count: usize,
    private_perk_allocator: AppendedTagAllocator,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::item::build) fn author(
    package_directory: &Path,
    sources: &mut sources::ProjectSources,
    resolved: &[resolve::ResolvedWeapon],
    (custom_plugs, mod_plugs): (&[ResolvedCustomPlug], &[ResolvedCustomPlug]),
    entry_plans: &[Vec<crate::subclass::compile::EntryPlan<ResolvedPrivateSandboxPerk>>],
    templates: &PerkTemplates,
    assets: &mut assets::Plan,
    progress: &mut Progress<'_>,
) -> AuthoringResult<(
    Payloads,
    custom_plugs::CustomPlugPayloads,
    custom_plugs::CustomPlugPayloads,
)> {
    let weapon_runtime_tag_allocator =
        AppendedTagAllocator::new(HOST_PACKAGE_ID, assets.weapon_runtime_start);
    let mut weapon_runtime_new_tags = Vec::new();
    // The badge artwork opens the asset packages. Each weapon's imported runtime assets then
    // reserve a group of their own, so they roll over into further packages as one fills
    // rather than filling the host package, whose id is fixed.
    let badge_tag_count = assets.badge.new_tags.len();
    let mut asset_packages = crate::asset_packages::AssetPackages::primary(
        std::mem::take(&mut assets.badge.new_tags),
        std::mem::take(&mut assets.badge.reference_overrides),
    )?;
    let mut runtime_asset_tags = Vec::new();
    let mut impacts = Vec::new();
    #[cfg(feature = "d2-model-importer")]
    let mut particle_symbols = BTreeMap::new();
    #[cfg(not(feature = "d2-model-importer"))]
    let particle_symbols = BTreeMap::new();
    let private_perk_runtime_append_start =
        extended_overlay_append_start(package_directory, PRIVATE_PERK_RUNTIME_PACKAGE_ID)?;
    if private_perk_runtime_append_start < PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT {
        return Err(validation(format!(
            "Private perk-runtime package historical high-water mark \
             {private_perk_runtime_append_start} precedes its latest stock count \
             {PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT}"
        )));
    }
    // A build's private copies can outgrow the package's table, so they continue in standalone
    // packages of their own.
    let private_perk_runtime_tag_allocator =
        AppendedTagAllocator::private_runtime(private_perk_runtime_append_start);
    let mut private_perk_runtime_new_tags = Vec::new();
    let mut entity_assignments = sources.stock_entity_assignments.clone();
    let mut finished_sandbox_perks = std::mem::take(&mut sources.stock_finished_sandbox_perks);
    let mut sandbox_perk_indices = std::mem::take(&mut sources.stock_sandbox_perk_indices);
    let stock = EntitySources {
        sandbox_patterns: &sources.stock_sandbox_patterns,
        entity_assignments: &sources.stock_entity_assignments,
    };
    // One store for the build: weapons keep their runtime entities in it, and the subclasses
    // compiled below keep their abilities' copies.
    let runtime_cache = super::Cache::open(&sources.manager, &stock, progress);
    let authored_pattern_global_ids = super::author_entities(
        &sources.manager,
        stock,
        &runtime_cache,
        resolved,
        weapon_runtime_tag_allocator,
        super::RuntimeAssets {
            packages: &mut asset_packages,
            placed: &mut runtime_asset_tags,
            impacts: &mut impacts,
            #[cfg(feature = "d2-model-importer")]
            particle_symbols: &mut particle_symbols,
        },
        &mut entity_assignments,
        &mut weapon_runtime_new_tags,
        progress,
    )?;
    let mut catalog = custom_plugs::PerkCatalog {
        entity_assignments: &mut entity_assignments,
        finished_sandbox_perks: &mut finished_sandbox_perks,
        sandbox_perk_indices: &mut sandbox_perk_indices,
        private_perk_runtime_new_tags: &mut private_perk_runtime_new_tags,
        private_perk_runtime_tag_allocator,
        particle_symbols: &particle_symbols,
        weapon: None,
    };
    // Each private perk, and each part of an authored subclass entry, reports as it starts.
    const PRIVATE_PERKS: &str = "Compiling Private Perks";
    progress.start(PRIVATE_PERKS);
    let custom_payloads = custom_plugs::author_payloads(
        &sources.manager,
        custom_plugs,
        resolved,
        &templates.definition,
        &templates.strings,
        (&mut catalog, &mut *progress),
    )
    .map_err(|error| error.context(PRIVATE_PERKS))?;
    // A mod's perk compiles the same way, into the mod's own item place.
    let mod_payloads = custom_plugs::author_payloads(
        &sources.manager,
        mod_plugs,
        resolved,
        &templates.definition,
        &templates.strings,
        (&mut catalog, &mut *progress),
    )
    .map_err(|error| error.context(PRIVATE_PERKS))?;
    progress.finish(PRIVATE_PERKS);
    const SUBCLASSES: &str = "Compiling Subclasses";
    progress.start(SUBCLASSES);
    let subclass_entries = resolved
        .iter()
        .zip(entry_plans)
        .map(|(donor, plans)| {
            let mut compiler = custom_plugs::RecordCompiler {
                manager: &sources.manager,
                catalog: &mut catalog,
                assets: (&mut asset_packages, &mut runtime_asset_tags),
                cache: &runtime_cache,
                item: &donor.weapon.text.name,
                progress: &mut *progress,
            };
            crate::subclass::compile::compile(
                donor.subclass_list.as_ref(),
                plans,
                &mut compiler,
                &mut sources.subclass_tables,
            )
            .map_err(|error| donor.weapon.in_recipe(error))
        })
        .collect::<AuthoringResult<Vec<_>>>()
        .map_err(|error| error.context(SUBCLASSES))?;
    progress.finish(SUBCLASSES);
    let mut grafted_graphs = resolved
        .iter()
        .flat_map(|weapon| {
            crate::weapon::behavior::requested_graphs(&weapon.weapon.overrides.additional_behaviors)
        })
        .collect::<Vec<_>>();
    for donor in resolved {
        if let Some(graph) = crate::vehicle::authoring::projectile_dependency(
            &sources.manager,
            donor.weapon.overrides.sparrow.as_ref(),
        )? {
            grafted_graphs.push(graph);
        }
    }
    grafted_graphs.sort_unstable();
    grafted_graphs.dedup();
    Ok((
        Payloads {
            impacts,
            entity_assignments,
            finished_sandbox_perks,
            sandbox_perk_indices,
            pattern_global_ids: authored_pattern_global_ids,
            weapon_tags: weapon_runtime_new_tags,
            asset_packages,
            private_perk_tags: private_perk_runtime_new_tags,
            private_perk_append_start: private_perk_runtime_append_start,
            subclass_entries,
            weapon_allocator: weapon_runtime_tag_allocator,
            runtime_asset_tags,
            badge_tag_count,
            private_perk_allocator: private_perk_runtime_tag_allocator,
            grafted_graphs,
        },
        custom_payloads,
        mod_payloads,
    ))
}

impl Payloads {
    pub(in crate::item::build) fn dependencies(
        &self,
        manager: &PackageManager,
        assets: &assets::Plan,
        entity_assignment_tag: TagHash,
    ) -> AuthoringResult<Option<Vec<u8>>> {
        if self.private_perk_tags.is_empty()
            && self.weapon_tags.is_empty()
            && self.runtime_asset_tags.is_empty()
            && self.grafted_graphs.is_empty()
            && assets.hud_asset_start == self.badge_tag_count
        {
            return Ok(None);
        }
        let root_entry = manager
            .get_entry(RUNTIME_DEPENDENCY_ROOT)
            .ok_or_else(|| invalid("Investment runtime dependency root is missing"))?;
        let companion_entry = manager
            .get_entry(RUNTIME_DEPENDENCY_COMPANION)
            .ok_or_else(|| invalid("Investment runtime dependency companion is missing"))?;
        let root_payload = read_tag(
            manager,
            RUNTIME_DEPENDENCY_ROOT,
            "investment runtime dependency root",
        )?;
        if root_entry.reference != 0x8080_56A6
            || root_entry.file_type != 16
            || companion_entry.reference != 0x8080_9EF9
            || companion_entry.file_type != 8
            || read_u32(&root_payload, 0x10)? != entity_assignment_tag.0
        {
            return Err(invalid(
                "Investment runtime loading root no longer matches its audited source",
            ));
        }
        let source = read_tag(
            manager,
            RUNTIME_DEPENDENCY_COMPANION,
            "investment runtime dependency index",
        )?;
        let mut additions = assets.perk_icon_dependencies.clone();
        for index in assets.hud_asset_start..self.badge_tag_count {
            additions.push(
                AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0).assigned_tag(
                    index,
                    "HUD dependency",
                    "HUD icon",
                )?,
            );
        }
        for index in 0..self.private_perk_tags.len() {
            additions.push(self.private_perk_allocator.assigned_tag(
                index,
                "Runtime dependency",
                "private perk",
            )?);
        }
        for index in 0..self.weapon_tags.len() {
            additions.push(self.weapon_allocator.assigned_tag(
                index,
                "Runtime dependency",
                "weapon runtime",
            )?);
        }
        additions.extend(self.runtime_asset_tags.iter().copied());
        additions.extend(super::dependencies::native_prerequisites(
            manager,
            [
                (
                    self.private_perk_allocator,
                    self.private_perk_tags.as_slice(),
                ),
                (self.weapon_allocator, self.weapon_tags.as_slice()),
            ]
            .into_iter()
            .chain(self.asset_packages.packages.iter().map(|package| {
                (
                    AppendedTagAllocator::new(package.id, 0),
                    package.tags.as_slice(),
                )
            })),
            &self.grafted_graphs,
        )?);
        Ok(Some(
            crate::shared_tag_dependency_index::enroll_dependencies(
                &source,
                RUNTIME_DEPENDENCY_COMPANION,
                RUNTIME_DEPENDENCY_ROOT,
                &additions,
            )?,
        ))
    }
}
