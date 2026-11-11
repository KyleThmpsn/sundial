//! A Reach charging trigger as the native Charge component's Charge Time (input 0) and Hold Time
//! (input 1), both in seconds. The native charge state runs for the charge time, then follows the
//! donor's hold policy, Charge definition byte `+2E8`. A Reach trigger charges for its charging
//! time and, when its overcharged action is Discharge, fires on its own once it has stayed
//! charged for its charged time, which the hold time carries.
//!
//! An input reaches the native charge one of two ways. A donor whose stat translator copies a
//! value into it takes a literal there. Otherwise the input starts from its own literal in the
//! Charge definition (`80803ABE`): `m_stat_collection` at `+10` holds one `0x20`-byte row per
//! input, whose `+00` reference names the input's numeric member, and component initialization
//! reads a kind 0 member's Float32 at `+04`. The build writes that literal. A translator logic row
//! that computes the input leaves neither route. A donor without a Charge component does not
//! charge and keeps its own firing.
use super::{literal, number};
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use sundial::package_authoring::runtime::WeaponRuntimeEntitySource;

/// Overcharged actions: none holds the charge, explode destroys the weapon and discharge fires.
const DISCHARGE: i64 = 2;
const EXPLODE: i64 = 1;
/// Charge flag 0, Can Fire From Partial Charge.
const PARTIAL: u64 = 1;

/// The Charge definition's class word, which precedes the definition its resource names.
const DEFINITION: u32 = 0x8080_3ABE;
const DEFINITION_SIZE: usize = 0x418;
/// `m_stat_collection`, one row per input, and the hold policy byte.
const COLLECTION: usize = 0x10;
const ROW: usize = 0x20;
const MODE: usize = 0x2E8;
/// The hold policy whose charged state fires at once, without reading the hold time: charge
/// update `D090F0` takes its fire path on the first update in state 4 unless its caller cancels.
const FIRES_CHARGED: i8 = 2;
/// The stat translator binding.
const TRANSLATOR: u32 = 0x2F98_1564;

/// The donor's native Charge component.
struct Donor {
    /// Definition byte `+2E8`, the policy the charged state follows.
    mode: i8,
    /// Each input's Float32 literal, when its stat collection row names a kind 0 member.
    literals: [Option<f32>; 3],
    /// Charge inputs a translator logic row computes, which a literal does not reach.
    computed: BTreeSet<u32>,
}

impl Donor {
    /// How `input` reaches the native charge, or `None` when nothing the import writes does.
    fn route(&self, routes: &BTreeSet<(u32, u32)>, input: u32) -> Option<&'static str> {
        if routes.contains(&(4, input)) {
            Some("translator")
        } else if self
            .literals
            .get(input as usize)
            .copied()
            .flatten()
            .is_some()
            && !self.computed.contains(&input)
        {
            Some("definition")
        } else {
            None
        }
    }
}

/// The charge receipt of the donor `item`, the literal inputs it writes and the Charge Time they
/// display, in milliseconds.
pub(super) fn receipt(
    native: &mut Reader,
    item: u32,
    trigger: &Value,
    routes: &BTreeSet<(u32, u32)>,
) -> Result<(Value, Vec<Value>, Option<i32>)> {
    let donor = donor(native, item)
        .map_err(|error| format!("The donor's Charge component cannot be read: {error:#}"));
    let (receipt, inputs) = translate(
        trigger,
        routes,
        donor.as_ref().map(Option::as_ref).map_err(String::as_str),
    )?;
    let milliseconds = inputs
        .first()
        .and_then(|input| input["value"].as_f64())
        .map(|seconds| (seconds * 1000.).round() as i32);
    Ok((receipt, inputs, milliseconds))
}

/// The donor's Charge component, or `None` when the donor has none.
fn donor(native: &mut Reader, item: u32) -> Result<Option<Donor>> {
    use sundial::package_authoring::{self as authoring, entity, runtime};
    let manager = authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let weapon = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, item)
        .map_err(anyhow::Error::msg)?;
    if !entity::weapon_component_binding_hashes(&weapon.payload)
        .map_err(anyhow::Error::msg)?
        .contains(&entity::WEAPON_TRIGGER_CHARGE_COMPONENT_KEY)
    {
        return Ok(None);
    }
    let bindings = entity::weapon_component_bindings(
        &weapon.payload,
        entity::WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
    )
    .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        bail!("The donor has several Charge resources");
    };
    let owner = native.tag(binding.owner_tag, Some(0x8080_9C36))?;
    let start = usize::try_from(binding.resource_offset)?;
    ensure!(
        owner.u32(start)? == binding.owner_tag,
        "The donor's Charge resource prefix differs"
    );
    // A component resource prefix names its definition by an absolute offset in the owner.
    let definition = usize::try_from(owner.u64(start + 8)?)?;
    ensure!(
        definition >= 4
            && definition % 8 == 0
            && definition + DEFINITION_SIZE <= owner.0.len()
            && owner.u32(definition - 4)? == DEFINITION
            && owner.u32(definition)? == binding.owner_tag
            && owner.u32(definition + 4)? == binding.concrete_class,
        "The donor's Charge definition layout differs"
    );
    let mut literals = [None; 3];
    for (input, slot) in literals.iter_mut().enumerate() {
        let row = definition + COLLECTION + input * ROW;
        if owner.u64(row)? == 0 {
            continue;
        }
        let at = owner.pointer(row)?;
        if at > definition && at + 8 <= definition + DEFINITION_SIZE && owner.u8(at)? == 0 {
            *slot = Some(owner.f32(at + 4)?);
        }
    }
    let mode = owner.u8(definition + MODE)? as i8;
    Ok(Some(Donor {
        mode,
        literals,
        computed: computed(native, &weapon)?,
    }))
}

/// The Charge inputs a logic row of the donor's selected translator table computes. The table is
/// the one its translation group keys, or table zero, as the build selects it.
fn computed(native: &mut Reader, weapon: &WeaponRuntimeEntitySource) -> Result<BTreeSet<u32>> {
    let bindings =
        sundial::package_authoring::entity::weapon_component_bindings(&weapon.payload, TRANSLATOR)
            .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        bail!("The donor has no single stat translator");
    };
    let p = native.tag(binding.owner_tag, Some(0x8080_9C36))?;
    let d = p.pointer(24)?;
    let tables = p.array(d + 0x48, 48, Some(0x8080_3975))?;
    let keys = p.array(d + 0x238, 40, Some(0x8080_38C7))?;
    let selected = keys
        .iter()
        .position(|&k| p.u32(k + 16).ok() == Some(weapon.weapon_translation_group_hash))
        .unwrap_or(0);
    let table = *tables
        .get(selected)
        .context("Stat translator tables and keys differ")?;
    let mut inputs = BTreeSet::new();
    for row in p.array(table + 32, 40, Some(0x8080_397C))? {
        for output in p.array(row, 12, Some(0x8080_3987))? {
            if p.u32(output)? == 4 {
                inputs.insert(p.u32(output + 4)?);
            }
        }
    }
    Ok(inputs)
}

/// What the donor's hold policy does once the charge completes, where it is traced.
fn policy(mode: i8) -> &'static str {
    match mode {
        0 => "Hold policy 0 fires when released, or once the hold time has passed",
        1 => "Hold policy 1 fires when released, and overcharges once the hold time has passed",
        2 => "Hold policy 2 fires as soon as the charge completes",
        3 => {
            "Hold policy 3 fires when released and moves on to its next charge state once the hold time has passed while held"
        }
        _ => "The donor's hold policy is not traced",
    }
}

/// The charge receipt and the literal inputs it writes. `routes` is the donor's routed inputs and
/// `donor` its Charge component, or why it could not be read.
fn translate(
    trigger: &Value,
    routes: &BTreeSet<(u32, u32)>,
    donor: std::result::Result<Option<&Donor>, &str>,
) -> Result<(Value, Vec<Value>)> {
    let charging = number(&trigger["charging_seconds"])?;
    let charged = number(&trigger["charged_seconds"])?;
    let overcharged = trigger["overcharged_action"].as_i64().unwrap_or_default();
    let mut receipt = json!({"trigger_behavior":trigger["behavior"],"charging_seconds":charging,
        "charged_seconds":charged,"overcharged_action":overcharged,
        "charge_flags":trigger["charge_flags"],"charged_drain_per_second":trigger["charged_drain_per_second"],
        "fire_fractions":trigger["fire_fractions"]});
    let refuse = |mut receipt: Value, reason: &str| -> Result<(Value, Vec<Value>)> {
        receipt["status"] = json!("native");
        receipt["reason"] = json!(reason);
        Ok((receipt, Vec::new()))
    };
    if !(0.05..=30.).contains(&charging) {
        return refuse(receipt, "The source charge time lies outside native limits");
    }
    let donor = match donor {
        Ok(Some(donor)) => donor,
        Ok(None) => {
            return refuse(
                receipt,
                "The donor has no Charge component (binding 7E4A5223), so it does not charge. A charge on such a host needs a new component owner and its event rows",
            );
        }
        Err(reason) => return refuse(receipt, reason),
    };
    receipt["donor_charge"] = json!({"hold_policy":donor.mode,"policy":policy(donor.mode),
        "definition_literals":donor.literals,"computed_inputs":donor.computed});
    let Some(route) = donor.route(routes, 0) else {
        return refuse(
            receipt,
            "The donor's stat translator writes no Charge Time, and its Charge definition holds no literal for it",
        );
    };
    let mut inputs = vec![literal(4, 0, charging)];
    let mut routed = vec![json!({"input":0,"route":route})];
    let mut omitted = Vec::new();
    let fires_charged = donor.mode == FIRES_CHARGED;
    match overcharged {
        DISCHARGE if fires_charged => {
            if charged > 0. {
                omitted.push(format!(
                    "The source fires {charged} s after it is charged, while the donor fires as soon as its charge completes"
                ));
            }
        }
        DISCHARGE if !(0. ..=30.).contains(&charged) => {
            omitted.push("The source charged time lies outside native limits".to_owned());
        }
        DISCHARGE => match donor.route(routes, 1) {
            Some(route) => {
                inputs.push(literal(4, 1, charged));
                routed.push(json!({"input":1,"route":route}));
            }
            None => omitted.push(
                "The donor writes no Hold Time through its stat translator or its Charge definition, so the donor's hold time stays"
                    .to_owned(),
            ),
        },
        EXPLODE => omitted.push(
            "The source explodes when overcharged, which has no native counterpart. The donor's hold time stays"
                .to_owned(),
        ),
        _ if fires_charged => omitted.push(
            "The source holds its charge until released, while the donor fires as soon as its charge completes"
                .to_owned(),
        ),
        _ => omitted.push(
            "The source holds its charge until released. The donor's hold time stays".to_owned(),
        ),
    }
    // A latch-autofire trigger fires on a tap and starts its charge action only once held for
    // its autofire time. No native Charge input is known to hold such a delay.
    if trigger["behavior"] == 2 {
        let delay = number(&trigger["autofire_seconds"])?;
        if delay > 0. {
            omitted.push(format!(
                "The source starts charging once its trigger is held for {delay} s, which is not translated"
            ));
        }
    }
    if trigger["charge_flags"].as_u64().unwrap_or_default() & PARTIAL != 0 {
        omitted.push(
            "The source fires a partial charge for its charged fraction, which is not translated"
                .to_owned(),
        );
    }
    let drain = trigger["charged_drain_per_second"]
        .as_f64()
        .unwrap_or_default();
    if drain > 0. {
        omitted.push(format!(
            "The source drains {drain} of its battery each second while charged, which is not translated"
        ));
    }
    if trigger["fire_fractions"]
        .as_array()
        .is_some_and(|fractions| !fractions.is_empty())
    {
        omitted.push(
            "The source fires additional shots at charge fractions, which have no native input. The native Barrel fires its pellets together on one shot"
                .to_owned(),
        );
    }
    receipt["status"] = json!(if omitted.is_empty() {
        "translated"
    } else {
        "partial"
    });
    receipt["inputs"] = json!(inputs);
    receipt["routes"] = json!(routed);
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!([
        "A translator route replaces the copied stat value with a literal. A definition route replaces the Charge definition's literal, which component initialization reads before any route. Component modifiers act on either.",
        "The donor's hold policy decides what releasing early or holding past the hold time does.",
        "The donor's cadence, charging effects and discharge effects stay. Gameplay is unverified."
    ]);
    Ok((receipt, inputs))
}
