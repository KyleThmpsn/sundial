//! A charged pair's charged shot as a private copy of the donor's full-charge pattern override.
//!
//! A Reach charging trigger with two barrels fires its primary barrel when released before full
//! charge and its secondary barrel once charged. The weapon fires the primary barrel's converted
//! projectile as its own shot. A native charge weapon such as Devil's Ruin changes what it fires
//! once charged through a perk on its intrinsic: a General Predicate (`80803DCE`) holds while
//! `weapon_charge_fraction` (`7AA347DD`, named-key field `+D4`) lies between 1 and 1 (`+D8`,
//! `+DC`), and a Pattern Override (kind 26, `80803E12`) names the projectile the Barrel fires
//! instead. The intrinsic's private copy of that perk names the secondary barrel's converted
//! projectile there. Attachments that only carry property modifiers for the donor's own charged
//! shot are left out, and a carrier that steers by the source's guidance gets the weapon tracking
//! count (kind 30, `1E 01 01`) that every stock tracking perk holds. The donor is found by this
//! structure, not by its identity. A Spew-Charge pair needs no perk, since the weapon fires its
//! charged barrel's projectile as its own shot when the donor's charge completes.
use super::super::{ImportRequest, cache, projectile, resource};
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::path::Path;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{
        Action, Program,
        decompile::{self, Recovery},
    },
};

/// The private program's name on the intrinsic.
const PROGRAM: &str = "Reach Charged Release";
const GENERAL_PREDICATE: u32 = 0x8080_3DCE;
const CHARGE_FRACTION: u32 = 0x7AA3_47DD;
const PATTERN_OVERRIDE: u32 = 0x8080_3E12;
/// A Property Modifiers owner's definition and its settings records.
const MODIFIERS: u32 = 0x8080_3AFE;
const MODIFIER_ROW: u32 = 0x8080_3B06;
/// An item definition's investment resource, and its finished sandbox perk rows.
const INVESTMENT: u32 = 0x8080_77B9;
const ITEM_PERK: u32 = 0x8080_77BC;

/// The donor perk that changes the fired projectile at full charge.
struct DonorPerk {
    lane: usize,
    plug: u32,
    perk: u16,
    action: u32,
    payload: Vec<u8>,
    pattern: u32,
}

/// The finished sandbox perk indices a plug item grants.
fn plug_perks(plug: &Payload) -> Result<Vec<u16>> {
    if plug.u64(0x70)? == 0 {
        return Ok(Vec::new());
    }
    let resource = crate::tiger::shadowkeep::resource(plug, 0x70, INVESTMENT)?;
    plug.array(resource + 0x10, 0x18, Some(ITEM_PERK))?
        .into_iter()
        .map(|row| plug.u16(row))
        .collect()
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Whether `condition` holds only at a full charge: a General Predicate on the weapon's charge
/// fraction whose bounds are both 1.
fn full_charge(condition: &action::DecodedCondition) -> bool {
    condition.class == GENERAL_PREDICATE
        && word(&condition.native, 0xD4) == Some(CHARGE_FRACTION)
        && [0xD8, 0xDC]
            .iter()
            .all(|&at| word(&condition.native, at).map(f32::from_bits) == Some(1.))
}

/// The donor's one perk whose first behavior starts at full charge and overrides the fired pattern
/// with a projectile, on the plug each socket selects: the recipe's first column choice, or the
/// socket's default.
fn donor_perk(native: &mut Reader, recipe: &Value, donor: u32) -> Result<DonorPerk> {
    use sundial::package_authoring::{self as authoring, entity, runtime};
    let manager = authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let weapon = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        entity::weapon_component_binding_hashes(&weapon.payload)
            .map_err(anyhow::Error::msg)?
            .contains(&entity::WEAPON_TRIGGER_CHARGE_COMPONENT_KEY),
        "The donor has no Trigger Charge component"
    );
    let globals = manager
        .read_tag(
            authoring::resolve_live_named_tag(&manager, "investment_globals", None)
                .map_err(anyhow::Error::msg)?,
        )
        .map_err(anyhow::Error::msg)?;
    let sockets = crate::tiger::shadowkeep::sockets(native, donor)?;
    let mut found = Vec::new();
    for (lane, socket) in sockets["sockets"]
        .as_array()
        .context("Donor sockets")?
        .iter()
        .enumerate()
    {
        let choices = socket["choices"].as_array().context("Donor socket plugs")?;
        let chosen = match recipe["overrides"]["socket_columns"][lane]["choices"][0].as_str() {
            Some(hash) => Some(u32::from_str_radix(hash.trim_start_matches("0x"), 16)?),
            None => choices
                .iter()
                .find(|choice| choice["default"] == true)
                .and_then(|choice| choice["hash"].as_u64())
                .map(u32::try_from)
                .transpose()?,
        };
        let Some(plug) = chosen else { continue };
        let Some(tag) = choices
            .iter()
            .find(|choice| choice["hash"] == plug)
            .and_then(|choice| choice["tag"].as_str())
        else {
            continue;
        };
        let definition = native.tag(u32::from_str_radix(tag, 16)?, None)?;
        // A plug whose perks or actions do not read is not the full-charge perk's plug.
        let Ok(perks) = plug_perks(&definition) else {
            continue;
        };
        for perk in perks {
            let Ok(runtime) = authoring::sandbox_perk::load_sandbox_perk_runtime_action(
                &manager,
                &globals,
                usize::from(perk),
            ) else {
                continue;
            };
            let Ok(decoded) = action::decode(&runtime.action_payload) else {
                continue;
            };
            let Some(group) = decoded.groups.first() else {
                continue;
            };
            let patterns = group
                .effects
                .iter()
                .filter(|effect| effect.class == PATTERN_OVERRIDE)
                .filter_map(|effect| effect.referenced_tag)
                .collect::<Vec<_>>();
            if !group.activation.iter().any(full_charge) || patterns.is_empty() {
                continue;
            }
            let &[pattern] = patterns.as_slice() else {
                bail!("The donor's full-charge perk {perk} overrides the pattern more than once");
            };
            found.push(DonorPerk {
                lane,
                plug,
                perk,
                action: runtime.action_tag.0,
                payload: runtime.action_payload,
                pattern,
            });
        }
    }
    match <[DonorPerk; 1]>::try_from(found) {
        Ok([perk]) => {
            let graph = native.tag(perk.pattern, Some(0x8080_9C0F))?;
            ensure!(
                graph.u8(0x96)? == 18,
                "The donor's full-charge pattern is not a projectile"
            );
            Ok(perk)
        }
        Err(found) if found.is_empty() => {
            bail!("The donor has no perk that overrides its fired pattern at full charge")
        }
        Err(_) => {
            bail!("The donor has several perks that override its fired pattern at full charge")
        }
    }
}

/// The settings rows of `graph` when it is an entity whose every component is a Property
/// Modifiers owner, so attaching it only changes its holder's inputs.
fn modifier_rows(native: &mut Reader, graph: u32) -> Result<Option<usize>> {
    let entity = native.tag(graph, Some(0x8080_9C0F))?;
    let components = entity.array(16, 12, Some(0x8080_9C04))?;
    if components.is_empty() {
        return Ok(None);
    }
    let mut rows = 0;
    for row in components {
        let owner = native.tag(entity.u32(row)?, Some(0x8080_9C36))?;
        let definition = owner.pointer(24)?;
        if owner.u32(definition - 4)? != MODIFIERS {
            return Ok(None);
        }
        rows += owner.array(definition + 88, 88, Some(MODIFIER_ROW))?.len();
    }
    Ok(Some(rows))
}

/// The private intrinsic program: the donor's full-charge perk with its pattern override naming
/// `root`, its pure modifier attachments left out and, for a steered carrier, the tracking count.
/// Returns the program, the left-out attachments and the actions kept from the donor.
fn program(
    native: &mut Reader,
    perk: &DonorPerk,
    root: &str,
    steered: bool,
) -> Result<(Program, Vec<Value>, Vec<&'static str>)> {
    let (mut program, recovery) =
        decompile::recover(&perk.payload, PROGRAM, |tag| tag).map_err(anyhow::Error::msg)?;
    if let Recovery::NativeForm(reason) = recovery {
        bail!("The donor's full-charge perk has no editable program form: {reason}");
    }
    let mut left = Vec::new();
    let mut kept = Vec::new();
    let mut actions = Vec::new();
    for mut action in std::mem::take(&mut program.actions) {
        let label = action.label();
        match &mut action {
            Action::Pattern { asset } => {
                ensure!(
                    asset.graph == perk.pattern && asset.imported.is_none(),
                    "The full-charge pattern override moved while decompiling"
                );
                asset.imported = Some(root.to_owned());
            }
            Action::Attach { asset, .. } => {
                if let Some(rows) = modifier_rows(native, asset.graph)? {
                    left.push(json!({"effect":label,"graph":format!("{:08X}",asset.graph),
                        "modifier_rows":rows,
                        "reason":"Property modifiers that tune the donor's own charged shot. The converted projectile carries the source shot"}));
                    continue;
                }
                kept.push(label);
            }
            _ => kept.push(label),
        }
        actions.push(action);
    }
    ensure!(
        actions
            .iter()
            .filter(
                |action| matches!(action, Action::Pattern { asset } if asset.imported.is_some())
            )
            .count()
            == 1,
        "The full-charge perk has no single pattern override"
    );
    let count = Action::WeaponReferenceCount { selector: 1 };
    if steered && !actions.contains(&count) {
        actions.push(count);
    }
    program.actions = actions;
    program.validate().map_err(anyhow::Error::msg)?;
    Ok((program, left, kept))
}

/// Writes the program on the intrinsic's private variant, creating the variant when the recipe
/// has none for that socket. An earlier import's program is replaced. Another program on the
/// same perk is refused.
fn variant(recipe: &mut Value, perk: &DonorPerk, program: &Program, guided: bool) -> Result<()> {
    let entry = json!({"source_perk_index":perk.perk,"program":serde_json::to_value(program)?,
        "runtime_values":[],"action_float_values":[]});
    let variants = recipe["overrides"]
        .as_object_mut()
        .context("Recipe overrides")?
        .entry("socket_plug_variants")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("Recipe private variants")?;
    let matching = variants
        .iter()
        .enumerate()
        .filter(|(_, row)| row["socket_index"] == perk.lane && row["choice_index"] == 0)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ensure!(matching.len() <= 1, "Repeated private intrinsic variants");
    let row = if let Some(&index) = matching.first() {
        &mut variants[index]
    } else {
        let description = if guided {
            "Hold the trigger to charge the weapon. A full charge fires its charged shot, which tracks its target."
        } else {
            "Hold the trigger to charge the weapon. A full charge fires its charged shot."
        };
        variants.push(json!({"socket_index":perk.lane,"choice_index":0,
            "source_plug_hash":format!("0x{:08X}",perk.plug),"name":"Full Charge",
            "description":description,"additional_sandbox_perks":[],"sandbox_perks":[]}));
        variants.last_mut().context("Private intrinsic")?
    };
    ensure!(
        crate::graph::hash(row, "source_plug_hash")? == perk.plug,
        "The private intrinsic names another plug than the full-charge perk's"
    );
    let programs = row
        .as_object_mut()
        .context("Private intrinsic")?
        .entry("sandbox_perks")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("Intrinsic private programs")?;
    let existing = programs
        .iter()
        .position(|row| row["source_perk_index"] == perk.perk);
    match existing {
        Some(index) => {
            ensure!(
                programs[index]["program"]["name"] == PROGRAM,
                "The full-charge perk already carries another authored program"
            );
            programs[index] = entry;
        }
        None => programs.push(entry),
    }
    Ok(())
}

/// The release receipt of a Spew-Charge pair, whose weapon fires the charged barrel's projectile
/// as its own shot, `partial` for the spew it leaves out, or `native` when no projectile converted.
fn spewed(mut receipt: Value, pair: super::ChargedPair, graph: &Value) -> Value {
    let converted = graph["projectile"]["root"].is_string();
    receipt["status"] = json!(if converted { "partial" } else { "native" });
    receipt["fired_barrel"] = json!(pair.fired());
    receipt["reason"] = json!(
        "The source spews its primary barrel while it charges and fires its secondary barrel once charged, so the weapon fires the secondary barrel's projectile as its own shot when the donor's charge completes"
    );
    receipt["omitted"] =
        json!(["The primary barrel's spew during the charge, an aiming tracer, is not fired"]);
    receipt["projectile"] = graph["projectile"]["conversion"]["source"].clone();
    receipt["limits"] = json!([
        "The donor's charge and hold policy decide when the shot fires. Charge timing is the charge receipt's. Gameplay is unverified."
    ]);
    receipt
}

/// The charged release receipt, or `None` for a source without a charged pair. A translated
/// release adds the charged barrel's projectile to `graph["additional_projectiles"]` and the
/// private program to `recipe`. A refused one records why and changes neither.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn prepare(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    native: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    graph: &mut Value,
    recipe: &mut Value,
    root: &Path,
) -> Result<Option<Value>> {
    let root_id = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let gameplay = &behavior["nodes"][format!("{root_id:08X}")]["data"]["object"]["gameplay"];
    let Some(pair) = super::charged_pair(gameplay) else {
        return Ok(None);
    };
    let barrels = gameplay["barrels"].as_array().context("Source barrels")?;
    let trigger = &gameplay["triggers"][pair.trigger];
    let mut receipt = json!({"tap_barrel":pair.tap,"charged_barrel":pair.charged,
        "trigger_behavior":trigger["behavior"],"secondary_action":trigger["secondary_action"],
        "charging_seconds":trigger["charging_seconds"],"gameplay_verified":false});
    let refuse = |mut receipt: Value, reason: String| -> Result<Option<Value>> {
        receipt["status"] = json!("native");
        receipt["reason"] = json!(reason);
        Ok(Some(receipt))
    };
    // A Spew-Charge trigger's one shot is its charged barrel's, which the weapon fires itself
    // through the donor's charge. Nothing is left for a full-charge perk to fire.
    if pair.released().is_none() {
        return Ok(Some(spewed(receipt, pair, graph)));
    }
    let Some(fired) = graph["projectile"]["root"].as_str().map(str::to_owned) else {
        return refuse(
            receipt,
            "The weapon fires no converted tap projectile, so the charged shot has no launch to share".into(),
        );
    };
    if recipe["overrides"]["fired_graph"] != json!({ "imported": fired }) {
        return refuse(
            receipt,
            "The recipe fires another graph than the converted tap projectile".into(),
        );
    }
    let counts = [pair.tap, pair.charged]
        .map(|index| super::super::native::projectile_count(&barrels[index], behavior));
    // The Barrel fires one pellet count for both shots, and only the weapon's own fired graph is
    // checked against it, so each barrel must fire a single projectile.
    if counts.iter().any(|count| !matches!(count, Ok(1))) {
        return refuse(
            receipt,
            "The charged release needs both barrels to fire one projectile per shot".into(),
        );
    }
    let donor = crate::graph::hash(&recipe["donor"], "item_hash")?;
    let perk = match donor_perk(native, recipe, donor) {
        Ok(perk) => perk,
        Err(error) => return refuse(receipt, format!("{error:#}")),
    };
    receipt["donor_perk"] = json!({"socket_index":perk.lane,"plug":format!("0x{:08X}",perk.plug),
        "source_perk_index":perk.perk,"action":format!("{:08X}",perk.action),
        "pattern":format!("{:08X}",perk.pattern),
        "activation":"General Predicate 80803DCE, weapon_charge_fraction 7AA347DD between 1 and 1"});
    // A charged shot leaves the tap shot's Barrel, so it may fly its source speed curve only
    // where the tap projectile's launch does.
    let curve = graph["projectile"]["conversion"]["launch_policy"] == "source_player_trajectory";
    let section = match projectile::charged(
        cache,
        pages,
        native,
        behavior,
        projectiles,
        entry,
        root,
        curve,
    ) {
        Ok(section) => section,
        Err(error) => {
            return refuse(
                receipt,
                format!("The charged barrel's projectile was not converted: {error:#}"),
            );
        }
    };
    if section["conversion"]["launch_policy"] != graph["projectile"]["conversion"]["launch_policy"]
    {
        let _ = std::fs::remove_dir_all(root.join(format!("projectile-barrel-{}", pair.charged)));
        return refuse(
            receipt,
            "The charged projectile flies by another launch policy than the tap projectile, and both leave one Barrel".into(),
        );
    }
    let symbol = section["root"]
        .as_str()
        .context("Charged projectile root")?
        .to_owned();
    let guidance = section["conversion"]["guidance"]["translation"].clone();
    let steered = guidance == projectile::GUIDED;
    let guided = section["conversion"]["guidance"]["source"]["rate_bounds_radians_per_second"][1]
        .as_f64()
        .is_some_and(|rate| rate > 0.);
    let (program, left, kept) = match program(native, &perk, &symbol, steered) {
        Ok(program) => program,
        Err(error) => {
            let _ =
                std::fs::remove_dir_all(root.join(format!("projectile-barrel-{}", pair.charged)));
            return refuse(receipt, format!("{error:#}"));
        }
    };
    if let Err(error) = variant(recipe, &perk, &program, steered) {
        let _ = std::fs::remove_dir_all(root.join(format!("projectile-barrel-{}", pair.charged)));
        return refuse(receipt, format!("{error:#}"));
    }
    let mut omitted = Vec::new();
    if !curve {
        omitted.push(json!(
            "The charged projectile keeps its carrier's speed fitted to the Barrel's launch, as the tap projectile does, rather than its source speed"
        ));
    }
    if guided && !steered {
        omitted.push(json!(
            "The source charged projectile steers toward its target, which its carrier does not translate, so the shot flies straight and the tracking count is not added"
        ));
    }
    receipt["status"] = json!(if omitted.is_empty() {
        "translated"
    } else {
        "partial"
    });
    receipt["pattern_override"] =
        json!({"native":format!("{:08X}",perk.pattern),"imported":symbol});
    receipt["projectile"] = json!({"source":section["conversion"]["source"],
        "native_carrier":section["conversion"]["native_carrier"],
        "launch_policy":section["conversion"]["launch_policy"],"guidance":guidance});
    receipt["tracking_count"] = json!(steered);
    receipt["left_out"] = json!(left);
    receipt["retained_native"] = json!(kept);
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!([
        "The charged shot plays the weapon's own firing route. The charged barrel's firing recording is not translated.",
        "The donor's charge and hold policy decide when the full-charge perk starts and ends, and whether releasing or holding fires the charged shot. Charge timing is the charge receipt's.",
        "Effects kept from the donor's perk act on the donor's runtime presentation, which an imported model may not show. Gameplay is unverified."
    ]);
    let sections = graph
        .as_object_mut()
        .context("Asset graph")?
        .entry("additional_projectiles")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("Additional projectiles")?;
    ensure!(
        sections.is_empty(),
        "The graph already holds an additional projectile"
    );
    sections.push(section);
    Ok(Some(receipt))
}
