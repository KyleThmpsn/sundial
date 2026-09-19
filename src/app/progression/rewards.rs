//! Plan queued and direct inventory delivery together for progression claims.
use super::*;
mod consumables;
#[cfg(test)]
pub(super) mod tests;
pub(super) use consumables::{direct_count, draw_review};

pub(super) fn queue(
    document: &mut Value,
    catalog: &Catalog,
    rewards: impl IntoIterator<Item = (u64, i32)>,
) -> Result<(), String> {
    if document.get("_native_progression").is_none() {
        return Err("This account format has no Pending Rewards queue".into());
    }
    let context = document
        .get("_reward_context")
        .ok_or("This account format has no Pending Rewards queue")?;
    let character = context["character"]
        .as_u64()
        .ok_or("Select a character to receive the rewards")?;
    let count = context["character_count"]
        .as_u64()
        .ok_or("The reward character list is unavailable")?;
    if character >= count {
        return Err("Select a character to receive the rewards".into());
    }
    let mut queued = document
        .get("_progression_rewards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut direct = document
        .get("_progression_consumables")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (hash, quantity) in rewards {
        let definition = catalog
            .inventory_definition(hash)
            .ok_or("The reward item has no inventory definition")?;
        let kind = if definition.metadata.is_profile_items_candidate() {
            1
        } else if definition.metadata.is_instanced_character_candidate() {
            0
        } else {
            if definition.metadata.is_character_material_candidate() {
                consumables::plan(context, &mut direct, catalog, definition, quantity)?;
                continue;
            }
            return Err(format!(
                "{} cannot be queued with its installed inventory type ({} / {})",
                definition.name,
                definition.metadata.scope.label(),
                definition.metadata.stackability.label()
            ));
        };
        if kind == 0
            && definition.item.is_some_and(|item| {
                item.class_type != 3 && Some(item.class_type) != context["class"].as_u64()
            })
        {
            return Err(format!(
                "{} requires a different character class. Select that character before claiming this reward.",
                definition.name
            ));
        }
        if quantity <= 0 || (kind == 0 && quantity > 10000) {
            return Err("The reward quantity exceeds the delivery limit".into());
        }
        if kind == 0 {
            for _ in 0..quantity {
                queued.push(serde_json::json!({"kind":0,"hash":hash,"quantity":1}));
            }
        } else {
            let maximum = definition
                .metadata
                .max_stack_size
                .unwrap_or(1)
                .min(i32::MAX as u32) as i32;
            if maximum == 0 {
                return Err("The reward has an invalid stack limit".into());
            }
            let mut remaining = quantity;
            if (remaining as u32).div_ceil(maximum as u32) > 10000 {
                return Err("The reward quantity exceeds the delivery limit".into());
            }
            while remaining > 0 {
                let count = remaining.min(maximum);
                queued.push(serde_json::json!({"kind":1,"hash":hash,"quantity":count}));
                remaining -= count;
            }
        }
    }
    document["_progression_rewards"] = Value::Array(queued);
    if !direct.is_empty() {
        document["_progression_consumables"] = Value::Array(direct);
    }
    Ok(())
}
