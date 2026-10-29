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

#[allow(unused_mut)]
#[allow(clippy::cognitive_complexity)]
pub(super) fn emit_packages(
    package_directory: &Path,
    manager: PackageManager,
    mut emission: PackageEmission,
    weapons: &[WeaponCloneSpec],
    branding: crate::branding::Branding,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<NewWeaponProjectBundle> {
    // These passes run before the package count is known, so their labels don't advance it.
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
            &manager,
            &mut emission,
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
    ornament::apply(&manager, &mut emission, weapons)?;
    timed("ornaments", started);
    progress.start("Applying Reskins");
    let started = std::time::Instant::now();
    reskin::apply(
        package_directory,
        &manager,
        &mut emission,
        weapons,
        &mut replacements,
    )?;
    timed("reskins", started);
    progress.start("Applying Vehicle Art");
    let started = std::time::Instant::now();
    vehicle_art::apply(
        package_directory,
        &manager,
        &mut emission,
        weapons,
        &mut replacements,
    )?;
    timed("vehicle art", started);
    progress.diagnostic(format!("Item art timings: {}.", timings.join(", ")));
    // The crosshair table joins the other UI tables of its package.
    let (crosshair_table, replacements): (Vec<_>, Vec<_>) = replacements
        .into_iter()
        .partition(|r| r.tag == crate::weapon::crosshair::TABLE);
    let (imported_runtime, replacements): (Vec<_>, Vec<_>) = replacements
        .into_iter()
        .partition(|r| r.tag.pkg_id() == emission.table_tags.entity_assignment_tag.pkg_id());
    let (imported_strings, imported_shared): (Vec<_>, Vec<_>) = replacements
        .into_iter()
        .partition(|r| r.tag.pkg_id() == emission.table_tags.item_string_table_tag.pkg_id());
    let PackageEmission {
        lore,
        hud_table,
        ability_banks: mut shared_packages,
        hud_statuses,
        table_tags:
            super::sources::TableTags {
                item_table_tag,
                item_hash_index_table_tag,
                item_string_table_tag,
                item_metadata_table_tag,
                sandbox_pattern_table_tag,
                finished_sandbox_perk_table_tag,
                sandbox_perk_index_table_tag,
                item_icon_table_tag,
                item_dense_presentation_table_tag,
                item_metadata_index_table_tag,
                sandbox_pattern_index_table_tag,
                collectible_table_tag,
                collectible_display_table_tag,
                objective_table_tag,
                objective_string_table_tag,
                record_table_tag,
                record_string_table_tag,
                presentation_node_table_tag,
                presentation_node_string_table_tag,
                shared_expression_pool_table_tag,
                localized_index_tag,
                unlock_flag_bank_table_tag,
                unlock_table_tag,
                unlock_display_tag,
                entity_assignment_tag,
            },
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
        mut runtime_dependencies,
        host_new_tags,
        maps,
    } = emission;
    // Private copies past the private perk-runtime package's table go to standalone packages of
    // their own, from the top of the build's window down.
    let (private_perk_runtime_new_tags, private_runtime_overflow) =
        crate::appended_tags::AppendedTagAllocator::private_runtime(
            private_perk_runtime_append_start,
        )
        .split(private_perk_runtime_new_tags)?;
    // The UI tables by package, so each package takes one overlay: HUD statuses and their names,
    // subclass glyph colors and tree decisions, the ammo HUD table and the crosshair table. The
    // subclass tree decisions share the crosshair table's package and the ammo HUD table's.
    let mut ui_packages = BTreeMap::<u16, Vec<ReplacementSpec>>::new();
    for replacement in hud_statuses
        .into_iter()
        .chain(hud_table)
        .chain(crosshair_table)
    {
        ui_packages
            .entry(replacement.tag.pkg_id())
            .or_default()
            .push(replacement);
    }
    // Imported character banks and selectors belong to their native sandbox
    // packages. Merge them with an existing optional overlay for the same id.
    for replacement in imported_shared {
        let id = replacement.tag.pkg_id();
        ui_packages
            .get_mut(&id)
            .unwrap_or_else(|| shared_packages.entry(id).or_default())
            .push(replacement);
    }
    let host = crate::package_profile::host();
    let mut host_reference_overrides = watermark_reference_overrides;
    host_reference_overrides.extend(media_references(&host_new_tags));
    let record_packages = record_packages(host_new_tags, host_reference_overrides)?;
    let standalone_records = record_packages
        .iter()
        .filter(|package| package.id != HOST_PACKAGE_ID)
        .count();
    // One dependency check, six required overlays, and all optional packages in this plan.
    progress.payloads(
        7 + standalone_records
            + asset_packages.packages.len()
            + maps.package_count()
            + private_runtime_overflow.len()
            + shared_packages.len()
            + ui_packages.len()
            + usize::from(!private_perk_runtime_new_tags.is_empty())
            + usize::from(
                runtime_dependencies.is_some() || !maps.is_empty() || standalone_records != 0,
            ),
    );
    progress.start("Checking Asset Dependencies");
    asset_packages.validate()?;
    let stock_loading;
    let loading = if let Some(payload) = runtime_dependencies.as_deref() {
        payload
    } else {
        stock_loading = manager
            .read_tag(RUNTIME_DEPENDENCY_COMPANION)
            .map_err(|error| invalid(error.to_string()))?;
        &stock_loading
    };
    crate::shared_tag_dependency_index::scoped::validate_asset_loading(
        &manager,
        asset_packages
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
    validate_sandbox_perk_runtime_map(&entity_assignments).map_err(validation)?;
    progress.finish("Checking Asset Dependencies");
    let mut packages = packages::Packages::new(package_directory);
    // Every standalone record is enrolled, including legacy overflow. Loader builds leave the
    // stock host's entry count intact. Legacy builds retain only the fitting appended prefix.
    let mut host_records = Vec::new();
    let mut host_new_tags = Vec::new();
    let mut host_reference_overrides = Vec::new();
    let mut loading_records = Vec::new();
    for package in record_packages {
        if package.id == HOST_PACKAGE_ID {
            host_new_tags = package.tags;
            host_reference_overrides = package.references;
            continue;
        }
        crate::package_profile::name_standalone_package(
            package.id,
            if host.is_standalone() {
                crate::package_profile::RECORDS_FAMILY
            } else {
                crate::package_profile::LEGACY_RECORDS_FAMILY
            },
        )
        .map_err(invalid)?;
        let allocator = AppendedTagAllocator::new(package.id, 0);
        for ordinal in 0..package.tags.len() {
            loading_records.push(allocator.assigned_tag(
                ordinal,
                "Investment loading",
                "record",
            )?);
        }
        host_records.push(packages.standalone(package)?);
    }
    if !loading_records.is_empty() {
        runtime_dependencies = Some(crate::shared_tag_dependency_index::enroll_dependencies(
            runtime_dependencies
                .as_deref()
                .unwrap_or(loading.as_slice()),
            RUNTIME_DEPENDENCY_COMPANION,
            RUNTIME_DEPENDENCY_ROOT,
            &loading_records,
        )?);
    }
    let expected_host_count = HOST_EXPECTED_ENTRY_COUNT + host_new_tags.len();
    let host_overlay = packages.overlay(
        HOST_PACKAGE_ID,
        vec![
            ReplacementSpec {
                tag: unlock_display_tag,
                payload: unlock_displays,
            },
            ReplacementSpec {
                tag: item_dense_presentation_table_tag,
                payload: dense,
            },
            ReplacementSpec {
                tag: item_icon_table_tag,
                payload: item_icons,
            },
        ],
        host_new_tags,
        host_reference_overrides,
    )?;
    packages.host(host_overlay, expected_host_count);
    let mut asset_packages = asset_packages;
    name_audio_media(&mut asset_packages);
    let mut assets_end = asset_packages.native_count();
    let mut assets = plan_assets(&mut packages, asset_packages)?;
    // Each map's packages, named after the map, take the next ids in the window after the asset
    // packages.
    assets_end = plan_maps(
        &mut packages,
        &manager,
        &maps,
        assets_end,
        &mut assets,
        &mut runtime_dependencies,
        &loading,
    )?;
    // Map registration reads template metadata during planning. Release all source handles
    // before any of the planned package jobs starts writing.
    drop(manager);
    let map_tables = maps.replacements;
    let private_runtime_overflow =
        plan_private_runtime_overflow(&mut packages, assets_end, private_runtime_overflow)?;
    let private_count = private_perk_runtime_new_tags.len();
    let private_perk_runtime = if private_perk_runtime_new_tags.is_empty() {
        None
    } else {
        Some(packages.overlay(
            PRIVATE_PERK_RUNTIME_PACKAGE_ID,
            Vec::new(),
            private_perk_runtime_new_tags,
            Vec::new(),
        )?)
    };
    if let Some(ticket) = private_perk_runtime {
        packages.private(ticket, private_perk_runtime_append_start, private_count);
    }
    let runtime_entities = packages.overlay(
        entity_assignment_tag.pkg_id(),
        {
            let mut replacements = vec![ReplacementSpec {
                tag: entity_assignment_tag,
                payload: entity_assignments,
            }];
            replacements.extend(imported_runtime);
            replacements
        },
        Vec::new(),
        Vec::new(),
    )?;
    let runtime_dependency_overlay = runtime_dependencies
        .map(|payload| {
            packages.overlay(
                RUNTIME_DEPENDENCY_COMPANION.pkg_id(),
                vec![ReplacementSpec {
                    tag: RUNTIME_DEPENDENCY_COMPANION,
                    payload,
                }],
                Vec::new(),
                Vec::new(),
            )
        })
        .transpose()?;
    let mut investment_replacements = vec![
        ReplacementSpec {
            tag: item_table_tag,
            payload: item_table,
        },
        ReplacementSpec {
            tag: collectible_table_tag,
            payload: collectibles,
        },
        ReplacementSpec {
            tag: collectible_display_table_tag,
            payload: collectible_displays,
        },
    ];
    // Each subclass table, the dye table, the stat group table and the plug set table join the
    // overlay that already replaces tables in their package. An imported model's dyes extend the
    // dye table after the custom ones, so its replacement already holds them.
    let mut subclass_replacements = subclass_tables
        .map(crate::subclass::tables::SubclassTables::replacements)
        .transpose()?
        .unwrap_or_default();
    subclass_replacements.extend(dye_table.filter(|table| {
        !imported_strings
            .iter()
            .any(|replacement| replacement.tag == table.tag)
    }));
    subclass_replacements.extend(stat_group_table);
    subclass_replacements.extend(plug_set_table);
    // The activity, display and private match tables join the investment overlays of their
    // packages.
    subclass_replacements.extend(map_tables);
    let mut subclass_tables_in = |package: u16| {
        let (owned, rest) = std::mem::take(&mut subclass_replacements)
            .into_iter()
            .partition::<Vec<_>, _>(|replacement| replacement.tag.pkg_id() == package);
        subclass_replacements = rest;
        owned
    };
    investment_replacements.extend(subclass_tables_in(item_table_tag.pkg_id()));
    let investment = packages.overlay(
        item_table_tag.pkg_id(),
        investment_replacements,
        Vec::new(),
        Vec::new(),
    )?;
    let mut string_replacements = vec![
        ReplacementSpec {
            tag: item_string_table_tag,
            payload: item_strings,
        },
        ReplacementSpec {
            tag: item_metadata_table_tag,
            payload: item_metadata,
        },
        ReplacementSpec {
            tag: presentation_node_string_table_tag,
            payload: node_strings,
        },
        ReplacementSpec {
            tag: objective_string_table_tag,
            payload: objective_strings,
        },
        ReplacementSpec {
            tag: record_string_table_tag,
            payload: record_strings,
        },
    ];
    if any_sandbox_pattern {
        string_replacements.push(ReplacementSpec {
            tag: sandbox_pattern_table_tag,
            payload: sandbox_patterns,
        });
    }
    if has_custom_plugs {
        string_replacements.push(ReplacementSpec {
            tag: finished_sandbox_perk_table_tag,
            payload: finished_sandbox_perks,
        });
    }
    string_replacements.extend(subclass_tables_in(item_string_table_tag.pkg_id()));
    let lore_definition = if let Some(lore) = lore {
        if lore.strings.tag.pkg_id() != item_string_table_tag.pkg_id()
            || lore.definitions.tag.pkg_id() != unlock_table_tag.pkg_id()
        {
            return Err(invalid(
                "Lore tables moved outside their audited package owners",
            ));
        }
        string_replacements.push(lore.strings);
        Some(lore.definitions)
    } else {
        None
    };
    string_replacements.extend(imported_strings);
    let strings = packages.overlay(
        item_string_table_tag.pkg_id(),
        string_replacements,
        Vec::new(),
        Vec::new(),
    )?;
    let mut localization_replacements = vec![ReplacementSpec {
        tag: localization.donor_header_tag,
        payload: localization.merged_header,
    }];
    localization_replacements.extend(localization.locale_data.into_iter().map(|locale| {
        ReplacementSpec {
            tag: locale.donor_tag,
            payload: locale.payload,
        }
    }));
    let localized = packages.overlay(
        localized_index_tag.pkg_id(),
        localization_replacements,
        Vec::new(),
        Vec::new(),
    )?;
    let mut unlock_replacements = vec![
        ReplacementSpec {
            tag: presentation_node_table_tag,
            payload: nodes,
        },
        ReplacementSpec {
            tag: objective_table_tag,
            payload: objectives,
        },
        ReplacementSpec {
            tag: record_table_tag,
            payload: records,
        },
        ReplacementSpec {
            tag: shared_expression_pool_table_tag,
            payload: pools,
        },
        ReplacementSpec {
            tag: unlock_table_tag,
            payload: unlocks,
        },
        ReplacementSpec {
            tag: unlock_flag_bank_table_tag,
            payload: unlock_banks,
        },
        ReplacementSpec {
            tag: item_metadata_index_table_tag,
            payload: item_metadata_index,
        },
    ];
    if any_sandbox_pattern {
        unlock_replacements.push(ReplacementSpec {
            tag: sandbox_pattern_index_table_tag,
            payload: sandbox_pattern_index,
        });
    }
    if has_custom_plugs {
        unlock_replacements.push(ReplacementSpec {
            tag: item_hash_index_table_tag,
            payload: item_hash_index,
        });
        unlock_replacements.push(ReplacementSpec {
            tag: sandbox_perk_index_table_tag,
            payload: sandbox_perk_indices,
        });
    }
    if let Some(lore) = lore_definition {
        unlock_replacements.push(lore);
    }
    unlock_replacements.extend(subclass_tables_in(unlock_table_tag.pkg_id()));
    if let Some(stray) = subclass_replacements.first() {
        return Err(invalid(format!(
            "Table {} is in package {:04x}, which no project overlay replaces",
            stray.tag,
            stray.tag.pkg_id()
        )));
    }
    let unlock = packages.overlay(
        unlock_table_tag.pkg_id(),
        unlock_replacements,
        Vec::new(),
        Vec::new(),
    )?;
    // A UI package that another project overlay also replaces would take two overlays.
    let project_packages = [
        HOST_PACKAGE_ID,
        PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        RUNTIME_DEPENDENCY_COMPANION.pkg_id(),
        item_table_tag.pkg_id(),
        item_string_table_tag.pkg_id(),
        localized_index_tag.pkg_id(),
        unlock_table_tag.pkg_id(),
        entity_assignment_tag.pkg_id(),
    ];
    if let Some(package) = ui_packages
        .keys()
        .find(|package| project_packages.contains(package) || shared_packages.contains_key(package))
    {
        return Err(invalid(format!(
            "UI tables in package {package:04x} share it with another authored overlay"
        )));
    }
    // Shared sandbox resources use one overlay per package, including private
    // ability rows and imported character animation branches.
    let mut shared_overlays = Vec::new();
    for (package_id, replacements) in shared_packages {
        shared_overlays.push(packages.overlay(package_id, replacements, Vec::new(), Vec::new())?);
    }
    let mut ui_overlays = Vec::new();
    for (package_id, replacements) in ui_packages {
        ui_overlays.push(packages.overlay(package_id, replacements, Vec::new(), Vec::new())?);
    }
    let mut order = assets;
    order.extend(host_records);
    order.extend(private_runtime_overflow);
    order.extend(ui_overlays);
    order.extend(shared_overlays);
    order.extend(private_perk_runtime);
    order.extend(runtime_dependency_overlay);
    order.extend([
        runtime_entities,
        host_overlay,
        investment,
        strings,
        localized,
        unlock,
    ]);
    let artifacts = packages.build(order, progress)?;
    Ok(NewWeaponProjectBundle {
        plan: NewWeaponProjectPlan {
            weapons: plans,
            sunrise: SunriseProjectMetadata {
                badge_node_hashes: SUNRISE_BADGE_NODE_HASHES,
                badge_name_hash: SUNRISE_BADGE_NAME_HASH,
                badge_description_hash: SUNRISE_BADGE_DESCRIPTION_HASH,
                badge_icon_tag,
                watermark_layer_tag,
                watermarked_icon_containers,
            },
        },
        artifacts,
    })
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
/// returns how many positions the build then uses. The client registers a package once a loading index names one of its
/// entries, and a name lookup alone does not. The investment loading root, whose index starts as
/// `runtime_dependencies` or else as `loading`, names the entries `Plan::registration` chooses,
/// so both packages are registered before an activity looks up its root.
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
            #[cfg(feature = "d2-model-importer")]
            if crate::package_profile::host().is_standalone() {
                let native = crate::package_profile::window()
                    .nth(assets_end)
                    .ok_or_else(|| {
                        validation("The build's maps exceed the free authored package IDs")
                    })?;
                if let Some(split) = maps.split_world(index, part, native)? {
                    let index_payload = runtime_dependencies.as_deref().unwrap_or(loading);
                    let registered = crate::shared_tag_dependency_index::dependency_entries(
                        index_payload,
                        RUNTIME_DEPENDENCY_COMPANION,
                        RUNTIME_DEPENDENCY_ROOT,
                    )?
                    .into_iter()
                    .map(|(package, _)| package)
                    .collect();
                    let registration = maps.registration_at(
                        &split.addresses,
                        &split.tags,
                        &split.references,
                        &registered,
                        manager,
                    )?;
                    // Every package of a world takes the world's family, as a stock
                    // destination's packages do, and only its id tells them apart.
                    let family = maps.family(index, part);
                    let counts = std::iter::once((native, 0, split.native_entries, family.clone()))
                        .chain(split.resource_ids.iter().enumerate().map(|(chunk, &id)| {
                            let start = split.native_entries + chunk * CAPACITY;
                            let end = (start + CAPACITY).min(split.tags.len());
                            (id, start, end, family.clone())
                        }));
                    for (id, start, end, family) in counts {
                        crate::package_profile::name_standalone_package(id, &family)
                            .map_err(validation)?;
                        let references = split.references.iter().filter(|r| (start..end).contains(&r.new_tag_ordinal))
                            .map(|r| Ok(crate::NewTagReferenceOverride {
                                new_tag_ordinal: r.new_tag_ordinal - start,
                                reference: match r.reference {
                                    crate::NewTagReference::Appended(at) => crate::NewTagReference::Tag(
                                        *split.addresses.get(at).ok_or_else(|| validation("Map entry reference exceeds its allocation"))?),
                                    other => other,
                                },
                            })).collect::<AuthoringResult<Vec<_>>>()?;
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
                    *runtime_dependencies =
                        Some(crate::shared_tag_dependency_index::enroll_dependencies(
                            index_payload,
                            RUNTIME_DEPENDENCY_COMPANION,
                            RUNTIME_DEPENDENCY_ROOT,
                            &registration,
                        )?);
                    continue;
                }
            }
            let ids = (0..maps.part_count(index, part))
                .map(|chunk| {
                    crate::package_profile::window()
                        .nth(assets_end + chunk)
                        .ok_or_else(|| {
                            validation(
                                "The build's maps need more packages than the free authored ids hold",
                            )
                        })
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            let (tags, named, references) = maps.package(index, part, &ids)?;
            for &id in &ids {
                crate::mod_family::build::native(id);
            }
            if tags.len().div_ceil(CAPACITY) != ids.len() {
                return Err(validation(
                    "Map package allocation does not match its entries",
                ));
            }
            let index_payload = runtime_dependencies.as_deref().unwrap_or(loading);
            let registered = crate::shared_tag_dependency_index::dependency_entries(
                index_payload,
                RUNTIME_DEPENDENCY_COMPANION,
                RUNTIME_DEPENDENCY_ROOT,
            )?
            .into_iter()
            .map(|(package, _)| package)
            .collect();
            let registration = maps.registration(&ids, &tags, &references, &registered, manager)?;
            let mut tags = tags.into_iter();
            for (chunk, &id) in ids.iter().enumerate() {
                let family = maps.family(index, part);
                crate::package_profile::name_standalone_package(id, &family).map_err(validation)?;
                let start = chunk * CAPACITY;
                let part_tags = tags.by_ref().take(CAPACITY).collect::<Vec<_>>();
                let end = start + part_tags.len();
                let part_references = references
                    .iter()
                    .filter(|reference| (start..end).contains(&reference.new_tag_ordinal))
                    .map(|reference| {
                        Ok(crate::NewTagReferenceOverride {
                            new_tag_ordinal: reference.new_tag_ordinal - start,
                            reference: match reference.reference {
                                crate::NewTagReference::Appended(target) => {
                                    let package = ids.get(target / CAPACITY).ok_or_else(|| {
                                        validation(
                                            "Map entry reference exceeds its allocated packages",
                                        )
                                    })?;
                                    crate::NewTagReference::Tag(TagHash::new(
                                        *package,
                                        (target % CAPACITY) as u16,
                                    ))
                                }
                                other => other,
                            },
                        })
                    })
                    .collect::<AuthoringResult<Vec<_>>>()?;
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
            assets_end += ids.len();
            *runtime_dependencies = Some(crate::shared_tag_dependency_index::enroll_dependencies(
                index_payload,
                RUNTIME_DEPENDENCY_COMPANION,
                RUNTIME_DEPENDENCY_ROOT,
                &registration,
            )?);
        }
    }
    Ok(assets_end)
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
