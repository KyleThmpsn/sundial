//! Consume a completed payload plan and emit its verified package artifacts.
use super::*;
mod art;
mod glow;
#[cfg(feature = "d2-model-importer")]
mod imported;
mod linking;
mod ornament;
mod packages;
mod reskin;
mod vehicle_art;

/// The position of an authored item's definition among the host package's new tags.
fn definition_ordinal(emission: &PackageEmission, item: u32) -> AuthoringResult<usize> {
    let (count, _, rows, _) =
        sundial::package_authoring::native_payload::native_array_at(&emission.item_table, 8)
            .map_err(invalid)?;
    let matches = (0..count)
        .map(|i| rows + i * 24)
        .filter(|&row| read_u32(&emission.item_table, row).ok() == Some(item))
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid("Authored item is missing or ambiguous"));
    };
    let tag = TagHash(read_u32(&emission.item_table, row + 16)?);
    let host = crate::package_profile::host();
    let ordinal = AppendedTagAllocator::records(host.first).ordinal(tag)?;
    if ordinal >= emission.host_new_tags.len() {
        return Err(invalid("Authored definition is outside authored tags"));
    }
    Ok(ordinal)
}

/// The private host tag holding an authored item's strings record.
#[cfg(feature = "d2-model-importer")]
fn strings_ordinal(emission: &PackageEmission, item: u32) -> AuthoringResult<usize> {
    let (count, _, rows, _) =
        sundial::package_authoring::native_payload::native_array_at(&emission.item_strings, 8)
            .map_err(invalid)?;
    let matches = (0..count)
        .map(|i| rows + i * 24)
        .filter(|&row| read_u32(&emission.item_strings, row).ok() == Some(item))
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid("Authored item strings are missing or ambiguous"));
    };
    let tag = TagHash(read_u32(&emission.item_strings, row + 16)?);
    let host = crate::package_profile::host();
    let ordinal = AppendedTagAllocator::records(host.first).ordinal(tag)?;
    if ordinal >= emission.host_new_tags.len() {
        return Err(invalid("Authored item strings must be a private record"));
    }
    Ok(ordinal)
}

pub(super) struct PackageEmission {
    pub(super) lore: Option<lore::Plan>,
    pub(super) hud_table: Option<ReplacementSpec>,
    pub(super) ability_banks: BTreeMap<u16, Vec<ReplacementSpec>>,
    /// Additional HUD tables, including private status names and subclass glyph colors.
    pub(super) hud_statuses: Vec<ReplacementSpec>,
    pub(super) table_tags: super::sources::TableTags,
    pub(super) has_custom_plugs: bool,
    pub(super) watermark_layer_tag: TagHash,
    pub(super) watermarked_icon_containers: Vec<TagHash>,
    pub(super) watermark_reference_overrides: Vec<crate::NewTagReferenceOverride>,
    pub(super) badge_icon_tag: TagHash,
    pub(super) asset_packages: crate::asset_packages::AssetPackages,
    pub(super) private_perk_runtime_append_start: usize,
    pub(super) private_perk_runtime_new_tags: Vec<NewTagSpec>,
    pub(super) entity_assignments: Vec<u8>,
    pub(super) finished_sandbox_perks: Vec<u8>,
    pub(super) sandbox_perk_indices: Vec<u8>,
    pub(super) localization: AuthoredLocalization,
    pub(super) item_table: Vec<u8>,
    pub(super) item_strings: Vec<u8>,
    pub(super) item_hash_index: Vec<u8>,
    pub(super) item_metadata: Vec<u8>,
    pub(super) item_metadata_index: Vec<u8>,
    pub(super) sandbox_patterns: Vec<u8>,
    pub(super) sandbox_pattern_index: Vec<u8>,
    pub(super) dense: Vec<u8>,
    pub(super) collectibles: Vec<u8>,
    pub(super) collectible_displays: Vec<u8>,
    pub(super) unlocks: Vec<u8>,
    pub(super) unlock_banks: Vec<u8>,
    pub(super) unlock_displays: Vec<u8>,
    /// The socket-entry-list and subclass display tables, when a subclass added a list.
    pub(super) subclass_tables: Option<crate::subclass::tables::SubclassTables>,
    /// The art-dye table, when a shader added custom dyes.
    pub(super) dye_table: Option<ReplacementSpec>,
    /// The stat group table, when a weapon has a stat group of its own.
    pub(super) stat_group_table: Option<ReplacementSpec>,
    /// The shared plug set table, when a private perk is offered everywhere.
    pub(super) plug_set_table: Option<ReplacementSpec>,
    pub(super) plans: Vec<NewWeaponPlan>,
    pub(super) any_sandbox_pattern: bool,
    pub(super) nodes: Vec<u8>,
    pub(super) node_strings: Vec<u8>,
    pub(super) objective_strings: Vec<u8>,
    pub(super) records: Vec<u8>,
    pub(super) record_strings: Vec<u8>,
    pub(super) objectives: Vec<u8>,
    pub(super) pools: Vec<u8>,
    /// Where each ordinary weapon's linked ornaments go in Collections, by the weapon's item hash.
    /// An Exotic's go beside the native ornament they copy.
    #[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
    pub(super) ornament_targets: BTreeMap<u32, super::placements::OrnamentTarget>,
    pub(super) item_icons: Vec<u8>,
    pub(super) runtime_dependencies: Option<Vec<u8>>,
    /// Records that load with a private relation, which the dependency index leaves out.
    pub(super) on_demand: BTreeSet<TagHash>,
    pub(super) host_new_tags: Vec<NewTagSpec>,
    /// The Crucible maps added as new activities.
    pub(super) maps: crate::map::Plan,
}

/// Wwise finds a medium by the media ID in its entry header, so every appended medium names
/// itself there, in the asset packages as in the host.
fn name_audio_media(asset_packages: &mut crate::asset_packages::AssetPackages) {
    for package in &mut asset_packages.packages {
        for reference in media_references(&package.tags) {
            if !package
                .references
                .iter()
                .any(|existing| existing.new_tag_ordinal == reference.new_tag_ordinal)
            {
                package.references.push(reference);
            }
        }
    }
}

fn media_references(
    tags: &[NewTagSpec],
) -> impl Iterator<Item = crate::NewTagReferenceOverride> + '_ {
    tags.iter()
        .enumerate()
        .filter(|(_, spec)| spec.storage == crate::NewTagStorageMode::AudioMedia)
        .map(|(ordinal, _)| crate::NewTagReferenceOverride {
            new_tag_ordinal: ordinal,
            reference: crate::NewTagReference::Appended(ordinal),
        })
}

/// Split a dense logical record vector, translating entry references to their actual packages.
fn record_packages(
    tags: Vec<NewTagSpec>,
    references: Vec<crate::NewTagReferenceOverride>,
) -> AuthoringResult<Vec<crate::asset_packages::AssetPackage>> {
    let host = crate::package_profile::host();
    let allocator = AppendedTagAllocator::records(host.first);
    let count = tags.len();
    let (first, overflow) = allocator.split(tags)?;
    let mut packages = std::iter::once((host.id, first))
        .chain(overflow)
        .map(|(id, tags)| crate::asset_packages::AssetPackage {
            id,
            tags,
            references: Vec::new(),
        })
        .collect::<Vec<_>>();
    for reference in references {
        if reference.new_tag_ordinal >= count {
            return Err(invalid(
                "Record reference owner is outside authored records",
            ));
        }
        let owner =
            allocator.assigned_tag(reference.new_tag_ordinal, "Record reference", "owner")?;
        let target = match reference.reference {
            crate::NewTagReference::Appended(ordinal) => {
                if ordinal >= count {
                    return Err(invalid(
                        "Record reference target is outside authored records",
                    ));
                }
                crate::NewTagReference::Tag(allocator.assigned_tag(
                    ordinal,
                    "Record reference",
                    "target",
                )?)
            }
            other => other,
        };
        let package = packages
            .iter_mut()
            .find(|package| package.id == owner.pkg_id())
            .ok_or_else(|| invalid("Record reference package is missing"))?;
        package.references.push(crate::NewTagReferenceOverride {
            new_tag_ordinal: usize::from(owner.entry_index())
                - if package.id == host.id { host.first } else { 0 },
            reference: target,
        });
    }
    Ok(packages)
}

/// A replacement of the stock table `tag`.
fn table(tag: TagHash, payload: Vec<u8>) -> ReplacementSpec {
    ReplacementSpec { tag, payload }
}

/// Removes the tables `owned` picks from `tables` and returns them. Both lists keep their order.
fn take(
    tables: &mut Vec<ReplacementSpec>,
    owned: impl FnMut(&ReplacementSpec) -> bool,
) -> Vec<ReplacementSpec> {
    let (taken, rest) = std::mem::take(tables)
        .into_iter()
        .partition::<Vec<_>, _>(owned);
    *tables = rest;
    taken
}

/// Picks the tables of package `id`.
fn in_package(id: u16) -> impl Fn(&ReplacementSpec) -> bool {
    move |replacement| replacement.tag.pkg_id() == id
}

/// Emits the build's packages. The item art passes finish the payloads, and the emission splits
/// into the tables each overlay replaces and the entries packages append. After the dependency
/// check, the packages that append entries are planned, then each table overlay, and every
/// planned package is built in one pass.
pub(super) fn emit_packages(
    package_directory: &Path,
    manager: PackageManager,
    mut emission: PackageEmission,
    weapons: &[WeaponCloneSpec],
    branding: crate::branding::Branding,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<NewWeaponProjectBundle> {
    let art = apply_art(
        package_directory,
        &manager,
        &mut emission,
        weapons,
        branding,
        progress,
    )?;
    // With every entry placed, the main mod keeps the rows of each Collections list that the
    // importers put their entries first in.
    crate::collection::front::keep_front_slots(
        &manager,
        &mut emission.nodes,
        emission.table_tags.presentation_node_table_tag,
    )?;
    let (tables, entries, project) = split(emission, art)?;
    progress.payloads(entries.package_count(&tables));
    let loading = check_dependencies(&manager, &entries, &tables, progress)?;
    let mut packages = packages::Packages::new(package_directory);
    let (planned, map_tables, runtime_dependencies) =
        entries.plan(&mut packages, manager, &loading)?;
    let order = tables
        .route(map_tables, runtime_dependencies)?
        .plan(&mut packages, planned)?;
    let artifacts = packages.build(order, progress)?;
    Ok(project.bundle(artifacts))
}

/// Runs the item art passes, which finish `emission`'s payloads, and returns the further tables
/// they replace. These passes run before the package count is known, so their labels don't
/// advance it.
fn apply_art(
    package_directory: &Path,
    manager: &PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
    branding: crate::branding::Branding,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let mut timings = Vec::new();
    let mut timed = |label: &'static str, started: std::time::Instant| {
        timings.push(format!("{label} {:.3}s", started.elapsed().as_secs_f64()));
    };
    #[cfg(feature = "d2-model-importer")]
    let mut replacements = {
        // Each pass names every recipe's model, or every graph it links, as it goes.
        let started = std::time::Instant::now();
        imported::validate_graphs(weapons, progress, "Checking")?;
        timed("model check", started);
        let started = std::time::Instant::now();
        let (replacements, stages) = imported::apply(
            package_directory,
            manager,
            emission,
            weapons,
            branding,
            progress,
        )?;
        timed("model linking", started);
        progress.diagnostic(format!("Model linking timings: {stages}."));
        // The files must still be the ones selected now that linking has read them.
        let started = std::time::Instant::now();
        imported::validate_graphs(weapons, progress, "Rechecking")?;
        timed("model recheck", started);
        replacements
    };
    #[cfg(not(feature = "d2-model-importer"))]
    let mut replacements: Vec<ReplacementSpec> = Vec::new();
    // Only imported models draw watermarks during emission.
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = branding;
    progress.start("Applying Ornaments");
    let started = std::time::Instant::now();
    ornament::apply(manager, emission, weapons)?;
    timed("ornaments", started);
    progress.start("Applying Reskins");
    let started = std::time::Instant::now();
    reskin::apply(
        package_directory,
        manager,
        emission,
        weapons,
        &mut replacements,
    )?;
    timed("reskins", started);
    progress.start("Applying Vehicle Art");
    let started = std::time::Instant::now();
    vehicle_art::apply(
        package_directory,
        manager,
        emission,
        weapons,
        &mut replacements,
    )?;
    timed("vehicle art", started);
    progress.diagnostic(format!("Item art timings: {}.", timings.join(", ")));
    Ok(replacements)
}

/// What the project records about itself beside its packages.
struct Project {
    plans: Vec<NewWeaponPlan>,
    badge_icon_tag: TagHash,
    watermark_layer_tag: TagHash,
    watermarked_icon_containers: Vec<TagHash>,
}

impl Project {
    fn bundle(self, artifacts: Vec<crate::ExtendedOverlayArtifact>) -> NewWeaponProjectBundle {
        let badge = crate::badge::Identity::current();
        NewWeaponProjectBundle {
            plan: NewWeaponProjectPlan {
                weapons: self.plans,
                sunrise: SunriseProjectMetadata {
                    badge_node_hashes: badge.node_hashes,
                    badge_name_hash: badge.name_hash,
                    badge_description_hash: badge.description_hash,
                    badge_icon_tag: self.badge_icon_tag,
                    watermark_layer_tag: self.watermark_layer_tag,
                    watermarked_icon_containers: self.watermarked_icon_containers,
                },
            },
            artifacts,
        }
    }
}

/// The entries the build appends, and the packages that take them.
struct Entries {
    /// The host's own tables, which its overlay replaces beside the records it appends.
    host_tables: Vec<ReplacementSpec>,
    /// The host's records, split between the host and standalone record packages.
    records: Vec<crate::asset_packages::AssetPackage>,
    assets: crate::asset_packages::AssetPackages,
    maps: crate::map::Plan,
    private_start: usize,
    /// The private copies the private perk-runtime package's table holds.
    private: Vec<NewTagSpec>,
    /// The private copies past that table, by the standalone package that takes them.
    private_overflow: Vec<(u16, Vec<NewTagSpec>)>,
    /// The build's investment loading index, when it changes the stock one.
    runtime_dependencies: Option<Vec<u8>>,
    /// Records that load with a private relation, which the index leaves out.
    on_demand: BTreeSet<TagHash>,
}

impl Entries {
    /// One dependency check, six required overlays, and all optional packages in this plan.
    fn package_count(&self, tables: &Tables) -> usize {
        let standalone_records = self
            .records
            .iter()
            .filter(|package| package.id != HOST_PACKAGE_ID)
            .count();
        7 + standalone_records
            + self.assets.packages.len()
            + self.maps.package_count()
            + self.private_overflow.len()
            + tables.shared.len()
            + tables.ui.len()
            + usize::from(!self.private.is_empty())
            + usize::from(
                self.runtime_dependencies.is_some()
                    || !self.maps.is_empty()
                    || standalone_records != 0,
            )
    }

    /// Plans the host overlay and every package that appends entries: the standalone records,
    /// the asset packages, each map's packages and the private copies. Returns them with the
    /// maps' tables and the investment loading index, which then registers every planned package.
    fn plan(
        self,
        packages: &mut packages::Packages<'_>,
        manager: PackageManager,
        loading: &[u8],
    ) -> AuthoringResult<(Planned, Vec<ReplacementSpec>, Option<Vec<u8>>)> {
        let Entries {
            host_tables,
            records,
            mut assets,
            maps,
            private_start,
            private,
            private_overflow,
            mut runtime_dependencies,
            on_demand,
        } = self;
        let (records, host) = plan_records(
            packages,
            records,
            host_tables,
            (&mut runtime_dependencies, &on_demand),
            loading,
        )?;
        name_audio_media(&mut assets);
        let assets_end = assets.native_count();
        let mut planned_assets = plan_assets(packages, assets)?;
        // Each map's packages, named after the map, take the next ids in the window after the
        // asset packages.
        let assets_end = plan_maps(
            packages,
            &manager,
            &maps,
            assets_end,
            &mut planned_assets,
            &mut runtime_dependencies,
            loading,
        )?;
        // Map registration reads template metadata during planning. Release all source handles
        // before any of the planned package jobs starts writing.
        drop(manager);
        // The activity tables Sunrise can't merge. An importer whose maps join the main mod's
        // builds its own on the main mod's finished tables.
        for activity in maps.activity_tables() {
            crate::mod_family::build::produced(
                crate::mod_family::build::Shared::Activities,
                [activity.tag.0],
            );
            crate::mod_family::build::finish_shared(activity.tag.0, &activity.payload);
        }
        let overflow = plan_private_runtime_overflow(packages, assets_end, private_overflow)?;
        let private = plan_private(packages, private_start, private)?;
        let planned = Planned {
            assets: planned_assets,
            records,
            host,
            overflow,
            private,
        };
        Ok((planned, maps.replacements, runtime_dependencies))
    }
}

/// The stock tables the build replaces, listed under the overlay of their package. The optional
/// tables wait for `route` to find theirs.
struct Tables {
    /// The sandbox-perk runtime map, which leads the runtime entity overlay.
    entity_assignments: ReplacementSpec,
    /// The art passes' tables in the runtime entity package.
    art_runtime: Vec<ReplacementSpec>,
    investment: Overlay,
    strings: Overlay,
    localized: Overlay,
    unlock: Overlay,
    subclass: Option<crate::subclass::tables::SubclassTables>,
    /// The dye, stat group and plug set tables, when the build changes them.
    optional: Vec<ReplacementSpec>,
    lore: Option<lore::Plan>,
    /// The art passes' tables in the item string package.
    art_strings: Vec<ReplacementSpec>,
    /// The UI tables by package.
    ui: BTreeMap<u16, Vec<ReplacementSpec>>,
    /// The shared resources by package.
    shared: BTreeMap<u16, Vec<ReplacementSpec>>,
}

impl Tables {
    /// Routes each subclass, dye, stat group, plug set and map table to the overlay that already
    /// replaces tables in its package, and the lore and art passes' string tables to theirs.
    /// `runtime_dependencies` is the build's investment loading index, when it changes the stock
    /// one.
    fn route(
        self,
        map_tables: Vec<ReplacementSpec>,
        runtime_dependencies: Option<Vec<u8>>,
    ) -> AuthoringResult<Overlays> {
        let Tables {
            entity_assignments,
            art_runtime,
            mut investment,
            mut strings,
            localized,
            mut unlock,
            subclass,
            optional,
            lore,
            art_strings,
            ui,
            shared,
        } = self;
        let mut extras = subclass
            .map(crate::subclass::tables::SubclassTables::replacements)
            .transpose()?
            .unwrap_or_default();
        extras.extend(optional);
        extras.extend(map_tables);
        investment
            .tables
            .extend(take(&mut extras, in_package(investment.id)));
        strings
            .tables
            .extend(take(&mut extras, in_package(strings.id)));
        if let Some(lore) = lore {
            if lore.strings.tag.pkg_id() != strings.id || lore.definitions.tag.pkg_id() != unlock.id
            {
                return Err(invalid(
                    "Lore tables moved outside their audited package owners",
                ));
            }
            strings.tables.push(lore.strings);
            unlock.tables.push(lore.definitions);
        }
        strings.tables.extend(art_strings);
        unlock
            .tables
            .extend(take(&mut extras, in_package(unlock.id)));
        if let Some(stray) = extras.first() {
            return Err(invalid(format!(
                "Table {} is in package {:04x}, which no project overlay replaces",
                stray.tag,
                stray.tag.pkg_id()
            )));
        }
        let overlays = Overlays {
            entities: Overlay::of(
                entity_assignments.tag,
                std::iter::once(entity_assignments)
                    .chain(art_runtime)
                    .collect(),
            ),
            dependencies: runtime_dependencies.map(|payload| {
                Overlay::of(
                    RUNTIME_DEPENDENCY_COMPANION,
                    vec![table(RUNTIME_DEPENDENCY_COMPANION, payload)],
                )
            }),
            investment,
            strings,
            localized,
            unlock,
            shared,
            ui,
        };
        overlays.check_ui()?;
        Ok(overlays)
    }
}

/// The stock tables of package `id` the build replaces, which one overlay of it holds.
struct Overlay {
    id: u16,
    tables: Vec<ReplacementSpec>,
}

impl Overlay {
    /// The overlay of `owner`'s package, holding `tables`.
    fn of(owner: TagHash, tables: Vec<ReplacementSpec>) -> Self {
        Self {
            id: owner.pkg_id(),
            tables,
        }
    }

    fn plan(self, packages: &mut packages::Packages<'_>) -> AuthoringResult<packages::Ticket> {
        packages.tables(self.id, self.tables)
    }
}

/// Every table overlay of the build.
struct Overlays {
    entities: Overlay,
    dependencies: Option<Overlay>,
    investment: Overlay,
    strings: Overlay,
    localized: Overlay,
    unlock: Overlay,
    /// One overlay per package of shared resources, including private ability rows and
    /// imported character animation branches.
    shared: BTreeMap<u16, Vec<ReplacementSpec>>,
    ui: BTreeMap<u16, Vec<ReplacementSpec>>,
}

impl Overlays {
    /// A UI package that another project overlay also replaces would take two overlays.
    fn check_ui(&self) -> AuthoringResult<()> {
        let project = [
            HOST_PACKAGE_ID,
            PRIVATE_PERK_RUNTIME_PACKAGE_ID,
            RUNTIME_DEPENDENCY_COMPANION.pkg_id(),
            self.investment.id,
            self.strings.id,
            self.localized.id,
            self.unlock.id,
            self.entities.id,
        ];
        if let Some(package) = self
            .ui
            .keys()
            .find(|package| project.contains(package) || self.shared.contains_key(package))
        {
            return Err(invalid(format!(
                "UI tables in package {package:04x} share it with another authored overlay"
            )));
        }
        Ok(())
    }

    /// Plans each overlay after the packages that append entries, and returns every planned
    /// package in the order the build emits them.
    fn plan(
        self,
        packages: &mut packages::Packages<'_>,
        planned: Planned,
    ) -> AuthoringResult<Vec<packages::Ticket>> {
        let entities = self.entities.plan(packages)?;
        let dependencies = self
            .dependencies
            .map(|overlay| overlay.plan(packages))
            .transpose()?;
        let investment = self.investment.plan(packages)?;
        let strings = self.strings.plan(packages)?;
        let localized = self.localized.plan(packages)?;
        let unlock = self.unlock.plan(packages)?;
        let shared = plan_each(packages, self.shared)?;
        let ui = plan_each(packages, self.ui)?;
        let mut order = planned.assets;
        order.extend(planned.records);
        order.extend(planned.overflow);
        order.extend(ui);
        order.extend(shared);
        order.extend(planned.private);
        order.extend(dependencies);
        order.extend([
            entities,
            planned.host,
            investment,
            strings,
            localized,
            unlock,
        ]);
        Ok(order)
    }
}

/// One overlay for each package's `overlays` tables.
fn plan_each(
    packages: &mut packages::Packages<'_>,
    overlays: BTreeMap<u16, Vec<ReplacementSpec>>,
) -> AuthoringResult<Vec<packages::Ticket>> {
    overlays
        .into_iter()
        .map(|(id, tables)| Overlay { id, tables }.plan(packages))
        .collect()
}

/// The packages that append entries, planned before the table overlays.
struct Planned {
    /// The asset packages, then each map's.
    assets: Vec<packages::Ticket>,
    records: Vec<packages::Ticket>,
    host: packages::Ticket,
    overflow: Vec<packages::Ticket>,
    private: Option<packages::Ticket>,
}

/// Sorts the emission and the art passes' tables `art` into the tables the build replaces, the
/// entries it appends and what the project records about itself. Each table is listed under the
/// overlay of its package. Private copies past the private perk-runtime package's table go to
/// standalone packages of their own, from the top of the build's window down.
fn split(
    emission: PackageEmission,
    mut art: Vec<ReplacementSpec>,
) -> AuthoringResult<(Tables, Entries, Project)> {
    let PackageEmission {
        lore,
        hud_table,
        ability_banks,
        hud_statuses,
        table_tags: tags,
        has_custom_plugs,
        watermark_layer_tag,
        watermarked_icon_containers,
        watermark_reference_overrides,
        badge_icon_tag,
        asset_packages,
        private_perk_runtime_append_start,
        private_perk_runtime_new_tags,
        entity_assignments,
        finished_sandbox_perks,
        sandbox_perk_indices,
        localization,
        item_table,
        item_strings,
        item_hash_index,
        item_metadata,
        item_metadata_index,
        sandbox_patterns,
        sandbox_pattern_index,
        dense,
        collectibles,
        collectible_displays,
        unlocks,
        unlock_banks,
        unlock_displays,
        subclass_tables,
        dye_table,
        stat_group_table,
        plug_set_table,
        plans,
        any_sandbox_pattern,
        nodes,
        node_strings,
        objective_strings,
        records,
        record_strings,
        objectives,
        pools,
        ornament_targets: _,
        item_icons,
        runtime_dependencies,
        on_demand,
        host_new_tags,
        maps,
    } = emission;
    // The crosshair table joins the other UI tables of its package. The art passes' tables in the
    // runtime entity and item string packages join those overlays, and the rest are shared
    // resources.
    let crosshair = take(&mut art, |replacement| {
        replacement.tag == crate::weapon::crosshair::TABLE
    });
    let art_runtime = take(&mut art, in_package(tags.entity_assignment_tag.pkg_id()));
    let art_strings = take(&mut art, in_package(tags.item_string_table_tag.pkg_id()));
    let (private, private_overflow) =
        AppendedTagAllocator::private_runtime(private_perk_runtime_append_start)
            .split(private_perk_runtime_new_tags)?;
    let mut shared = ability_banks;
    let ui = group_ui(
        hud_statuses.into_iter().chain(hud_table).chain(crosshair),
        art,
        &mut shared,
    );
    let mut host_references = watermark_reference_overrides;
    host_references.extend(media_references(&host_new_tags));
    let host_records = record_packages(host_new_tags, host_references)?;
    // An imported model's dyes extend the dye table after the custom ones, so an art pass's
    // replacement of it already holds them.
    let optional = dye_table
        .filter(|dyes| {
            !art_strings
                .iter()
                .any(|replacement| replacement.tag == dyes.tag)
        })
        .into_iter()
        .chain(stat_group_table)
        .chain(plug_set_table)
        .collect();
    let mut strings = vec![
        table(tags.item_string_table_tag, item_strings),
        table(tags.item_metadata_table_tag, item_metadata),
        table(tags.presentation_node_string_table_tag, node_strings),
        table(tags.objective_string_table_tag, objective_strings),
        table(tags.record_string_table_tag, record_strings),
    ];
    let mut unlock = vec![
        table(tags.presentation_node_table_tag, nodes),
        table(tags.objective_table_tag, objectives),
        table(tags.record_table_tag, records),
        table(tags.shared_expression_pool_table_tag, pools),
        table(tags.unlock_table_tag, unlocks),
        table(tags.unlock_flag_bank_table_tag, unlock_banks),
        table(tags.item_metadata_index_table_tag, item_metadata_index),
    ];
    if any_sandbox_pattern {
        strings.push(table(tags.sandbox_pattern_table_tag, sandbox_patterns));
        unlock.push(table(
            tags.sandbox_pattern_index_table_tag,
            sandbox_pattern_index,
        ));
    }
    if has_custom_plugs {
        strings.push(table(
            tags.finished_sandbox_perk_table_tag,
            finished_sandbox_perks,
        ));
        unlock.push(table(tags.item_hash_index_table_tag, item_hash_index));
        unlock.push(table(
            tags.sandbox_perk_index_table_tag,
            sandbox_perk_indices,
        ));
    }
    // A build with a bank of its own changes only the string bank index, which names it.
    let bank = if localization.own_bank {
        table(tags.localized_index_tag, localization.index)
    } else {
        table(localization.donor_header_tag, localization.merged_header)
    };
    let localized = std::iter::once(bank)
        .chain(
            localization
                .locale_data
                .into_iter()
                .map(|locale| table(locale.donor_tag, locale.payload)),
        )
        .collect();
    let tables = Tables {
        entity_assignments: table(tags.entity_assignment_tag, entity_assignments),
        art_runtime,
        investment: Overlay::of(
            tags.item_table_tag,
            vec![
                table(tags.item_table_tag, item_table),
                table(tags.collectible_table_tag, collectibles),
                table(tags.collectible_display_table_tag, collectible_displays),
            ],
        ),
        strings: Overlay::of(tags.item_string_table_tag, strings),
        localized: Overlay::of(tags.localized_index_tag, localized),
        unlock: Overlay::of(tags.unlock_table_tag, unlock),
        subclass: subclass_tables,
        optional,
        lore,
        art_strings,
        ui,
        shared,
    };
    let entries = Entries {
        host_tables: vec![
            table(tags.unlock_display_tag, unlock_displays),
            table(tags.item_dense_presentation_table_tag, dense),
            table(tags.item_icon_table_tag, item_icons),
        ],
        records: host_records,
        assets: asset_packages,
        maps,
        private_start: private_perk_runtime_append_start,
        private,
        private_overflow,
        runtime_dependencies,
        on_demand,
    };
    let project = Project {
        plans,
        badge_icon_tag,
        watermark_layer_tag,
        watermarked_icon_containers,
    };
    Ok((tables, entries, project))
}

/// Groups the UI tables by package, so each package takes one overlay: HUD statuses and their
/// names, subclass glyph colors and tree decisions, the ammo HUD table and the crosshair table.
/// The subclass tree decisions share the crosshair table's package and the ammo HUD table's.
/// Shared character resources belong to their native packages, and each joins the UI overlay of
/// its package when there is one, or else `shared`.
fn group_ui(
    ui_tables: impl IntoIterator<Item = ReplacementSpec>,
    resources: Vec<ReplacementSpec>,
    shared: &mut BTreeMap<u16, Vec<ReplacementSpec>>,
) -> BTreeMap<u16, Vec<ReplacementSpec>> {
    let mut ui = BTreeMap::<u16, Vec<ReplacementSpec>>::new();
    for replacement in ui_tables {
        ui.entry(replacement.tag.pkg_id())
            .or_default()
            .push(replacement);
    }
    for replacement in resources {
        let id = replacement.tag.pkg_id();
        ui.get_mut(&id)
            .unwrap_or_else(|| shared.entry(id).or_default())
            .push(replacement);
    }
    ui
}

/// Checks the asset packages and their loading, and the completed sandbox-perk runtime map, and
/// returns the investment loading index the checks read: the build's own, or else the stock one.
fn check_dependencies(
    manager: &PackageManager,
    entries: &Entries,
    tables: &Tables,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<Vec<u8>> {
    progress.start("Checking Asset Dependencies");
    entries.assets.validate()?;
    let stock_loading;
    let loading = if let Some(payload) = entries.runtime_dependencies.as_deref() {
        payload
    } else {
        stock_loading = manager
            .read_tag(RUNTIME_DEPENDENCY_COMPANION)
            .map_err(|error| invalid(error.to_string()))?;
        &stock_loading
    };
    crate::shared_tag_dependency_index::scoped::validate_asset_loading(
        manager,
        entries
            .assets
            .packages
            .iter()
            .map(|package| (package.id, package.tags.as_slice())),
        loading,
        crate::LoadingOwner {
            owner: RUNTIME_DEPENDENCY_ROOT,
            companion: RUNTIME_DEPENDENCY_COMPANION,
        },
    )?;
    let loading = loading.to_vec();
    // Validate the completed map after every authoring pass, not only the stock source.
    validate_sandbox_perk_runtime_map(&tables.entity_assignments.payload).map_err(validation)?;
    progress.finish("Checking Asset Dependencies");
    Ok(loading)
}

/// Plans each standalone record package and the host overlay, which holds the host's own tables
/// and the records it keeps. Every standalone record is enrolled in `runtime_dependencies`, or
/// else in `loading`, including legacy overflow, apart from those `on_demand` names, which load
/// with their private relation. Loader builds leave the stock host's entry count intact. Legacy
/// builds retain only the fitting appended prefix.
fn plan_records(
    packages: &mut packages::Packages<'_>,
    records: Vec<crate::asset_packages::AssetPackage>,
    host_tables: Vec<ReplacementSpec>,
    (runtime_dependencies, on_demand): (&mut Option<Vec<u8>>, &BTreeSet<TagHash>),
    loading: &[u8],
) -> AuthoringResult<(Vec<packages::Ticket>, packages::Ticket)> {
    let family = if crate::package_profile::host().is_standalone() {
        crate::package_profile::RECORDS_FAMILY
    } else {
        crate::package_profile::LEGACY_RECORDS_FAMILY
    };
    let mut standalone = Vec::new();
    let mut host = None;
    let mut loading_records = Vec::new();
    for package in records {
        if package.id == HOST_PACKAGE_ID {
            host = Some(package);
            continue;
        }
        crate::package_profile::name_standalone_package(package.id, family).map_err(invalid)?;
        let allocator = AppendedTagAllocator::new(package.id, 0);
        for ordinal in 0..package.tags.len() {
            let tag = allocator.assigned_tag(ordinal, "Investment loading", "record")?;
            if !on_demand.contains(&tag) {
                loading_records.push(tag);
            }
        }
        standalone.push(packages.standalone(package)?);
    }
    if !loading_records.is_empty() {
        *runtime_dependencies = Some(enroll(
            runtime_dependencies.as_deref().unwrap_or(loading),
            &loading_records,
        )?);
    }
    let (host_tags, host_references) = host.map_or_else(Default::default, |package| {
        (package.tags, package.references)
    });
    let expected_host_count = HOST_EXPECTED_ENTRY_COUNT + host_tags.len();
    let host = packages.overlay(HOST_PACKAGE_ID, host_tables, host_tags, host_references)?;
    packages.host(host, expected_host_count);
    Ok((standalone, host))
}

fn plan_assets(
    packages: &mut packages::Packages<'_>,
    asset_packages: crate::asset_packages::AssetPackages,
) -> AuthoringResult<Vec<packages::Ticket>> {
    asset_packages
        .packages
        .into_iter()
        .map(|package| packages.standalone(package))
        .collect()
}

/// Plans each map's packages into `planned`, at the window's ids from position `assets_end`, and
/// returns how many positions the build then uses. The client registers a package once a loading
/// index names one of its entries, and a name lookup alone does not. The investment loading root,
/// whose index starts as `runtime_dependencies` or else as `loading`, names the entries
/// `Plan::registration` chooses, so both packages are registered before an activity looks up its
/// root.
#[allow(clippy::too_many_arguments)]
fn plan_maps(
    packages: &mut packages::Packages<'_>,
    manager: &PackageManager,
    maps: &crate::map::Plan,
    mut assets_end: usize,
    planned: &mut Vec<packages::Ticket>,
    runtime_dependencies: &mut Option<Vec<u8>>,
    loading: &[u8],
) -> AuthoringResult<usize> {
    const CAPACITY: usize = crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT;
    for index in 0..maps.len() {
        for part in maps.parts(index) {
            // Every package of a world takes the world's family, as a stock destination's
            // packages do, and only its id tells them apart.
            let family = maps.family(index, part);
            let index_payload = runtime_dependencies.as_deref().unwrap_or(loading);
            #[cfg(feature = "d2-model-importer")]
            if crate::package_profile::host().is_standalone() {
                let native = window_id(
                    assets_end,
                    "The build's maps exceed the free authored package IDs",
                )?;
                if let Some(split) = maps.split_world(index, part, native, manager)? {
                    let registration = maps.registration_at(
                        &split.addresses,
                        &split.tags,
                        &split.references,
                        &registered(index_payload)?,
                        manager,
                    )?;
                    let resources = split.resource_ids.iter().enumerate().map(|(chunk, &id)| {
                        let start = split.native_entries + chunk * CAPACITY;
                        (id, start, (start + CAPACITY).min(split.tags.len()))
                    });
                    for (id, start, end) in
                        std::iter::once((native, 0, split.native_entries)).chain(resources)
                    {
                        crate::package_profile::name_standalone_package(id, &family)
                            .map_err(validation)?;
                        let references = chunk_references(&split.references, start..end, |at| {
                            split.addresses.get(at).copied().ok_or_else(|| {
                                validation("Map entry reference exceeds its allocation")
                            })
                        })?;
                        let names = split
                            .names
                            .iter()
                            .filter(|name| TagHash(name.tag).pkg_id() == id)
                            .cloned()
                            .collect();
                        planned.push(packages.named(
                            id,
                            split.tags[start..end].to_vec(),
                            names,
                            references,
                        )?);
                    }
                    assets_end += 1;
                    *runtime_dependencies = Some(enroll(index_payload, &registration)?);
                    continue;
                }
            }
            let ids = (0..maps.part_count(index, part))
                .map(|chunk| {
                    window_id(
                        assets_end + chunk,
                        "The build's maps need more packages than the free authored ids hold",
                    )
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            let (tags, named, references) = maps.package(index, part, &ids, manager)?;
            for &id in &ids {
                crate::mod_family::build::native(id);
            }
            if tags.len().div_ceil(CAPACITY) != ids.len() {
                return Err(validation(
                    "Map package allocation does not match its entries",
                ));
            }
            let registration = maps.registration(
                &ids,
                &tags,
                &references,
                &registered(index_payload)?,
                manager,
            )?;
            planned.extend(plan_chunks(
                packages,
                &family,
                &ids,
                tags,
                &named,
                &references,
            )?);
            assets_end += ids.len();
            *runtime_dependencies = Some(enroll(index_payload, &registration)?);
        }
    }
    Ok(assets_end)
}

/// Plans a map part's `tags` into packages `ids`, a full package of entries each, all named after
/// `family`. Each package keeps the `references` its entries own and the `named` tags of its id.
fn plan_chunks(
    packages: &mut packages::Packages<'_>,
    family: &str,
    ids: &[u16],
    tags: Vec<NewTagSpec>,
    named: &[crate::format::NamedTag],
    references: &[crate::NewTagReferenceOverride],
) -> AuthoringResult<Vec<packages::Ticket>> {
    const CAPACITY: usize = crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT;
    let mut tags = tags.into_iter();
    let mut planned = Vec::new();
    for (chunk, &id) in ids.iter().enumerate() {
        crate::package_profile::name_standalone_package(id, family).map_err(validation)?;
        let start = chunk * CAPACITY;
        let part_tags = tags.by_ref().take(CAPACITY).collect::<Vec<_>>();
        let end = start + part_tags.len();
        let part_references = chunk_references(references, start..end, |target| {
            let package = ids
                .get(target / CAPACITY)
                .ok_or_else(|| validation("Map entry reference exceeds its allocated packages"))?;
            Ok(TagHash::new(*package, (target % CAPACITY) as u16))
        })?;
        // The first package keeps aliases to stock donor tags as well as the world names.
        let part_named = named
            .iter()
            .filter(|name| {
                let owner = TagHash(name.tag).pkg_id();
                owner == id || (chunk == 0 && !ids.contains(&owner))
            })
            .cloned()
            .collect();
        planned.push(packages.named(id, part_tags, part_named, part_references)?);
    }
    Ok(planned)
}

/// The package id at `position` in the build's window, or a failure naming `shortage`.
fn window_id(position: usize, shortage: &str) -> AuthoringResult<u16> {
    crate::package_profile::window()
        .nth(position)
        .ok_or_else(|| validation(shortage))
}

/// The packages the investment loading index `index` registers.
fn registered(index: &[u8]) -> AuthoringResult<BTreeSet<u16>> {
    Ok(crate::shared_tag_dependency_index::dependency_entries(
        index,
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
    )?
    .into_iter()
    .map(|(package, _)| package)
    .collect())
}

/// The investment loading index `index` with `entries` enrolled in its root.
fn enroll(index: &[u8], entries: &[TagHash]) -> AuthoringResult<Vec<u8>> {
    crate::shared_tag_dependency_index::enroll_dependencies(
        index,
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
        entries,
    )
}

/// The `references` owned by entries `range`, numbered from the range's start, with each appended
/// target resolved to its tag by `target`.
fn chunk_references(
    references: &[crate::NewTagReferenceOverride],
    range: std::ops::Range<usize>,
    target: impl Fn(usize) -> AuthoringResult<TagHash>,
) -> AuthoringResult<Vec<crate::NewTagReferenceOverride>> {
    references
        .iter()
        .filter(|reference| range.contains(&reference.new_tag_ordinal))
        .map(|reference| {
            Ok(crate::NewTagReferenceOverride {
                new_tag_ordinal: reference.new_tag_ordinal - range.start,
                reference: match reference.reference {
                    crate::NewTagReference::Appended(at) => {
                        crate::NewTagReference::Tag(target(at)?)
                    }
                    other => other,
                },
            })
        })
        .collect()
}

/// Plans the private perk-runtime overlay, when the build has private copies for its table.
fn plan_private(
    packages: &mut packages::Packages<'_>,
    start: usize,
    tags: Vec<NewTagSpec>,
) -> AuthoringResult<Option<packages::Ticket>> {
    if tags.is_empty() {
        return Ok(None);
    }
    let count = tags.len();
    let ticket = packages.overlay(
        PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        Vec::new(),
        tags,
        Vec::new(),
    )?;
    packages.private(ticket, start, count);
    Ok(Some(ticket))
}

/// Reserve each private overflow output after checking it takes no id of the asset and map
/// packages, the window's first `assets_end`.
fn plan_private_runtime_overflow(
    packages: &mut packages::Packages<'_>,
    assets_end: usize,
    overflow: Vec<(u16, Vec<crate::NewTagSpec>)>,
) -> AuthoringResult<Vec<packages::Ticket>> {
    let window = crate::package_profile::window();
    let mut planned = Vec::new();
    for (id, tags) in overflow {
        if !(window.base..=window.top).contains(&id)
            || (0..assets_end).any(|position| window.nth(position) == Some(id))
        {
            return Err(validation(
                "The build's private copies and assets need more packages than the free authored ids hold",
            ));
        }
        planned.push(packages.standalone(crate::asset_packages::AssetPackage {
            id,
            tags,
            references: Vec::new(),
        })?);
    }
    Ok(planned)
}

fn validate_private_runtime(
    artifact: Option<&crate::ExtendedOverlayArtifact>,
    append_start: usize,
    tag_count: usize,
) -> AuthoringResult<()> {
    let expected_private_perk_runtime_count = append_start
        .checked_add(tag_count)
        .ok_or_else(|| invalid("Private perk-runtime entry count overflowed"))?;
    if let Some(private_perk_runtime) = artifact
        && (private_perk_runtime.plan.original_entry_count
            != PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT
            || private_perk_runtime.plan.append_start_entry_count != append_start
            || private_perk_runtime.plan.reserved_entry_count
                != append_start - PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT
            || private_perk_runtime.plan.final_entry_count != expected_private_perk_runtime_count
            || private_perk_runtime.plan.appended_tags.len() != tag_count)
    {
        return Err(validation(
            "Private perk-runtime package did not preserve its stock entry table and authored tail",
        ));
    }
    Ok(())
}
