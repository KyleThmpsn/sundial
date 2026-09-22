//! Translate investment identities through hashes, never through cross-build row indices.
#[cfg(test)]
mod tests;

use super::{localization, ornaments, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn table(r: &mut Reader, class: u32) -> Result<std::sync::Arc<Payload>> {
    let tags = r.classes(class);
    ensure!(tags.len() == 1, "ambiguous investment table {class:08X}");
    r.tag(tags[0], Some(class))
}

fn resource(p: &Payload, at: usize, class: u32) -> Result<usize> {
    ensure!(p.u64(at)? != 0, "missing investment resource at {at:X}");
    let target = p.pointer(at)?;
    ensure!(
        target >= 4 && p.u32(target - 4)? == class,
        "unexpected investment resource at {at:X}"
    );
    Ok(target)
}

fn stat_values(item: &Payload, block: usize, stats: &Payload) -> Result<Vec<Value>> {
    let definitions = stats.array(8, 0x24, Some(0x8080586F))?;
    let mut values = Vec::new();
    for row in item.array(block, 0x40, Some(0x80807386))? {
        let index = item.u32(row)? as usize;
        let definition = *definitions
            .get(index)
            .context("source stat index outside catalog")?;
        // Modern stats can carry conditions/expressions. A literal override cannot reproduce them.
        let literal = [8, 24, 40]
            .into_iter()
            .all(|at| item.u64(row + at).ok() == Some(0));
        values.push(json!({"hash":stats.u32(definition)?,"value":i32::from_le_bytes(item.bytes(row + 4)?),"literal":literal}));
    }
    Ok(values)
}

fn perk_hashes(item: &Payload, block: usize, perks: &Payload) -> Result<Vec<u32>> {
    let perk_rows = perks.array(8, 12, Some(0x808076AE))?;
    let mut perk_hashes = Vec::new();
    for row in item.array(block + 16, 0x20, Some(0x80807387))? {
        let definition = *perk_rows
            .get(item.u32(row)? as usize)
            .context("source perk index outside catalog")?;
        perk_hashes.push(perks.u32(definition)?);
    }
    Ok(perk_hashes)
}

/// Saved alongside the model export so source-only reads are not repeated per donor.
pub fn source(r: &mut Reader, hash: u32, index: usize, item: &Payload) -> Result<Value> {
    let tag = localization::item_strings(r, hash, index)?;
    let strings = r.tag(tag, Some(0x8080549F))?;
    let block = resource(item, 0x68, 0x80807381)?;
    let values = stat_values(item, block, table(r, 0x8080586B)?.as_ref())?;
    let perk_hashes = perk_hashes(item, block, table(r, 0x808076AA)?.as_ref())?;
    let ammo = strings.u16(resource(&strings, 0x20, 0x808054E5)?)?;
    let sockets = ornaments::gameplay_sockets(r, hash)?;
    Ok(
        json!({"stats":values,"perks":perk_hashes,"ammo":ammo,"bucket":strings.u32(0xC0)?,"rarity":item.u8(0xA0)?,"sockets":sockets["sockets"]}),
    )
}

fn damage(perks: &[u32]) -> Option<&'static str> {
    // An empty base-perk list has no elemental damage provider. Keep that
    // absence explicitly instead of inheriting an elemental donor's damage.
    if perks.is_empty() {
        return Some("kinetic");
    }
    // Stable elemental sandbox-perk identities, not weapon-specific compatibility exceptions.
    let kinds = perks
        .iter()
        .filter_map(|hash| match hash {
            0x8C011E66 | 0x66653D11 | 0x781E5D20 => Some("kinetic"), // Kinetic, Stasis, Strand
            0xCCC507A5 | 0xB0C2E8FA => Some("arc"),
            0xCFCF0160 | 0x30D3A473 => Some("solar"),
            0x10A9B235 | 0x4F978D3C => Some("void"),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    (kinds.len() == 1).then(|| *kinds.first().unwrap())
}

fn properties(source: &Value) -> Value {
    let mut result = json!({});
    for (field, value) in [
        (
            "ammo_type",
            match source["ammo"].as_u64() {
                Some(1) => Some("primary"),
                Some(2) => Some("special"),
                Some(3) => Some("heavy"),
                _ => None,
            },
        ),
        (
            "inventory_slot",
            match source["bucket"].as_u64() {
                Some(0x59570ADA) => Some("kinetic"),
                Some(0x92F16AD9) => Some("energy"),
                Some(0x38DCDD35) => Some("power"),
                _ => None,
            },
        ),
        (
            "rarity",
            match source["rarity"].as_u64() {
                Some(1) => Some("common"),
                Some(2) => Some("uncommon"),
                Some(3) => Some("rare"),
                Some(4) => Some("legendary"),
                Some(5) => Some("exotic"),
                _ => None,
            },
        ),
    ] {
        if let Some(value) = value {
            result[field] = json!(value);
        }
    }
    if let Some(rows) = source["perks"].as_array() {
        let perks = rows
            .iter()
            .filter_map(|v| v.as_u64().and_then(|v| u32::try_from(v).ok()))
            .collect::<Vec<_>>();
        if let Some(value) = damage(&perks) {
            result["modern_damage_type"] = json!(value);
        }
    }
    result
}

mod limits;

/// Older plug-driven elemental sockets cannot be made Kinetic by changing a
/// parent damage marker. Select another gameplay donor before conversion.
pub(crate) fn check_donor(source: &Value, item: &Payload) -> Result<()> {
    let sockets = item.array(item.pointer(0x68)?, 0x50, Some(0x808077C4))?;
    let types = sockets
        .iter()
        .map(|at| item.u16(*at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        damage_carrier_compatible(source, &types),
        "Kinetic imports require a donor without a plug-driven elemental socket"
    );
    Ok(())
}

fn damage_carrier_compatible(source: &Value, socket_types: &[u16]) -> bool {
    properties(source)["modern_damage_type"] != "kinetic" || !socket_types.contains(&68)
}

fn stat_overrides(
    source: &Value,
    native: &BTreeMap<u32, u16>,
    fallbacks: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let mut result = BTreeMap::new();
    for stat in source["stats"].as_array().context("source stats")? {
        let hash = u32::try_from(stat["hash"].as_u64().context("stat hash")?)?;
        if let Some(&index) = native.get(&hash).filter(|_| stat["literal"] == true) {
            ensure!(
                result
                    .insert(
                        index,
                        json!({"definition_index":index,"value":stat["value"]})
                    )
                    .is_none(),
                "duplicate source stat identity"
            );
        } else {
            fallbacks.push(json!({"stat":hash,"reason":"missing target definition or conditional source value"}));
        }
    }
    Ok(result.into_values().collect())
}

fn retain_authored_stats(existing: &Value, mapped: Vec<Value>) -> Result<Vec<Value>> {
    let mut stats = BTreeMap::new();
    for row in existing.as_array().into_iter().flatten().chain(&mapped) {
        let index = row["definition_index"]
            .as_u64()
            .context("recipe stat index")?;
        stats.insert(index, row.clone());
    }
    Ok(stats.into_values().collect())
}

fn categories(socket: &Value) -> BTreeSet<u64> {
    socket["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["category"].as_u64())
        .collect()
}

fn default_hash(socket: &Value) -> Option<u64> {
    socket["choices"]
        .as_array()?
        .iter()
        .find(|v| v["default"] == true)?["hash"]
        .as_u64()
}

fn columns(
    source: &Value,
    native: &Value,
    available: &BTreeMap<u32, u64>,
    fallbacks: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let sources = source["sockets"].as_array().context("source sockets")?;
    let targets = native["sockets"].as_array().context("target sockets")?;
    let mut used = BTreeSet::new();
    let mut result = vec![Value::Null; targets.len()];
    for (lane, target) in targets.iter().enumerate() {
        let allowed = categories(target);
        let Some((source_index, socket)) = sources
            .iter()
            .enumerate()
            .filter(|(i, s)| !used.contains(i) && !categories(s).is_disjoint(&allowed))
            .min_by_key(|(i, _)| (i.abs_diff(lane), *i))
        else {
            continue;
        };
        used.insert(source_index);
        let supported = |plug: &&Value| {
            let Some(hash) = plug["hash"].as_u64().and_then(|v| u32::try_from(v).ok()) else {
                return false;
            };
            available.get(&hash).is_some_and(|category| {
                allowed.contains(category) && Some(*category) == plug["category"].as_u64()
            }) && plug["art_indices"].as_array().is_some_and(Vec::is_empty)
        };
        let plugs = socket["choices"].as_array().context("source choices")?;
        let mut choices = Vec::new();
        if let Some(plug) = plugs
            .iter()
            .filter(supported)
            .find(|p| p["default"] == true)
        {
            choices.push(plug["hash"].as_u64().context("plug hash")?);
        } else if let Some(hash) = default_hash(target) {
            choices.push(hash);
            fallbacks.push(json!({"socket":lane,"source_socket":source_index,"source_default":default_hash(socket),"retained_default":hash}));
        }
        for plug in plugs.iter().filter(supported) {
            let hash = plug["hash"].as_u64().context("plug hash")?;
            if !choices.contains(&hash) {
                choices.push(hash);
            }
        }
        // Preserve the entire donor column only when none of the source choices survived.
        if choices.is_empty() || !plugs.iter().any(|p| supported(&p)) {
            continue;
        }
        result[lane] =
            json!({"choices":choices.iter().map(|h|format!("0x{h:08X}")).collect::<Vec<_>>()});
    }
    Ok(result)
}

fn fill_native_defaults(
    native: &Value,
    columns: &mut Value,
    fallbacks: &mut Vec<Value>,
) -> Result<()> {
    let sockets = native["sockets"].as_array().context("native sockets")?;
    for (lane, socket) in sockets.iter().enumerate() {
        let Some(hash) = socket["curated_fallback"].as_u64() else {
            continue;
        };
        if columns.is_null() {
            *columns = json!(vec![Value::Null; sockets.len()]);
        }
        let columns = columns.as_array_mut().context("recipe socket columns")?;
        if columns.is_empty() {
            columns.resize(sockets.len(), Value::Null);
        }
        let column = columns
            .get_mut(lane)
            .context("recipe lacks donor socket lane")?;
        if column.is_null() {
            *column = json!({"choices":[format!("0x{hash:08X}")]});
            fallbacks.push(json!({"socket":lane,"native_randomized_default":hash}));
        }
    }
    Ok(())
}

/// Resolve against the actual gameplay donor after compatibility defaults have been applied.
pub fn apply(source: &Value, native: &Path, output: &Path, recipe: &mut Value) -> Result<Value> {
    let donor = super::profile::hash(&recipe["donor"], "item_hash")?;
    let mut r = Reader::discovery(native, output, false)?;
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let stats = r.tag(root.u32(8 + 95 * 16)?, None)?;
    let mut definitions = BTreeMap::new();
    for (index, at) in stats
        .array(8, 32, Some(0x80807D09))?
        .into_iter()
        .enumerate()
    {
        ensure!(
            definitions
                .insert(stats.u32(at)?, u16::try_from(index)?)
                .is_none(),
            "duplicate native stat hash"
        );
    }
    let native_sockets = ornaments::native(&mut r, donor)?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items.array(8, 24, Some(0x80807BE8))?;
    let donor_row = *rows
        .iter()
        .find(|at| items.u32(**at).ok() == Some(donor))
        .context("native gameplay donor")?;
    check_donor(
        source,
        r.tag(items.u32(donor_row + 16)?, Some(0x80807BEA))?
            .as_ref(),
    )?;
    let needed = source["sockets"]
        .as_array()
        .context("source sockets")?
        .iter()
        .flat_map(|s| s["choices"].as_array().into_iter().flatten())
        .filter_map(|v| v["hash"].as_u64())
        .collect::<BTreeSet<_>>();
    let mut available = BTreeMap::new();
    for at in rows {
        let hash = items.u32(at)?;
        if !needed.contains(&u64::from(hash)) {
            continue;
        }
        let plug = r.tag(items.u32(at + 16)?, Some(0x80807BEA))?;
        let fields = (0x100..plug.0.len().min(0x300).saturating_sub(8))
            .step_by(4)
            .filter(|&at| plug.u32(at).ok() == Some(0x808077E3))
            .collect::<Vec<_>>();
        if let [field] = fields.as_slice() {
            let art = plug.array(plug.pointer(0x88)?, 4, Some(0x808077B5))?;
            if art.is_empty() {
                ensure!(
                    available
                        .insert(hash, u64::from(plug.u32(field + 4)?))
                        .is_none(),
                    "ambiguous native plug hash"
                );
            }
        }
    }
    let mut fallbacks = Vec::new();
    let mut mapped = properties(source);
    let stats = retain_authored_stats(
        &recipe["overrides"]["investment_stats"],
        stat_overrides(source, &definitions, &mut fallbacks)?,
    )?;
    mapped["investment_stats"] =
        json!(limits::read(&mut r, &globals, donor, recipe)?.retain(stats, &mut fallbacks)?);
    let sockets = columns(source, &native_sockets, &available, &mut fallbacks)?;
    if sockets.iter().any(|s| !s.is_null()) {
        mapped["socket_columns"] = json!(sockets);
    }
    for (key, value) in mapped.as_object().context("mapped properties")? {
        // Retained compatibility profiles may contain intentional donor-specific socket authoring.
        // Update ordinary imports, but do not erase those private runtime fixes.
        if key == "socket_columns"
            && recipe["overrides"]
                .get(key)
                .is_some_and(|v| v.as_array().is_some_and(|v| !v.is_empty()))
        {
            continue;
        }
        recipe["overrides"][key] = value.clone();
    }
    if native_sockets["sockets"]
        .as_array()
        .context("native sockets")?
        .iter()
        .any(|socket| socket["curated_fallback"].as_u64().is_some())
    {
        fill_native_defaults(
            &native_sockets,
            &mut recipe["overrides"]["socket_columns"],
            &mut fallbacks,
        )?;
    }
    let report = json!({"source":source,"mapped":mapped,"fallbacks":fallbacks});
    super::reader::write_json(&output.join("gameplay-mapping.json"), &report)?;
    Ok(report)
}
