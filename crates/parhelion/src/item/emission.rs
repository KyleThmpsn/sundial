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
    if tag.pkg_id() != HOST_PACKAGE_ID {
        return Err(invalid("Authored item must be a private definition"));
    }
    let ordinal = (tag.entry_index() as usize)
        .checked_sub(HOST_EXPECTED_ENTRY_COUNT)
        .ok_or_else(|| invalid("Authored item is a stock definition"))?;
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
    let ordinal = (tag.pkg_id() == HOST_PACKAGE_ID)
        .then(|| (tag.entry_index() as usize).checked_sub(HOST_EXPECTED_ENTRY_COUNT))
        .flatten()
        .filter(|ordinal| *ordinal < emission.host_new_tags.len())
        .ok_or_else(|| invalid("Authored item strings must be a private record"))?;
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
    pub(super) item_icons: Vec<u8>,
    pub(super) runtime_dependencies: Option<Vec<u8>>,
    pub(super) host_new_tags: Vec<NewTagSpec>,
}

/// Wwise finds a medium by the media ID in its entry header, so every appended medium names
/// itself there, in the asset packages as in the host.
fn name_audio_media(asset_packages: &mut crate::asset_packages::AssetPackages) {
    for package in &mut asset_packages.packages {
        for (ordinal, spec) in package.tags.iter().enumerate() {
            if spec.storage == crate::NewTagStorageMode::AudioMedia
                && !package
                    .references
                    .iter()
                    .any(|reference| reference.new_tag_ordinal == ordinal)
            {
                package.references.push(crate::NewTagReferenceOverride {
                    new_tag_ordinal: ordinal,
                    reference: crate::NewTagReference::Appended(ordinal),
                });
            }
        }
    }
}

#[allow(unused_mut)]
pub(super) fn emit_packages(
    package_directory: &Path,
    manager: PackageManager,
    mut emission: PackageEmission,
    weapons: &[WeaponCloneSpec],
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<NewWeaponProjectBundle> {
    #[cfg(feature = "d2-model-importer")]
    let mut replacements = imported::apply(package_directory, &manager, &mut emission, weapons)?;
    #[cfg(not(feature = "d2-model-importer"))]
    let mut replacements: Vec<ReplacementSpec> = Vec::new();
    ornament::apply(&manager, &mut emission, weapons)?;
    reskin::apply(
        package_directory,
        &manager,
        &mut emission,
        weapons,
        &mut replacements,
    )?;
    vehicle_art::apply(
        package_directory,
        &manager,
        &mut emission,
        weapons,
        &mut replacements,
    )?;
    // The crosshair table joins the other UI tables of its package.
    let (crosshair_table, replacements): (Vec<_>, Vec<_>) = replacements
        .into_iter()
        .partition(|r| r.tag == crate::weapon::crosshair::TABLE);
    let (imported_runtime, imported_strings): (Vec<_>, Vec<_>) = replacements
        .into_iter()
        .partition(|r| r.tag.pkg_id() == emission.table_tags.entity_assignment_tag.pkg_id());
    let PackageEmission {
        lore,
        hud_table,
        ability_banks,
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
        item_icons,
        runtime_dependencies,
        host_new_tags,
    } = emission;
    // Private copies past the private perk-runtime package's table go to standalone packages of
    // their own, from the top of the authored range down.
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
    // One dependency check, six required overlays, and all optional packages in this plan.
    progress.payloads(
        7 + asset_packages.packages.len()
            + private_runtime_overflow.len()
            + ability_banks.len()
            + ui_packages.len()
            + usize::from(!private_perk_runtime_new_tags.is_empty())
            + usize::from(runtime_dependencies.is_some()),
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
    // The package writer must not retain the source manager's open file handles.
    drop(manager);
    // Validate the completed map after every authoring pass, not only the stock source.
    validate_sandbox_perk_runtime_map(&entity_assignments).map_err(validation)?;
    progress.finish("Checking Asset Dependencies");
    let mut packages = packages::Packages::new(package_directory);
    let mut host_reference_overrides = watermark_reference_overrides;
    for (ordinal, spec) in host_new_tags.iter().enumerate() {
        if spec.storage == crate::NewTagStorageMode::AudioMedia {
            host_reference_overrides.push(crate::NewTagReferenceOverride {
                new_tag_ordinal: ordinal,
                reference: crate::NewTagReference::Appended(ordinal),
            });
        }
    }
    let expected_host_count = HOST_EXPECTED_ENTRY_COUNT + host_new_tags.len();
    let host = packages.overlay(
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
    packages.host(host, expected_host_count);
    let mut asset_packages = asset_packages;
    name_audio_media(&mut asset_packages);
    let assets_end = usize::from(crate::package_profile::PARHELION_ASSET_PACKAGE_ID)
        + asset_packages.packages.len();
    let assets = plan_assets(&mut packages, asset_packages)?;
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
        .find(|package| project_packages.contains(package) || ability_banks.contains_key(package))
    {
        return Err(invalid(format!(
            "UI tables in package {package:04x} share it with another authored overlay"
        )));
    }
    // The sandbox banks with a charge row added, one overlay per package that has any, only
    // when a private perk applies the key.
    let mut ability_bank_overlays = Vec::new();
    for (package_id, replacements) in ability_banks {
        ability_bank_overlays.push(packages.overlay(
            package_id,
            replacements,
            Vec::new(),
            Vec::new(),
        )?);
    }
    let mut ui_overlays = Vec::new();
    for (package_id, replacements) in ui_packages {
        ui_overlays.push(packages.overlay(package_id, replacements, Vec::new(), Vec::new())?);
    }
    let mut order = assets;
    order.extend(private_runtime_overflow);
    order.extend(ui_overlays);
    order.extend(ability_bank_overlays);
    order.extend(private_perk_runtime);
    order.extend(runtime_dependency_overlay);
    order.extend([
        runtime_entities,
        host,
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

/// Reserve each private overflow output after checking its range against the asset packages.
fn plan_private_runtime_overflow(
    packages: &mut packages::Packages<'_>,
    assets_end: usize,
    overflow: Vec<(u16, Vec<crate::NewTagSpec>)>,
) -> AuthoringResult<Vec<packages::Ticket>> {
    let mut planned = Vec::new();
    for (id, tags) in overflow {
        if usize::from(id) < assets_end {
            return Err(validation(
                "The build's private copies and assets need more packages than the authored range holds",
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
