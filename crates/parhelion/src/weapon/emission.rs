//! Consume a completed payload plan and emit its verified package artifacts.
use super::*;
mod packages;

pub(super) struct PackageEmission {
    pub(super) lore: Option<lore::Plan>,
    pub(super) hud_table: Option<ReplacementSpec>,
    pub(super) item_table_tag: TagHash,
    pub(super) item_hash_index_table_tag: TagHash,
    pub(super) item_string_table_tag: TagHash,
    pub(super) item_metadata_table_tag: TagHash,
    pub(super) sandbox_pattern_table_tag: TagHash,
    pub(super) finished_sandbox_perk_table_tag: TagHash,
    pub(super) sandbox_perk_index_table_tag: TagHash,
    pub(super) item_icon_table_tag: TagHash,
    pub(super) item_dense_presentation_table_tag: TagHash,
    pub(super) item_metadata_index_table_tag: TagHash,
    pub(super) sandbox_pattern_index_table_tag: TagHash,
    pub(super) collectible_table_tag: TagHash,
    pub(super) collectible_display_table_tag: TagHash,
    pub(super) objective_table_tag: TagHash,
    pub(super) objective_string_table_tag: TagHash,
    pub(super) record_table_tag: TagHash,
    pub(super) record_string_table_tag: TagHash,
    pub(super) presentation_node_table_tag: TagHash,
    pub(super) presentation_node_string_table_tag: TagHash,
    pub(super) shared_expression_pool_table_tag: TagHash,
    pub(super) localized_index_tag: TagHash,
    pub(super) unlock_flag_bank_table_tag: TagHash,
    pub(super) unlock_table_tag: TagHash,
    pub(super) unlock_display_tag: TagHash,
    pub(super) entity_assignment_tag: TagHash,
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

pub(super) fn emit_packages(
    package_directory: &Path,
    emission: PackageEmission,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<NewWeaponProjectBundle> {
    let PackageEmission {
        lore,
        hud_table,
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
    // One dependency check, six required overlays, and the packages present in this plan.
    progress.payloads(
        7 + asset_packages.packages.len()
            + usize::from(!private_perk_runtime_new_tags.is_empty())
            + usize::from(runtime_dependencies.is_some())
            + usize::from(hud_table.is_some()),
    );
    progress.start("Checking Asset Dependencies");
    asset_packages.validate()?;
    let loading_manager =
        sundial::package_authoring::open_shadowkeep_package_manager(package_directory)
            .map_err(invalid)?;
    let stock_loading;
    let loading = if let Some(payload) = runtime_dependencies.as_deref() {
        payload
    } else {
        stock_loading = loading_manager
            .read_tag(RUNTIME_DEPENDENCY_COMPANION)
            .map_err(|error| invalid(error.to_string()))?;
        &stock_loading
    };
    crate::shared_tag_dependency_index::scoped::validate_asset_loading(
        &loading_manager,
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
    drop(loading_manager);
    // Validate the completed map after every authoring pass, not only the stock source.
    validate_sandbox_perk_runtime_map(&entity_assignments).map_err(validation)?;
    progress.finish("Checking Asset Dependencies");
    let mut packages = packages::Packages {
        directory: package_directory,
        progress,
    };
    let host = packages.overlay(
        HOST_PACKAGE_ID,
        &[
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
        &host_new_tags,
        &watermark_reference_overrides,
    )?;
    let expected_host_count = HOST_EXPECTED_ENTRY_COUNT + host_new_tags.len();
    if host.plan.final_entry_count != expected_host_count {
        return Err(validation(format!(
            "Project host ended at {} entries instead of {expected_host_count}",
            host.plan.final_entry_count
        )));
    }
    let mut assets = Vec::new();
    for package in &asset_packages.packages {
        let artifact = packages.standalone(package)?;
        if artifact.plan.original_entry_count != 0
            || artifact.plan.final_entry_count != package.tags.len()
            || artifact.plan.appended_tags.len() != package.tags.len()
        {
            return Err(validation(
                "An asset package did not contain its complete authored resource group",
            ));
        }
        assets.push(artifact);
    }
    let private_perk_runtime = if private_perk_runtime_new_tags.is_empty() {
        None
    } else {
        Some(packages.overlay(
            PRIVATE_PERK_RUNTIME_PACKAGE_ID,
            &[],
            &private_perk_runtime_new_tags,
            &[],
        )?)
    };
    let expected_private_perk_runtime_count = private_perk_runtime_append_start
        .checked_add(private_perk_runtime_new_tags.len())
        .ok_or_else(|| invalid("Private perk-runtime entry count overflowed"))?;
    if let Some(private_perk_runtime) = &private_perk_runtime {
        if private_perk_runtime.plan.original_entry_count
            != PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT
            || private_perk_runtime.plan.append_start_entry_count
                != private_perk_runtime_append_start
            || private_perk_runtime.plan.reserved_entry_count
                != private_perk_runtime_append_start - PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT
            || private_perk_runtime.plan.final_entry_count != expected_private_perk_runtime_count
            || private_perk_runtime.plan.appended_tags.len() != private_perk_runtime_new_tags.len()
        {
            return Err(validation(
                "Private perk-runtime package did not preserve its stock entry table and authored tail",
            ));
        }
    }
    let runtime_entities = packages.overlay(
        entity_assignment_tag.pkg_id(),
        &[ReplacementSpec {
            tag: entity_assignment_tag,
            payload: entity_assignments,
        }],
        &[],
        &[],
    )?;
    let runtime_dependency_overlay = runtime_dependencies
        .map(|payload| {
            packages.overlay(
                RUNTIME_DEPENDENCY_COMPANION.pkg_id(),
                &[ReplacementSpec {
                    tag: RUNTIME_DEPENDENCY_COMPANION,
                    payload,
                }],
                &[],
                &[],
            )
        })
        .transpose()?;
    let investment = packages.overlay(
        item_table_tag.pkg_id(),
        &[
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
        ],
        &[],
        &[],
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
    let strings = packages.overlay(
        item_string_table_tag.pkg_id(),
        &string_replacements,
        &[],
        &[],
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
        &localization_replacements,
        &[],
        &[],
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
    let unlock = packages.overlay(unlock_table_tag.pkg_id(), &unlock_replacements, &[], &[])?;
    for artifact in [
        &runtime_entities,
        &investment,
        &strings,
        &localized,
        &unlock,
    ] {
        if artifact.plan.original_entry_count != artifact.plan.final_entry_count
            || !artifact.plan.appended_tags.is_empty()
        {
            return Err(validation(format!(
                "Project overlay {:04x} unexpectedly changed its stock entry table",
                artifact.plan.chain.identity.package_id
            )));
        }
    }

    let hud_overlay = hud_table
        .map(|replacement| packages.overlay(replacement.tag.pkg_id(), &[replacement], &[], &[]))
        .transpose()?;
    let mut artifacts = assets;
    artifacts.extend(hud_overlay);
    artifacts.extend(private_perk_runtime);
    artifacts.extend(runtime_dependency_overlay);
    artifacts.extend([
        runtime_entities,
        host,
        investment,
        strings,
        localized,
        unlock,
    ]);
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
