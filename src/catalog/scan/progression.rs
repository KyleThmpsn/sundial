//! Optional progression reads and joins that retain partial-scan diagnostics.
use super::{retain_progression_enrichment, retain_progression_scan, sources::Sources};
use crate::catalog::collections::{
    CollectibleDef, MaterialRequirementSetDef, PendingCollectibleDef,
    PendingMaterialRequirementSet, materialize_collectibles, materialize_material_requirement_sets,
    scan_collectibles, scan_material_requirement_sets, scan_shared_expression_pool,
};
use crate::catalog::progression::{
    PendingProgressionContext, PresentationNodeDef, ProgressionPackageData,
    attach_presentation_node_objective_owners, scan_activity_condition_contexts,
    scan_collectible_condition_contexts, scan_collectible_item_paths,
    scan_location_condition_contexts, scan_metric_objective_owners,
    scan_milestone_objective_owners, scan_objectives, scan_presentation_nodes,
    scan_record_objective_owners, scan_trait_definitions, scan_unlock_flag_definitions,
    scan_unlock_flag_displays, scan_unlock_value_definitions,
};
use crate::catalog::{
    CatalogProgress, CollectionConditionTokenDef, ObjectiveDef, ObjectiveOwnerTraitDef,
    UnlockDefinition,
};
use crate::investment_localization::LocalizedStringCache;
use std::collections::HashMap;

pub(super) struct ProgressionScan {
    pub shared_expression_pool: Vec<Vec<CollectionConditionTokenDef>>,
    pub unlock_flag_definitions: Vec<UnlockDefinition>,
    pub unlock_value_definitions: Vec<UnlockDefinition>,
    pub objectives: Vec<ObjectiveDef>,
    pub trait_definitions: Vec<ObjectiveOwnerTraitDef>,
    pub collectible_item_paths: HashMap<usize, Vec<Vec<String>>>,
    pub collectible_condition_contexts: HashMap<usize, Vec<PendingProgressionContext>>,
    pub pending_material_requirement_sets: Vec<PendingMaterialRequirementSet>,
    pub pending_collectibles: Vec<PendingCollectibleDef>,
    pub errors: Vec<String>,
}

pub(super) fn read(
    sources: &Sources,
    localized_cache: &mut LocalizedStringCache,
    report: &mut dyn FnMut(CatalogProgress),
) -> ProgressionScan {
    let Sources {
        manager,
        root,
        globals_data,
        localized_tags,
    } = sources;
    let mut progression_package_errors = Vec::new();
    let shared_expression_pool = retain_progression_scan(
        "Shared unlock expression pool",
        scan_shared_expression_pool(manager, root),
        &mut progression_package_errors,
    );
    report(CatalogProgress::stage("Reading unlock definitions…"));
    let mut unlock_flag_definitions = retain_progression_scan(
        "Unlock flag definitions",
        scan_unlock_flag_definitions(manager, root),
        &mut progression_package_errors,
    );
    if !unlock_flag_definitions.is_empty() {
        let display_result = scan_unlock_flag_displays(
            manager,
            globals_data,
            localized_tags,
            localized_cache,
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
        scan_unlock_value_definitions(manager, root),
        &mut progression_package_errors,
    );
    report(CatalogProgress::stage("Reading objective definitions…"));
    let mut objectives = retain_progression_scan(
        "Objective definitions",
        scan_objectives(
            manager,
            root,
            globals_data,
            localized_tags,
            localized_cache,
            &mut unlock_flag_definitions,
            &mut unlock_value_definitions,
        ),
        &mut progression_package_errors,
    );
    let mut progression_package =
        ProgressionPackageData::new(manager, root, globals_data, localized_tags, localized_cache);
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
    let trait_definitions = retain_progression_scan(
        "Trait definitions",
        scan_trait_definitions(&mut progression_package),
        &mut progression_package_errors,
    );
    enrich_links(
        &mut progression_package,
        &mut objectives,
        &presentation_nodes,
        &trait_definitions,
        &mut unlock_flag_definitions,
        &mut unlock_value_definitions,
        &mut progression_package_errors,
    );
    let collectible_item_paths = retain_progression_scan(
        "Collectible item paths",
        scan_collectible_item_paths(manager, root, &presentation_nodes),
        &mut progression_package_errors,
    );
    let collectible_condition_contexts = retain_progression_scan(
        "Collectible unlock conditions",
        scan_collectible_condition_contexts(manager, root, &presentation_nodes),
        &mut progression_package_errors,
    );
    let pending_material_requirement_sets = retain_progression_scan(
        "Material requirement sets",
        scan_material_requirement_sets(manager, root),
        &mut progression_package_errors,
    );
    let material_requirement_enrichment = (!pending_material_requirement_sets.is_empty())
        .then_some(pending_material_requirement_sets.as_slice());
    let pending_collectibles = retain_progression_scan(
        "Collectible definitions",
        scan_collectibles(
            manager,
            root,
            &presentation_nodes,
            material_requirement_enrichment,
        ),
        &mut progression_package_errors,
    );
    ProgressionScan {
        shared_expression_pool,
        unlock_flag_definitions,
        unlock_value_definitions,
        objectives,
        trait_definitions,
        collectible_item_paths,
        collectible_condition_contexts,
        pending_material_requirement_sets,
        pending_collectibles,
        errors: progression_package_errors,
    }
}

fn enrich_links(
    progression_package: &mut ProgressionPackageData<'_>,
    objectives: &mut [ObjectiveDef],
    presentation_nodes: &[PresentationNodeDef],
    trait_definitions: &[ObjectiveOwnerTraitDef],
    unlock_flag_definitions: &mut [UnlockDefinition],
    unlock_value_definitions: &mut [UnlockDefinition],
    errors: &mut Vec<String>,
) {
    attach_presentation_node_objective_owners(objectives, presentation_nodes);
    if let Err(error) = scan_milestone_objective_owners(progression_package, objectives) {
        errors.push(format!("Milestone objective owners: {error}"));
    }
    if let Err(error) = scan_metric_objective_owners(
        progression_package,
        presentation_nodes,
        objectives,
        trait_definitions,
    ) {
        errors.push(format!("Metric objective owners: {error}"));
    }
    if let Err(error) = scan_record_objective_owners(
        progression_package,
        presentation_nodes,
        objectives,
        unlock_flag_definitions,
        unlock_value_definitions,
    ) {
        errors.push(format!("Record objective owners: {error}"));
    }
    let location_contexts = match scan_location_condition_contexts(progression_package) {
        Ok(locations) => locations,
        Err(error) => {
            errors.push(format!("Location unlock conditions: {error}"));
            Vec::new()
        }
    };
    if let Err(error) = scan_activity_condition_contexts(
        progression_package,
        &location_contexts,
        unlock_flag_definitions,
        unlock_value_definitions,
    ) {
        errors.push(format!("Activity unlock conditions: {error}"));
    }
}

pub(super) struct ResolvedCollections {
    pub material_requirement_sets: Vec<MaterialRequirementSetDef>,
    pub collectibles: Vec<CollectibleDef>,
}

pub(super) fn resolve_collections(
    pending_material_requirement_sets: Vec<PendingMaterialRequirementSet>,
    pending_collectibles: Vec<PendingCollectibleDef>,
    hashes: &[u64],
    items: &mut crate::catalog::items::ItemScan,
    errors: &mut Vec<String>,
) -> ResolvedCollections {
    let material_requirement_sets =
        match materialize_material_requirement_sets(pending_material_requirement_sets, hashes) {
            Ok(sets) => sets,
            Err(error) => {
                errors.push(format!("Material requirement sets: {error}"));
                Vec::new()
            }
        };
    if material_requirement_sets.is_empty() {
        items.item_material_requirement_set_indices.clear();
    }
    let collectibles = match materialize_collectibles(
        pending_collectibles,
        hashes,
        &items.names,
        &items.type_names,
    ) {
        Ok(collectibles) => collectibles,
        Err(error) => {
            errors.push(format!("Collectible definitions: {error}"));
            Vec::new()
        }
    };
    ResolvedCollections {
        material_requirement_sets,
        collectibles,
    }
}
