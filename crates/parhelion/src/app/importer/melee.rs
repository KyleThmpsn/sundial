//! Publish the private attack that belongs to the imported weapon's strike graph.
use super::*;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub(super) fn merge(prepared: &WeaponRecipe, updated: &mut WeaponRecipe) -> Result<()> {
    let Some(reference) = &prepared.overrides.imported_graph else {
        return Ok(());
    };
    reference.validate(prepared.identity.item_hash.parse_u32()?)?;
    let graph: Value = serde_json::from_slice(&std::fs::read(
        reference.directory.join("asset-graph.json"),
    )?)?;
    if graph["source"] != "halo_reach" || graph["melee"].is_null() {
        return Ok(());
    }
    let activation = &graph["melee"]["activation"];
    let lane = u16::try_from(
        activation["socket_index"]
            .as_u64()
            .context("Melee socket")?,
    )?;
    let index = u16::try_from(
        activation["source_perk_index"]
            .as_u64()
            .context("Melee effect")?,
    )?;
    let plug = u32::try_from(
        activation["source_plug"]
            .as_u64()
            .context("Melee source plug")?,
    )?;
    let property = u32::try_from(activation["property"].as_u64().context("Melee property")?)?;
    let variants = prepared
        .overrides
        .socket_plug_variants
        .iter()
        .filter(|variant| variant.socket_index == lane && variant.choice_index == 0)
        .collect::<Vec<_>>();
    let [variant] = variants.as_slice() else {
        anyhow::bail!("Prepared strikes need one private intrinsic variant");
    };
    ensure!(
        variant.source_plug_hash.parse_u32()? == plug
            && variant.additional_sandbox_perks.contains(&index),
        "Prepared melee intrinsic differs from its graph"
    );
    let programs = variant
        .sandbox_perks
        .iter()
        .filter(|perk| perk.source_perk_index == index)
        .collect::<Vec<_>>();
    let [program] = programs.as_slice() else {
        anyhow::bail!("Prepared strikes need one private activation program");
    };
    let value = serde_json::to_value(program)?;
    let key = format!("0x{property:08X}");
    ensure!(
        graph["audio"]["impact_key"] == format!("{property:08X}")
            && value["program"]["actions"]
                .as_array()
                .is_some_and(|actions| actions.iter().any(|action| action["key"] == key)),
        "Prepared strike activation does not select its contact profile"
    );
    let positions = updated
        .overrides
        .socket_plug_variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.socket_index == lane && variant.choice_index == 0)
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    ensure!(
        positions.len() <= 1,
        "Saved intrinsic variants are ambiguous"
    );
    if let Some(&position) = positions.first() {
        let current = &mut updated.overrides.socket_plug_variants[position];
        ensure!(
            current.source_plug_hash.parse_u32()? == plug,
            "The saved intrinsic uses another source plug. Its authored settings were preserved"
        );
        let existing = current
            .sandbox_perks
            .iter()
            .filter(|perk| perk.source_perk_index == index)
            .collect::<Vec<_>>();
        ensure!(existing.len() <= 1, "Saved melee effects are ambiguous");
        if let Some(existing) = existing.first() {
            ensure!(
                *existing == *program,
                "The saved intrinsic edits the required melee effect. Its authored program was preserved"
            );
        } else {
            current.sandbox_perks.push((*program).clone());
        }
        if !current.additional_sandbox_perks.contains(&index) {
            current.additional_sandbox_perks.push(index);
        }
    } else {
        let mut added = (*variant).clone();
        added.sandbox_perks = vec![(*program).clone()];
        added.additional_sandbox_perks = vec![index];
        updated.overrides.socket_plug_variants.push(added);
    }
    Ok(())
}
