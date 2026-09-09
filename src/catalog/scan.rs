//! Installed-package catalog orchestration: sources, progression, items, and final joins.
use super::{
    CatalogProgress,
    cache::CatalogContents,
    icons::scan_item_icon_containers,
    items::{
        ItemScan, ItemScanContext, item_power_cap, scan_ability_displays,
        scan_inventory_bucket_descriptors, scan_items, scan_power_cap_definitions,
        scan_sandbox_perk_catalog, scan_stat_definitions, scan_stat_groups,
    },
    progression::{
        attach_progression_references, expand_shared_condition_contexts,
        scan_package_condition_contexts, scan_progression_definitions, sort_progression_contexts,
    },
};
use crate::{
    investment_localization::LocalizedStringCache, investment_schema::investment_globals_table_tag,
};
use std::path::Path;
use tiger_pkg::TagHash;
mod progression;
mod sources;

pub(super) fn retain_progression_scan<T: Default>(
    section: &str,
    result: Result<T, String>,
    errors: &mut Vec<String>,
) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!("{section}: {error}"));
            T::default()
        }
    }
}

pub(super) fn retain_progression_enrichment<T>(
    section: &str,
    result: Result<(), String>,
    value: T,
    errors: &mut Vec<String>,
) -> T {
    if let Err(error) = result {
        errors.push(format!("{section}: {error}"));
    }
    value
}

pub(super) fn scan_packages(
    install: &Path,
    report: &mut dyn FnMut(CatalogProgress),
) -> Result<CatalogContents, String> {
    report(CatalogProgress::stage(
        "Opening the installed game packages…",
    ));
    let sources = sources::read(install)?;
    let sources::Sources {
        manager,
        globals_data,
        root,
        localized_tags,
    } = &sources;
    let mut localized_cache = LocalizedStringCache::new();
    let power_cap_definitions = scan_power_cap_definitions(manager, root)?;
    let icon_containers_by_index = scan_item_icon_containers(manager, globals_data)?;
    report(CatalogProgress::stage("Reading subclass ability names…"));
    let ability_displays =
        scan_ability_displays(manager, globals_data, localized_tags, &mut localized_cache)?;
    let item_stat_definitions = scan_stat_definitions(
        manager,
        root,
        globals_data,
        localized_tags,
        &mut localized_cache,
        &icon_containers_by_index,
    )?;
    let item_stat_groups = scan_stat_groups(manager, globals_data)?;
    let stat_names = item_stat_definitions
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    let sandbox_perk_catalog = scan_sandbox_perk_catalog(manager, globals_data)?;
    let mut progression = progression::read(&sources, &mut localized_cache, report);
    let inventory_buckets = scan_inventory_bucket_descriptors(manager, root)?;
    let tables = sources::ItemTables::read(&sources)?;
    let progression_definitions = retain_progression_scan(
        "Progression definitions",
        scan_progression_definitions(
            manager,
            root,
            globals_data,
            localized_tags,
            &mut localized_cache,
            &tables.hashes,
            &icon_containers_by_index,
        ),
        &mut progression.errors,
    );
    if let Err(error) = attach_progression_references(
        &progression_definitions,
        &mut progression.unlock_flag_definitions,
        &mut progression.unlock_value_definitions,
    ) {
        progression
            .errors
            .push(format!("Progression unlock references: {error}"));
    }
    let mut item_scan = scan_items(
        ItemScanContext {
            manager,
            root,
            hashes: &tables.hashes,
            definition_tags: &tables.definition_tags,
            string_map: &tables.string_map,
            string_count: tables.string_count,
            string_rows: tables.string_rows,
            plug_set_table: &tables.plug_set_table,
            icon_containers_by_index: &icon_containers_by_index,
            inventory_buckets: &inventory_buckets,
            item_stat_definitions: &item_stat_definitions,
            stat_names: &stat_names,
            sandbox_perk_catalog: Some(&sandbox_perk_catalog),
            trait_definition_count: progression.trait_definitions.len(),
            ability_displays: &ability_displays,
            collectible_item_paths: &progression.collectible_item_paths,
            collectible_condition_contexts: &progression.collectible_condition_contexts,
            localized_tags,
            localized_cache: &mut localized_cache,
            objectives: &mut progression.objectives,
            unlock_flag_definitions: &mut progression.unlock_flag_definitions,
            unlock_value_definitions: &mut progression.unlock_value_definitions,
        },
        report,
    )?;
    enrich_item_metadata(&sources, &mut item_scan, &power_cap_definitions);
    if let Err(error) = scan_package_condition_contexts(
        &sources.manager,
        &sources.root,
        &progression.shared_expression_pool,
        &mut progression.unlock_flag_definitions,
        &mut progression.unlock_value_definitions,
    ) {
        progression
            .errors
            .push(format!("Package condition references: {error}"));
    }
    expand_shared_condition_contexts(
        &progression.shared_expression_pool,
        &mut progression.unlock_flag_definitions,
        &mut progression.unlock_value_definitions,
    );
    sort_progression_contexts(&mut progression.unlock_flag_definitions);
    sort_progression_contexts(&mut progression.unlock_value_definitions);
    let progression::ResolvedCollections {
        material_requirement_sets,
        collectibles,
    } = progression::resolve_collections(
        progression.pending_material_requirement_sets,
        progression.pending_collectibles,
        &tables.hashes,
        &mut item_scan,
        &mut progression.errors,
    );

    item_scan.diagnostics.append_to(&mut progression.errors);
    let progression_package_error =
        (!progression.errors.is_empty()).then(|| progression.errors.join("\n"));
    let package_names = manager
        .package_paths
        .iter()
        .filter(|(_, package)| !package.name.trim().is_empty())
        .map(|(&package_id, package)| (package_id, package.name.clone()))
        .collect();
    Ok(CatalogContents {
        items: item_scan.items,
        names: item_scan.names,
        type_names: item_scan.type_names,
        package_item_names: item_scan.package_item_names,
        package_item_type_names: item_scan.package_item_type_names,
        descriptions: item_scan.descriptions,
        icon_containers: item_scan.icon_containers,
        item_package_metadata: item_scan.item_package_metadata,
        item_stat_definitions,
        power_cap_definitions,
        item_stat_groups,
        trait_definitions: progression.trait_definitions,
        reusable_plug_set_count: tables.reusable_plug_set_count,
        socket_entry_list_count: tables.socket_entry_list_count,
        package_names,
        inventory_metadata: item_scan.inventory_metadata,
        objectives: progression.objectives,
        unlock_flag_definitions: progression.unlock_flag_definitions,
        unlock_value_definitions: progression.unlock_value_definitions,
        collectibles,
        shared_expression_pool: progression.shared_expression_pool,
        material_requirement_sets,
        item_material_requirement_set_indices: item_scan.item_material_requirement_set_indices,
        progression_definitions,
        progression_package_error,
        plug_pools: Vec::new(),
    })
}

fn enrich_item_metadata(
    sources: &sources::Sources,
    items: &mut ItemScan,
    power_cap_definitions: &[super::PowerCapDefinition],
) {
    let sources::Sources {
        manager,
        globals_data,
        ..
    } = sources;
    for metadata in items.item_package_metadata.values_mut() {
        metadata.power_cap = item_power_cap(&metadata.power_cap_groups, power_cap_definitions);
    }
    // Resolve by each item's referenced pattern index, not the pattern row's item identity:
    // multiple stock weapons legitimately share a runtime row.
    if let Some(patterns) = investment_globals_table_tag(
        globals_data,
        crate::investment_schema::GLOBALS_SANDBOX_PATTERN_TABLE_SLOT,
    )
    .ok()
    .and_then(|tag| manager.read_tag(TagHash(tag)).ok())
    {
        for metadata in items.item_package_metadata.values_mut() {
            metadata.weapon_translation_group = metadata
                .weapon_pattern_index
                .and_then(|index| {
                    crate::weapon_entity::sandbox_pattern_identity_at(&patterns, usize::from(index))
                        .ok()
                        .flatten()
                })
                .map(|identity| identity.weapon_translation_group_hash);
        }
    }
}
