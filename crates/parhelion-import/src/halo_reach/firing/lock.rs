//! A Reach weapon's target tracking as the donor's lock-on timing, and the receipts that say which
//! of its projectiles steer and what holds the weapon's tracking count.
//!
//! A Reach weapon's target tracking (`weap +0x310`, rows of `0x38`) names the kinds of target it
//! locks on to and the seconds a lock takes to acquire, lasts once the target leaves and takes to
//! decay after that. Shadowkeep's Weapon Controller times its lock-on by its definition's
//! generated field `target_tracking` (FNV-1 `A0680E9E`, type `80804355`, 16 bytes), which lock
//! state machine `E9BB40` reads as four Float32 lanes in seconds: the lock time, an alternate lock
//! time a caller flag selects, the grace after the candidate leaves and the decay after the grace.
//! The source acquire time takes both lock lanes. The lock updates only while the weapon's
//! tracking count, effect kind 30 (`1E 01 01`), is positive. The weapon melee intrinsic adds that
//! count beside a fired carrier that steers by the source's guidance, and the charged release
//! program beside a steered charged projectile. The recipe takes the timing as a typed runtime
//! value on the donor's Weapon Controller, found through the donor's runtime field discovery, so
//! the build re-resolves it on the effective entity. The native lock-on has no target filter, so
//! the source's tracking types are recorded, not translated.
use super::number;
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sundial::package_authoring::runtime::{
    WeaponRuntimeField, WeaponRuntimeValue, WeaponRuntimeValueKind, WeaponRuntimeValueOverride,
};

/// The Weapon Controller binding and its generated `target_tracking` field.
const CONTROLLER: u32 = 0x39AF_D7D3;
const TARGET_TRACKING: u32 = 0xA068_0E9E;
/// The longest lock timing written, in seconds.
const SECONDS: f32 = 30.;

/// The program action that changes the weapon's tracking count, as a recipe stores it.
fn counts(action: &Value) -> bool {
    action["operation"] == "weapon_reference_count"
}

/// Every private program in `recipe` that holds the weapon's tracking count.
fn count_holders(recipe: &Value) -> Vec<Value> {
    let variants = recipe["overrides"]["socket_plug_variants"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    variants
        .iter()
        .flat_map(|variant| {
            variant["sandbox_perks"]
                .as_array()
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .filter(|perk| {
                    perk["program"]["actions"]
                        .as_array()
                        .is_some_and(|actions| actions.iter().any(counts))
                })
                .map(move |perk| {
                    json!({"socket_index":variant["socket_index"],"choice_index":variant["choice_index"],
                        "source_perk_index":perk["source_perk_index"],"program":perk["program"]["name"],
                        "trigger":perk["program"]["trigger"]})
                })
        })
        .collect()
}

/// The tracking receipt: each converted projectile's guidance and the programs that hold the
/// tracking count. `None` when no source projectile the weapon fires steers.
fn tracking(graph: &Value, recipe: &Value) -> Option<Value> {
    let additional = graph["additional_projectiles"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let carriers = std::iter::once(&graph["projectile"])
        .chain(additional)
        .filter(|section| section["conversion"].is_object())
        .map(|section| {
            let guidance = &section["conversion"]["guidance"];
            json!({"source":section["conversion"]["source"],"barrel":section["barrel"],
                "native_carrier":section["conversion"]["native_carrier"],
                "translation":guidance["translation"],"source_guidance":guidance["source"]})
        })
        .collect::<Vec<_>>();
    let guided = |carrier: &&Value| {
        carrier["source_guidance"]["rate_bounds_radians_per_second"][1]
            .as_f64()
            .is_some_and(|rate| rate > 0.)
    };
    let sources = carriers.iter().filter(guided).count();
    if sources == 0 {
        return None;
    }
    let steered = carriers
        .iter()
        .filter(guided)
        .filter(|carrier| carrier["translation"] == super::super::projectile::GUIDED)
        .count();
    let holders = count_holders(recipe);
    let mut omitted = Vec::new();
    if steered < sources {
        omitted.push(
            "A source projectile that steers flies on a carrier that does not take its guidance",
        );
    }
    if steered > 0 && holders.is_empty() {
        omitted.push(
            "No private program holds the weapon's tracking count, so its lock-on never updates",
        );
    }
    let status = match (steered, omitted.is_empty()) {
        (0, _) => "native",
        (_, true) => "translated",
        (_, false) => "partial",
    };
    Some(
        json!({"status":status,"carriers":carriers,"tracking_count":holders,"omitted":omitted,
        "basis":"Every stock tracking perk holds effect kind 30 (1E 01 01). While the count is positive, the Weapon Controller's lock update CCB550 runs and lock state machine E9BB40 times the lock by the definition's target_tracking",
        "limits":["How a fired projectile's target acquisition reads the weapon's lock is not traced. Gameplay is unverified."]}),
    )
}

/// The donor's Weapon Controller `target_tracking` field, as runtime field discovery finds it on
/// the donor's runtime entity.
fn field(native: &mut Reader, donor: u32) -> Result<WeaponRuntimeField> {
    use sundial::package_authoring::{self as authoring, runtime};
    let manager = authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let source = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let graph = runtime::load_weapon_runtime_graph_for_entity(
        &manager,
        source.item_hash,
        source.pattern_global_id_hash,
        source.entity_tag,
        &source.payload,
    )
    .map_err(anyhow::Error::msg)?;
    let fields = graph
        .fields()
        .filter(|field| {
            field.locator.binding_hash.get() == CONTROLLER
                && field.locator.byte_size == 16
                && field
                    .locator
                    .path
                    .first()
                    .is_some_and(|step| step.name_hash == TARGET_TRACKING)
        })
        .collect::<Vec<_>>();
    match fields.as_slice() {
        [field] => Ok((*field).clone()),
        [] => bail!("The donor's Weapon Controller exposes no target_tracking field"),
        _ => bail!("The donor's Weapon Controller exposes several target_tracking fields"),
    }
}

/// `lanes` as the value `field` stores, and the donor's own lanes.
fn value(field: &WeaponRuntimeField, lanes: [f32; 4]) -> Result<(WeaponRuntimeValue, [f32; 4])> {
    let bits = lanes.map(f32::to_bits);
    let (value, stock) = match (&field.kind, &field.value) {
        (WeaponRuntimeValueKind::Vector4Float32, WeaponRuntimeValue::Vector4Float32Bits(stock)) => {
            (
                WeaponRuntimeValue::Vector4Float32Bits(bits),
                stock.map(f32::from_bits),
            )
        }
        (WeaponRuntimeValueKind::FixedBytes { size: 16 }, WeaponRuntimeValue::Bytes(stock)) => {
            let stock: [u8; 16] = stock
                .as_slice()
                .try_into()
                .context("The donor's target_tracking bytes differ")?;
            let stock: [f32; 4] = std::array::from_fn(|lane| {
                f32::from_le_bytes(stock[lane * 4..lane * 4 + 4].try_into().unwrap_or_default())
            });
            (
                WeaponRuntimeValue::Bytes(
                    bits.iter().flat_map(|bits| bits.to_le_bytes()).collect(),
                ),
                stock,
            )
        }
        _ => bail!("The donor's target_tracking field has another value kind"),
    };
    sundial::package_authoring::runtime::encode_weapon_runtime_field_value(field, &value)
        .map_err(anyhow::Error::msg)?;
    Ok((value, stock))
}

/// Puts `entry` in the recipe's runtime values, unless a value at the same field is already there,
/// which wins as authored values do when an import is refreshed. Returns whether it was written.
fn place(recipe: &mut Value, entry: Value) -> Result<bool> {
    let values = recipe["overrides"]
        .as_object_mut()
        .context("Recipe overrides")?
        .entry("runtime_values")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("Recipe runtime values")?;
    if values.iter().any(|row| row["locator"] == entry["locator"]) {
        return Ok(false);
    }
    values.push(entry);
    Ok(true)
}

/// The lock-on receipt of a weapon with target tracking, or `None` for one without, after
/// writing the lock timing into `recipe`. `count` says whether a program holds the tracking count.
fn lock_on(
    native: &mut Reader,
    gameplay: &Value,
    recipe: &mut Value,
    count: bool,
) -> Result<Option<Value>> {
    let Some(rows) = gameplay["target_tracking"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    else {
        return Ok(None);
    };
    let source = &rows[0];
    let mut receipt = json!({"source":rows,"units":"Seconds in both games"});
    let refuse = |mut receipt: Value, reason: String| -> Result<Option<Value>> {
        receipt["status"] = json!("native");
        receipt["reason"] = json!(reason);
        Ok(Some(receipt))
    };
    let [acquire, grace, decay] = [
        number(&source["acquire_seconds"])?,
        number(&source["grace_seconds"])?,
        number(&source["decay_seconds"])?,
    ];
    if ![acquire, grace, decay]
        .iter()
        .all(|seconds| (0. ..=SECONDS).contains(seconds))
    {
        return refuse(
            receipt,
            "The source lock timing lies outside native limits".into(),
        );
    }
    let donor = crate::graph::hash(&recipe["donor"], "item_hash")?;
    let lanes = [acquire, acquire, grace, decay];
    let found =
        field(native, donor).and_then(|field| value(&field, lanes).map(|value| (value, field)));
    let ((value, stock), field) = match found {
        Ok(found) => found,
        Err(error) => return refuse(receipt, format!("{error:#}")),
    };
    let entry = serde_json::to_value(WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value,
    })?;
    let written = place(recipe, entry)?;
    let mut omitted = Vec::new();
    if rows.len() > 1 {
        omitted.push(json!("Only the first source tracking block is translated"));
    }
    if source["types"]
        .as_array()
        .is_some_and(|types| !types.is_empty())
    {
        omitted.push(json!(format!(
            "The source locks only onto {}. The native lock-on takes one candidate an update from its owner's target helper and has no target filter, so the donor's candidates stand",
            source["types"]
        )));
    }
    let fractions = gameplay["triggers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|trigger| trigger["fire_fractions"].as_array())
        .map(Vec::len)
        .max()
        .unwrap_or_default();
    if fractions > 0 {
        omitted.push(json!(format!(
            "The source fires a shot at each of {fractions} charge fractions, each toward the target tracked then, so one charge can spread its shots over {fractions} targets. No native input fires at charge fractions, and the native lock-on holds one selected lock in its two slots"
        )));
    }
    if !source["tracking_sound"].is_null() || !source["locked_sound"].is_null() {
        omitted.push(json!(
            "The source tracking and locked sounds have no native lock-on cue"
        ));
    }
    if !count {
        omitted.push(json!(
            "No private program holds the weapon's tracking count, so the lock-on does not update"
        ));
    }
    receipt["status"] = json!(if omitted.is_empty() {
        "translated"
    } else {
        "partial"
    });
    receipt["native"] = json!({"field":"target_tracking","name_hash":format!("{TARGET_TRACKING:08X}"),
        "binding":format!("{CONTROLLER:08X}"),"root_schema":format!("{:08X}", field.locator.root_schema),
        "value_offset":format!("{:X}", field.locator.value_offset),"path":field.path_label,
        "donor_lanes":stock,"lanes":lanes,"written":written,
        "lane_roles":["Lock time","Alternate lock time","Grace","Decay"]});
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!([
        "The alternate lock time takes the source acquire time, since the caller flag that selects it is not traced.",
        "The native lock-on holds one selected lock in its two slots, so a weapon locks one target at a time.",
        "A recipe value already at this field is kept. Gameplay is unverified."
    ]);
    Ok(Some(receipt))
}

/// Writes the weapon's `tracking` receipt, when a projectile it fires steers, and its `lock_on`
/// receipt, when the source has target tracking, under `graph["source_gameplay"]`, and the lock
/// timing into `recipe`. Runs after every program that can hold the tracking count is written.
pub(in crate::halo_reach) fn prepare(
    native: &mut Reader,
    behavior: &Value,
    graph: &mut Value,
    recipe: &mut Value,
) -> Result<()> {
    if graph["source_gameplay"]["source"] != "halo_reach" {
        return Ok(());
    }
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let gameplay = &behavior["nodes"][format!("{root:08X}")]["data"]["object"]["gameplay"];
    let count = !count_holders(recipe).is_empty();
    if let Some(receipt) = tracking(graph, recipe) {
        graph["source_gameplay"]["tracking"] = receipt;
    }
    if let Some(receipt) = lock_on(native, gameplay, recipe, count)? {
        graph["source_gameplay"]["lock_on"] = receipt;
    }
    Ok(())
}
