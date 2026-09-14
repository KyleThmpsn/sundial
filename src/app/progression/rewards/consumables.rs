use super::*;
use crate::catalog::InventoryDefinition;

pub(super) fn plan(
    context: &Value,
    planned: &mut Vec<Value>,
    catalog: &Catalog,
    item: InventoryDefinition<'_>,
    quantity: i32,
) -> Result<(), String> {
    let stacks = context["consumables"]
        .as_array()
        .ok_or("Character consumables are unavailable")?;
    let inventory = context["inventory"]
        .as_array()
        .ok_or("Character inventory is unavailable")?;
    let equipped = context["equipment"].as_array().into_iter().flatten();
    if equipped
        .clone()
        .any(|row| row[1].as_u64() == Some(item.hash))
    {
        return Err(format!("{} is in an equipment slot", item.name));
    }
    let maximum = item
        .metadata
        .max_stack_size
        .filter(|n| *n > 0)
        .ok_or("The consumable has no stack cap")?
        .min(i32::MAX as u32);
    if quantity <= 0 {
        return Err("The consumable quantity must be positive".into());
    }
    let existing = stacks
        .iter()
        .filter(|row| row["definition_hash"].as_u64() == Some(item.hash))
        .map(|row| row["quantity"].as_i64())
        .chain(
            inventory
                .iter()
                .filter(|row| row[1].as_u64() == Some(item.hash))
                .map(|row| row[2].as_i64()),
        )
        .collect::<Vec<_>>();
    if existing.len() > 1 {
        return Err(format!("{} already occupies multiple stacks", item.name));
    }
    let before = match existing.first() {
        Some(Some(value)) if *value > 0 => *value,
        Some(_) => return Err(format!("{} has an invalid stack quantity", item.name)),
        None => 0,
    };
    let staged = planned
        .iter()
        .filter(|row| row["hash"].as_u64() == Some(item.hash))
        .map(|row| row["quantity"].as_i64().unwrap_or(0))
        .sum::<i64>();
    // Sunrise validates CharacterStacks as definition-unique. It currently supports
    // one stack per consumable, including items whose native bucket has spare rows.
    if before + staged + i64::from(quantity) > i64::from(maximum) {
        return Err(format!(
            "{}: stack cap {maximum}. Sunrise allows one stack per consumable.",
            item.name
        ));
    }
    let serial = context["next_serial"]
        .as_u64()
        .ok_or("Character inventory serial is unavailable")?;
    if serial
        .checked_add(planned.len() as u64)
        .is_none_or(|serial| serial >= i32::MAX as u64)
    {
        return Err("The character inventory serial is exhausted".into());
    }
    if before == 0 && staged == 0 {
        let mut hashes = stacks
            .iter()
            .filter_map(|row| row["definition_hash"].as_u64())
            .collect::<HashSet<_>>();
        let occupied = inventory
            .iter()
            .chain(equipped)
            .filter_map(|row| row[1].as_u64())
            .collect::<Vec<_>>();
        for reward in planned.iter() {
            if let Some(hash) = reward["hash"].as_u64()
                && !occupied.contains(&hash)
            {
                hashes.insert(hash);
            }
        }
        if hashes.len() >= 32 {
            return Err("This character has no free consumable slots".into());
        }
        let used = hashes
            .iter()
            .copied()
            .chain(occupied)
            .map(|hash| {
                catalog
                    .inventory_metadata(hash)
                    .ok_or("An inventory item's bucket is unavailable")
            })
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter(|metadata| {
                metadata.scope == item.metadata.scope
                    && metadata.native_bucket_id == item.metadata.native_bucket_id
            })
            .count();
        let capacity = item
            .metadata
            .authored_row_capacity()
            .ok_or("The consumable has no inventory slot limit")?;
        if used >= usize::from(capacity) {
            return Err(format!(
                "No inventory space for {}. {}: {used}/{capacity} slots used, including planned rewards.",
                item.name,
                item.metadata.bucket_label(),
            ));
        }
    }
    planned.push(serde_json::json!({"hash":item.hash,"quantity":quantity,"maximum":maximum}));
    Ok(())
}

pub(in crate::app::progression) fn direct_count(before: &Value, after: &Value) -> usize {
    count(after).saturating_sub(count(before))
}

fn count(document: &Value) -> usize {
    document
        .get("_progression_consumables")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

pub(in crate::app::progression) fn draw_review(
    ui: &mut egui::Ui,
    before: &Value,
    after: &Value,
    catalog: &Catalog,
) {
    if direct_count(before, after) == 0 {
        return;
    }
    ui.label("Sunrise Pending Rewards doesn’t support granting consumables. Apply anyway and add consumables directly to your inventory?");
    ui.collapsing("Consumables", |ui| {
        let mut totals = std::collections::BTreeMap::<u64, i64>::new();
        for row in after["_progression_consumables"]
            .as_array()
            .into_iter()
            .flatten()
            .skip(count(before))
        {
            *totals.entry(row["hash"].as_u64().unwrap_or(0)).or_default() +=
                row["quantity"].as_i64().unwrap_or(0);
        }
        for (hash, quantity) in totals {
            ui.label(destiny_text(
                ui,
                format!(
                    "{} × {quantity}",
                    catalog.package_item_name(hash).unwrap_or("Consumable")
                ),
            ));
        }
    });
}
