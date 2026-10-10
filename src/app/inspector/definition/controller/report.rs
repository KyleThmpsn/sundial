//! The Markdown report an inspected definition copies to the clipboard.
use super::*;

pub(super) fn hash_inspector_report(content: &HashInspectorContent<'_>) -> String {
    let mut report = String::new();
    append_report_overview(&mut report, content);
    append_report_source(&mut report, content);
    append_report_related_records(&mut report, content);
    append_report_item_data(&mut report, content);
    append_report_progression_data(&mut report, content);
    append_report_structure_data(&mut report, content);
    append_report_collection_data(&mut report, content);
    append_report_unlock_data(&mut report, content);
    report.trim_end().to_owned()
}

fn append_report_overview(report: &mut String, content: &HashInspectorContent<'_>) {
    report.push_str("# Sundial definition inspector report\n\n");
    report.push_str("## Overview\n\n| Field | Value |\n| --- | --- |\n");
    report_table_row(report, "Hash", &format_hash_hex_and_decimal(content.hash));
    report_table_row(
        report,
        "Name",
        content.resolved_name.as_deref().unwrap_or("Not resolved"),
    );
    report_table_row(
        report,
        "Catalog Locations",
        &content.match_count.to_string(),
    );
    report_table_row(
        report,
        "Sections",
        &hash_inspector_sections(content.matches)
            .into_iter()
            .map(HashInspectorSection::label)
            .collect::<Vec<_>>()
            .join(", "),
    );
    report.push_str("\n### Catalog locations\n\n| Location | Matches |\n| --- | ---: |\n");
    if content.match_groups.is_empty() {
        report.push_str("| None | 0 |\n");
    } else {
        for group in content.match_groups {
            report_table_row(report, group.label, &group.count.to_string());
        }
    }
}

fn append_report_source(report: &mut String, content: &HashInspectorContent<'_>) {
    let Some(context) = content.source_context else {
        return;
    };
    report.push_str("\n## Selected instance\n\n| Field | Value |\n| --- | --- |\n");
    report_table_row(report, "Opened from", &context.source);
    if let Some(instance_id) = &context.instance_id {
        report_table_row(report, "Instance", instance_id);
    }
    if let Some(level) = context.authored_level {
        report_table_row(report, "Authored level", &level.to_string());
        report_table_row(
            report,
            "Displayed Power",
            &crate::app::item_editor::displayed_item_power(level).to_string(),
        );
    }
    if let Some(flags) = context.flags {
        report_table_row(report, "Flags", &format!("0x{flags:02X} · {flags}"));
    }
    if let Some(plug_count) = context.plug_count {
        report_table_row(report, "Authored plugs", &plug_count.to_string());
    }
    append_report_json(
        report,
        "Opening-time source snapshot (not live state)",
        &serde_json::json!(context),
    );
}

fn append_report_related_records(report: &mut String, content: &HashInspectorContent<'_>) {
    let records = related_catalog_records(content);
    if records.is_empty() {
        return;
    }
    report.push_str(
        "\n## Related records\n\n| Kind | Name | Hash | Current state |\n| --- | --- | --- | --- |\n",
    );
    for record in records {
        report.push_str("| ");
        report.push_str(&markdown_cell(record.kind));
        report.push_str(" | ");
        report.push_str(&markdown_cell(&record.label));
        report.push_str(" | ");
        report.push_str(&format_hash_hex(record.hash));
        report.push_str(" | ");
        report.push_str(&markdown_cell(
            record
                .state
                .as_ref()
                .map_or("N/A", |state| state.text.as_str()),
        ));
        report.push_str(" |\n");
    }
}

fn append_report_item_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    let has_item_data = matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.inventory_metadata.is_some()
        || matches.item_stat_definition.is_some()
        || !matches.investment_stat_references.is_empty()
        || !matches.bucket_items.is_empty();
    if !has_item_data {
        return;
    }
    let investment_references = matches
        .investment_stat_references
        .iter()
        .map(|(item_hash, stat)| {
            serde_json::json!({
                "item_hash": item_hash,
                "item_name": content.catalog.package_item_name(*item_hash),
                "stat": stat,
            })
        })
        .collect::<Vec<_>>();
    let bucket_items = matches
        .bucket_items
        .iter()
        .map(|item| serde_json::json!({ "definition": item }))
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "item_definition": matches.item,
        "package_metadata": matches.item_package_metadata,
        "inventory_metadata": matches.inventory_metadata,
        "material_requirement_set_indices": matches.item_material_requirement_set_indices,
        "item_stat_definition": matches.item_stat_definition,
        "resolved_stat_group": content.catalog.item_stat_group(content.hash),
        "resolved_socket_pools": matches.item.map(|item| super::super::item_details::resolved_socket_pools(content.catalog, item)),
        "resolved_item_traits": matches.item_package_metadata.map(|metadata| metadata.trait_indices.iter().map(|index| serde_json::json!({
            "index": index, "definition": content.catalog.trait_definitions().get(usize::from(*index)),
        })).collect::<Vec<_>>()),
        "investment_stat_references": investment_references,
        "inventory_bucket_items": bucket_items,
    });
    append_report_json(report, "Item package data", &data);
}

fn append_report_progression_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    let has_progression_data = !matches.progression_definitions.is_empty()
        || !matches.progression_reward_matches.is_empty()
        || !matches.progression_faction_matches.is_empty()
        || !matches.objectives.is_empty()
        || !matches.owner_matches.is_empty()
        || !matches.trait_matches.is_empty()
        || !matches.context_matches.is_empty()
        || !matches.record_matches.is_empty()
        || !matches.record_references.is_empty()
        || !matches.artifact_mods.is_empty()
        || matches.season_pass_reward.is_some()
        || matches.mission_scenario.is_some();
    if !has_progression_data {
        return;
    }
    let definitions = matches
        .progression_definitions
        .iter()
        .map(|(index, definition)| serde_json::json!({ "index": index, "definition": definition }))
        .collect::<Vec<_>>();
    let rewards = matches
        .progression_reward_matches
        .iter()
        .map(|(index, definition, reward_index)| {
            serde_json::json!({
                "progression_index": index,
                "progression": definition,
                "reward_index": reward_index,
                "matched_reward": definition.reward_items[*reward_index],
            })
        })
        .collect::<Vec<_>>();
    let factions = matches
        .progression_faction_matches
        .iter()
        .map(|(index, definition, faction_index, faction)| {
            serde_json::json!({
                "progression_index": index,
                "progression": definition,
                "faction_index": faction_index,
                "matched_faction": faction,
            })
        })
        .collect::<Vec<_>>();
    let objectives = matches
        .objectives
        .iter()
        .map(|(index, objective)| serde_json::json!({ "index": index, "objective": objective }))
        .collect::<Vec<_>>();
    let owners = matches
        .owner_matches
        .iter()
        .map(|(objective_index, objective, owner)| {
            serde_json::json!({
                "objective_index": objective_index,
                "objective": objective,
                "matched_owner": owner,
            })
        })
        .collect::<Vec<_>>();
    let traits = matches
        .trait_matches
        .iter()
        .map(|(objective_index, objective, owner, trait_definition)| {
            serde_json::json!({
                "objective_index": objective_index,
                "objective": objective,
                "owner": owner,
                "matched_trait": trait_definition,
            })
        })
        .collect::<Vec<_>>();
    let readers = matches
        .context_matches
        .iter()
        .map(|(kind, definition_index, context)| {
            serde_json::json!({
                "source_kind": kind,
                "source_definition_index": definition_index,
                "matched_reader": context,
            })
        })
        .collect::<Vec<_>>();
    let records = matches
        .record_matches
        .iter()
        .map(|(index, record)| serde_json::json!({ "index": index, "record": record }))
        .collect::<Vec<_>>();
    let record_references = matches
        .record_references
        .iter()
        .map(|(index, record, kind)| {
            serde_json::json!({ "index": index, "record": record, "reference": kind })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "progression_definitions": definitions,
        "reward_references": rewards,
        "faction_references": factions,
        "objectives": objectives,
        "objective_owner_references": owners,
        "objective_trait_references": traits,
        "progression_readers": readers,
        "records": records,
        "record_references": record_references,
        "artifact_mods": matches.artifact_mods,
        "season_pass_reward": matches.season_pass_reward.map(|grant| grant.label()),
        "dawn_mission_scenario": matches.mission_scenario,
        "dawn_activity": content.document.and_then(|document| document.get("_dawn_activity")),
    });
    append_report_json(report, "Progression package data", &data);
}

fn append_report_structure_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    if matches.item_stat_group.is_none() && matches.power_cap_definition.is_none() {
        return;
    }
    let data = serde_json::json!({
        "stat_group": matches.item_stat_group.map(|(index, group)| serde_json::json!({ "index": index, "group": group })),
        "stat_group_items": matches.stat_group_items.iter().map(|hash| format_hash_hex(*hash)).collect::<Vec<_>>(),
        "power_cap": matches.power_cap_definition.map(|(index, cap)| serde_json::json!({ "index": index, "definition": cap })),
        "power_cap_items": matches.power_cap_items.iter().map(|hash| format_hash_hex(*hash)).collect::<Vec<_>>(),
    });
    append_report_json(report, "Item structure data", &data);
}

fn append_report_collection_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    if matches.collectible_matches.is_empty() && matches.material_requirement_set_matches.is_empty()
    {
        return;
    }
    let collectibles = matches
        .collectible_matches
        .iter()
        .map(|collectible| {
            let state = content.collection_state().map(|snapshot| {
                crate::app::collections_page::collectible_state(
                    collectible,
                    snapshot,
                    content.catalog,
                )
                .0
            });
            serde_json::json!({ "definition": collectible, "current_state": state })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({
        "collectibles": collectibles,
        "material_requirement_sets": matches.material_requirement_set_matches,
    });
    append_report_json(report, "Collections package data", &data);
}

fn append_report_unlock_data(report: &mut String, content: &HashInspectorContent<'_>) {
    let matches = content.matches;
    if matches.flag_definitions.is_empty() && matches.value_definitions.is_empty() {
        return;
    }
    let flags = matches
        .flag_definitions
        .iter()
        .map(|(index, definition)| {
            let state = content
                .collection_state()
                .map(|snapshot| snapshot.flag_text(*index, definition));
            serde_json::json!({ "index": index, "definition": definition, "current_state": state })
        })
        .collect::<Vec<_>>();
    let values = matches
        .value_definitions
        .iter()
        .map(|(index, definition)| {
            let state = content
                .collection_state()
                .map(|snapshot| snapshot.value_text(*index, definition));
            serde_json::json!({ "index": index, "definition": definition, "current_state": state })
        })
        .collect::<Vec<_>>();
    let data = serde_json::json!({ "flag_definitions": flags, "value_definitions": values });
    append_report_json(report, "Unlock package data", &data);
}

fn append_report_json<T: serde::Serialize>(report: &mut String, heading: &str, value: &T) {
    report.push_str("\n## ");
    report.push_str(heading);
    report.push_str("\n\n```json\n");
    match serde_json::to_string_pretty(value) {
        Ok(json) => report.push_str(&json),
        Err(error) => report.push_str(&format!("{{\"serialization_error\":\"{error}\"}}")),
    }
    report.push_str("\n```\n");
}

fn report_table_row(report: &mut String, label: &str, value: &str) {
    report.push_str("| ");
    report.push_str(&markdown_cell(label));
    report.push_str(" | ");
    report.push_str(&markdown_cell(value));
    report.push_str(" |\n");
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}
