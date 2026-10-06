//! Finalize payload sizes and dependency enrollment before releasing sources and emitting.
use super::*;

pub(super) struct Output {
    /// The art-dye table with a row for each custom shader dye.
    pub dye_table: Option<ReplacementSpec>,
    pub lore: Option<lore::Plan>,
    pub assets: assets::Plan,
    pub icons: assets::IconRows,
    pub runtime: runtime::Payloads,
    pub tables: tables::WeaponTables,
    pub collections: collections::Tables,
    pub localization: AuthoredLocalization,
    pub has_custom_plugs: bool,
    /// The sandbox banks with a charge row added, by package, replaced only when a private
    /// perk applies the key.
    pub ability_banks: BTreeMap<u16, Vec<ReplacementSpec>>,
    /// The HUD status table and name bank, when a private perk shows a HUD status of its own.
    pub hud_statuses: Vec<ReplacementSpec>,
    /// The stat group table, when a weapon has a stat group of its own.
    pub stat_group_table: Option<ReplacementSpec>,
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
            &mut self.tables.subclass.lists,
            &mut self.tables.subclass.displays,
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
) -> AuthoringResult<(emission::PackageEmission, PackageManager)> {
    output.synchronize_payloads()?;
    let Output {
        dye_table,
        lore,
        assets,
        icons,
        runtime,
        tables,
        collections,
        localization,
        has_custom_plugs,
        ability_banks,
        hud_statuses,
        stat_group_table,
    } = output;
    if localization.donor_header_tag.pkg_id() != sources.table_tags.localized_index_tag.pkg_id()
        || localization.locale_data.iter().any(|locale| {
            locale.donor_tag.pkg_id() != sources.table_tags.localized_index_tag.pkg_id()
        })
    {
        return Err(invalid(
            "Project localization header and locale data unexpectedly have different package owners",
        ));
    }
    let runtime_dependencies = runtime.dependencies(
        &sources.manager,
        &assets,
        sources.table_tags.entity_assignment_tag,
    )?;
    let subclass_lists_added = tables.subclass.count()? != sources.subclass_tables.count()?;

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
    let emission = emission::PackageEmission {
        lore,
        hud_table: assets.hud_table,
        ability_banks,
        hud_statuses,
        table_tags: sources.table_tags,
        has_custom_plugs,
        watermark_layer_tag: assets.watermark.watermark_layer_tag,
        watermarked_icon_containers: assets.watermark.icon_container_tags,
        watermark_reference_overrides: assets.watermark.reference_overrides,
        badge_icon_tag: assets.badge.container_tag,
        asset_packages: runtime.asset_packages,
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
        subclass_tables: subclass_lists_added.then_some(tables.subclass),
        dye_table,
        stat_group_table,
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
    };
    // Emission reads the same packages, and releases them before writing its own.
    Ok((emission, sources.manager))
}
