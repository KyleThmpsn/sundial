//! Base investment inputs and bounded numeric outputs from the source weapon's own tables.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod perk;
mod translator;
pub use perk::{apply_perks, author_perks, release_lane};

fn table(r: &mut Reader, class: u32) -> Result<std::sync::Arc<Payload>> {
    let tags = r.classes(class);
    ensure!(tags.len() == 1, "Ambiguous Marathon stat table {class:08X}");
    r.tag(tags[0], Some(class))
}

fn definition(r: &mut Reader, identity: u32) -> Result<std::sync::Arc<Payload>> {
    let items = table(r, 0x80809685)?;
    let rows = items.array(8, 32, Some(0x80809689))?;
    let item = rows
        .into_iter()
        .find(|&at| items.u32(at).ok() == Some(identity))
        .context("Source weapon stat identity")?;
    r.tag(items.u32(item + 16)?, Some(0x8080968B))
}

fn stats(r: &mut Reader, item: &Payload) -> Result<Vec<Value>> {
    let block = crate::tiger::shadowkeep::resource(item, 0x70, 0x80809245)?;
    let definitions = table(r, 0x80806E50)?;
    let definitions_rows = definitions.array(8, 48, Some(0x80806E54))?;
    let mut seen = BTreeSet::new();
    item.array(block + 8, 40, Some(0x8080924B))?
        .into_iter()
        .map(|at| {
            let definition = *definitions_rows
                .get(item.u32(at)? as usize)
                .context("Marathon stat index outside catalog")?;
            let hash = definitions.u32(definition)?;
            ensure!(seen.insert(hash), "Duplicate Marathon stat identity");
            Ok(
                json!({"hash":hash,"value":i32::from_le_bytes(item.bytes(at+4)?),
                "literal":item.bytes::<32>(at+8)? == [0;32]}),
            )
        })
        .collect()
}

/// The generated schema describes this field by name and type. Built-in component IDs are
/// not tag references. A translator subtype includes the inherited field declaration itself.
fn tables_field(source: &mut Reader, class: u32) -> Result<Option<usize>> {
    if !source
        .manager
        .get_entry(tiger_pkg::TagHash(class))
        .is_some_and(|entry| entry.reference == 0x80800000)
    {
        return Ok(None);
    }
    let schema = source.tag(class, Some(0x80800000))?;
    let mut offsets = BTreeSet::new();
    for at in (0..schema.0.len().saturating_sub(24)).step_by(8) {
        if schema.u32(at + 8)? != 0x80803F7E || schema.u32(at + 16)? & 0x10000 == 0 {
            continue;
        }
        let name = schema.pointer(at)?;
        if schema.0.get(name..name + 27) == Some(b"grouped_translation_tables\0") {
            offsets.insert(schema.u32(at + 12)? as usize);
        }
    }
    ensure!(offsets.len() <= 1, "Ambiguous Marathon translation field");
    Ok(offsets.into_iter().next())
}

fn curves(source: &mut Reader, entity: u32) -> Result<Option<(Payload, usize)>> {
    let entity = source.tag(entity, Some(0x8080BAAD))?;
    let mut found = None;
    for at in entity.array(8, 12, Some(0x8080BAA2))? {
        let owner = source.tag(entity.u32(at)?, Some(0x8080BADB))?;
        let base = owner.pointer(24)?;
        let Some(field) = tables_field(source, owner.u32(base - 4)?)? else {
            continue;
        };
        let groups = owner.array(base + field, 48, Some(0x80803F8A))?;
        // A multiple-group translator needs the source group's actual selection path.
        if groups.len() != 1 {
            continue;
        }
        ensure!(found.is_none(), "Ambiguous Marathon stat translator");
        found = Some(((*owner).clone(), groups[0]));
    }
    Ok(found)
}

fn sample(p: &Payload, row: usize, value: i32) -> Result<Vec<f32>> {
    ensure!(
        p.u32(row + 0x24)? == 0x811C9DC5,
        "Source stat domain differs"
    );
    ensure!(p.u32(row + 0x28)? == 0, "Source stat sampling flags differ");
    let low = p.f32(row + 0x2C)?;
    let high = p.f32(row + 0x30)?;
    let mode = p.u32(row + 0x34)?;
    ensure!(
        low.is_finite() && high.is_finite() && high > low && mode <= 1,
        "Unsupported source stat sampling range"
    );
    let tiers = p.array(row + 16, 16, Some(0x8080BC4D))?;
    ensure!(
        !tiers.is_empty() && tiers.len() <= 256,
        "Invalid source stat tiers"
    );
    let x = ((value as f32).clamp(low, high) - low) / (high - low) * (tiers.len() - 1) as f32;
    let first = x.floor() as usize;
    let fraction = if mode == 0 { 0. } else { x - first as f32 };
    let read = |index: usize| -> Result<Vec<f32>> {
        p.array(tiers[index], 4, Some(0x8080000F))?
            .into_iter()
            .map(|at| p.f32(at))
            .collect()
    };
    let a = read(first)?;
    let b = read((first + 1).min(tiers.len() - 1))?;
    ensure!(
        !a.is_empty() && a.len() == b.len(),
        "Source stat tier width differs"
    );
    Ok(a.iter()
        .zip(b)
        .map(|(a, b)| a + (b - a) * fraction)
        .collect())
}

/// Write a source baseline. Runtime perks, attachments and source trigger controllers are
/// separate contracts. Only conventional capacities and single-shot cadence are exported.
///
/// Source scores are not on the native scale. Nearly every source weapon scores 10 in its
/// handling stats, and each weapon's own curves give that score its effect. A stat is carried
/// only where the donor value with the same effect can be found. Every other score keeps the
/// donor's value.
pub fn author_gameplay(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Value,
    identity: u32,
) -> Result<()> {
    let item = definition(source, identity)?;
    // Weapons name no tier, and how the game ranks them is not traced, so they are Legendary.
    let (tier, rarity) = match super::tier::grade(&item).and_then(super::tier::tier) {
        Some((tier, rarity)) => (Some(tier), rarity),
        None => (None, "legendary"),
    };
    let values = stats(source, &item)?;
    let definitions = crate::tiger::shadowkeep::stat_definitions(native)?;
    let donor = hash(graph, "native_item")?;
    let parts = graph["source_assembly"]
        .as_array()
        .context("Source gameplay assembly")?;
    let root = hash(
        parts.first().context("Empty source gameplay assembly")?,
        "entity",
    )?;
    let curves = curves(source, root)?;
    let mut mapped = Vec::new();
    let mut matched = Vec::new();
    let mut omitted = Vec::new();
    if let Some((p, group)) = &curves {
        let (chosen, evidence, skipped) = translator::matched(native, donor, p, *group, &values);
        for (stat, value) in chosen {
            let index = definitions.get(&stat).context("Native stat definition")?;
            mapped.push(json!({"definition_index":index,"value":value}));
        }
        matched = evidence;
        omitted = skipped;
    }
    for stat in &values {
        let name = hash(stat, "hash")?;
        if curves.is_none() || !translator::translates(name) {
            omitted.push(json!({"stat":name,"reason":"The source score follows the source weapon's own curves, so the donor keeps its native value"}));
        }
    }
    let mut inputs = Vec::new();
    let mut sampled = Vec::new();
    let mut aim = Value::Null;
    let mut display = crate::stat_group::native(native, donor)?;
    if let Some((p, group)) = curves {
        let mut used = BTreeSet::new();
        for row in p.array(group, 56, Some(0x80803F8E))? {
            let input = p.u32(row + 32)?;
            let name = match input {
                0 => 0xFF664809,
                1 => 0xE6BE4C5A,
                3 => 0xD3E91ADA,
                _ => continue,
            };
            ensure!(used.insert(input), "Duplicate source base-stat curve");
            let Some(stat) = values
                .iter()
                .find(|v| v["hash"].as_u64() == Some(name) && v["literal"] == true)
            else {
                continue;
            };
            let value = i32::try_from(stat["value"].as_i64().context("Source stat value")?)?;
            let output = sample(&p, row, value)?;
            sampled.push(json!({"input":input,"stat":name,"investment":value,"outputs":output}));
            match (input,output.as_slice()) {
                (0,[rate,mode]) if (0.05..=100.).contains(rate) && [0.,1.].contains(mode) => {
                    inputs.extend((0..4).map(|index|json!({"component":2,"input":index,"value":rate})));
                    shown(&mut display, &definitions, name as u32, (*rate*60.).round() as i32)?;
                }
                (1, [capacity, chamber]) | (1,[capacity,chamber,1.])
                    if (1. ..=500.).contains(capacity) && capacity.fract()==0. && [0.,1.].contains(chamber) => {
                    inputs.extend([0,3].map(|index|json!({"component":1,"input":index,"value":capacity})));
                    shown(&mut display, &definitions, name as u32, *capacity as i32)?;
                }
                (3,[magnification]) if (1. ..=10.).contains(magnification) => {
                    aim=json!({"magnification":magnification});
                }
                _ => omitted.push(json!({"stat":name,"reason":"Source output requires burst, charge, energy-consumption or another controller"})),
            }
        }
    } else {
        omitted.push(json!({"reason":"No unique single-group source stat translator"}));
    }
    graph["source_item_identity"] = json!(identity);
    graph["source_gameplay"] = json!({"source":"marathon","revision":2,"stats":values,
        "investment_stats":mapped,"matched":matched,"display_group":display,"inputs":inputs,"sampled":sampled,"omitted":omitted,
        "limits":"Base source curves only. Matched stats compare directly copied curve outputs at whole donor values. Donor firing mode, damage, perks, charge and energy consumption are retained. Source sampling between exact tiers is a format inference. Gameplay is unverified."});
    if !aim.is_null() {
        graph["source_gameplay"]["aim"] = aim;
    }
    if let Some(tier) = tier {
        graph["source_gameplay"]["tier"] = json!(tier);
    }
    graph["source_gameplay"]["rarity"] = json!(rarity);
    Ok(())
}

fn shown(group: &mut Value, definitions: &BTreeMap<u32, u16>, hash: u32, value: i32) -> Result<()> {
    let index = *definitions
        .get(&hash)
        .context("Native numeric stat definition")?;
    let maximum = group["maximum_value"]
        .as_i64()
        .context("Native stat display maximum")?;
    ensure!(maximum > 0, "Native display group has no positive range");
    for row in group["stats"]
        .as_array_mut()
        .context("Native stat display rows")?
    {
        if row["definition_index"].as_u64() == Some(u64::from(index)) {
            row["display"] = json!([[0, value], [maximum, value]]);
        }
    }
    Ok(())
}

/// Existing authored values win when an import is refreshed. A fresh recipe receives every
/// mapped base input. Values without native semantics remain in source_gameplay for inspection.
pub fn apply_gameplay(recipe: &mut Value, graph: &Value) -> Result<()> {
    let mut values = BTreeMap::new();
    for row in graph["source_gameplay"]["investment_stats"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            recipe["overrides"]["investment_stats"]
                .as_array()
                .into_iter()
                .flatten(),
        )
    {
        values.insert(hash(row, "definition_index")?, row.clone());
    }
    recipe["overrides"]["investment_stats"] = json!(values.into_values().collect::<Vec<_>>());
    if recipe["overrides"]["custom_stat_group"].is_null()
        && recipe["overrides"]["stat_group_index"].is_null()
        && graph["source_gameplay"]["display_group"].is_object()
    {
        recipe["overrides"]["custom_stat_group"] =
            graph["source_gameplay"]["display_group"].clone();
    }
    if recipe["overrides"]["rarity"].is_null() && graph["source_gameplay"]["rarity"].is_string() {
        recipe["overrides"]["rarity"] = graph["source_gameplay"]["rarity"].clone();
    }
    Ok(())
}
