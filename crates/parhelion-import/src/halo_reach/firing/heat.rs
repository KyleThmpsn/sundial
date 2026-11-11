//! Reach heat as the native magazine and reload. A barrel that adds a fixed heat per round locks
//! once the weapon's heat passes its overheated threshold and fires again when the heat falls to
//! its recovery threshold at the overheated loss rate. Shadowkeep's Weapon Controller can hold a
//! native heat component, but only stock turrets compose it, in a controller without a magazine,
//! and where it takes each round's heat from is not established. So the rounds fired from cold
//! before the lock become the magazine and the vent becomes both reload times. An authored
//! capacity can replace those rounds as the magazine's scale.
//!
//! Reach also loses heat while the trigger is up. A private program on the weapon's intrinsic
//! carries that as a refill: once the weapon has not fired for [`COOLING_DELAY`], it adds rounds to
//! the magazine at the source's heat loss rate at the magazine's scale until the trigger is pulled
//! or the magazine is full. Its start is Box Breathing's "not firing, held" check without the
//! aiming requirement, it ends at once and its rearm timer repeats it.
use super::number;
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sundial::package_authoring::sandbox_perk::program::{Action, NativeNode, Program, Trigger};

/// The source lock: the rounds fired from cold before the weapon overheats, the seconds it then
/// takes to cool to its recovery threshold, and its heat at the lock.
#[derive(Clone, Copy)]
pub(super) struct Lockout {
    pub shots: i64,
    pub vent: f32,
    pub peak: f32,
}

/// Seconds without firing before a heat-locked magazine starts to refill. Reach loses heat as soon
/// as the trigger is up, so this is an authored delay.
const COOLING_DELAY: f32 = 2.;
/// The target time between refill steps. Each step adds whole rounds.
const COOLING_STEP: f32 = 0.2;
/// The private refill program's name.
pub(in crate::halo_reach) const COOLING: &str = "Reach Heat Cooling";
/// The stock finished perk row the refill program takes, as the workbench's new effects do.
const CARRIER: u16 = 1178;
/// Box Breathing's start, stock perk 342 action `80BBC990`: General Predicate kind 20 that passes
/// once `weapon_firing` (`59E4FF8D`) has read 0, from 0 to 0, for 1.5 seconds while aiming. Its
/// health, shield, fireteam, interface and magazine ranges are the permissive stock ones.
const NOT_FIRING: &str = "0000803FFF140100000000000000803F000080BF000080BF000000000000803F000000000000803F000000000000803F000000000000004200000000000000002000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001040000000000000000803F000000000000803F000000000000803F000000000000803F000000000000803F000000000000803F000000000000803F000000000000000000000000FFFFFFFF00000000000000008DFFE45900000000000000000000C03F00000000000000000000000000000000000000000000000000000000";
/// The predicate's weapon state byte, Aiming Down Sights (4) in Box Breathing, and its hold.
const WEAPON_STATE: usize = 0x81;
const HOLD: usize = 0xE0;
/// Always, kind 0, as an ending: the effect ends as soon as it starts.
const AT_ONCE: [u8; 8] = [0x00, 0x00, 0x80, 0x3F, 0xFF, 0x00, 0x00, 0x01];

fn refuse(mut receipt: Value, reason: &str) -> Result<Option<(Value, Option<Lockout>)>> {
    receipt["status"] = json!("native");
    receipt["reason"] = json!(reason);
    Ok(Some((receipt, None)))
}

/// The heat receipt and lock of a barrel that generates heat, or `None` for one that does not.
/// A receipt without a lock already holds its status and reason.
pub(super) fn read(gameplay: &Value, barrel: &Value) -> Result<Option<(Value, Option<Lockout>)>> {
    let per_round = number(&barrel["heat_per_round"])?;
    let function = barrel["heat_function"].as_str().unwrap_or_default();
    if per_round <= 0. && function.is_empty() {
        return Ok(None);
    }
    let heat = &gameplay["heat"];
    let mut receipt = json!({"heat_per_round":per_round,"heat_function":function,"source":heat});
    if !heat.is_object() {
        return refuse(receipt, "The source weapon's heat was not read");
    }
    if !function.is_empty() {
        return refuse(
            receipt,
            "The source heat per round comes from an object function, which is not evaluated",
        );
    }
    let threshold = number(&heat["overheated_threshold"])?;
    let recovery = number(&heat["recovery_threshold"])?;
    let cooling = number(&heat["overheated_loss_per_second"])?;
    if threshold <= 0. {
        return refuse(receipt, "The source has no overheated threshold");
    }
    if recovery >= threshold {
        return refuse(
            receipt,
            "The source recovers at once, since its recovery threshold is not below its overheated threshold",
        );
    }
    if !heat["overheated_loss_function"]
        .as_str()
        .unwrap_or_default()
        .is_empty()
    {
        return refuse(
            receipt,
            "The source overheated heat loss comes from an object function, which is not evaluated",
        );
    }
    if cooling <= 0. {
        return refuse(receipt, "The source never cools once overheated");
    }
    // The weapon overheats on the first round that carries its heat past the threshold, and the
    // round still fires.
    let shots = (f64::from(threshold) / f64::from(per_round)).floor() as i64 + 1;
    if shots > 500 {
        return refuse(
            receipt,
            "The source fires more rounds before overheating than a native magazine holds",
        );
    }
    let peak = shots as f32 * per_round;
    let vent = (peak - recovery.max(0.)) / cooling;
    ensure!(peak.is_finite(), "Source heat at the lock is not finite");
    if !(0.1..=30.).contains(&vent) {
        return refuse(receipt, "The source vent time lies outside native limits");
    }
    let loss = number(&heat["loss_per_second"])?;
    let mut omitted = Vec::<Value>::new();
    if number(&heat["venting_seconds"])? > 0.
        || !heat["venting_loss_function"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    {
        omitted.push(json!(
            "The source venting time and venting heat loss function for an early vent are not translated"
        ));
    }
    if number(&heat["detonation_fraction"])? > 0. {
        omitted.push(json!(
            "The source chance to explode when fired above its heat detonation threshold is not translated"
        ));
    }
    // A magazine is an approximation of heat, so a translated lock is always partial.
    receipt["status"] = json!("partial");
    receipt["overheat_shots"] = json!(shots);
    receipt["peak_heat"] = json!(peak);
    receipt["vent_seconds"] = json!(vent);
    receipt["native"] = json!(
        "Magazine capacity and refill (inputs 0 and 3) and Reload Time and Empty Reload Time (Reload inputs 1 and 2)"
    );
    receipt["inputs"] = json!([]);
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!([
        format!(
            "The source loses {loss} heat a second while it does not fire. A private refill carries that once the weapon has not fired for {COOLING_DELAY} seconds, as the cooling receipt describes. The native weapon locks once its magazine is empty, and its vent refills it from the reserves."
        ),
        "A reload with rounds remaining vents for the full time, while the source cannot vent early.",
        "No Shadowkeep firearm carries heat. The native heat component, instance class 80803859 with definition 8080395F, cools and refuses fire only in stock turrets, whose controller has no magazine, and the storage of its heat per round is not established, so the magazine stands in for it.",
        "The reload plays the source overheat motion where the first-person translation selects it, and the donor's HUD meter shows the vent. Gameplay is unverified."
    ]);
    Ok(Some((receipt, Some(Lockout { shots, vent, peak }))))
}

/// The refill that carries the source's heat loss while the trigger is up, for a lock whose
/// magazine holds `rounds`: rounds a second at that scale, whole rounds per step and the step time.
pub(super) fn cooling(heat: &Value, lock: &Lockout, rounds: i64) -> Result<Value> {
    let omitted = |reason: &str| Ok(json!({"status":"omitted","reason":reason}));
    if !heat["loss_function"]
        .as_str()
        .unwrap_or_default()
        .is_empty()
    {
        return omitted(
            "The source heat loss comes from an object function, which is not evaluated",
        );
    }
    let loss = number(&heat["loss_per_second"])?;
    if loss <= 0. || lock.peak <= 0. || rounds <= 0 {
        return omitted("The source loses no heat while it does not fire");
    }
    let rate = loss * rounds as f32 / lock.peak;
    let step = (rate * COOLING_STEP).round().max(1.);
    let interval = (step / rate * 1000.).round();
    if !(50. ..=10_000.).contains(&interval) {
        return omitted("The source heat loss gives a refill step outside native timer limits");
    }
    Ok(
        json!({"status":"translated","rounds_per_second":rate,"delay_seconds":COOLING_DELAY,
        "rounds_per_step":step as i64,"step_ms":interval as u32,"magazine":rounds,
        "basis":"The source heat loss per second times the magazine over the heat at the lock"}),
    )
}

/// The private refill program of a translated `cooling` receipt, on the weapon's intrinsic, and its
/// report. `None` when the receipt has no refill.
pub(in crate::halo_reach) fn program(
    native: &mut Reader,
    recipe: &mut Value,
    cooling: &Value,
) -> Result<Option<Value>> {
    if cooling["status"] != "translated" {
        return Ok(None);
    }
    let rounds = i32::try_from(
        cooling["rounds_per_step"]
            .as_i64()
            .context("Heat refill step")?,
    )?;
    let step = u32::try_from(
        cooling["step_ms"]
            .as_u64()
            .context("Heat refill interval")?,
    )?;
    let mut predicate = hex::decode(NOT_FIRING)?;
    ensure!(
        predicate.len() == 0x100
            && predicate[5] == 20
            && predicate[WEAPON_STATE] == 4
            && predicate[0xD4..0xD8] == 0x59E4_FF8Du32.to_le_bytes()
            && predicate[0xD8..0xE0] == [0; 8],
        "Not-firing predicate template differs"
    );
    // Any weapon state, so the magazine refills whether or not the weapon is aimed or in hand.
    predicate[WEAPON_STATE] = 0;
    predicate[HOLD..HOLD + 4].copy_from_slice(&COOLING_DELAY.to_le_bytes());
    let program = Program {
        name: COOLING.into(),
        trigger: Trigger::Native,
        duration_ms: 0,
        cooldown_ms: step,
        chance_permyriad: 10_000,
        actions: vec![Action::add_rounds(rounds)],
        native_trigger: Some(NativeNode {
            kind: 20,
            bytes: predicate,
        }),
        native_removal: Some(NativeNode {
            kind: 0,
            bytes: AT_ONCE.to_vec(),
        }),
        ..Program::default()
    };
    let perk = json!({"program":program,"source_perk_index":CARRIER,
        "runtime_values":[],"action_float_values":[]});
    let (lane, plug) = crate::d2_mot::glaive::intrinsic_program(
        native,
        recipe,
        CARRIER,
        perk,
        (
            COOLING,
            "Refills the magazine while the weapon does not fire, as its heat would cool.",
        ),
    )?;
    Ok(Some(json!({"status":"translated","socket_index":lane,
        "source_plug":format!("{plug:08X}"),"source_perk_index":CARRIER,"program":COOLING,
        "start":format!("Not firing for {COOLING_DELAY} seconds, Box Breathing's General Predicate without its aiming state"),
        "step":format!("Add {rounds} rounds to the magazine, end at once, rearm after {step} ms"),
        "gameplay_verified":false,
        "limits":["That the predicate's hold carries over each rearm, so the refill steps at its interval rather than waiting the delay again, is inferred from its continuous hold and is unverified in game.",
            "Firing any weapon reads weapon_firing, so firing another weapon pauses the refill."]})))
}
