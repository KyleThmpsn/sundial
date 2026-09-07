//! Load and validate the immutable source generation before resolving any weapon.
use super::*;

pub(super) struct ProjectSources {
    pub(super) manager: PackageManager,
    pub(super) globals_data: Vec<u8>,
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
    pub(super) stock_item_table: Vec<u8>,
    pub(super) stock_item_strings: Vec<u8>,
    pub(super) stock_item_metadata: Vec<u8>,
    pub(super) stock_item_hash_index: Vec<u8>,
    pub(super) stock_sandbox_patterns: Vec<u8>,
    pub(super) stock_finished_sandbox_perks: Vec<u8>,
    pub(super) stock_sandbox_perk_indices: Vec<u8>,
    pub(super) stock_item_icons: Vec<u8>,
    pub(super) stock_dense: Vec<u8>,
    pub(super) stock_metadata_index: Vec<u8>,
    pub(super) stock_sandbox_pattern_index: Vec<u8>,
    pub(super) stock_collectibles: Vec<u8>,
    pub(super) stock_collectible_displays: Vec<u8>,
    pub(super) stock_objectives: Vec<u8>,
    pub(super) stock_objective_strings: Vec<u8>,
    pub(super) stock_records: Vec<u8>,
    pub(super) stock_record_strings: Vec<u8>,
    pub(super) stock_nodes: Vec<u8>,
    pub(super) stock_node_strings: Vec<u8>,
    pub(super) stock_pools: Vec<u8>,
    pub(super) localized_index: Vec<u8>,
    pub(super) stock_unlock_banks: Vec<u8>,
    pub(super) stock_unlocks: Vec<u8>,
    pub(super) stock_unlock_displays: Vec<u8>,
    pub(super) stock_entity_assignments: Vec<u8>,
    pub(super) stock_item_count: usize,
    pub(super) item_rows: usize,
    pub(super) stock_item_rows_by_hash: BTreeMap<u32, Vec<usize>>,
    pub(super) string_rows: usize,
    pub(super) stock_collectible_count: usize,
    pub(super) collectible_rows: usize,
    pub(super) stock_unlock_count: usize,
    pub(super) unlock_rows: usize,
}

pub(super) fn load_project_sources(package_directory: &Path) -> AuthoringResult<ProjectSources> {
    let manager = open_manager(package_directory)?;
    let globals = resolve_live_named_tag(&manager, "investment_globals", None).map_err(invalid)?;
    let globals_data = read_tag(&manager, globals, "investment globals")?;
    let root_tag = globals_child_tag(&globals_data, 0)?;
    let root_entry = manager
        .get_entry(root_tag)
        .ok_or_else(|| invalid(format!("Investment root {root_tag} is not live")))?;
    if root_entry.reference != INVESTMENT_ROOT_CLASS {
        return Err(invalid(format!(
            "Investment root {root_tag} has class 0x{:08X}, expected 0x{INVESTMENT_ROOT_CLASS:08X}",
            root_entry.reference
        )));
    }
    let root = read_tag(&manager, root_tag, "investment root")?;
    let item_table_tag = root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT)?;
    let item_hash_index_table_tag = root_child_tag(&root, ROOT_ITEM_HASH_INDEX_TABLE_SLOT)?;
    let item_hash_index_entry = manager
        .get_entry(item_hash_index_table_tag)
        .ok_or_else(|| {
            invalid(format!(
                "Item hash-index table {item_hash_index_table_tag} is not live"
            ))
        })?;
    if item_hash_index_entry.reference != ITEM_HASH_INDEX_TABLE_CLASS {
        return Err(invalid(format!(
            "Item hash-index table {item_hash_index_table_tag} has class 0x{:08X}, expected 0x{ITEM_HASH_INDEX_TABLE_CLASS:08X}",
            item_hash_index_entry.reference
        )));
    }
    let item_string_table_tag = globals_child_tag(&globals_data, GLOBALS_ITEM_STRING_TABLE_SLOT)?;
    let item_metadata_table_tag =
        globals_child_tag(&globals_data, GLOBALS_ITEM_METADATA_TABLE_SLOT)?;
    let sandbox_pattern_table_tag =
        globals_child_tag(&globals_data, GLOBALS_SANDBOX_PATTERN_TABLE_SLOT)?;
    let finished_sandbox_perk_table_tag =
        globals_child_tag(&globals_data, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)?;
    let sandbox_perk_index_table_tag = root_child_tag(&root, ROOT_SANDBOX_PERK_INDEX_TABLE_SLOT)?;
    let sandbox_perk_index_entry =
        manager
            .get_entry(sandbox_perk_index_table_tag)
            .ok_or_else(|| {
                invalid(format!(
                    "Sandbox-perk metadata table {sandbox_perk_index_table_tag} is not live"
                ))
            })?;
    if sandbox_perk_index_entry.reference != SANDBOX_PERK_INDEX_CATALOG_CLASS {
        return Err(invalid(format!(
            "Sandbox-perk metadata table {sandbox_perk_index_table_tag} has class 0x{:08X}, expected 0x{SANDBOX_PERK_INDEX_CATALOG_CLASS:08X}",
            sandbox_perk_index_entry.reference
        )));
    }
    let item_icon_table_tag = globals_child_tag(&globals_data, GLOBALS_ITEM_ICON_TABLE_SLOT)?;
    let item_dense_presentation_table_tag =
        globals_child_tag(&globals_data, GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT)?;
    let item_metadata_index_table_tag = root_child_tag(&root, ROOT_ITEM_METADATA_INDEX_TABLE_SLOT)?;
    let sandbox_pattern_index_table_tag =
        root_child_tag(&root, ROOT_SANDBOX_PATTERN_INDEX_TABLE_SLOT)?;
    let collectible_table_tag = root_child_tag(&root, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT)?;
    let collectible_display_table_tag =
        globals_child_tag(&globals_data, GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT)?;
    let objective_table_tag = root_child_tag(&root, ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT)?;
    let objective_string_table_tag =
        globals_child_tag(&globals_data, GLOBALS_OBJECTIVE_STRING_TABLE_SLOT)?;
    let record_table_tag = root_child_tag(&root, ROOT_RECORD_DEFINITION_TABLE_SLOT)?;
    let record_string_table_tag =
        globals_child_tag(&globals_data, GLOBALS_RECORD_STRING_TABLE_SLOT)?;
    let presentation_node_table_tag =
        root_child_tag(&root, ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT)?;
    let presentation_node_string_table_tag =
        globals_child_tag(&globals_data, GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT)?;
    let shared_expression_pool_table_tag =
        root_child_tag(&root, ROOT_SHARED_EXPRESSION_POOL_TABLE_SLOT)?;
    let material_requirement_table_tag =
        root_child_tag(&root, ROOT_MATERIAL_REQUIREMENT_TABLE_SLOT)?;
    let localized_index_tag =
        globals_child_tag(&globals_data, GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT)?;
    let unlock_flag_bank_table_tag = root_child_tag(&root, ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT)?;
    let unlock_table_tag = root_child_tag(&root, ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT)?;
    let unlock_display_tag =
        globals_child_tag(&globals_data, GLOBALS_UNLOCK_FLAG_DISPLAY_TABLE_SLOT)?;
    let entity_assignment_tag = TagHash(SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG);
    let entity_assignment_entry = manager.get_entry(entity_assignment_tag).ok_or_else(|| {
        invalid(format!(
            "Sandbox-pattern entity-assignment tag {entity_assignment_tag} is not live"
        ))
    })?;
    if entity_assignment_entry.reference != SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS {
        return Err(invalid(format!(
            "Sandbox-pattern entity-assignment tag {entity_assignment_tag} has class 0x{:08X}, expected 0x{SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS:08X}",
            entity_assignment_entry.reference
        )));
    }
    let sandbox_perk_runtime_map_tag = TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG);
    let sandbox_perk_runtime_map_entry = manager
        .get_entry(sandbox_perk_runtime_map_tag)
        .ok_or_else(|| {
            invalid(format!(
                "Sandbox-perk runtime map {sandbox_perk_runtime_map_tag} is not live"
            ))
        })?;
    if sandbox_perk_runtime_map_entry.reference != SANDBOX_PERK_RUNTIME_MAP_CLASS {
        return Err(invalid(format!(
            "Sandbox-perk runtime map {sandbox_perk_runtime_map_tag} has class 0x{:08X}, expected 0x{SANDBOX_PERK_RUNTIME_MAP_CLASS:08X}",
            sandbox_perk_runtime_map_entry.reference
        )));
    }
    if sandbox_perk_runtime_map_tag != entity_assignment_tag
        || SANDBOX_PERK_RUNTIME_MAP_CLASS != SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS
    {
        return Err(invalid(
            "Sandbox-pattern and sandbox-perk assignments no longer share the expected native table",
        ));
    }

    validate_project_package_owners(
        &[
            unlock_display_tag,
            item_icon_table_tag,
            item_dense_presentation_table_tag,
        ],
        &[
            item_table_tag,
            collectible_table_tag,
            collectible_display_table_tag,
        ],
        &[
            item_string_table_tag,
            item_metadata_table_tag,
            sandbox_pattern_table_tag,
            finished_sandbox_perk_table_tag,
            presentation_node_string_table_tag,
            objective_string_table_tag,
            record_string_table_tag,
        ],
        &[
            unlock_table_tag,
            presentation_node_table_tag,
            objective_table_tag,
            record_table_tag,
            shared_expression_pool_table_tag,
            unlock_flag_bank_table_tag,
            item_metadata_index_table_tag,
            sandbox_perk_index_table_tag,
            sandbox_pattern_index_table_tag,
        ],
    )?;
    let stock_item_table = read_tag(&manager, item_table_tag, "item table")?;
    let stock_item_strings = read_tag(&manager, item_string_table_tag, "item-string table")?;
    let stock_item_metadata = read_tag(&manager, item_metadata_table_tag, "item-metadata table")?;
    let stock_item_hash_index =
        read_tag(&manager, item_hash_index_table_tag, "item hash-index table")?;
    let stock_sandbox_patterns =
        read_tag(&manager, sandbox_pattern_table_tag, "sandbox-pattern table")?;
    let stock_finished_sandbox_perks = read_tag(
        &manager,
        finished_sandbox_perk_table_tag,
        "finished sandbox-perk table",
    )?;
    let stock_sandbox_perk_indices = read_tag(
        &manager,
        sandbox_perk_index_table_tag,
        "sandbox-perk metadata table",
    )?;
    let stock_item_icons = read_tag(&manager, item_icon_table_tag, "item-icon table")?;
    let stock_dense = read_tag(
        &manager,
        item_dense_presentation_table_tag,
        "dense item-presentation table",
    )?;
    let stock_metadata_index = read_tag(
        &manager,
        item_metadata_index_table_tag,
        "item-metadata index table",
    )?;
    let stock_sandbox_pattern_index = read_tag(
        &manager,
        sandbox_pattern_index_table_tag,
        "sandbox-pattern index table",
    )?;
    let stock_collectibles = read_tag(&manager, collectible_table_tag, "collectible table")?;
    let stock_collectible_displays = read_tag(
        &manager,
        collectible_display_table_tag,
        "collectible-display table",
    )?;
    let stock_objectives = read_tag(&manager, objective_table_tag, "objective table")?;
    let stock_objective_strings = read_tag(
        &manager,
        objective_string_table_tag,
        "objective-string table",
    )?;
    let stock_records = read_tag(&manager, record_table_tag, "record-definition table")?;
    let stock_record_strings = read_tag(&manager, record_string_table_tag, "record-string table")?;
    let stock_nodes = read_tag(
        &manager,
        presentation_node_table_tag,
        "presentation-node table",
    )?;
    let stock_node_strings = read_tag(
        &manager,
        presentation_node_string_table_tag,
        "presentation-node string table",
    )?;
    let stock_pools = read_tag(
        &manager,
        shared_expression_pool_table_tag,
        "shared numeric-expression pool table",
    )?;
    let material_requirements = read_tag(
        &manager,
        material_requirement_table_tag,
        "material-requirement table",
    )?;
    validate_weapon_material_sets(&material_requirements)?;
    let localized_index = read_tag(&manager, localized_index_tag, "localized-string index")?;
    let stock_unlock_banks = read_tag(
        &manager,
        unlock_flag_bank_table_tag,
        "unlock-flag bank table",
    )?;
    let stock_unlocks = read_tag(&manager, unlock_table_tag, "unlock-flag table")?;
    let stock_unlock_displays = read_tag(&manager, unlock_display_tag, "unlock-flag displays")?;
    let stock_entity_assignments = read_tag(
        &manager,
        entity_assignment_tag,
        "sandbox-pattern entity assignments",
    )?;
    validate_finished_sandbox_perk_catalog(&stock_finished_sandbox_perks).map_err(invalid)?;
    validate_sandbox_perk_index_catalog(&stock_sandbox_perk_indices).map_err(invalid)?;
    let stock_finished_perk_count =
        finished_sandbox_perk_count(&stock_finished_sandbox_perks).map_err(invalid)?;
    let stock_perk_index_count =
        sandbox_perk_index_count(&stock_sandbox_perk_indices).map_err(invalid)?;
    if stock_finished_perk_count != stock_perk_index_count {
        return Err(invalid(format!(
            "Finished sandbox-perk catalog has {stock_finished_perk_count} rows, but its indexed metadata companion has {stock_perk_index_count}"
        )));
    }
    validate_sandbox_perk_runtime_map(&stock_entity_assignments).map_err(invalid)?;

    let (stock_item_count, _, item_rows) = terminal_index_table_layout(
        &stock_item_table,
        ITEM_DEFINITION_INDEX_ROW_CLASS,
        "item-definition index",
    )?;
    validate_damage_plug_item_rows(&stock_item_table, item_rows, stock_item_count)?;
    let stock_item_rows_by_hash =
        index_item_rows_by_hash(&stock_item_table, item_rows, stock_item_count)?;
    let (string_count, _, string_rows) = terminal_index_table_layout(
        &stock_item_strings,
        ITEM_STRING_INDEX_ROW_CLASS,
        "item-string index",
    )?;
    let (stock_collectible_count, _, collectible_rows, collectible_class) =
        array_at(&stock_collectibles, 8)?;
    let (stock_unlock_count, _, unlock_rows, unlock_class) = array_at(&stock_unlocks, 8)?;
    if collectible_class != COLLECTIBLE_DEFINITION_ROW_CLASS
        || unlock_class != UNLOCK_FLAG_DEFINITION_ROW_CLASS
    {
        return Err(invalid(
            "Installed collectible or unlock definitions have unexpected native row classes",
        ));
    }
    if stock_item_count != string_count {
        return Err(invalid(
            "Installed item and item-string table counts disagree",
        ));
    }
    let host_entry_count = manager
        .lookup
        .tag32_entries_by_pkg
        .get(&HOST_PACKAGE_ID)
        .map_or(0, Vec::len);
    if host_entry_count != HOST_EXPECTED_ENTRY_COUNT {
        return Err(invalid(format!(
            "The stock investment host has {host_entry_count} entries instead of {HOST_EXPECTED_ENTRY_COUNT}"
        )));
    }
    let private_perk_runtime_entry_count = manager
        .lookup
        .tag32_entries_by_pkg
        .get(&PRIVATE_PERK_RUNTIME_PACKAGE_ID)
        .map_or(0, Vec::len);
    if private_perk_runtime_entry_count != PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT {
        return Err(invalid(format!(
            "The stock private perk-runtime host has {private_perk_runtime_entry_count} entries instead of {PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT}"
        )));
    }

    Ok(ProjectSources {
        manager,
        globals_data,
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
        stock_item_table,
        stock_item_strings,
        stock_item_metadata,
        stock_item_hash_index,
        stock_sandbox_patterns,
        stock_finished_sandbox_perks,
        stock_sandbox_perk_indices,
        stock_item_icons,
        stock_dense,
        stock_metadata_index,
        stock_sandbox_pattern_index,
        stock_collectibles,
        stock_collectible_displays,
        stock_objectives,
        stock_objective_strings,
        stock_records,
        stock_record_strings,
        stock_nodes,
        stock_node_strings,
        stock_pools,
        localized_index,
        stock_unlock_banks,
        stock_unlocks,
        stock_unlock_displays,
        stock_entity_assignments,
        stock_item_count,
        item_rows,
        stock_item_rows_by_hash,
        string_rows,
        stock_collectible_count,
        collectible_rows,
        stock_unlock_count,
        unlock_rows,
    })
}
