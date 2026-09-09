//! Finalize payload sizes and dependency enrollment before releasing sources and emitting.
use super::*;

pub(super) struct Output {
    pub assets: assets::Plan,
    pub icons: assets::IconRows,
    pub runtime: runtime::Payloads,
    pub tables: tables::WeaponTables,
    pub collections: collections::Tables,
    pub localization: AuthoredLocalization,
    pub has_custom_plugs: bool,
}

impl Output {
    fn synchronize_payloads(&mut self) -> AuthoringResult<()> {
        // Retain the original validation order so malformed inputs fail at the same boundary.
        for payload in [
            &mut self.tables.item_table,
            &mut self.tables.item_strings,
            &mut self.tables.item_hash_index,
            &mut self.tables.item_metadata,
            &mut self.tables.item_metadata_index,
            &mut self.tables.sandbox_patterns,
            &mut self.tables.sandbox_pattern_index,
            &mut self.icons.payload,
            &mut self.tables.dense,
            &mut self.tables.collectibles,
            &mut self.tables.collectible_displays,
            &mut self.collections.nodes,
            &mut self.collections.node_strings,
            &mut self.collections.objectives,
            &mut self.collections.objective_strings,
            &mut self.collections.records,
            &mut self.collections.record_strings,
            &mut self.collections.pools,
            &mut self.tables.unlocks,
            &mut self.tables.unlock_banks,
            &mut self.tables.unlock_displays,
            &mut self.runtime.entity_assignments,
            &mut self.runtime.finished_sandbox_perks,
            &mut self.runtime.sandbox_perk_indices,
        ] {
            *payload = synchronize_payload_size(std::mem::take(payload))?;
        }
        for tag in self
            .tables
            .definitions
            .iter_mut()
            .chain(&mut self.tables.authored_strings)
        {
            tag.payload = synchronize_payload_size(std::mem::take(&mut tag.payload))?;
        }
        self.localization.index =
            synchronize_payload_size(std::mem::take(&mut self.localization.index))?;
        self.localization.merged_header =
            synchronize_payload_size(std::mem::take(&mut self.localization.merged_header))?;
        for locale in &mut self.localization.locale_data {
            locale.payload = synchronize_payload_size(std::mem::take(&mut locale.payload))?;
        }
        Ok(())
    }
}

pub(super) fn prepare(
    sources: sources::ProjectSources,
    mut output: Output,
) -> AuthoringResult<emission::PackageEmission> {
    output.synchronize_payloads()?;
    let Output {
        assets,
        icons,
        runtime,
        tables,
        collections,
        localization,
        has_custom_plugs,
    } = output;
    if localization.donor_header_tag.pkg_id() != sources.localized_index_tag.pkg_id()
        || localization
            .locale_data
            .iter()
            .any(|locale| locale.donor_tag.pkg_id() != sources.localized_index_tag.pkg_id())
    {
        return Err(invalid(
            "Project localization header and locale data unexpectedly have different package owners",
        ));
    }
    let runtime_dependencies =
        runtime.dependencies(&sources.manager, &assets, sources.entity_assignment_tag)?;
    // The package writer must not retain the source manager's open file handles.
    drop(sources.manager);

    let mut host_new_tags = Vec::new();
    for (definition, strings) in tables.definitions.into_iter().zip(tables.authored_strings) {
        host_new_tags.push(definition);
        host_new_tags.push(strings);
    }
    if host_new_tags.len() != assets.weapon_tag_count {
        return Err(validation(
            "Authored host weapon-tag count did not converge",
        ));
    }
    host_new_tags.extend(assets.watermark.new_tags);
    if HOST_EXPECTED_ENTRY_COUNT + host_new_tags.len() != assets.weapon_runtime_start {
        return Err(validation(
            "Authored host runtime-tag allocation did not converge",
        ));
    }
    host_new_tags.extend(runtime.weapon_tags);
    Ok(emission::PackageEmission {
        hud_table: assets.hud_table,
        item_table_tag: sources.item_table_tag,
        item_hash_index_table_tag: sources.item_hash_index_table_tag,
        item_string_table_tag: sources.item_string_table_tag,
        item_metadata_table_tag: sources.item_metadata_table_tag,
        sandbox_pattern_table_tag: sources.sandbox_pattern_table_tag,
        finished_sandbox_perk_table_tag: sources.finished_sandbox_perk_table_tag,
        sandbox_perk_index_table_tag: sources.sandbox_perk_index_table_tag,
        item_icon_table_tag: sources.item_icon_table_tag,
        item_dense_presentation_table_tag: sources.item_dense_presentation_table_tag,
        item_metadata_index_table_tag: sources.item_metadata_index_table_tag,
        sandbox_pattern_index_table_tag: sources.sandbox_pattern_index_table_tag,
        collectible_table_tag: sources.collectible_table_tag,
        collectible_display_table_tag: sources.collectible_display_table_tag,
        objective_table_tag: sources.objective_table_tag,
        objective_string_table_tag: sources.objective_string_table_tag,
        record_table_tag: sources.record_table_tag,
        record_string_table_tag: sources.record_string_table_tag,
        presentation_node_table_tag: sources.presentation_node_table_tag,
        presentation_node_string_table_tag: sources.presentation_node_string_table_tag,
        shared_expression_pool_table_tag: sources.shared_expression_pool_table_tag,
        localized_index_tag: sources.localized_index_tag,
        unlock_flag_bank_table_tag: sources.unlock_flag_bank_table_tag,
        unlock_table_tag: sources.unlock_table_tag,
        unlock_display_tag: sources.unlock_display_tag,
        entity_assignment_tag: sources.entity_assignment_tag,
        has_custom_plugs,
        watermark_layer_tag: assets.watermark.watermark_layer_tag,
        watermarked_icon_containers: assets.watermark.icon_container_tags,
        watermark_reference_overrides: assets.watermark.reference_overrides,
        badge_icon_tag: assets.badge.container_tag,
        asset_packages: crate::asset_packages::AssetPackages::primary(
            assets.badge.new_tags,
            assets.badge.reference_overrides,
        )?,
        private_perk_runtime_append_start: runtime.private_perk_append_start,
        private_perk_runtime_new_tags: runtime.private_perk_tags,
        entity_assignments: runtime.entity_assignments,
        finished_sandbox_perks: runtime.finished_sandbox_perks,
        sandbox_perk_indices: runtime.sandbox_perk_indices,
        localization,
        item_table: tables.item_table,
        item_strings: tables.item_strings,
        item_hash_index: tables.item_hash_index,
        item_metadata: tables.item_metadata,
        item_metadata_index: tables.item_metadata_index,
        sandbox_patterns: tables.sandbox_patterns,
        sandbox_pattern_index: tables.sandbox_pattern_index,
        dense: tables.dense,
        collectibles: tables.collectibles,
        collectible_displays: tables.collectible_displays,
        unlocks: tables.unlocks,
        unlock_banks: tables.unlock_banks,
        unlock_displays: tables.unlock_displays,
        plans: tables.plans,
        any_sandbox_pattern: tables.any_sandbox_pattern,
        nodes: collections.nodes,
        node_strings: collections.node_strings,
        objective_strings: collections.objective_strings,
        records: collections.records,
        record_strings: collections.record_strings,
        objectives: collections.objectives,
        pools: collections.pools,
        item_icons: icons.payload,
        runtime_dependencies,
        host_new_tags,
    })
}
