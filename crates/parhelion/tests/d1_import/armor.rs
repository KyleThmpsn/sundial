//! Armor acceptance follows original grid variants and emitted equipment contributions.
use super::*;

pub(super) fn check(
    source: &Source,
    native: &mut Reader,
    item_tag: u32,
    recipe: &WeaponRecipe,
    receipt: &Value,
) -> Result<Value> {
    let armor = source
        .armor()?
        .into_iter()
        .find(|item| Some(u64::from(item.hash)) == receipt["source"].as_u64())
        .context("Original armor")?;
    let original = source.read(armor.definition)?;
    let ordinal = original.u16(original.pointer(0x58)?)?;
    let tables = source.classes(0x808018C2);
    ensure!(tables.len() == 1, "Ambiguous source grids");
    let table = source.read(tables[0])?;
    let rows = table.array(8, 24, Some(0x808017CB))?;
    let grid = source.read(table.u32(rows[usize::from(ordinal)] + 16)?)?;
    let nodes = grid.array(16, 64, Some(0x80801728))?;
    let emitted = native.tag(item_tag, Some(0x80807BEA))?;
    ensure!(
        emitted
            .array(emitted.pointer(0x70)? + 16, 24, Some(0x808077BC))?
            .is_empty(),
        "Imported armor retained direct donor perks"
    );
    let report = &receipt["graph"]["perk_translation"];
    let exported = &receipt["graph"]["source_talents"];
    let mut checked = Vec::new();
    for choice in report["translated"].as_array().context("Armor talents")? {
        let node_index = usize::try_from(number(&choice["node"])?)?;
        let step_index = usize::try_from(number(&choice["step"])?)?;
        let step = &exported["nodes"][node_index]["steps"][step_index];
        let steps = grid.array(nodes[node_index] + 24, 144, Some(0x80801758))?;
        let at = steps[usize::try_from(number(&step["shared_step"])?)?];
        ensure!(
            grid.u64(at + 0x70)? == 0,
            "Appearance step became a gameplay talent"
        );
        let variants = grid.array(at, 88, Some(0x808016DE))?;
        let index = step["source_variant"].as_u64().unwrap_or(0) as usize;
        ensure!(
            variants.len() == 1 || step["source_variant"].is_u64(),
            "Ambiguous source variant"
        );
        let variant = variants[index];
        let grants = grid.array(variant, 16, Some(0x808018E8))?;
        let translated = choice["ability_grants"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for (index, &grant) in grants.iter().enumerate() {
            let key = grid.u32(grant)?;
            ensure!(
                translated
                    .iter()
                    .any(|r| r["source_grant"].as_u64() == Some(index as u64)
                        && r["source_key"].as_u64() == Some(u64::from(key))),
                "Ability grant was silently dropped"
            );
        }
        ensure!(
            grid.u64(variant + 16)? == 0,
            "Unqualified D1 armor stat tiers were carried as native points"
        );
        let slot = number(&choice["socket"])?;
        let choice_index = number(&choice["choice"])?;
        let authored = recipe
            .overrides
            .socket_plug_variants
            .iter()
            .find(|v| {
                u32::from(v.socket_index) == slot && u32::from(v.choice_index) == choice_index
            })
            .context("Translated armor plug")?;
        ensure!(
            authored.replace_effects,
            "Armor talent inherited template behavior"
        );
        ensure!(
            authored.investment_stats.is_empty(),
            "Armor talent inherited template stats"
        );
        checked.push(json!({"node":node_index,"step":step_index,"socket":slot,
            "choice":choice_index,"source_variant_offset":variant}));
    }
    Ok(json!({"choices":checked,"direct_donor_perks":0,"gameplay_verified":false}))
}
