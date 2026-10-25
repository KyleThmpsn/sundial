//! Require the saved source rider through the staged seat and native transition selector.
use super::*;

pub(super) fn verify(
    staged: &mut Reader,
    stock: &mut Reader,
    graph: &Value,
    runtime: &Value,
) -> Result<()> {
    let Some(path) = env::var_os("PARHELION_REACH_RIDER_WITNESS") else {
        return Ok(());
    };
    let witness: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        witness.as_object().is_some_and(|v| !v.is_empty()),
        "Empty required rider corpus"
    );
    let identity = u32::try_from(
        graph["item_hash"]
            .as_u64()
            .context("Imported item identity")?,
    )?;
    let Some(expected) = witness.get(format!("{identity:08X}")) else {
        return Ok(());
    };
    let character = &graph["animation"]["character"];
    ensure!(
        character["source_reference"] == expected["reference"]
            && character["source_graph"]["path"] == expected["source"],
        "Required source rider was lost"
    );
    let entity = staged.tag(
        u32::from_str_radix(
            runtime["runtime_entity"]
                .as_str()
                .context("Vehicle entity")?,
            16,
        )?,
        Some(0x80809c0f),
    )?;
    let mut profiles = Vec::new();
    for row in entity.array(16, 12, Some(0x80809c04))? {
        let owner = staged.tag(entity.u32(row)?, Some(0x80809c36))?;
        let data = usize::try_from(owner.u64(owner.pointer(16)? + 8)?)?;
        if owner.u32(data - 4)? == 0x80803d7d {
            profiles.push(owner.u32(data + 0xec)?);
        }
    }
    ensure!(
        profiles.len() == 1 && Some(u64::from(profiles[0])) == expected["private"].as_u64(),
        "Vehicle seat does not select its private source rider"
    );
    let lookup_tag = u32::try_from(
        expected["native"]["lookup"]
            .as_u64()
            .context("Qualified rider lookup")?,
    )?;
    let lookup = staged.tag(lookup_tag, Some(0x80809c36))?;
    let d = usize::try_from(lookup.u64(lookup.pointer(16)? + 8)?)?;
    let bank = staged.tag(lookup.u32(d + 0x90)?, Some(0x808036f6))?;
    let old_bank = stock.tag(lookup.u32(d + 0x90)?, Some(0x808036f6))?;
    let old_count = old_bank.array(0x68, 32, Some(0x80809002))?.len();
    let count = bank.array(0x68, 32, Some(0x80809002))?.len();
    let dictionary = staged.tag(lookup.u32(d + 0x94)?, Some(0x80808ee1))?;
    let selector = staged.tag(lookup.u32(d + 0x110)?, Some(0x80803478))?;
    let table = staged.tag(lookup.u32(d + 0x10c)?, Some(0x8080344d))?;
    let choices = parhelion_import::tiger::animation::state::parameter::table_choices_for(
        &selector,
        &dictionary,
        &BTreeMap::from([(2, profiles[0])]),
    )?;
    ensure!(
        !choices.is_empty(),
        "Private rider has no selected transition row"
    );
    let mut transitions = Vec::new();
    for choice in choices {
        for field in [0, 16, 32] {
            let descriptors = parhelion_import::tiger::animation::motion_table::descriptors(
                &table,
                parhelion_import::tiger::animation::motion_table::Kind::Rider,
                choice,
                field,
            )?;
            ensure!(
                !descriptors.is_empty() && descriptors.iter().all(|&d| d >= old_count && d < count),
                "Source rider transition still selects a stock action"
            );
            transitions.push(json!({"choice":choice,"field":field,"descriptors":descriptors}));
        }
    }
    fs::write(
        staged.output.join(format!("rider-{identity:08X}.json")),
        serde_json::to_vec_pretty(&json!({
            "profile":profiles[0],"reference":expected["reference"],"transitions":transitions,
            "limits":"Staged seat and private enter, mounted and exit routes. Live rider alignment and transitions require gameplay acceptance."
        }))?,
    )?;
    Ok(())
}
