//! The reachable item metric block. Category rows index presentation nodes.
use super::*;
use crate::package_payload::native_array_at;

pub fn emblem_metric_categories(data: &[u8]) -> Result<Vec<u16>, String> {
    let relative = i64_at(data, ITEM_METRIC_BLOCK_POINTER_OFFSET)?;
    if relative == 0 {
        return Ok(vec![]);
    }
    let block = relative_offset(ITEM_METRIC_BLOCK_POINTER_OFFSET, 0, relative)?;
    if block < 4 || u32_at(data, block - 4)? != ITEM_METRIC_BLOCK_CLASS {
        return Err("Emblem has an unsupported metric block".into());
    }
    // An empty authoring descriptor deliberately allows no tracker categories.
    if data.get(block..block + 16) == Some(&[0; 16]) {
        return Ok(vec![]);
    }
    let (count, _, rows, class) = native_array_at(data, block)?;
    if class != ITEM_METRIC_CATEGORY_ROW_CLASS || count > u16::MAX as usize {
        return Err("Emblem has an unsupported metric category array".into());
    }
    let indices = (0..count)
        .map(|i| u16_at(data, rows + i * 2))
        .collect::<Result<Vec<_>, _>>()?;
    if indices.contains(&u16::MAX) {
        return Err("Emblem metric category names a disabled node".into());
    }
    Ok(indices)
}
