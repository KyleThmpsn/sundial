//! Recipe choices for the native metric-category block.
use super::*;
use crate::{AuthoringResult, error::invalid, tag_payload::*};
use sundial::{
    investment::load_emblem_tracker_categories,
    package_authoring::{PackageManager, investment_schema::*},
};

/// Absence follows the base. An empty selection intentionally allows no trackers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatTrackers {
    All,
    Selected { categories: Vec<HexHash> },
}

impl StatTrackers {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if let Self::Selected { categories } = self {
            let hashes = categories
                .iter()
                .map(HexHash::parse_u32)
                .collect::<Result<std::collections::BTreeSet<_>, _>>()
                .map_err(|e| e.to_string())?;
            if hashes.len() != categories.len() {
                return Err("Stat tracker categories must be distinct".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn apply_trackers(
    manager: &PackageManager,
    data: &mut Vec<u8>,
    choice: &StatTrackers,
) -> AuthoringResult<()> {
    choice.validate().map_err(invalid)?;
    emblem_metric_categories(data).map_err(invalid)?;
    let options = load_emblem_tracker_categories(manager).map_err(invalid)?;
    let indices = match choice {
        StatTrackers::All => options.iter().map(|o| o.index).collect::<Vec<_>>(),
        StatTrackers::Selected { categories } => {
            let hashes = categories
                .iter()
                .map(HexHash::parse_u32)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| invalid(e.to_string()))?;
            for hash in &hashes {
                if !options.iter().any(|o| o.hash == *hash) {
                    return Err(invalid(format!(
                        "Stat tracker category 0x{hash:08X} is not available in this game"
                    )));
                }
            }
            options
                .iter()
                .filter(|o| hashes.contains(&o.hash))
                .map(|o| o.index)
                .collect()
        }
    };
    let mut block = [0u8; 24];
    if read_i64(data, ITEM_METRIC_BLOCK_POINTER_OFFSET)? != 0 {
        let old = relative_target(data, ITEM_METRIC_BLOCK_POINTER_OFFSET)?;
        block.copy_from_slice(
            data.get(old..old + 24)
                .ok_or_else(|| invalid("Emblem metric block is truncated"))?,
        );
    }
    // Clear the default selection so it cannot name a tracker outside the chosen categories.
    block[..16].fill(0);
    block[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
    while data.len() % 8 != 4 {
        data.push(0);
    }
    data.extend_from_slice(&ITEM_METRIC_BLOCK_CLASS.to_le_bytes());
    let resource = data.len();
    data.extend_from_slice(&block);
    write_relative_pointer(data, ITEM_METRIC_BLOCK_POINTER_OFFSET, resource)?;
    append_native_array(
        data,
        resource,
        ITEM_METRIC_CATEGORY_ROW_CLASS,
        indices.len(),
        &indices
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    let size = u64::try_from(data.len()).map_err(|_| invalid("Emblem definition is too large"))?;
    write_u64(data, 0, size)?;
    if emblem_metric_categories(data).map_err(invalid)? != indices {
        return Err(invalid("Emblem did not retain the selected stat trackers"));
    }
    Ok(())
}
