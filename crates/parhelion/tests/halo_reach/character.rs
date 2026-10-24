//! Follow the staged human pose into every descriptor-indexed correction table.
use super::*;
mod rider;

fn selected(owner: &Payload, content: u32) -> Result<Vec<usize>> {
    let table = owner.pointer(24)? + 0x588;
    let mut rows = Vec::new();
    for row in owner.array(table, 0xE8, Some(0x80803E72))? {
        if owner.u32(row)? == content {
            rows.push(row);
        }
    }
    if rows.is_empty() {
        rows.push(table - 0xE8);
    }
    Ok(rows)
}

pub(super) fn verify(
    staged: &mut Reader,
    stock: &mut Reader,
    graph: &Value,
    runtime: &Value,
) -> Result<Value> {
    rider::verify(staged, stock, graph, runtime)?;
    let Some(character) = graph["animation"]
        .get("character")
        .filter(|value| !value.is_null())
    else {
        return Ok(Value::Null);
    };
    let binding = &character["binding"];
    let mut profiles = Vec::new();
    if binding["builtin"] == 3 {
        let entity_tag = u32::from_str_radix(
            runtime["runtime_entity"]
                .as_str()
                .context("Authored runtime")?,
            16,
        )?;
        let content = u32::from_str_radix(
            runtime["content_key"]
                .as_str()
                .context("Authored content")?,
            16,
        )?;
        let entity = staged.tag(entity_tag, Some(0x80809C0F))?;
        let bindings =
            sundial::package_authoring::entity::weapon_component_bindings(&entity.0, 0xD3A5500E)
                .map_err(anyhow::Error::msg)?;
        ensure!(
            bindings.len() == 1,
            "Authored character attachment ambiguous"
        );
        let owner = staged.tag(bindings[0].owner_tag, Some(0x80809C36))?;
        for row in selected(&owner, content)? {
            let profile = owner.u32(row + 0x6C)?;
            ensure!(
                Some(u64::from(profile)) == binding["private"].as_u64(),
                "Weapon does not select its imported character profile"
            );
            profiles.push(profile);
        }
        ensure!(!profiles.is_empty(), "No selected character attachment row");
        let owner = stock.tag(
            u32::try_from(binding["owner"].as_u64().context("Stock attachment")?)?,
            Some(0x80809C36),
        )?;
        for row in selected(
            &owner,
            u32::try_from(binding["content"].as_u64().context("Stock content")?)?,
        )? {
            ensure!(
                Some(u64::from(owner.u32(row + 0x6C)?)) == binding["parent"].as_u64(),
                "Stock donor character profile changed"
            );
        }
    }
    let pose_tag = u32::try_from(
        character["pose"]
            .as_u64()
            .context("Requested character pose")?,
    )?;
    let pose = staged.tag(pose_tag, Some(0x80809c36))?;
    let definition = pose.pointer(24)?;
    ensure!(
        pose.u32(definition - 4)? == 0x80803640,
        "Character pose definition differs"
    );
    let bank_tag = pose.u32(definition + 0x108)?;
    let bank = staged.tag(bank_tag, Some(0x808036f6))?;
    let original = stock.tag(bank_tag, Some(0x808036f6))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let old_descriptors = original.array(0x68, 32, Some(0x80809002))?;
    ensure!(
        descriptors.len() > old_descriptors.len(),
        "Character import added no reachable bank descriptors"
    );
    let mut tables = Vec::new();
    for (field, class, aliases) in [
        (0x114, 0x8080348e, &[8, 0x38][..]),
        (0x11c, 0x808034eb, &[0x28][..]),
    ] {
        let tag = pose.u32(definition + field)?;
        let table = staged.tag(tag, Some(class))?;
        let original = stock.tag(tag, Some(class))?;
        for &alias in aliases {
            let rows = table.array(alias, 2, Some(0x80800006))?;
            let old_rows = original.array(alias, 2, Some(0x80800006))?;
            ensure!(
                rows.len() == descriptors.len() && old_rows.len() == old_descriptors.len(),
                "Character correction table {tag:08X} at {alias:X} does not cover its bank"
            );
            for (&old, &new) in old_rows.iter().zip(&rows) {
                ensure!(
                    original.u16(old)? == table.u16(new)?,
                    "Character import changed a stock correction route"
                );
            }
            for &row in &rows[old_rows.len()..] {
                ensure!(
                    table.u16(row)? == u16::MAX,
                    "Absolute authored pose still selects a donor correction"
                );
            }
            tables.push(json!({"tag":format!("{tag:08X}"),"field":alias,
                "descriptors":rows.len(),"stock_routes_preserved":old_rows.len(),
                "private_routes_disabled":rows.len()-old_rows.len()}));
        }
    }
    Ok(
        json!({"pose":format!("{pose_tag:08X}"),"bank":format!("{bank_tag:08X}"),
        "tables":tables,"attachment_profiles":profiles,"gameplay_verified":false}),
    )
}
