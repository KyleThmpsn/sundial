//! Shadowkeep emblem metric categories, read from the metric presentation relationships.
use crate::{
    investment_localization::{LocalizedStringCache, resolve_string},
    investment_schema::*,
    package_payload::{native_array_at, u16_at, u32_at},
    package_runtime::{reader::PackageManager, resolve_live_named_tag},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug)]
pub struct EmblemTrackerCategory {
    pub index: u16,
    pub hash: u32,
    pub name: String,
    pub description: String,
    pub metric_count: usize,
}

/// Returns the broad native categories. A restricted category is omitted when another
/// category already contains all its metrics. No category is invented for orphan metrics.
pub fn load_emblem_tracker_categories(
    manager: &PackageManager,
) -> Result<Vec<EmblemTrackerCategory>, String> {
    let read = |tag| {
        manager
            .read_tag(tiger_pkg::TagHash(tag))
            .map_err(|e| format!("Emblem stat trackers: {e}"))
    };
    let globals = read(resolve_live_named_tag(manager, "investment_globals", None)?.0)?;
    let root = read(investment_globals_table_tag(&globals, 0)?)?;
    let metrics = read(investment_root_table_tag(&root, 55)?)?;
    let nodes = read(investment_root_table_tag(
        &root,
        ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT,
    )?)?;
    let strings = read(investment_globals_table_tag(
        &globals,
        GLOBALS_PRESENTATION_NODE_STRING_TABLE_SLOT,
    )?)?;
    let localized = read(investment_globals_table_tag(
        &globals,
        GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT,
    )?)?;
    let (n, _, rows, class) = native_array_at(&metrics, 8)?;
    if class != 0x8080_55B8 {
        return Err("Unsupported metric definition rows".into());
    }
    let (node_count, _, node_rows, class) = native_array_at(&nodes, 8)?;
    if class != PRESENTATION_NODE_DEFINITION_ROW_CLASS {
        return Err("Unsupported tracker category nodes".into());
    }
    let (text_count, _, text_rows, _) = native_array_at(&strings, 8)?;
    if text_count != node_count {
        return Err("Tracker category definitions and strings do not align".into());
    }
    let (tag_count, _, tag_rows, _) = native_array_at(&localized, 8)?;
    let tags = (0..tag_count)
        .map(|i| u32_at(&localized, tag_rows + i * 8 + 4).map(tiger_pkg::TagHash))
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_node = BTreeMap::<u16, BTreeSet<usize>>::new();
    for i in 0..n {
        let parents = index_list(
            &metrics,
            rows + i * 72 + 24,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
        )?;
        for parent in parents {
            if usize::from(parent) >= node_count {
                return Err("Metric references a missing tracker category".into());
            }
            by_node.entry(parent).or_default().insert(i);
        }
    }
    let mut cache: LocalizedStringCache = HashMap::new();
    by_node
        .iter()
        .filter(|(index, set)| {
            !by_node.iter().any(|(other, other_set)| {
                *index != other
                    && set.is_subset(other_set)
                    && (set.len() < other_set.len() || other < *index)
            })
        })
        .map(|(&index, set)| {
            let hash = u32_at(
                &nodes,
                node_rows
                    + usize::from(index) * PRESENTATION_NODE_DEFINITION_ROW_SIZE
                    + PRESENTATION_NODE_HASH_OFFSET,
            )?;
            let text = text_rows + usize::from(index) * PRESENTATION_NODE_STRING_ROW_SIZE;
            if u32_at(&strings, text)? != hash {
                return Err("Tracker category strings name another node".into());
            }
            let name = resolve_string(manager, &tags, &mut cache, &strings, text + 8)
                .unwrap_or_else(|| format!("Category 0x{hash:08X}"));
            let description =
                resolve_string(manager, &tags, &mut cache, &strings, text + 16).unwrap_or_default();
            Ok(EmblemTrackerCategory {
                index,
                hash,
                name,
                description,
                metric_count: set.len(),
            })
        })
        .collect()
}

fn index_list(data: &[u8], descriptor: usize, expected: u32) -> Result<Vec<u16>, String> {
    if data.get(descriptor..descriptor + 16) == Some(&[0; 16]) {
        return Ok(vec![]);
    }
    let (count, _, rows, class) = native_array_at(data, descriptor)?;
    if class != expected || count > u16::MAX as usize {
        return Err("Unsupported tracker category array".into());
    }
    (0..count).map(|i| u16_at(data, rows + i * 2)).collect()
}
