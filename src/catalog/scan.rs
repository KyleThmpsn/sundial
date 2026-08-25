//! Installed-package catalog scanning and cross-domain build orchestration.

use std::path::Path;

use tiger_pkg::TagHash;

use crate::{orbit_map, package_runtime};

use super::{
    CatalogProgress,
    cache::CatalogContents,
    collections::{
        materialize_collectibles, materialize_material_requirement_sets, scan_collectibles,
        scan_material_requirement_sets,
    },
    icons::scan_item_icon_containers,
    items::{
        ItemScan, ItemScanContext, scan_ability_displays, scan_inventory_bucket_descriptors,
        scan_items, scan_sandbox_perk_hashes, scan_sandbox_perk_runtime_definitions,
        scan_stat_definitions,
    },
    localization::LocalizedStringCache,
    package::{array_at, u32_at},
    progression::{
        ProgressionPackageData, attach_presentation_node_objective_owners,
        scan_activity_condition_contexts, scan_collectible_condition_contexts,
        scan_collectible_item_paths, scan_location_condition_contexts,
        scan_metric_objective_owners, scan_milestone_objective_owners, scan_objectives,
        scan_presentation_nodes, scan_progression_definitions, scan_record_objective_owners,
        scan_unlock_flag_definitions, scan_unlock_flag_displays, scan_unlock_value_definitions,
        sort_progression_contexts,
    },
};

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
    let manager = package_runtime::open_shadowkeep_packages(install)?;
    let globals = manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .ok_or("The install has no investment_globals tag")?;
    let globals_data = manager
        .read_tag(globals.hash)
        .map_err(|e| format!("Could not read investment globals: {e}"))?;
    let localized_index = manager
        .read_tag(TagHash(u32_at(&globals_data, 16 + 72 * 16)?))
        .map_err(|e| format!("Could not read localized-string index: {e}"))?;
    let (localized_count, localized_rows, _) = array_at(&localized_index, 8)?;
    let localized_tags: Vec<TagHash> = (0..localized_count)
        .filter_map(|i| {
            u32_at(&localized_index, localized_rows + i * 8 + 4)
                .ok()
                .map(TagHash)
        })
        .collect();
    let mut localized_cache = LocalizedStringCache::new();
    let root = manager
        .read_tag(TagHash(u32_at(&globals_data, 16)?))
        .map_err(|e| format!("Could not read investment root: {e}"))?;
    let icon_containers_by_index = scan_item_icon_containers(&manager, &globals_data)?;
    report(CatalogProgress::stage("Reading subclass ability names…"));
    let ability_displays = scan_ability_displays(&manager, &localized_tags, &mut localized_cache);
    let item_stat_definitions = scan_stat_definitions(
        &manager,
        &root,
        &globals_data,
        &localized_tags,
        &mut localized_cache,
        &icon_containers_by_index,
    );
    let stat_names = item_stat_definitions
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    let sandbox_perk_hashes = scan_sandbox_perk_hashes(&manager, &root);
    let sandbox_perk_definitions =
        scan_sandbox_perk_runtime_definitions(&manager, &root, &globals_data, &sandbox_perk_hashes);
    report(CatalogProgress::stage("Reading Orbit backdrops…"));
    let orbit_map::Scan {
        backdrops: orbit_backdrops,
        entries: orbit_map_entries,
    } = orbit_map::scan(&manager)?;
    let mut progression_package_errors = Vec::new();
    report(CatalogProgress::stage("Reading unlock definitions…"));
    let mut unlock_flag_definitions = retain_progression_scan(
        "Unlock flag definitions",
        scan_unlock_flag_definitions(&manager, &root),
        &mut progression_package_errors,
    );
    if !unlock_flag_definitions.is_empty() {
        let display_result = scan_unlock_flag_displays(
            &manager,
            &globals_data,
            &localized_tags,
            &mut localized_cache,
            &mut unlock_flag_definitions,
        );
        unlock_flag_definitions = retain_progression_enrichment(
            "Unlock flag displays",
            display_result,
            unlock_flag_definitions,
            &mut progression_package_errors,
        );
    }
    let mut unlock_value_definitions = retain_progression_scan(
        "Unlock value definitions",
        scan_unlock_value_definitions(&manager, &root),
        &mut progression_package_errors,
    );
    report(CatalogProgress::stage("Reading objective definitions…"));
    let mut objectives = retain_progression_scan(
        "Objective definitions",
        scan_objectives(
            &manager,
            &root,
            &globals_data,
            &localized_tags,
            &mut localized_cache,
            &mut unlock_flag_definitions,
            &mut unlock_value_definitions,
        ),
        &mut progression_package_errors,
    );
    let mut progression_package = ProgressionPackageData::new(
        &manager,
        &root,
        &globals_data,
        &localized_tags,
        &mut localized_cache,
    );
    let presentation_nodes = match scan_presentation_nodes(
        &mut progression_package,
        objectives.len(),
        &mut unlock_flag_definitions,
        &mut unlock_value_definitions,
    ) {
        Ok(nodes) => nodes,
        Err(error) => {
            progression_package_errors.push(format!("Presentation nodes: {error}"));
            Vec::new()
        }
    };
    attach_presentation_node_objective_owners(&mut objectives, &presentation_nodes);
    if let Err(error) = scan_milestone_objective_owners(&mut progression_package, &mut objectives) {
        progression_package_errors.push(format!("Milestone objective owners: {error}"));
    }
    if let Err(error) = scan_metric_objective_owners(
        &mut progression_package,
        &presentation_nodes,
        &mut objectives,
    ) {
        progression_package_errors.push(format!("Metric objective owners: {error}"));
    }
    if let Err(error) = scan_record_objective_owners(
        &mut progression_package,
        &presentation_nodes,
        &mut objectives,
        &mut unlock_flag_definitions,
        &mut unlock_value_definitions,
    ) {
        progression_package_errors.push(format!("Record objective owners: {error}"));
    }
    let location_contexts = match scan_location_condition_contexts(&mut progression_package) {
        Ok(locations) => locations,
        Err(error) => {
            progression_package_errors.push(format!("Location unlock conditions: {error}"));
            Vec::new()
        }
    };
    if let Err(error) = scan_activity_condition_contexts(
        &mut progression_package,
        &location_contexts,
        &mut unlock_flag_definitions,
        &mut unlock_value_definitions,
    ) {
        progression_package_errors.push(format!("Activity unlock conditions: {error}"));
    }
    let collectible_item_paths = retain_progression_scan(
        "Collectible item paths",
        scan_collectible_item_paths(&manager, &root, &presentation_nodes),
        &mut progression_package_errors,
    );
    let collectible_condition_contexts = retain_progression_scan(
        "Collectible unlock conditions",
        scan_collectible_condition_contexts(&manager, &root, &presentation_nodes),
        &mut progression_package_errors,
    );
    let pending_material_requirement_sets = retain_progression_scan(
        "Material requirement sets",
        scan_material_requirement_sets(&manager, &root),
        &mut progression_package_errors,
    );
    let pending_collectibles = if pending_material_requirement_sets.is_empty() {
        Vec::new()
    } else {
        retain_progression_scan(
            "Collectible definitions",
            scan_collectibles(
                &manager,
                &root,
                &presentation_nodes,
                &pending_material_requirement_sets,
            ),
            &mut progression_package_errors,
        )
    };
    let inventory_buckets = scan_inventory_bucket_descriptors(&manager, &root)?;
    let plug_set_table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 51 * 16)?))
        .map_err(|e| format!("Could not read reusable plug sets: {e}"))?;
    let item_table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 48 * 16)?))
        .map_err(|e| format!("Could not read item table: {e}"))?;
    let (count, rows, _) = array_at(&item_table, 8)?;
    let string_map = manager
        .read_tag(TagHash(u32_at(&globals_data, 16 + 33 * 16)?))
        .map_err(|e| format!("Could not read item strings: {e}"))?;
    let (string_count, string_rows, _) = array_at(&string_map, 8)?;
    if count != string_count {
        return Err("The installed item and string tables do not match".into());
    }

    let hashes: Vec<u64> = (0..count)
        .map(|i| u32_at(&item_table, rows + i * 24).map(u64::from))
        .collect::<Result<_, _>>()?;
    let definition_tags: Vec<u32> = (0..count)
        .map(|i| u32_at(&item_table, rows + i * 24 + 16))
        .collect::<Result<_, _>>()?;
    let progression_definitions = retain_progression_scan(
        "Progression definitions",
        scan_progression_definitions(
            &manager,
            &root,
            &globals_data,
            &localized_tags,
            &mut localized_cache,
            &hashes,
            &icon_containers_by_index,
        ),
        &mut progression_package_errors,
    );
    let ItemScan {
        items,
        names,
        type_names,
        package_item_names,
        package_item_type_names,
        descriptions,
        icon_containers,
        item_package_metadata,
        inventory_metadata,
        mut item_material_requirement_set_indices,
        diagnostics: item_scan_diagnostics,
    } = scan_items(
        ItemScanContext {
            manager: &manager,
            root: &root,
            hashes: &hashes,
            definition_tags: &definition_tags,
            string_map: &string_map,
            string_count,
            string_rows,
            plug_set_table: &plug_set_table,
            icon_containers_by_index: &icon_containers_by_index,
            inventory_buckets: &inventory_buckets,
            item_stat_definitions: &item_stat_definitions,
            stat_names: &stat_names,
            sandbox_perk_hashes: &sandbox_perk_hashes,
            ability_displays: &ability_displays,
            collectible_item_paths: &collectible_item_paths,
            collectible_condition_contexts: &collectible_condition_contexts,
            localized_tags: &localized_tags,
            localized_cache: &mut localized_cache,
            objectives: &mut objectives,
            unlock_flag_definitions: &mut unlock_flag_definitions,
            unlock_value_definitions: &mut unlock_value_definitions,
        },
        report,
    )?;
    sort_progression_contexts(&mut unlock_flag_definitions);
    sort_progression_contexts(&mut unlock_value_definitions);
    let material_requirement_sets =
        match materialize_material_requirement_sets(pending_material_requirement_sets, &hashes) {
            Ok(sets) => sets,
            Err(error) => {
                progression_package_errors.push(format!("Material requirement sets: {error}"));
                Vec::new()
            }
        };
    if material_requirement_sets.is_empty() {
        item_material_requirement_set_indices.clear();
    }
    let collectibles =
        match materialize_collectibles(pending_collectibles, &hashes, &names, &type_names) {
            Ok(collectibles) => collectibles,
            Err(error) => {
                progression_package_errors.push(format!("Collectible definitions: {error}"));
                Vec::new()
            }
        };
    item_scan_diagnostics.append_to(&mut progression_package_errors);
    let progression_package_error =
        (!progression_package_errors.is_empty()).then(|| progression_package_errors.join("\n"));
    let package_names = manager
        .package_paths
        .iter()
        .filter(|(_, package)| !package.name.trim().is_empty())
        .map(|(&package_id, package)| (package_id, package.name.clone()))
        .collect();
    Ok(CatalogContents {
        items,
        orbit_backdrops,
        orbit_map_entries,
        names,
        type_names,
        package_item_names,
        package_item_type_names,
        descriptions,
        icon_containers,
        item_package_metadata,
        item_stat_definitions,
        sandbox_perk_definitions,
        package_names,
        inventory_metadata,
        objectives,
        unlock_flag_definitions,
        unlock_value_definitions,
        collectibles,
        material_requirement_sets,
        item_material_requirement_set_indices,
        progression_definitions,
        progression_package_error,
        plug_pools: Vec::new(),
    })
}
