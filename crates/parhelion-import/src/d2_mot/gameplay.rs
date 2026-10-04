//! Translate investment identities through hashes, never through cross-build row indices.

pub mod perks;

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

/// The stat display group the source item names: each stat it shows, by hash, with its display
/// flags and its curve from investment value to shown value. The item string's stats resource
/// holds the group's row in its second byte, 255 for none. A survey of 3,000 modern weapons found
/// the stats of the group so named among the item's own, apart from stats its plugs supply.
fn source_stat_group(r: &mut Reader, strings: &Payload, stats: &Payload) -> Result<Value> {
    if strings.u64(0x68)? == 0 {
        return Ok(Value::Null);
    }
    let index = strings.u8(resource(strings, 0x68, 0x808054CA)? + 1)?;
    if index == u8::MAX {
        return Ok(Value::Null);
    }
    let definitions = stats.array(8, 0x24, Some(0x8080586F))?;
    let groups = table(r, 0x808054BE)?;
    let row = *groups
        .array(8, 0x38, Some(0x808054C4))?
        .get(usize::from(index))
        .context("source stat group outside its table")?;
    let mut shown = Vec::new();
    for scaled in groups.array(row + 0x10, 0x18, Some(0x808054C8))? {
        let definition = *definitions
            .get(usize::from(groups.u8(scaled)?))
            .context("source stat group names a stat outside the catalog")?;
        let display = groups
            .array(scaled + 8, 8, Some(0x80807A25))?
            .into_iter()
            .map(|at| {
                Ok([
                    i32::from_le_bytes(groups.bytes(at)?),
                    i32::from_le_bytes(groups.bytes(at + 4)?),
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        shown.push(json!({"hash":stats.u32(definition)?,"numeric":groups.u8(scaled + 1)? != 0,"linear":groups.u8(scaled + 3)? != 0,"display":display}));
    }
    Ok(
        json!({"hash":groups.u32(row)?,"maximum":i32::from_le_bytes(groups.bytes(row + 0x30)?),"stats":shown}),
    )
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

fn source_ammo(strings: &Payload) -> Result<u16> {
    if strings.u64(0x20)? == 0 {
        Ok(0)
    } else {
        strings.u16(resource(strings, 0x20, 0x808054E5)?)
    }
}

pub(crate) struct BrowseMetadata {
    pub rarity: u8,
    pub ammo: Option<u16>,
    pub damage: Option<&'static str>,
}

/// Lightweight discovery uses the same validated source fields as gameplay extraction.
/// This metadata describes the modern item, not the translated native behavior.
pub(crate) fn browse_metadata(
    r: &mut Reader,
    item: &Payload,
    strings: &Payload,
    weapon: bool,
) -> Result<BrowseMetadata> {
    let mut metadata = BrowseMetadata {
        rarity: item.u8(0xA0)?,
        ammo: None,
        damage: None,
    };
    if weapon {
        metadata.ammo = Some(source_ammo(strings)?);
        let perks = if item.u64(0x68)? == 0 {
            Vec::new()
        } else {
            let block = resource(item, 0x68, 0x80807381)?;
            perk_hashes(item, block, table(r, 0x808076AA)?.as_ref())?
        };
        metadata.damage = source_element(&perks);
    }
    Ok(metadata)
}

/// Saved alongside the model export so source-only reads are not repeated per donor.
pub fn source(r: &mut Reader, hash: u32, index: usize, item: &Payload) -> Result<Value> {
    let tag = localization::item_strings(r, hash, index)?;
    let strings = r.tag(tag, Some(0x8080549F))?;
    let stats = table(r, 0x8080586B)?;
    let (values, perk_hashes) = if item.u64(0x68)? == 0 {
        (Vec::new(), Vec::new())
    } else {
        let block = resource(item, 0x68, 0x80807381)?;
        (
            stat_values(item, block, &stats)?,
            perk_hashes(item, block, table(r, 0x808076AA)?.as_ref())?,
        )
    };
    let group = source_stat_group(r, &strings, &stats)?;
    // The item type's English name, as the catalog lists it.
    let item_type = localization::Resolver::default().label(r, &strings, 0x8C)?;
    let ammo = source_ammo(&strings)?;
    let sockets = ornaments::gameplay_sockets(r, hash)?;
    let family = super::service::Family::source(item, &strings)?;
    let bucket = family
        .map(|(_, bucket, _)| bucket)
        .unwrap_or(strings.u32(0xC0)?);
    Ok(
        json!({"item_type":item_type,"stats":values,"stat_group":group,"perks":perk_hashes,"ammo":ammo,"bucket":bucket,"rarity":item.u8(0xA0)?,"sockets":sockets["sockets"]}),
    )
}

/// The source item's element from its base perks: kinetic, arc, solar, void, stasis or strand.
pub(crate) fn source_element(perks: &[u32]) -> Option<&'static str> {
    // An empty base-perk list has no elemental damage provider. Keep that
    // absence explicitly instead of inheriting an elemental donor's damage.
    if perks.is_empty() {
        return Some("kinetic");
    }
    // Stable elemental sandbox-perk identities, not weapon-specific compatibility exceptions.
    // The Stasis and Strand perks end on a damage-type effect with operand 5 and 6.
    let kinds = perks
        .iter()
        .filter_map(|hash| match hash {
            0x8C011E66 => Some("kinetic"),
            0x66653D11 => Some("stasis"),
            0x781E5D20 => Some("strand"),
            0xCCC507A5 | 0xB0C2E8FA => Some("arc"),
            0xCFCF0160 | 0x30D3A473 => Some("solar"),
            0x10A9B235 | 0x4F978D3C => Some("void"),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    (kinds.len() == 1).then(|| *kinds.first().unwrap())
}

/// The base perks the source JSON lists.
fn exported_perks(source: &Value) -> Option<Vec<u32>> {
    Some(
        source["perks"]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_u64().and_then(|v| u32::try_from(v).ok()))
            .collect(),
    )
}

/// The source item's element as its exported base perks give it.
pub(crate) fn exported_element(source: &Value) -> Option<&'static str> {
    source_element(&exported_perks(source)?)
}

fn damage(perks: &[u32]) -> Option<&'static str> {
    // The legacy runtime cannot represent Stasis or Strand damage. Keep the
    // gameplay donor's element instead of authoring a Kinetic marker.
    source_element(perks).filter(|element| !matches!(*element, "stasis" | "strand"))
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
    if let Some(value) = exported_perks(source).and_then(|perks| damage(&perks)) {
        result["modern_damage_type"] = json!(value);
    }
    result
}

fn reset_planned_damage(recipe: &mut Value) -> Result<()> {
    // The initial recipe may have a coarse planned element. Source gameplay
    // properties are authoritative, including the absence of a supported one.
    recipe["overrides"]
        .as_object_mut()
        .context("recipe overrides")?
        .remove("modern_damage_type");
    Ok(())
}

mod limits;
mod stat_group;

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
    let mut stand_ins = Vec::new();
    for stat in source["stats"].as_array().context("source stats")? {
        let hash = u32::try_from(stat["hash"].as_u64().context("stat hash")?)?;
        let Some((target, stand_in)) = stat_group::target(hash) else {
            fallbacks.push(json!({"stat":hash,"reason":"Shadowkeep gives this stat's identity to another stat"}));
            continue;
        };
        if let Some(&index) = native
            .get(&target)
            .filter(|index| stat["literal"] == true && u8::try_from(**index).is_ok())
        {
            let row = json!({"definition_index":index,"value":stat["value"]});
            if stand_in {
                stand_ins.push((index, hash, row));
            } else {
                ensure!(
                    result.insert(index, row).is_none(),
                    "duplicate source stat identity"
                );
            }
        } else {
            fallbacks.push(json!({"stat":hash,"reason":"missing or unrepresentable target definition, or conditional source value"}));
        }
    }
    // The source's own value of a stat comes before a stand-in for it.
    for (index, hash, row) in stand_ins {
        match result.entry(index) {
            std::collections::btree_map::Entry::Occupied(_) => fallbacks
                .push(json!({"stat":hash,"reason":"the source sets the stat it stands in for"})),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(row);
            }
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
    let weapon = recipe["kind"].as_str().is_none_or(|kind| kind == "weapon");
    if weapon {
        check_donor(
            source,
            r.tag(items.u32(donor_row + 16)?, Some(0x80807BEA))?
                .as_ref(),
        )?;
    }
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
    if !weapon {
        for field in ["ammo_type", "inventory_slot", "modern_damage_type"] {
            mapped
                .as_object_mut()
                .context("mapped properties")?
                .remove(field);
        }
        for perk in source["perks"].as_array().into_iter().flatten() {
            fallbacks.push(json!({"perk":perk,"reason":"The native base supplies gear runtime perks. Source perk behavior has not been converted."}));
        }
    }
    reset_planned_damage(recipe)?;
    let stats = retain_authored_stats(
        &recipe["overrides"]["investment_stats"],
        stat_overrides(source, &definitions, &mut fallbacks)?,
    )?;
    let custom = if weapon {
        stat_group::plan(source, &definitions, &mut fallbacks)?
    } else {
        None
    };
    let limits = if let Some((maximum, rows)) = &custom {
        // The recipe's own group replaces any stock one.
        recipe["overrides"]
            .as_object_mut()
            .context("recipe overrides")?
            .remove("stat_group_index");
        mapped["custom_stat_group"] = stat_group::recipe(*maximum, rows);
        limits::Limits::of(Some(*maximum), rows)
    } else {
        let group = limits::read(&mut r, &globals, donor, recipe)?;
        recipe["overrides"]
            .as_object_mut()
            .context("recipe overrides")?
            .remove("custom_stat_group");
        limits::Limits::of(group.maximum, &group.shown)
    };
    mapped["investment_stats"] = json!(limits.retain(stats, &mut fallbacks)?);
    let sockets = columns(source, &native_sockets, &available, &mut fallbacks)?;
    if sockets.iter().any(|s| !s.is_null()) {
        mapped["socket_columns"] = json!(sockets);
    }
    apply_mapped_properties(recipe, &mapped)?;
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

fn apply_mapped_properties(recipe: &mut Value, mapped: &Value) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests;
